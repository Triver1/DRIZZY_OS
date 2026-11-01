//! Files: a read-only file browser and the home index behind the
//! launcher's file search.
//!
//! One Prism Ink window: a quiet rail of places, the folder, a large
//! preview, and one input bar over them that holds the path and the filter.
//! Keyboard first: arrows or j/k move, Enter and Right open, Backspace and
//! Left go up, typing filters the folder (Ctrl+F, or a click on the bar,
//! when it has to start with j or k), `/` or `~/` starts a path with Tab
//! completing, Esc clears the bar and then closes, `.` shows hidden files.
//! Nothing here writes to the disk: no delete, rename or move.
//!
//! All I/O is off the main thread and generation-guarded. Workers exist
//! only while they have work (`listing::Mailbox`), rows are virtualised
//! (only what is on screen is shaped or drawn), and every texture goes
//! back to the GPU when the window does. Names are shown through
//! `kind::shown`, so a newline or a bidi override cannot rearrange the UI,
//! and nothing about a file is ever logged.

mod dbus;
pub mod fuzzy;
pub mod index;
pub mod kind;
mod listing;
mod preview;
#[cfg(test)]
mod testdir;

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use glyphon::FontSystem;

pub use dbus::start as start_dbus;
pub use listing::Mailbox;
pub use preview::child_main as decode_child_main;

use fuzzy::{Query, Scratch, Span};
use kind::Kind;
use listing::{CompleteReq, Completed, Entry, ListReq, Listing, Place, Places, Special};
use preview::{Content, Shown};

use crate::{
    anim::{Spring, SpringCfg},
    bar::{Font, Rect, TextCache},
    gfx::{self, DrawList, IconDraw, ImageData, ImageDraw, TextDraw},
    icons::IconSet,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    window::{Chrome, Ctx, Cursor, KeyInput, Placement, WinScene, keys},
};

// ── geometry (logical px, body-relative) ────────────────────────────────

const M: f32 = 40.0;
const BW: f32 = 1180.0;
const BH: f32 = 720.0;
const MIN_W: f32 = 820.0;
const MIN_H: f32 = 480.0;
const RADIUS: f32 = 26.0;
const RAIL_W: f32 = 182.0;
const RAIL_ROW: f32 = 34.0;
/// The input bar, and where the content under it starts.
const BAR_Y: f32 = 14.0;
const BAR_H: f32 = 44.0;
const BAR_R: f32 = 14.0;
const HEAD_H: f32 = 66.0;
const RAIL_TOP: f32 = HEAD_H + 2.0;
/// One colour per slice of the wordmark.
const TITLE_SLICES: i32 = 36;
const FOOT_H: f32 = 44.0;
const ROW_H: f32 = 34.0;
const LIST_PAD: f32 = 10.0;
const ICON: f32 = 17.0;
const PREVIEW_K: f32 = 0.36;
const PV_PAD: f32 = 24.0;
const META_H: f32 = 74.0;
const WHEN_W: f32 = 112.0;
const SIZE_W: f32 = 74.0;
/// Rows are gone this far past the list's edges (icons are not clipped).
const EDGE: f32 = 12.0;

// ── look ────────────────────────────────────────────────────────────────

const F_TITLE: Font = Font::new(25.0, 400).family(TITLE_FACE).italic();
const F_TITLE_PLAIN: Font = Font::new(20.0, 600).family("Geist");
const F_CRUMB: Font = Font::new(14.0, 550).family("Geist");
const F_QUERY: Font = Font::new(14.0, 500).family("Geist");
const F_ROW: Font = Font::new(13.0, 500).family("Geist");
const F_SMALL: Font = Font::new(12.0, 500).family("Geist");
const F_SECTION: Font = Font::new(10.5, 650).family("Geist").track(0.08);
const F_KEY: Font = Font::new(10.5, 650).family("Geist");
const F_PNAME: Font = Font::new(15.0, 600).family("Geist");
const F_MONO: Font = Font::new(11.5, 400).family("Geist Mono");
const F_PLAINTEXT: Font = Font::new(12.0, 400).family("Geist");
const MONO: &str = "Geist Mono";
/// The wordmark's face, loaded with the mono one.
const TITLE_FACE: &str = "Instrument Serif";
const TITLE_PATTERN: &str = "Instrument Serif:italic";

const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
/// Quiet, but it has to survive a bright wallpaper behind the glass.
const META: Rgba = Rgba::hex(0xbcbec7, 1.0);
const DIM: Rgba = Rgba::hex(0x8a8c97, 1.0);
/// The input bar: the launcher's search field.
const FIELD: Rgba = Rgba::hex(0xffffff, 0.055);
const SEL: Rgba = Rgba::hex(0xffffff, 0.07);
const HOVER: Rgba = Rgba::hex(0xffffff, 0.045);
const SHADOW: Rgba = Rgba::hex(0x000000, 0.6);
// The prism ramp, quiet: only source code uses them.
const KEYWORD: Rgba = Rgba::hex(0xc9b2ff, 1.0);
const STRING: Rgba = Rgba::hex(0xffd2a8, 1.0);
const COMMENT: Rgba = Rgba::hex(0x9a9ca6, 1.0);
const CODE: Rgba = Rgba::hex(0xc9cbd3, 1.0);

// ── motion ──────────────────────────────────────────────────────────────

const OPEN_W: SpringCfg = SpringCfg::new(0.3, 0.84);
const OPEN_H: SpringCfg = SpringCfg::new(0.34, 0.86);
const CLOSE: SpringCfg = SpringCfg::new(0.24, 1.0);
const SCROLL: SpringCfg = SpringCfg::new(0.22, 1.0);
const SEL_HEAD: SpringCfg = SpringCfg::new(0.2, 0.8);
const SEL_TAIL: SpringCfg = SpringCfg::new(0.28, 0.92);
const RAIL: SpringCfg = SpringCfg::new(0.26, 0.82);
const FADE: SpringCfg = SpringCfg::new(0.18, 1.0);
const CARET: SpringCfg = SpringCfg::new(0.16, 0.92);
const REVEAL: SpringCfg = SpringCfg::new(0.2, 0.9);
/// A preview starts once the selection has rested this long…
const SETTLE: f32 = 0.04;
/// …or this long after it started moving, for held keys.
const SETTLE_MAX: f32 = 0.22;
/// A preview that takes longer than this gets a placeholder.
const PLACEHOLDER_AFTER: f32 = 0.12;
const BLINK_FOR: f32 = 10.0;
const DOUBLE: f32 = 0.42;
const HOVER_SLOP: f32 = 4.0;
/// Images kept on the GPU while the window is open, for flipping back.
const IMAGE_CACHE: usize = 8;
const PAGE_ROWS: usize = 12;

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

// ── the command line and the wire ───────────────────────────────────────

/// A place to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub path: PathBuf,
    /// Where it was found, in output coordinates: the window grows out of it.
    pub from: Option<Rect>,
}

const CMD: &str = "files";

fn pct_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len());
    for &c in b {
        if c.is_ascii_alphanumeric() || matches!(c, b'/' | b'.' | b'_' | b'-' | b'~' | b'+') {
            s.push(c as char);
        } else {
            s.push_str(&format!("%{c:02X}"));
        }
    }
    s
}

fn pct_decode(s: &[u8]) -> Vec<u8> {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut o = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%'
            && let (Some(a), Some(b)) = (s.get(i + 1).copied().and_then(hex), s.get(i + 2).copied().and_then(hex))
        {
            o.push(a * 16 + b);
            i += 3;
        } else {
            o.push(s[i]);
            i += 1;
        }
    }
    o
}

/// The local path in a plain path or a `file://` URI. Other schemes are
/// not ours to open.
pub fn local_path(arg: &[u8]) -> Option<PathBuf> {
    let Some(rest) = arg.strip_prefix(b"file://") else {
        // `smb://…`, `https://…`
        let scheme = arg.iter().position(|&b| b == b':').filter(|&p| p > 0 && arg[..p].iter().all(u8::is_ascii_alphanumeric));
        return (scheme.is_none_or(|p| !arg[p..].starts_with(b"://")) && !arg.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(arg)));
    };
    let path = match rest.iter().position(|&b| b == b'/') {
        Some(0) => rest,
        Some(p) if rest[..p] == *b"localhost" => &rest[p..],
        _ => return None,
    };
    Some(PathBuf::from(OsString::from_vec(pct_decode(path))))
}

/// The one-line command for `Fact::Command` and the control socket. The
/// path is percent-encoded: it may hold newlines, or not be UTF-8.
pub fn reveal_line(path: &Path, from: Option<Rect>) -> String {
    let mut s = format!("{CMD} {}", pct_encode(path.as_os_str().as_bytes()));
    if let Some(r) = from {
        s.push_str(&format!(" {} {} {} {}", r.x, r.y, r.w, r.h));
    }
    s
}

/// `Some(None)`: the bare command (toggle). `Some(Some(_))`: show a place.
pub fn parse_command(c: &str) -> Option<Option<Request>> {
    let rest = c.strip_prefix(CMD)?;
    if rest.is_empty() {
        return Some(None);
    }
    let mut it = rest.strip_prefix(' ')?.split(' ');
    let path = PathBuf::from(OsString::from_vec(pct_decode(it.next()?.as_bytes())));
    let n: Vec<f32> = it.filter_map(|v| v.parse().ok()).collect();
    let from = (n.len() == 4).then(|| Rect::new(n[0], n[1], n[2], n[3]));
    Some(Some(Request { path, from }))
}

/// `nimbus msg files [path]`: handled before anything else in `main`, so
/// any bytes work as a path. Relative paths are the caller's. None when
/// these are other arguments; else the exit code (1 when nimbus is not
/// running or the argument is not a local path, so a wrapper can fall back).
pub fn msg_main() -> Option<i32> {
    let mut a = std::env::args_os().skip(1);
    if a.next()?.as_os_str() != OsStr::new("msg") || a.next()?.as_os_str() != OsStr::new(CMD) {
        return None;
    }
    let line = match a.next() {
        None => CMD.to_string(),
        Some(arg) => {
            let Some(p) = local_path(arg.as_bytes()) else {
                eprintln!("nimbus msg files: not a local path");
                return Some(1);
            };
            reveal_line(&std::path::absolute(&p).unwrap_or(p), None)
        }
    };
    Some(match crate::ipc::send(&line) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("nimbus msg: {e} (is nimbus running?)");
            1
        }
    })
}

static PENDING: Mutex<Option<Request>> = Mutex::new(None);

/// What the next `open` shows.
pub fn set_pending(r: Request) {
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(r);
    }
}

/// Hand a request to a window that is already open.
pub fn deliver(r: Request) -> Fact {
    Fact::Custom(Arc::new(r))
}

/// Open with the default app: `xdg-open`, direct argv, no shell. Runs
/// detached in its own session, like something the user started.
pub fn open_default(path: &Path) {
    let path = path.to_path_buf();
    let _ = std::thread::Builder::new().name("nimbus-open".into()).spawn(move || {
        use std::process::{Command, Stdio};
        let run = |prog: &str, args: &[&OsStr]| Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !run("setsid", &[OsStr::new("-f"), OsStr::new("xdg-open"), path.as_os_str()]) {
            run("xdg-open", &[path.as_os_str()]);
        }
    });
}

// ── state ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Row(usize),
    Place(usize),
    Project(usize),
    Crumb(usize),
    Hidden,
    /// The input bar: a click puts the caret in it.
    Bar,
    /// The bar's clear control.
    Clear,
}

struct Crumb {
    label: String,
    path: PathBuf,
}

/// A navigation under way: the old folder stays up until the new one is in.
struct Going {
    select: Option<OsString>,
    dirn: f32,
}

#[derive(Clone, Copy)]
struct Tex {
    key: u64,
    w: u32,
    h: u32,
    src: (u32, u32),
}

struct Cached {
    path: PathBuf,
    size: u64,
    mtime: i64,
    tex: Tex,
}

/// What the preview pane shows now.
struct Pv {
    path: PathBuf,
    name: String,
    label: String,
    kind: Kind,
    is_dir: bool,
    special: Special,
    size: u64,
    mtime: i64,
    tex: Option<Tex>,
    content: Option<Content>,
    link: Option<String>,
    /// Content has arrived (or will never come).
    ready: bool,
    born: f32,
    fade: Spring,
    holder: Spring,
}

struct Pan {
    raw: f32,
    last: Instant,
    vel: f32,
}

pub struct Browser {
    chrome: Chrome,
    text: TextCache,
    icons: Rc<IconSet>,
    scale: f32,
    owner: u64,
    tx: calloop::channel::Sender<Fact>,
    dry: bool,
    home: PathBuf,
    /// Body size.
    bw: f32,
    bh: f32,
    /// Monotonic "now" for relative dates, fixed per window.
    now: i64,

    dir: PathBuf,
    crumbs: Vec<Crumb>,
    entries: Vec<Entry>,
    error: Option<String>,
    more: bool,
    loaded: bool,
    list_gen: u64,
    going: Option<Going>,
    lists: Mailbox<ListReq>,
    completions: Mailbox<CompleteReq>,
    /// Bumped by every edit: a completion for older text is dropped.
    tab_gen: u64,
    show_hidden: bool,
    /// Indices into `entries`, in display order.
    view: Vec<u32>,
    /// Entries that are not hidden.
    plain_count: usize,
    filter: String,
    filtering: bool,
    query: Query,
    scratch: Scratch,
    sel: usize,
    /// Where the selection was, per folder this window visited.
    memo: HashMap<PathBuf, OsString>,

    places: Vec<Place>,
    projects: Vec<Place>,

    scroll: Spring,
    head: Spring,
    tail: Spring,
    hl: Spring,
    rail_y: Spring,
    rail_a: Spring,
    list_t: Spring,
    list_dirn: f32,
    caret: Spring,
    filter_t: Spring,
    pan: Option<Pan>,
    last_input: f32,

    pv: Option<Pv>,
    pv_gen: u64,
    latest: Arc<AtomicU64>,
    previews: Mailbox<preview::Req>,
    pv_due: Option<f32>,
    pv_wait: Option<f32>,
    cache: Vec<Cached>,
    next_key: u64,
    uploads: Vec<(u64, ImageData)>,
    frees: Vec<u64>,
    /// Geist Mono and the wordmark's face: None while they are looked up.
    mono: Option<bool>,
    serif: Option<bool>,

    /// Text the rows need per entry, made when a row is first drawn.
    row_text: HashMap<u32, (String, String)>,
    /// Match spans per entry for the current filter.
    row_spans: HashMap<u32, Vec<Span>>,
    extents: Vec<(f32, f32)>,

    pointer: Option<(f32, f32)>,
    hover_from: Option<(f32, f32)>,
    hover_live: bool,
    hits: Vec<(Rect, Hit)>,
    pressed: Option<Hit>,
    last_click: Option<(usize, Instant)>,
    list_rect: Rect,
    /// Files opened (the headless demo's record).
    opened: Vec<PathBuf>,
    /// Requests dropped for being stale (demo and tests).
    stale: u32,
}

/// The open-window command for a fresh window.
pub fn open(ctx: &mut Ctx) -> Box<dyn WinScene> {
    let req = PENDING.lock().ok().and_then(|mut p| p.take());
    Box::new(Browser::new(ctx, req, false))
}

fn crumbs_of(dir: &Path, home: &Path) -> Vec<Crumb> {
    let mut v = vec![];
    let (first, rest) = match dir.strip_prefix(home) {
        Ok(r) => (Crumb { label: "Home".into(), path: home.to_path_buf() }, r),
        Err(_) => (Crumb { label: "/".into(), path: PathBuf::from("/") }, dir.strip_prefix("/").unwrap_or(dir)),
    };
    let mut p = first.path.clone();
    v.push(first);
    for c in rest.iter() {
        p.push(c);
        v.push(Crumb { label: kind::shown(c), path: p.clone() });
    }
    v
}

impl Browser {
    fn new(ctx: &mut Ctx, req: Option<Request>, dry: bool) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        let bw = BW.min(ctx.out_w - 2.0 * M - 40.0).max(MIN_W);
        let bh = BH.min(ctx.out_h - 2.0 * M - 60.0).max(MIN_H);
        let (sw, sh) = (bw + 2.0 * M, bh + 2.0 * M);
        let homedir = home();
        let body = Rect::new(M, M, bw, bh);
        // Grow out of where it was found (the launcher's row), else out of
        // a pill in the middle of the screen.
        let start = match req.as_ref().and_then(|r| r.from) {
            Some(r) => Rect::new(r.x - ((ctx.out_w - sw) * 0.5).round(), r.y - ((ctx.out_h - sh) * 0.5).round(), r.w, r.h),
            None => Rect::new(sw * 0.5 - 150.0, sh * 0.5 - 26.0, 300.0, 52.0),
        };
        let owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
        let spring = Spring::new;
        let rows = ((bh - HEAD_H - 6.0 - PV_PAD - META_H) / 15.0).floor().max(4.0) as usize;
        let mut s = Self {
            chrome: Chrome::new(start, body, OPEN_W, OPEN_H),
            text,
            icons: ctx.icons.clone(),
            scale: ctx.scale,
            owner,
            tx: ctx.fact_tx.clone(),
            dry,
            bw,
            bh,
            now: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64),
            dir: homedir.clone(),
            crumbs: crumbs_of(&homedir, &homedir),
            home: homedir,
            entries: vec![],
            error: None,
            more: false,
            loaded: false,
            list_gen: 0,
            going: None,
            lists: Mailbox::new("nimbus-files-list", listing::run_list),
            completions: Mailbox::new("nimbus-files-complete", listing::run_complete),
            tab_gen: 0,
            show_hidden: false,
            view: vec![],
            plain_count: 0,
            filter: String::new(),
            filtering: false,
            query: Query::default(),
            scratch: Scratch::default(),
            sel: 0,
            memo: HashMap::new(),
            places: vec![],
            projects: vec![],
            scroll: spring(0.0, SCROLL),
            head: spring(0.0, SEL_HEAD),
            tail: spring(0.0, SEL_TAIL),
            hl: spring(0.0, FADE),
            rail_y: spring(0.0, RAIL),
            rail_a: spring(0.0, FADE),
            list_t: spring(1.0, REVEAL),
            list_dirn: 1.0,
            caret: spring(0.0, CARET),
            filter_t: spring(0.0, FADE),
            pan: None,
            last_input: f32::NAN,
            pv: None,
            pv_gen: 0,
            latest: Arc::new(AtomicU64::new(0)),
            previews: Mailbox::new("nimbus-files-preview", move |r| preview::run_preview(r, rows)),
            pv_due: None,
            pv_wait: None,
            cache: vec![],
            next_key: 0,
            uploads: vec![],
            frees: vec![],
            mono: None,
            serif: None,
            row_text: HashMap::new(),
            row_spans: HashMap::new(),
            extents: vec![],
            pointer: None,
            hover_from: None,
            hover_live: false,
            hits: vec![],
            pressed: None,
            last_click: None,
            list_rect: Rect::default(),
            opened: vec![],
            stale: 0,
        };
        s.start_workers();
        let path = req.map_or_else(|| s.home.clone(), |r| r.path);
        s.go_to(path, None, 1.0, true);
        s
    }

    fn start_workers(&mut self) {
        let (tx, owner, h) = (self.tx.clone(), self.owner, self.home.clone());
        let _ = std::thread::Builder::new().name("nimbus-files-places".into()).spawn(move || {
            let (places, projects) = listing::places(&h);
            let _ = tx.send(Fact::Custom(Arc::new(Places { owner, places: Mutex::new(places), projects: Mutex::new(projects) })));
        });
        // The extra faces are only loaded when a browser first needs them.
        static LOOKED_UP: AtomicBool = AtomicBool::new(false);
        if LOOKED_UP.load(Ordering::Relaxed) {
            self.mono = Some(MONO_OK.load(Ordering::Relaxed));
            self.serif = Some(SERIF_OK.load(Ordering::Relaxed));
            return;
        }
        let (tx, owner) = (self.tx.clone(), self.owner);
        let _ = std::thread::Builder::new().name("nimbus-font".into()).spawn(move || {
            let find = |pattern: &str, family: &str| {
                let out = std::process::Command::new("fc-match").args(["-f", "%{family}\n%{file}", pattern]).output();
                let s = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
                let (fam, file) = s.split_once('\n').unwrap_or(("", ""));
                (fam.contains(family) && !file.is_empty()).then(|| PathBuf::from(file))
            };
            let (mono, serif) = (find(MONO, MONO), find(TITLE_PATTERN, TITLE_FACE));
            MONO_OK.store(mono.is_some(), Ordering::Relaxed);
            SERIF_OK.store(serif.is_some(), Ordering::Relaxed);
            if let Ok(mut f) = FONT_FILES.lock() {
                f.extend(mono.iter().chain(&serif).cloned());
            }
            LOOKED_UP.store(true, Ordering::Relaxed);
            let _ = tx.send(Fact::Custom(Arc::new(FontFound { owner, mono: mono.is_some(), serif: serif.is_some() })));
        });
    }

    // ── model ───────────────────────────────────────────────────────────

    fn go(&mut self, dir: PathBuf, select: Option<OsString>, dirn: f32) {
        let select = select.or_else(|| self.memo.get(&dir).cloned());
        self.go_to(dir, select, dirn, false);
    }

    /// `reveal`: `path` may be a file or gone; the worker works it out.
    fn go_to(&mut self, path: PathBuf, select: Option<OsString>, dirn: f32, reveal: bool) {
        if self.loaded
            && let Some(e) = self.selected()
        {
            let n = e.name.clone();
            self.memo.insert(self.dir.clone(), n);
        }
        self.list_gen += 1;
        self.going = Some(Going { select, dirn });
        self.lists.push(ListReq { owner: self.owner, generation: self.list_gen, path, reveal, home: self.home.clone(), tx: self.tx.clone() });
    }

    fn reload(&mut self) {
        let dir = self.dir.clone();
        let keep = self.selected().map(|e| e.name.clone());
        self.go(dir, keep, 0.0);
    }

    fn up(&mut self) {
        let Some(p) = self.dir.parent().map(Path::to_path_buf) else { return };
        let name = self.dir.file_name().map(OsStr::to_os_string);
        self.go(p, name, -1.0);
    }

    fn selected(&self) -> Option<&Entry> {
        self.entries.get(*self.view.get(self.sel)? as usize)
    }

    fn take_listing(&mut self, l: &Listing) {
        let Some(going) = self.going.take() else { return };
        self.entries = l.entries.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
        self.dir = l.dir.clone();
        self.crumbs = crumbs_of(&self.dir, &self.home);
        self.error = l.error.clone();
        self.more = l.more;
        self.loaded = true;
        self.plain_count = self.entries.iter().filter(|e| !e.hidden).count();
        self.filter.clear();
        self.filtering = false;
        self.query = Query::default();
        self.filter_t.set(0.0);
        self.row_text.clear();
        let select = going.select.or_else(|| l.select.clone());
        // Asked to reveal something hidden: show hidden files then.
        if let Some(n) = &select
            && self.entries.iter().any(|e| e.name == *n && e.hidden)
        {
            self.show_hidden = true;
        }
        self.rebuild(select);
        self.scroll.snap(0.0);
        self.pan = None;
        let vh = self.list_h();
        let y = self.sel as f32 * ROW_H;
        if y + ROW_H > vh {
            self.scroll.snap((y + ROW_H * 1.5 - vh * 0.5).clamp(0.0, self.max_scroll()));
        }
        self.head.snap(y);
        self.tail.snap(y);
        self.hl.snap(0.0);
        self.hl.set(1.0);
        if going.dirn != 0.0 {
            self.list_dirn = going.dirn;
            self.list_t.snap(0.0);
            self.list_t.set(1.0);
        }
        let rail = self.active_rail();
        match rail {
            Some(y) if self.rail_a.target > 0.5 => self.rail_y.set(y),
            Some(y) => {
                self.rail_y.snap(y);
                self.rail_a.set(1.0);
            }
            None => self.rail_a.set(0.0),
        }
        self.want_preview();
    }

    /// Rebuild `view`; the selection follows `keep` (else the top).
    fn rebuild(&mut self, keep: Option<OsString>) {
        self.row_spans.clear();
        let hidden = self.show_hidden;
        let shown = |e: &&Entry| hidden || !e.hidden;
        self.view.clear();
        if self.query.is_empty() {
            self.view.extend(self.entries.iter().enumerate().filter(|(_, e)| shown(e)).map(|(i, _)| i as u32));
        } else {
            let want = self.query.mask();
            let sc = &mut self.scratch;
            let q = &self.query;
            let mut scored: Vec<(i32, u32)> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| shown(e) && e.mask & want == want)
                .filter_map(|(i, e)| Some((fuzzy::score_name(q, e.name.as_bytes(), sc)?, i as u32)))
                .collect();
            scored.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.view.extend(scored.into_iter().map(|e| e.1));
        }
        let at = keep.and_then(|n| self.view.iter().position(|&i| self.entries[i as usize].name == n));
        self.sel = at.unwrap_or(0).min(self.view.len().saturating_sub(1));
    }

    /// The bar holds a path being typed, not a filter.
    fn path_mode(&self) -> bool {
        listing::is_path(&self.filter)
    }

    fn filter_on(&self) -> bool {
        self.filtering || !self.filter.is_empty()
    }

    /// Put the caret in the bar.
    fn focus_bar(&mut self) {
        self.filtering = true;
        self.filter_t.set(1.0);
        self.last_input = self.chrome.time;
    }

    fn clear_filter(&mut self) {
        self.filter.clear();
        self.filtering = false;
        self.filter_t.set(0.0);
        self.refilter();
    }

    fn refilter(&mut self) {
        self.tab_gen += 1;
        self.last_input = self.chrome.time;
        let path = self.path_mode();
        let was_filtered = !self.query.is_empty();
        self.query = if path { Query::default() } else { Query::new(&self.filter) };
        // A path being typed leaves the folder alone.
        if path && !was_filtered {
            return;
        }
        let keep = self.selected().map(|e| e.name.clone()).filter(|_| self.query.is_empty());
        self.rebuild(keep);
        self.scroll.snap(0.0);
        let y = self.sel as f32 * ROW_H;
        self.head.snap(y);
        self.tail.snap(y);
        self.reveal(false);
        self.want_preview();
    }

    fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        let keep = self.selected().map(|e| e.name.clone());
        self.rebuild(keep);
        let y = self.sel as f32 * ROW_H;
        self.head.snap(y);
        self.tail.snap(y);
        self.scroll.set(self.scroll.target.min(self.max_scroll()));
        self.reveal(false);
        self.want_preview();
    }

    fn list_h(&self) -> f32 {
        self.bh - HEAD_H - FOOT_H
    }

    fn max_scroll(&self) -> f32 {
        (self.view.len() as f32 * ROW_H + 8.0 - self.list_h()).max(0.0)
    }

    fn select(&mut self, i: usize) {
        let n = self.view.len();
        if n == 0 {
            return;
        }
        let i = i.min(n - 1);
        if i == self.sel {
            return;
        }
        self.sel = i;
        let y = i as f32 * ROW_H;
        // A long jump is cut short: flying over thousands of rows is noise,
        // and every row flown over would have to be shaped.
        let far = self.list_h() * 2.0;
        if (self.head.get() - y).abs() > far {
            let from = y + far.copysign(self.head.get() - y);
            self.head.snap(from);
            self.tail.snap(from);
        }
        self.head.set(y);
        self.tail.set(y);
        self.reveal(true);
        self.want_preview();
    }

    fn reveal(&mut self, animate: bool) {
        let (y, vh) = (self.sel as f32 * ROW_H, self.list_h());
        let t = self.scroll.target;
        let to = if y < t {
            y
        } else if y + ROW_H + 8.0 > t + vh {
            y + ROW_H + 8.0 - vh
        } else {
            return;
        };
        let to = to.clamp(0.0, self.max_scroll());
        if !animate {
            self.scroll.snap(to);
            return;
        }
        let far = vh * 2.0;
        if (self.scroll.get() - to).abs() > far {
            self.scroll.snap(to + far.copysign(self.scroll.get() - to));
        }
        self.scroll.set(to);
    }

    fn open_selected(&mut self) {
        let Some(e) = self.selected() else { return };
        if !e.openable() {
            return;
        }
        let path = self.dir.join(&e.name);
        if e.is_dir {
            self.go(path, None, 1.0);
        } else {
            self.launch(path);
        }
    }

    /// Open a file with the default app and step out of its way.
    fn launch(&mut self, path: PathBuf) {
        if !self.dry {
            open_default(&path);
        }
        self.opened.push(path);
        WinScene::close(self);
    }

    fn active_rail(&self) -> Option<f32> {
        let mut best: Option<(usize, f32)> = None;
        let mut consider = |p: &Place, y: f32| {
            let n = p.path.components().count();
            if self.dir.starts_with(&p.path) && best.is_none_or(|b| n > b.0) {
                best = Some((n, y));
            }
        };
        for (i, p) in self.places.iter().enumerate() {
            consider(p, Self::place_y(i));
        }
        for (i, p) in self.projects.iter().enumerate() {
            consider(p, Self::project_y(self.places.len(), i));
        }
        best.map(|b| b.1)
    }

    fn place_y(i: usize) -> f32 {
        RAIL_TOP + i as f32 * RAIL_ROW
    }

    /// Below `places` place rows and the section label.
    fn project_y(places: usize, i: usize) -> f32 {
        RAIL_TOP + (places + i) as f32 * RAIL_ROW + 40.0
    }

    // ── preview ─────────────────────────────────────────────────────────

    fn pv_box(&self) -> (f32, f32) {
        ((self.bw * PREVIEW_K).round() - 2.0 * PV_PAD, self.bh - HEAD_H - 6.0 - PV_PAD - META_H)
    }

    fn free_tex(&mut self, key: u64) {
        self.uploads.retain(|(k, _)| *k != key);
        self.frees.push(key);
    }

    /// The selection changed: the pane forgets the old file at once, so a
    /// fast scroll never shows something stale next to the new name.
    fn want_preview(&mut self) {
        let (path, name, label, kd, is_dir, special, size, mtime) = match self.selected() {
            Some(e) => {
                let label = if e.special == Special::Device { "Special file".to_string() } else { kind::label(e.kind, &e.name) };
                (self.dir.join(&e.name), e.shown.to_string(), label, e.kind, e.is_dir, e.special, e.size, e.mtime)
            }
            None => {
                let name = self.crumbs.last().map_or(String::new(), |c| c.label.clone());
                (self.dir.clone(), name, "Folder".to_string(), Kind::Folder, true, Special::No, 0, 0)
            }
        };
        if self.pv.as_ref().is_some_and(|p| p.path == path && p.size == size && p.mtime == mtime) {
            return;
        }
        self.pv_gen += 1;
        self.latest.store(self.pv_gen, Ordering::Relaxed);
        let t = self.chrome.time;
        let mut pv = Pv {
            path: path.clone(),
            name,
            label,
            kind: kd,
            is_dir,
            special,
            size,
            mtime,
            tex: None,
            content: None,
            link: None,
            ready: false,
            born: t,
            fade: Spring::new(0.0, FADE),
            holder: Spring::new(0.0, FADE),
        };
        if let Some(at) = self.cache.iter().position(|c| c.path == path && c.size == size && c.mtime == mtime) {
            // Most recent last; flipping back is instant.
            let c = self.cache.remove(at);
            pv.tex = Some(c.tex);
            pv.ready = true;
            pv.fade.snap(1.0);
            self.cache.push(c);
            self.pv = Some(pv);
            self.pv_due = None;
            self.pv_wait = None;
            return;
        }
        if matches!(special, Special::Device | Special::BrokenLink) {
            pv.ready = true;
            pv.holder.snap(1.0);
            self.pv = Some(pv);
            self.pv_due = None;
            self.pv_wait = None;
            return;
        }
        self.pv = Some(pv);
        let w = *self.pv_wait.get_or_insert(t);
        self.pv_due = Some((t + SETTLE).min(w + SETTLE_MAX));
    }

    fn send_preview(&mut self) {
        self.pv_due = None;
        self.pv_wait = None;
        let Some(pv) = &self.pv else { return };
        let (w, h) = self.pv_box();
        self.previews.push(preview::Req {
            owner: self.owner,
            generation: self.pv_gen,
            latest: self.latest.clone(),
            path: pv.path.clone(),
            max: ((w * self.scale).round() as u32, (h * self.scale).round() as u32),
            tx: self.tx.clone(),
        });
    }

    fn take_preview(&mut self, s: &Shown) {
        let Some(content) = s.content.lock().ok().and_then(|mut g| g.take()) else { return };
        let Some(pv) = &mut self.pv else { return };
        pv.ready = true;
        pv.link = s.link.clone();
        match content {
            Content::Image { img, src } => {
                self.next_key += 1;
                let key = 0x6669_6c65_0000_0000 | ((self.owner & 0xffff) << 16) | (self.next_key & 0xffff);
                let tex = Tex { key, w: img.w, h: img.h, src };
                self.uploads.push((key, img));
                pv.tex = Some(tex);
                pv.fade.set(1.0);
                let (path, size, mtime) = (pv.path.clone(), pv.size, pv.mtime);
                self.cache.push(Cached { path, size, mtime, tex });
                if self.cache.len() > IMAGE_CACHE {
                    let old = self.cache.remove(0);
                    self.free_tex(old.tex.key);
                }
            }
            Content::Nothing { note } => {
                pv.content = Some(Content::Nothing { note });
                pv.holder.set(1.0);
            }
            // An empty folder has nothing to list: its icon stands in.
            Content::Folder { items, more, folders, sample } if sample.is_empty() => {
                pv.content = Some(Content::Folder { items, more, folders, sample });
                pv.holder.set(1.0);
            }
            c => {
                pv.content = Some(c);
                pv.fade.set(1.0);
            }
        }
    }

    fn free_textures(&mut self) {
        for c in std::mem::take(&mut self.cache) {
            self.frees.push(c.tex.key);
            self.uploads.retain(|(k, _)| *k != c.tex.key);
        }
        if let Some(pv) = &mut self.pv {
            pv.tex = None;
        }
    }

    // ── drawing helpers ─────────────────────────────────────────────────

    fn px(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [(r.x * s).floor() as i32, (r.y * s).floor() as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32]
    }

    fn text_at(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect) {
        if c.3 <= 0.003 || clip.w <= 0.5 || clip.h <= 0.5 {
            return;
        }
        let s = self.scale;
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: self.px(clip) });
    }

    /// Single-line text centred on `cy`; returns its width.
    #[allow(clippy::too_many_arguments)]
    fn put(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text_at(out, k, x, cy - lh * 0.5, c, clip);
        w
    }

    #[allow(clippy::too_many_arguments)]
    fn put_fit(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, max: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get_fit(fs, s, f, max);
        let lh = self.text.line_h(f);
        self.text_at(out, k, x, cy - lh * 0.5, c, clip);
        w
    }

    fn put_right(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, right: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text_at(out, k, right - w, cy - lh * 0.5, c, clip);
        w
    }

    fn icon(&self, out: &mut DrawList, id: u16, x: f32, y: f32, size: f32, c: Rgba) {
        if c.3 <= 0.003 {
            return;
        }
        let s = self.scale;
        out.icons.push(IconDraw { id, x: (x * s).round(), y: (y * s).round(), size: size * s, color: c });
    }

    fn keycap(&mut self, fs: &mut FontSystem, out: &mut DrawList, cap: &str, label: &str, right: f32, cy: f32, a: f32, lit: bool, clip: Rect) -> f32 {
        let (lk, lw) = self.text.get(fs, label, F_SMALL);
        let (ck, cw) = self.text.get(fs, cap, F_KEY);
        let x = right - lw;
        let llh = self.text.line_h(F_SMALL);
        self.text_at(out, lk, x, cy - llh * 0.5, (if lit { TEXT } else { META }).fade(a), clip);
        let kw = cw + 12.0;
        let kr = Rect::new(x - 7.0 - kw, cy - 10.0, kw, 20.0);
        if let Some(kc) = intersect(kr, clip) {
            out.shapes.push(paint::rect(self.scale, kc, 6.0).fill(Rgba::hex(0xffffff, if lit { 0.13 } else { 0.07 })).alpha(a).done());
        }
        let clh = self.text.line_h(F_KEY);
        self.text_at(out, ck, kr.x + 6.0, cy - clh * 0.5 - 0.5, SOFT.fade(a), clip);
        kr.x
    }

    /// A hairline along the vertical, fading in from both ends.
    fn vline(&self, out: &mut DrawList, x: f32, y: f32, h: f32, a: f32) {
        if a <= 0.002 {
            return;
        }
        let c = Rgba::hex(0xffffff, 0.09);
        let m = h * 0.5;
        out.shapes.push(paint::rect(self.scale, Rect::new(x, y, 1.0, m), 0.0).gradient(c.fade(0.0), c, 0.0, 1.0).alpha(a).done());
        out.shapes.push(paint::rect(self.scale, Rect::new(x, y + m, 1.0, h - m), 0.0).gradient(c, c.fade(0.0), 0.0, 1.0).alpha(a).done());
    }

    /// `name`, with its matched characters overdrawn in the highlight.
    #[allow(clippy::too_many_arguments)]
    fn marked(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, spans: &[Span], f: Font, x: f32, cy: f32, max: f32, base: Rgba, hi: Rgba, clip: Rect) {
        let (k, _) = self.text.get_fit(fs, s, f, max);
        let lh = self.text.line_h(f);
        let y = cy - lh * 0.5;
        self.text_at(out, k, x, y, base, clip);
        if spans.is_empty() {
            return;
        }
        let mut ext = std::mem::take(&mut self.extents);
        ext.clear();
        self.text.extents(k, spans, &mut ext);
        for &(a, b) in &ext {
            if let Some(c) = intersect(Rect::new(x + a - 0.5, clip.y, b - a + 1.0, clip.h), clip) {
                self.text_at(out, k, x, y, hi, c);
            }
        }
        self.extents = ext;
    }

    // ── sections ────────────────────────────────────────────────────────

    fn draw_rail(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, clip: Rect, a: f32) {
        let sc = self.scale;
        let ra = self.rail_a.get().clamp(0.0, 1.0) * a;
        if ra > 0.003 {
            let r = Rect::new(b.x + 10.0, b.y + self.rail_y.get() + 1.0, RAIL_W - 20.0, RAIL_ROW - 2.0);
            if let Some(cr) = intersect(r, clip) {
                out.shapes.push(paint::rect(sc, cr, 11.0).fill(SEL).prism(paint::HAIR, 1.5).alpha(ra).done());
            }
        }
        let places = std::mem::take(&mut self.places);
        let projects = std::mem::take(&mut self.projects);
        let active = self.rail_y.target;
        let on = self.rail_a.target > 0.5;
        for (group, list) in [(0, &places), (1, &projects)] {
            for (i, p) in list.iter().enumerate() {
                let y = if group == 0 { Self::place_y(i) } else { Self::project_y(places.len(), i) };
                let hit = if group == 0 { Hit::Place(i) } else { Hit::Project(i) };
                let row = Rect::new(b.x + 10.0, b.y + y, RAIL_W - 20.0, RAIL_ROW);
                if let Some(cr) = intersect(row, clip) {
                    self.hits.push((cr, hit));
                }
                let lit = on && (active - y).abs() < 1.0;
                let hovered = !lit && self.hover_hit() == Some(hit);
                if hovered {
                    out.shapes.push(paint::rect(sc, row.inflate(-1.0), 11.0).fill(HOVER).alpha(a).done());
                }
                let c = if lit { TEXT } else { SOFT };
                let id = self.icons.id(&[p.icon]);
                self.icon(out, id, row.x + 12.0, row.y + (RAIL_ROW - 16.0) * 0.5, 16.0, (if lit { SOFT } else { MUTED }).fade(a));
                self.put_fit(fs, out, &p.name, F_ROW, row.x + 38.0, row.y + RAIL_ROW * 0.5, RAIL_W - 62.0, c.fade(a), clip);
            }
        }
        if !projects.is_empty() {
            let y = b.y + Self::project_y(places.len(), 0) - 14.0;
            self.put(fs, out, "PROJECTS", F_SECTION, b.x + 22.0, y, META.fade(a * 0.85), clip);
        }
        self.places = places;
        self.projects = projects;
    }

    /// "Explorer", top left: one shaped run, cut into pixel-aligned columns
    /// that each carry their colour of the ramp. The kerning stays the
    /// font's, and it is static: nothing here costs anything at rest.
    fn draw_title(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32) {
        let Some(serif) = self.serif else { return };
        let f = if serif { F_TITLE } else { F_TITLE_PLAIN };
        let (k, w) = self.text.get(fs, "Explorer", f);
        let lh = self.text.line_h(f);
        let (x, y) = (b.x + 22.0, b.y + BAR_Y + (BAR_H - lh) * 0.5 - 1.0);
        let s = self.scale;
        let (px, py) = ((x * s).round(), (y * s).round());
        let (left, width) = (px as i32, (w * s).ceil() as i32);
        let (top, bot) = (py as i32 - 4, py as i32 + (lh * s).ceil() as i32 + 8);
        for i in 0..TITLE_SLICES {
            // An italic leans out of its box: the end columns reach past it.
            let x0 = if i == 0 { left - 12 } else { left + width * i / TITLE_SLICES };
            let x1 = if i + 1 == TITLE_SLICES { left + width + 12 } else { left + width * (i + 1) / TITLE_SLICES };
            let t = (i as f32 + 0.5) / TITLE_SLICES as f32;
            out.texts.push(TextDraw { key: k, x: px, y: py, color: paint::irid_at(t).fade(a), clip: [x0, top, x1, bot] });
        }
    }

    /// The launcher's magnifier.
    fn glass_glyph(&self, out: &mut DrawList, cx: f32, cy: f32, c: Rgba) {
        let sc = self.scale;
        let (ox, oy) = (cx - 1.5, cy - 1.5);
        out.shapes.push(paint::circle(sc, ox, oy, 6.5).fill(Rgba::TRANSPARENT).rim(1.9, c, c).done());
        for i in 0..6 {
            let t = 4.6 + i as f32 * 0.85;
            out.shapes.push(paint::circle(sc, ox + t, oy + t, 1.05).fill(c).done());
        }
    }

    /// One field for where you are and what you are looking for: the search
    /// glyph, the path as a quiet breadcrumb, then the caret, and the count
    /// at the far end. Like the launcher's field.
    fn draw_bar(&mut self, fs: &mut FontSystem, out: &mut DrawList, r: Rect, clip: Rect, a: f32) {
        let sc = self.scale;
        if let Some(f) = intersect(r, clip) {
            let lit = self.filter_t.get().clamp(0.0, 1.0);
            out.shapes.push(paint::rect(sc, f, BAR_R).fill(FIELD.fade(1.0 + 0.5 * lit)).alpha(a).done());
            self.hits.push((f, Hit::Bar));
        }
        let cy = r.y + r.h * 0.5;
        let lit = self.filter_t.get().clamp(0.0, 1.0);
        // The glyph is always there; it lights while the bar is in use.
        let on_c = if theme::prism() { paint::irid_at(0.32) } else { theme::accent().0 };
        self.glass_glyph(out, r.x + 23.0, cy, SOFT.mix(on_c, lit).fade(a));
        let text_x = r.x + 44.0;
        let mut right = r.x + r.w - 16.0;
        if lit > 0.003 {
            // Clear, at the far end.
            let (cx, id) = (right - 7.0, self.icons.id(&["window-close-symbolic"]));
            if self.hover_hit() == Some(Hit::Clear) {
                out.shapes.push(paint::circle(sc, cx, cy, 12.0).fill(HOVER).alpha(a * lit).done());
            }
            self.icon(out, id, cx - 7.0, cy - 7.0, 14.0, META.fade(a * lit));
            if self.filter_on() {
                self.hits.push((Rect::new(cx - 15.0, r.y, 34.0, r.h), Hit::Clear));
            }
            right -= 30.0 * smooth(lit);
        }
        // The count.
        let n = self.view.len();
        let total = if self.show_hidden { self.entries.len() } else { self.plain_count };
        let count = if !self.loaded || (self.going.is_some() && self.entries.is_empty()) {
            String::new()
        } else if !self.query.is_empty() {
            format!("{n} of {total}")
        } else if self.more {
            format!("{}+ items", thousands(n))
        } else if n == 1 {
            "1 item".to_string()
        } else {
            format!("{} items", thousands(n))
        };
        let cw = self.put_right(fs, out, &count, F_SMALL, right, cy, META.fade(a), clip);
        let limit = right - cw - 14.0;
        let path_mode = self.path_mode();
        let on = self.filter_on();
        let mut x = text_x;
        if !path_mode {
            // Breadcrumb, first segments elided when it does not fit.
            let room = (limit - text_x - if on { 150.0 } else { 0.0 }).max(80.0);
            let (_, sep_w) = self.text.get(fs, "/", F_CRUMB);
            let sep = sep_w + 14.0;
            let n_c = self.crumbs.len();
            let widths: Vec<f32> = (0..n_c)
                .map(|i| {
                    let l = self.crumbs[i].label.clone();
                    self.text.get(fs, &l, F_CRUMB).1
                })
                .collect();
            let (_, dots) = self.text.get(fs, "…", F_CRUMB);
            let width_from = |f: usize| -> f32 { widths[f..].iter().sum::<f32>() + sep * (n_c - f - 1) as f32 + if f > 0 { dots + sep } else { 0.0 } };
            let mut first = 0;
            while first + 1 < n_c && width_from(first) > room {
                first += 1;
            }
            if first > 0 {
                x += self.put(fs, out, "…", F_CRUMB, x, cy, META.fade(a), clip);
                x += 7.0;
                x += self.put(fs, out, "/", F_CRUMB, x, cy, DIM.fade(a), clip);
                x += 7.0;
            }
            for i in first..n_c {
                let last = i + 1 == n_c;
                let label = self.crumbs[i].label.clone();
                let hit = Hit::Crumb(i);
                let hov = !last && self.hover_hit() == Some(hit);
                let c = if last { TEXT } else if hov { TEXT } else { META };
                let w = self.put_fit(fs, out, &label, F_CRUMB, x, cy, room.max(40.0), c.fade(a), clip);
                if !last {
                    self.hits.push((Rect::new(x - 4.0, r.y, w + 8.0, r.h), hit));
                }
                x += w;
                if !last {
                    x += 7.0;
                    x += self.put(fs, out, "/", F_CRUMB, x, cy, DIM.fade(a), clip);
                    x += 7.0;
                }
            }
            x += 12.0;
        }
        // What you type goes right after the path, in the same field.
        if lit <= 0.003 {
            return;
        }
        let fa = lit * a;
        let field = Rect::new(x - 2.0, r.y, (limit - x + 2.0).max(20.0), r.h).clip_to(clip);
        let (k, w) = self.text.get(fs, &self.filter, F_QUERY);
        let lh = self.text.line_h(F_QUERY);
        // A long path keeps its end, where the caret is, in view.
        let shift = (w + 4.0 - field.w).max(0.0);
        if !self.filter.is_empty() {
            self.text_at(out, k, x - shift, cy - lh * 0.5, TEXT.fade(fa), field);
        } else if self.filtering {
            self.put(fs, out, "Filter this folder", F_QUERY, x + 4.0, cy, META.fade(fa * 0.8), field);
        }
        let target = if self.filter.is_empty() { 0.0 } else { w + 1.5 };
        if self.caret.target != target {
            self.caret.set(target);
        }
        let idle = self.idle();
        let blink_on = idle < 0.6 || idle >= BLINK_FOR || (((idle - 0.6) / 0.5).floor() as i64) % 2 == 1;
        if blink_on && on && !self.chrome.closing {
            let cx = x + self.caret.get() - shift;
            out.shapes.push(paint::rect(sc, Rect::new(cx, cy - 9.5, 2.0, 19.0), 1.0).on_fill(0.0, 1.0, 0.0).alpha(fa).done());
        }
    }

    fn idle(&self) -> f32 {
        if self.last_input.is_nan() { 0.0 } else { self.chrome.time - self.last_input }
    }

    fn draw_rows(&mut self, fs: &mut FontSystem, out: &mut DrawList, vp: Rect, clip: Rect, a: f32) {
        let sc = self.scale;
        let scroll = self.scroll.get();
        let t = self.list_t.get();
        let dx = (1.0 - t) * 22.0 * self.list_dirn;
        let la = a * smooth(t * 1.3);
        // A row fades only as it crosses an edge: one sitting exactly on the
        // edge is fully there.
        let edge = |top: f32, bot: f32| (1.0 - smooth((vp.y - top) / EDGE)) * (1.0 - smooth((bot - vp.y - vp.h) / EDGE));
        // Selection, as a blob between a snappy head and a trailing tail.
        let hl = self.hl.get().clamp(0.0, 1.0) * la;
        if hl > 0.003 && !self.view.is_empty() {
            let (hy, ty) = (self.head.get(), self.tail.get());
            let (lo, hi) = (hy.min(ty), hy.max(ty));
            let blob = Rect::new(vp.x + dx, vp.y + lo - scroll + 1.0, vp.w, hi - lo + ROW_H - 2.0);
            let over = (vp.y - blob.y).max(blob.y + blob.h - (vp.y + vp.h)).max(0.0);
            let ba = hl * (1.0 - smooth(over / 22.0));
            if ba > 0.003
                && let Some(b) = intersect(blob, vp)
            {
                out.shapes.push(paint::rect(sc, b, 11.0).fill(SEL).prism(paint::HAIR, 1.5).alpha(ba).done());
            }
        }
        // Hover: a faint wash, only once the pointer has really moved.
        if let Some(Hit::Row(i)) = self.hover_hit()
            && i != self.sel
        {
            let y = vp.y + i as f32 * ROW_H - scroll + 1.0;
            if let Some(b) = intersect(Rect::new(vp.x, y, vp.w, ROW_H - 2.0), vp) {
                out.shapes.push(paint::rect(sc, b, 11.0).fill(HOVER).alpha(la).done());
            }
        }
        // Rows: only what is on screen, nothing else is touched.
        let first = (scroll / ROW_H).floor().max(0.0) as usize;
        let last = (((scroll + vp.h) / ROW_H).ceil() as usize + 1).min(self.view.len());
        let name_x = vp.x + 14.0 + ICON + 12.0;
        let size_r = vp.x + vp.w - 14.0;
        let when_r = size_r - SIZE_W;
        let name_w = when_r - WHEN_W - name_x;
        let accent = theme::accent().0;
        let hi = TEXT.mix(accent, 0.55);
        for i in first..last {
            let e_i = self.view[i];
            let y = (vp.y + i as f32 * ROW_H - scroll).round();
            let ra = la * edge(y, y + ROW_H);
            self.hits.push((Rect::new(vp.x, y, vp.w, ROW_H).clip_to(vp), Hit::Row(i)));
            if ra <= 0.003 {
                continue;
            }
            let cy = y + ROW_H * 0.5;
            let e = &self.entries[e_i as usize];
            let (shown, is_dir, hidden, ek, mtime, size, sp) = (e.shown.to_string(), e.is_dir, e.hidden, e.kind, e.mtime, e.size, e.special);
            let is_sel = i == self.sel;
            let dim = if hidden { 0.62 } else { 1.0 };
            let col = if is_sel { TEXT } else if sp == Special::BrokenLink { MUTED } else { SOFT };
            let id = kind::icon(ek, &self.icons);
            self.icon(out, id, vp.x + 14.0 + dx, y + (ROW_H - ICON) * 0.5, ICON, (if is_dir { SOFT } else { MUTED }).fade(ra * dim));
            let spans = self.spans_of(e_i);
            let c = Rect::new(vp.x, y, vp.w, ROW_H).clip_to(clip);
            self.marked(fs, out, &shown, &spans, F_ROW, name_x + dx, cy, name_w, col.fade(ra * dim), hi.fade(ra * dim), c);
            let now = self.now;
            let (when, sz) = self.row_text.entry(e_i).or_insert_with(|| (kind::human_time(mtime, now), if is_dir || sp == Special::Device { String::new() } else { kind::human_size(size) })).clone();
            self.put_right(fs, out, &when, F_SMALL, when_r + dx, cy, META.fade(ra), c);
            self.put_right(fs, out, &sz, F_SMALL, size_r + dx, cy, META.fade(ra), c);
        }
        if self.row_text.len() > 4096 {
            self.row_text.clear();
        }
    }

    fn spans_of(&mut self, e: u32) -> Vec<Span> {
        if self.query.is_empty() {
            return vec![];
        }
        if let Some(s) = self.row_spans.get(&e) {
            return s.clone();
        }
        let ent = &self.entries[e as usize];
        let raw = ent.name.as_bytes();
        let s = if ent.shown.len() == raw.len() { fuzzy::name_spans(&self.query, raw, &mut self.scratch) } else { vec![] };
        self.row_spans.insert(e, s.clone());
        s
    }

    fn draw_empty(&mut self, fs: &mut FontSystem, out: &mut DrawList, vp: Rect, clip: Rect, a: f32) {
        if !self.view.is_empty() || !self.loaded {
            return;
        }
        let (title, sub): (String, &str) = if let Some(e) = &self.error {
            (e.clone(), "Backspace goes up")
        } else if !self.query.is_empty() {
            (format!("Nothing matches \u{201c}{}\u{201d}", kind::shown(OsStr::new(&self.filter))), "Esc clears the filter")
        } else if !self.entries.is_empty() {
            ("Only hidden files here".to_string(), "Press . to show them")
        } else {
            ("This folder is empty".to_string(), "")
        };
        let cy = vp.y + vp.h * 0.42;
        let id = self.icons.id(&["folder-symbolic"]);
        self.icon(out, id, vp.x + vp.w * 0.5 - 22.0, cy - 66.0, 44.0, DIM.fade(a));
        let (k, w) = self.text.get_fit(fs, &title, F_PNAME, vp.w - 60.0);
        let lh = self.text.line_h(F_PNAME);
        self.text_at(out, k, vp.x + (vp.w - w) * 0.5, cy - lh * 0.5, SOFT.fade(a), clip);
        if !sub.is_empty() {
            let (k, w) = self.text.get(fs, sub, F_SMALL);
            let lh = self.text.line_h(F_SMALL);
            self.text_at(out, k, vp.x + (vp.w - w) * 0.5, cy + 24.0 - lh * 0.5, MUTED.fade(a), clip);
        }
    }

    fn draw_foot(&mut self, fs: &mut FontSystem, out: &mut DrawList, r: Rect, clip: Rect, a: f32) {
        let cy = r.y + r.h * 0.5;
        let right = r.x + r.w;
        let lit = self.show_hidden;
        let k1 = self.keycap(fs, out, ".", "Hidden", right, cy, a, lit, clip);
        self.hits.push((Rect::new(k1 - 4.0, r.y, right - k1 + 8.0, r.h), Hit::Hidden));
        let k2 = self.keycap(fs, out, "Backspace", "Up", k1 - 16.0, cy, a, false, clip);
        self.keycap(fs, out, "Enter", "Open", k2 - 16.0, cy, a, false, clip);
    }

    fn draw_preview(&mut self, fs: &mut FontSystem, out: &mut DrawList, x: f32, y: f32, clip: Rect, a: f32) {
        let sc = self.scale;
        let (bw, bh) = self.pv_box();
        let Some(pv) = self.pv.take() else { return };
        let fade = pv.fade.get().clamp(0.0, 1.0);
        let holder = pv.holder.get().clamp(0.0, 1.0);
        // The type icon: while a preview is slow, or instead of one.
        let ia = holder * (1.0 - fade) * a;
        if ia > 0.003 {
            let sz = 88.0;
            let id = kind::icon(pv.kind, &self.icons);
            self.icon(out, id, x + (bw - sz) * 0.5, y + (bh - sz) * 0.46, sz, MUTED.fade(ia * 0.9));
        }
        let ca = fade * a;
        if let Some(t) = pv.tex {
            // Native size, on whole device pixels: crisp.
            let (w, h) = (t.w as f32 / sc, t.h as f32 / sc);
            let (ix, iy) = (((x + (bw - w) * 0.5) * sc).round() / sc, ((y + (bh - h) * 0.46) * sc).round() / sc);
            let r = Rect::new(ix, iy, w, h);
            if let Some(c) = intersect(r, clip)
                && ca > 0.003
            {
                let mut d = ImageDraw::new(t.key, [c.x * sc, c.y * sc, c.w * sc, c.h * sc]);
                d.uv = [(c.x - r.x) / r.w, (c.y - r.y) / r.h, (c.x + c.w - r.x) / r.w, (c.y + c.h - r.y) / r.h];
                d.alpha = ca;
                d.radius = 14.0 * sc;
                out.images.push(d);
            }
        }
        match &pv.content {
            Some(Content::Text { layers, code, .. }) => {
                // Not before the font lookup: a miss would stick to the
                // shaped text.
                if let Some(mono) = self.mono {
                    let box_r = Rect::new(x, y, bw, bh).clip_to(clip);
                    if mono {
                        let parts: [(&str, Rgba); 4] = [(&layers.plain, CODE), (&layers.keyword, KEYWORD), (&layers.string, STRING), (&layers.comment, COMMENT)];
                        for (i, (s, c)) in parts.into_iter().enumerate() {
                            if i > 0 && (!*code || s.trim().is_empty()) {
                                continue;
                            }
                            let (k, _) = self.text.get(fs, s, F_MONO);
                            self.text_at(out, k, x, y, c.fade(ca), box_r);
                        }
                    } else {
                        let (k, _) = self.text.get(fs, &layers.all, F_PLAINTEXT);
                        self.text_at(out, k, x, y, CODE.fade(ca), box_r);
                    }
                }
            }
            Some(Content::Folder { sample, .. }) => {
                let box_r = Rect::new(x, y, bw, bh).clip_to(clip);
                for (i, (name, k)) in sample.iter().enumerate() {
                    let cy = y + 15.0 + i as f32 * 30.0;
                    if cy + 15.0 > y + bh {
                        break;
                    }
                    let id = kind::icon(*k, &self.icons);
                    self.icon(out, id, x, cy - 8.0, 16.0, MUTED.fade(ca));
                    self.put_fit(fs, out, name, F_ROW, x + 28.0, cy, bw - 30.0, SOFT.fade(ca), box_r);
                }
            }
            _ => {}
        }
        // Metadata, anchored so it never moves as you browse.
        let my = y + bh + 14.0;
        self.put_fit(fs, out, &pv.name, F_PNAME, x, my + 10.0, bw, TEXT.fade(a), clip);
        let mut line2 = pv.label.clone();
        if pv.is_dir {
            if let Some(Content::Folder { items, more, folders, .. }) = &pv.content {
                line2 = format!(
                    "Folder \u{b7} {}{} item{}{}",
                    thousands(*items),
                    if *more { "+" } else { "" },
                    if *items == 1 { "" } else { "s" },
                    if *folders > 0 { format!(", {} folder{}", thousands(*folders), if *folders == 1 { "" } else { "s" }) } else { String::new() },
                );
            }
        } else {
            if let Some(t) = pv.tex {
                line2.push_str(&format!(" \u{b7} {} \u{d7} {}", t.src.0, t.src.1));
            }
            if pv.special != Special::Device {
                line2.push_str(&format!(" \u{b7} {}", kind::human_size(pv.size)));
            }
        }
        self.put_fit(fs, out, &line2, F_SMALL, x, my + 32.0, bw, META.fade(a), clip);
        let line3 = match (&pv.link, pv.special) {
            (_, Special::BrokenLink) => "Broken link".to_string(),
            (Some(l), _) => format!("Links to {l}"),
            _ => match &pv.content {
                Some(Content::Nothing { note: Some(n) }) => (*n).to_string(),
                _ => match kind::human_time(pv.mtime, self.now) {
                    t if t.is_empty() => String::new(),
                    t => format!("Modified {t}"),
                },
            },
        };
        self.put_fit(fs, out, &line3, F_SMALL, x, my + 50.0, bw, META.fade(a * 0.9), clip);
        self.pv = Some(pv);
    }

    fn hover_hit(&self) -> Option<Hit> {
        if !self.hover_live || self.chrome.grabbing {
            return None;
        }
        let (x, y) = self.pointer?;
        self.hit_at(x, y)
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }

    fn disarm_hover(&mut self) {
        self.hover_live = false;
        self.hover_from = self.pointer;
    }
}

/// `12,480`
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut o = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            o.push(',');
        }
        o.push(c);
    }
    o
}

static MONO_OK: AtomicBool = AtomicBool::new(false);
static SERIF_OK: AtomicBool = AtomicBool::new(false);
/// Faces found and not yet in the font system: whichever browser draws
/// next loads them, even if the one that asked has since closed.
static FONT_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `fc-match`'s answers for the extra faces.
struct FontFound {
    owner: u64,
    mono: bool,
    serif: bool,
}

impl Rect {
    fn clip_to(self, b: Rect) -> Rect {
        intersect(self, b).unwrap_or_default()
    }
}

impl WinScene for Browser {
    fn size(&self) -> (f32, f32) {
        (self.bw + 2.0 * M, self.bh + 2.0 * M)
    }
    fn placement(&self) -> Placement {
        Placement::Center
    }
    /// Keyboard first, from the moment it opens, like the launcher.
    fn exclusive_keyboard(&self) -> bool {
        true
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }

    fn apply(&mut self, f: &Fact) {
        self.on_fact(f);
    }

    fn on_fact(&mut self, f: &Fact) -> bool {
        let Fact::Custom(any) = f else { return false };
        if let Some(l) = any.downcast_ref::<Listing>() {
            if l.owner != self.owner {
                return false;
            }
            if l.generation != self.list_gen {
                self.stale += 1;
                return false;
            }
            self.take_listing(l);
        } else if let Some(s) = any.downcast_ref::<Shown>() {
            if s.owner != self.owner {
                return false;
            }
            if s.generation != self.pv_gen {
                self.stale += 1;
                return false;
            }
            self.take_preview(s);
        } else if let Some(p) = any.downcast_ref::<Places>() {
            if p.owner != self.owner {
                return false;
            }
            self.places = p.places.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
            self.projects = p.projects.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
            if let Some(y) = self.active_rail() {
                self.rail_y.snap(y);
                self.rail_a.set(1.0);
            }
        } else if let Some(r) = any.downcast_ref::<Request>() {
            self.go_to(r.path.clone(), None, 1.0, true);
        } else if let Some(ff) = any.downcast_ref::<FontFound>() {
            self.mono = Some(ff.mono);
            self.serif = Some(ff.serif);
            if ff.owner != self.owner {
                return false;
            }
        } else if let Some(c) = any.downcast_ref::<Completed>() {
            if c.owner != self.owner || c.generation != self.tab_gen {
                return false;
            }
            self.filter = c.text.clone();
            self.last_input = self.chrome.time;
        } else {
            return false;
        }
        true
    }

    fn set_scale(&mut self, s: f32) {
        if s != self.scale {
            self.scale = s;
            self.text.set_scale(s);
            // Previews were sized for the old scale.
            self.free_textures();
            self.pv = None;
            self.want_preview();
        }
    }

    fn step(&mut self, dt: f32) -> bool {
        self.chrome.wake_at = None;
        let mut m = self.chrome.step(dt);
        let t = self.chrome.time;
        if self.last_input.is_nan() {
            self.last_input = t;
        }
        let c = &mut self.chrome;
        if c.closing && c.open_w.get().abs() < 0.02 && c.open_h.get().abs() < 0.02 {
            c.open_w.snap(0.0);
            c.open_h.snap(0.0);
        }
        for s in [
            &mut self.scroll,
            &mut self.head,
            &mut self.tail,
            &mut self.hl,
            &mut self.rail_y,
            &mut self.rail_a,
            &mut self.list_t,
            &mut self.caret,
            &mut self.filter_t,
        ] {
            m |= s.step(dt);
        }
        let closing = self.chrome.closing;
        if let Some(d) = self.pv_due {
            if closing {
                self.pv_due = None;
            } else if t >= d {
                self.send_preview();
            } else {
                self.chrome.wake(d);
            }
        }
        if let Some(pv) = &mut self.pv {
            m |= pv.fade.step(dt);
            m |= pv.holder.step(dt);
            if !pv.ready && pv.holder.target < 0.5 {
                let at = pv.born + PLACEHOLDER_AFTER;
                if t >= at {
                    pv.holder.set(1.0);
                    m = true;
                } else if !closing {
                    self.chrome.wake(at);
                }
            }
        }
        // The caret blinks for a while after typing, then stays solid.
        if !closing && (self.filtering || !self.filter.is_empty()) {
            let idle = t - self.last_input;
            if idle < BLINK_FOR {
                let next = if idle < 0.6 { 0.6 } else { 0.6 + ((idle - 0.6) / 0.5).floor() * 0.5 + 0.5 };
                self.chrome.wake(self.last_input + next.min(BLINK_FOR));
            }
        }
        m
    }

    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        let sc = self.scale;
        if self.chrome.done() {
            self.free_textures();
            self.uploads.clear();
            out.frees.append(&mut self.frees);
            self.text.end_frame();
            return;
        }
        if let Ok(mut f) = FONT_FILES.lock() {
            for p in f.drain(..) {
                let _ = fs.db_mut().load_font_file(p);
            }
        }
        out.frees.append(&mut self.frees);
        out.uploads.append(&mut self.uploads);

        let r = self.chrome.morph();
        self.chrome.body = r;
        let (ow, oh) = (self.chrome.open_w.get(), self.chrome.open_h.get());
        let open = ow.min(oh).clamp(0.0, 1.0);
        let closing = self.chrome.closing;
        let body_a = if closing { smooth((ow.max(oh) - 0.02) / 0.12) } else { 0.9 + 0.1 * smooth(open / 0.2) };
        let rad = lerp(14.0, RADIUS, oh.clamp(0.0, 1.0));
        out.shapes.push(paint::ink(sc, r, rad).shadow(SHADOW, 10.0 + 16.0 * open, 4.0 + 8.0 * open).alpha(body_a).done());
        out.glass(r, rad, body_a);

        let a = if closing { smooth((open - 0.45) / 0.45) } else { smooth((open - 0.55) / 0.4) };
        let inner = r.inflate(-1.0);
        // Laid out for the open size from the body's corner; while it
        // morphs, the body clips it.
        let (bw, bh) = (self.bw, self.bh);
        let pw = (bw * PREVIEW_K).round();
        let list_x = RAIL_W + LIST_PAD;
        let list_w = bw - RAIL_W - pw - 2.0 * LIST_PAD;
        let vp = Rect::new(r.x + list_x, r.y + HEAD_H, list_w, bh - HEAD_H - FOOT_H);
        self.list_rect = vp.clip_to(inner);
        if a > 0.003 {
            self.vline(out, r.x + RAIL_W, r.y + 18.0, bh - 36.0, a);
            self.vline(out, r.x + bw - pw, r.y + HEAD_H + 8.0, bh - HEAD_H - 26.0, a);
            self.draw_title(fs, out, r, a);
            self.draw_rail(fs, out, r, inner, a);
            let bar = Rect::new(r.x + RAIL_W + LIST_PAD, r.y + BAR_Y, bw - RAIL_W - LIST_PAD - BAR_Y, BAR_H);
            self.draw_bar(fs, out, bar, inner, a);
            let view = vp.clip_to(inner);
            self.draw_rows(fs, out, vp, view, a);
            self.draw_empty(fs, out, vp, view, a);
            let fy = r.y + bh - FOOT_H;
            paint::divider(&mut out.shapes, sc, vp.x + 4.0, fy, vp.w - 8.0, a);
            self.draw_foot(fs, out, Rect::new(vp.x + 14.0, fy, vp.w - 28.0, FOOT_H), inner, a);
            self.draw_preview(fs, out, r.x + bw - pw + PV_PAD, r.y + HEAD_H + 6.0, inner, a);
        }

        self.text.end_frame();
        let (dx, dy) = self.chrome.translate(out, sc);
        for (h, _) in &mut self.hits {
            h.x += dx;
            h.y += dy;
        }
        self.list_rect.x += dx;
        self.list_rect.y += dy;
    }

    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pointer = Some((x, y));
        let (ax, ay) = *self.hover_from.get_or_insert((x, y));
        if !self.hover_live && (x - ax).hypot(y - ay) > HOVER_SLOP {
            self.hover_live = true;
        }
        None
    }

    fn pointer_leave(&mut self) {
        self.pointer = None;
        self.hover_from = None;
        self.hover_live = false;
    }

    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        if self.chrome.closing {
            return None;
        }
        match self.hit_at(x, y) {
            Some(h) => {
                if let Hit::Row(i) = h {
                    self.select(i);
                }
                self.pressed = Some(h);
            }
            None if self.chrome.body.contains(x, y) => self.chrome.start_grab(),
            None => {}
        }
        None
    }

    fn release(&mut self) -> (Option<Action>, bool) {
        self.chrome.end_grab();
        let Some(h) = self.pressed.take() else { return (None, false) };
        let Some((x, y)) = self.pointer else { return (None, false) };
        if self.hit_at(x, y) != Some(h) {
            return (None, false);
        }
        match h {
            Hit::Row(i) => {
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(j, t)| j == i && now.duration_since(t).as_secs_f32() < DOUBLE);
                self.last_click = if double { None } else { Some((i, now)) };
                if double {
                    self.open_selected();
                }
            }
            Hit::Place(i) => {
                if let Some(p) = self.places.get(i) {
                    let p = p.path.clone();
                    self.go(p, None, 1.0);
                }
            }
            Hit::Project(i) => {
                if let Some(p) = self.projects.get(i) {
                    let p = p.path.clone();
                    self.go(p, None, 1.0);
                }
            }
            Hit::Crumb(i) => {
                if let Some(c) = self.crumbs.get(i) {
                    let (p, name) = (c.path.clone(), self.crumbs.get(i + 1).map(|n| n.path.clone()));
                    let sel = name.and_then(|n| n.file_name().map(OsStr::to_os_string));
                    self.go(p, sel, -1.0);
                }
            }
            Hit::Hidden => self.toggle_hidden(),
            Hit::Bar => self.focus_bar(),
            Hit::Clear => self.clear_filter(),
        }
        (None, false)
    }

    fn scroll(&mut self, dy: f32) -> Option<Action> {
        if !self.chrome.closing {
            self.pan = None;
            self.scroll.set((self.scroll.target + dy * ROW_H * 2.5).clamp(0.0, self.max_scroll()));
        }
        None
    }

    /// Touchpad: the list follows the fingers and glides on when they lift.
    fn pan(&mut self, _dx: f32, dy: f32, end: bool) -> bool {
        if self.chrome.closing {
            return true;
        }
        let max = self.max_scroll();
        if dy != 0.0 {
            let now = Instant::now();
            let p = self.pan.get_or_insert(Pan { raw: self.scroll.get(), last: now, vel: 0.0 });
            let dt = now.duration_since(p.last).as_secs_f32().max(1.0 / 500.0);
            p.raw = (p.raw + dy).clamp(-80.0, max + 80.0);
            p.vel = p.vel * 0.6 + dy / dt * 0.4;
            p.last = now;
            let raw = p.raw;
            // Past the ends it gives a little, and comes back.
            let shown = if raw < 0.0 {
                raw * 0.35
            } else if raw > max {
                max + (raw - max) * 0.35
            } else {
                raw
            };
            self.scroll.snap(shown);
        }
        if end && let Some(p) = self.pan.take() {
            self.scroll.vel = p.vel.clamp(-6000.0, 6000.0);
            self.scroll.set((self.scroll.get() + self.scroll.vel * 0.2).clamp(0.0, max));
        }
        true
    }

    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if self.chrome.closing {
            return (None, false);
        }
        self.disarm_hover();
        let s = self.sel;
        let n = self.view.len();
        let on = self.filter_on();
        match k.sym {
            keys::ESCAPE => {
                if !on {
                    return (None, true);
                }
                self.clear_filter();
            }
            keys::RETURN | keys::KP_ENTER => {
                if self.path_mode() {
                    let p = listing::expand(&self.filter, &self.home);
                    self.go_to(p, None, 1.0, true);
                } else {
                    self.open_selected();
                }
            }
            keys::TAB => {
                if self.path_mode() {
                    self.completions.push(CompleteReq { owner: self.owner, generation: self.tab_gen, text: self.filter.clone(), home: self.home.clone(), tx: self.tx.clone() });
                }
            }
            keys::RIGHT => {
                if self.selected().is_some_and(|e| e.is_dir) {
                    self.open_selected();
                }
            }
            keys::LEFT => self.up(),
            keys::BACKSPACE => {
                if !self.filter.is_empty() {
                    if k.ctrl {
                        self.filter.clear();
                    } else {
                        self.filter.pop();
                    }
                    self.refilter();
                } else if self.filtering {
                    self.filtering = false;
                    self.filter_t.set(0.0);
                } else {
                    self.up();
                }
            }
            keys::UP => self.select(s.saturating_sub(1)),
            keys::DOWN => self.select(s + 1),
            keys::PAGE_UP => self.select(s.saturating_sub(PAGE_ROWS)),
            keys::PAGE_DOWN => self.select(s + PAGE_ROWS),
            keys::HOME => self.select(0),
            keys::END => self.select(n.saturating_sub(1)),
            0xffc2 => self.reload(), // F5
            _ if k.ctrl => match k.sym {
                0x6a | 0x4a => self.select(s + 1),
                0x6b | 0x4b => self.select(s.saturating_sub(1)),
                0x72 | 0x52 => self.reload(),
                0x68 | 0x48 => self.toggle_hidden(),
                0x66 | 0x46 => self.focus_bar(),
                0x75 | 0x55 if on => self.clear_filter(),
                _ => {}
            },
            _ => {
                let Some(t) = k.text.as_deref().filter(|t| !k.alt && !t.is_empty() && !t.chars().any(char::is_control)) else { return (None, false) };
                if !on {
                    match t {
                        "j" => {
                            self.select(s + 1);
                            return (None, false);
                        }
                        "k" => {
                            self.select(s.saturating_sub(1));
                            return (None, false);
                        }
                        "." => {
                            self.toggle_hidden();
                            return (None, false);
                        }
                        " " => return (None, false),
                        _ => {}
                    }
                }
                if self.filter.chars().count() < if self.path_mode() { 240 } else { 48 } {
                    self.filter.push_str(t);
                    self.focus_bar();
                    self.refilter();
                }
            }
        }
        (None, false)
    }

    fn cursor(&self) -> Cursor {
        if self.chrome.grabbing {
            return Cursor::Grabbing;
        }
        match self.pointer {
            Some((x, y)) => match self.hit_at(x, y) {
                Some(Hit::Bar) => Cursor::Text,
                Some(_) => Cursor::Pointer,
                None => Cursor::Default,
            },
            None => Cursor::Default,
        }
    }

    fn close(&mut self) {
        let c = &mut self.chrome;
        if !c.closing {
            c.open_w.cfg = CLOSE;
            c.open_h.cfg = CLOSE;
        }
        c.close();
        // Whatever is running stops being wanted.
        self.pv_due = None;
        self.latest.store(u64::MAX, Ordering::Relaxed);
    }

    fn reopen(&mut self) {
        self.chrome.open_w.cfg = OPEN_W;
        self.chrome.open_h.cfg = OPEN_H;
        self.chrome.reopen();
        self.latest.store(self.pv_gen, Ordering::Relaxed);
        if self.pv.as_ref().is_some_and(|p| !p.ready) {
            self.pv_due = Some(self.chrome.time);
        }
    }

    fn release_textures(&mut self, frees: &mut Vec<u64>) {
        self.latest.store(u64::MAX, Ordering::Relaxed);
        self.free_textures();
        self.uploads.clear();
        frees.append(&mut self.frees);
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.latest.store(u64::MAX, Ordering::Relaxed);
        // Dropped without its last frame: the next render anywhere frees
        // what is still on the GPU. Pixels not uploaded yet just go.
        let mut keys = std::mem::take(&mut self.frees);
        let waiting: Vec<u64> = self.uploads.iter().map(|u| u.0).collect();
        keys.extend(self.cache.iter().map(|c| c.tex.key).filter(|k| !waiting.contains(k)));
        if !keys.is_empty() {
            gfx::free_later(keys);
        }
        // A closed window costs nothing: the listing goes now, and the
        // allocator gives its pages back (off this thread: it can take ms).
        self.entries = Vec::new();
        self.view = Vec::new();
        self.row_text = HashMap::new();
        self.row_spans = HashMap::new();
        let _ = std::thread::Builder::new().name("nimbus-trim".into()).spawn(|| {
            // SAFETY: no arguments to get wrong, callable from any thread.
            unsafe { libc::malloc_trim(0) };
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip_any_bytes() {
        for raw in [&b"/home/a/plain.txt"[..], b"/home/a/two words.txt", b"/tmp/line\nbreak", b"/tmp/caf\xe9", b"/tmp/100%.txt", "/tmp/日本語".as_bytes(), b"/tmp/a+b c%20d"] {
            let p = PathBuf::from(OsStr::from_bytes(raw));
            let line = reveal_line(&p, None);
            assert!(!line.contains('\n') && line.is_ascii(), "{line}");
            assert_eq!(parse_command(&line), Some(Some(Request { path: p.clone(), from: None })));
        }
        let from = Rect::new(10.0, 20.5, 300.0, 40.0);
        let got = parse_command(&reveal_line(Path::new("/a b"), Some(from))).unwrap().unwrap();
        assert_eq!(got.path, Path::new("/a b"));
        let r = got.from.unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (10.0, 20.5, 300.0, 40.0));
    }

    #[test]
    fn commands_are_not_confused() {
        assert_eq!(parse_command("files"), Some(None));
        assert_eq!(parse_command("filesystem"), None);
        assert_eq!(parse_command("launcher"), None);
    }

    #[test]
    fn local_paths() {
        let lp = |s: &str| local_path(s.as_bytes());
        assert_eq!(lp("/plain/path"), Some(PathBuf::from("/plain/path")));
        assert_eq!(lp("relative/path"), Some(PathBuf::from("relative/path")));
        assert_eq!(lp("file:///a%20b/c"), Some(PathBuf::from("/a b/c")));
        assert_eq!(lp("file://localhost/a"), Some(PathBuf::from("/a")));
        // A colon does not make it a URI.
        assert_eq!(lp("/tmp/a:b"), Some(PathBuf::from("/tmp/a:b")));
        assert_eq!(lp("a:b"), Some(PathBuf::from("a:b")));
        for no in ["smb://host/share", "https://x/y", "file://otherhost/a", ""] {
            assert_eq!(lp(no), None, "{no}");
        }
    }

    #[test]
    fn thousands_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(12480), "12,480");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}

// ── headless demo ───────────────────────────────────────────────────────

mod demo;
pub use demo::demo;
