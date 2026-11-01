//! App launcher (Win+Space): a Prism Ink search window (flat matte ink,
//! the prism hairline, nothing glossy) that grows out of a
//! search pill, a grid of every app to browse and fuzzy-ranked results as
//! you type. Tiles glide to their new slots on springs (or the new grid is
//! wiped in under the field when most of it moves), the selection is a
//! liquid head/tail blob, and launching folds the window into the chosen
//! icon, which then bursts away.
//!
//! Threads: the catalog (desktop entries + icon index) is scanned on a
//! one-shot worker and cached across opens; icons are rasterised on a
//! worker that lives only while the window is open and only for tiles that
//! come into view, at the size they are drawn. Each icon is uploaded once
//! and every texture is freed when the window goes away (or, if it is
//! dropped with its surface, on the next render anywhere).
//!
//! Below the apps, a typed query also lists files from the home index
//! (`files::index`): quiet rows that the selection flows into. The search
//! runs on its own thread, so typing stays as instant as it is for apps,
//! and with an empty query there is no trace of it.

mod apps;
mod fuzzy;

use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use glyphon::FontSystem;

use apps::Catalog;

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    files::{self, Mailbox, index, kind},
    gfx::{self, DrawList, IconDraw, ImageData, ImageDraw, TextDraw},
    icons::IconSet,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    window::{Chrome, Ctx, Cursor, KeyInput, Placement, WinScene, keys},
};

// ── geometry (logical px, body-relative) ────────────────────────────────

const BW: f32 = 680.0;
const BH: f32 = 540.0;
const M: f32 = 40.0;
const SW: f32 = BW + 2.0 * M;
const SH: f32 = BH + 2.0 * M;
const PAD: f32 = 14.0;
const FIELD_H: f32 = 52.0;
const COLS: usize = 6;
const TW: f32 = (BW - 2.0 * PAD) / COLS as f32;
const GRID_TOP: f32 = PAD + FIELD_H + 10.0;
const ROW_H: f32 = 102.0;
const VIEW_ROWS: usize = 4;
const VIEW_H: f32 = ROW_H * VIEW_ROWS as f32;
const FOOT_Y: f32 = GRID_TOP + VIEW_H;
const ICON: f32 = 50.0;
const ICON_Y: f32 = 15.0;
const PILL_W: f32 = 240.0;
const PILL_R: f32 = 18.0;
/// Icons and labels fade over this distance before the viewport edges,
/// so nothing is ever sliced by the clip at more than a whisper.
const EDGE: f32 = 12.0;
/// "Unbounded" for clip bands (finite, so rect maths stays finite).
const FAR: f32 = 1e6;
/// Surface top on the output (`Placement::Top`).
const TOP_Y: f32 = 110.0;
/// The file section under the apps.
const FILE_ROW: f32 = 40.0;
const FILE_HEAD: f32 = 34.0;
const FILE_GAP: f32 = 4.0;
const FILE_ICON: f32 = 18.0;
const FILE_PAGE: usize = 8;
/// Footer entries at or above this are files (`foot` holds app indices).
const FILE_FOOT: u32 = 1 << 30;

// ── look ────────────────────────────────────────────────────────────────

const F_QUERY: Font = Font::new(19.0, 500).family("Geist");
const F_NAME: Font = Font::new(12.5, 550).family("Geist");
const F_LABEL: Font = Font::new(13.5, 600).family("Geist");
const F_SMALL: Font = Font::new(12.0, 500).family("Geist");
const F_KEY: Font = Font::new(10.5, 650).family("Geist");
const F_LETTER: Font = Font::new(22.0, 650).family("Geist");
const F_LETTER_S: Font = Font::new(11.0, 700).family("Geist");
const F_FILE: Font = Font::new(13.0, 500).family("Geist");
const F_SECTION: Font = Font::new(10.5, 650).family("Geist").track(0.08);

/// The search field: a faint white well on the ink.
const FIELD: Rgba = Rgba::hex(0xffffff, 0.055);
/// The selection blob: faint white (the prism hairline edges it).
const SEL: Rgba = Rgba::hex(0xffffff, 0.07);
/// Open body rounding; the field inside it is concentric.
const BODY_R: f32 = 28.0;
const SHADOW: Rgba = Rgba::hex(0x000000, 0.6);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
/// A file's folder: quiet, but readable on the selection and on bright
/// wallpapers.
const META_DIR: Rgba = Rgba::hex(0xbcbec7, 1.0);

// ── motion ──────────────────────────────────────────────────────────────

const OPEN_W: SpringCfg = SpringCfg::new(0.3, 0.82);
const OPEN_H: SpringCfg = SpringCfg::new(0.36, 0.84);
/// Closing: height leads (collapse into the field), width follows.
const CLOSE_W: SpringCfg = SpringCfg::new(0.3, 1.0);
const CLOSE_H: SpringCfg = SpringCfg::new(0.2, 1.0);
const FOLD: SpringCfg = SpringCfg::new(0.28, 1.0);
const SLIDE: SpringCfg = SpringCfg::new(0.28, 0.84);
const POP: SpringCfg = SpringCfg::new(0.28, 0.72);
/// Leavers and far movers' old selves get out of the way quickly.
const LEAVE: SpringCfg = SpringCfg::new(0.14, 1.0);
const SCROLL: SpringCfg = SpringCfg::new(0.24, 1.0);
const CARET: SpringCfg = SpringCfg::new(0.2, 0.92);
const FADE: SpringCfg = SpringCfg::new(0.2, 1.0);
/// Text swaps (footer line, count) wipe across on this.
const SWAP: SpringCfg = SpringCfg::new(0.18, 1.0);
const WIPE: SpringCfg = SpringCfg::new(0.24, 0.8);
const BOOM: SpringCfg = SpringCfg::new(0.26, 1.0);
/// Selection: snappy head, a tail that only just trails it.
const SEL_HEAD: SpringCfg = SpringCfg::new(0.22, 0.78);
const SEL_TAIL: SpringCfg = SpringCfg::new(0.3, 0.9);
/// Caret blinks (via wake-ups, ~2 Hz) for this long after the last key,
/// then stays solid so an idle launcher costs no frames.
const BLINK_FOR: f32 = 10.0;
/// After the wheel stops, the grid settles onto a whole row.
const WHEEL_REST: f32 = 0.16;
/// The pointer must travel this far before hover moves the selection.
const HOVER_SLOP: f32 = 4.0;

static SESSION: AtomicU64 = AtomicU64::new(1);

/// Worker → window payloads (inside `Fact::Custom`). The data sits in a
/// `Mutex<Option>` so the window can take it and the copy main.rs keeps
/// as "latest fact" holds nothing heavy.
enum Msg {
    Catalog(u64, Mutex<Option<Arc<Catalog>>>),
    /// (session, generation, results): stale generations are dropped.
    Icons(u64, u32, Mutex<Vec<(u32, Option<ImageData>)>>),
}

struct IconReq {
    idx: u32,
    name: String,
    px: u32,
}

struct Item {
    x: Spring,
    y: Spring,
    vis: Spring,
    /// Seconds until `vis` heads for `want` (cascades).
    delay: f32,
    want: f32,
    live: bool,
    /// Where a far mover was: it gets out of the way there while it pops
    /// in anew (or, during a wipe, the old grid's copy of the tile).
    ghost: Option<(f32, f32)>,
    gv: Spring,
}

impl Item {
    fn new() -> Self {
        Self {
            x: Spring::new(0.0, SLIDE),
            y: Spring::new(0.0, SLIDE),
            vis: Spring::new(0.0, POP),
            delay: 0.0,
            want: 0.0,
            live: false,
            ghost: None,
            gv: Spring::new(0.0, LEAVE),
        }
    }
}

enum Tex {
    Pending,
    Ready { key: u64, fade: Spring },
    Missing,
}

struct Launch {
    idx: u32,
    /// Icon rect at launch (morph space): the window folds into it.
    rect: Rect,
}

/// How one grid tile is drawn.
#[derive(Clone, Copy)]
struct TileArgs {
    x: f32,
    y: f32,
    v: f32,
    a: f32,
    /// Extra label alpha (labels of movers fade while in flight).
    label: f32,
    /// Icon scale (movers dip a little, so crossings read as depth).
    shrink: f32,
    clip: Rect,
    hit: Option<usize>,
}

pub struct Launcher {
    chrome: Chrome,
    text: TextCache,
    scale: f32,
    session: u64,
    /// Bumped whenever icons in flight become invalid (catalog, scale).
    epoch: u32,
    fact_tx: calloop::channel::Sender<Fact>,
    dry: bool,

    cat: Option<Arc<Catalog>>,
    /// A rescan that arrived mid-launch or mid-close (applied on reopen).
    pending_cat: Option<Arc<Catalog>>,
    freq: Vec<f32>,
    items: Vec<Item>,
    res: Vec<u32>,
    query: String,
    sel: usize,
    start_r: f32,

    scroll: Spring,
    wheel_at: Option<f32>,
    head: (Spring, Spring),
    tail: (Spring, Spring),
    hl: Spring,
    caret: Spring,
    ghost: Option<String>,
    holder: Spring,
    count: (String, String),
    count_t: Spring,
    foot: (Option<u32>, Option<u32>),
    foot_t: Spring,
    empty_t: Spring,
    /// Time of the last key; NaN until the first frame syncs the clock.
    last_input: f32,
    wipe: Spring,
    wiping: bool,

    icons: HashMap<u32, Tex>,
    uploads: Vec<(u64, ImageData)>,
    frees: Vec<u64>,
    icon_tx: Option<mpsc::Sender<IconReq>>,
    alive: Arc<AtomicBool>,
    /// Icon results dropped because they belonged to an older generation.
    stale: u32,

    pointer: Option<(f32, f32)>,
    /// Hover selects only once the pointer has really moved from here.
    hover_from: Option<(f32, f32)>,
    hover_live: bool,
    hits: Vec<(Rect, usize)>,
    /// Scratch (kept to avoid per-frame allocations).
    want: Vec<u32>,
    order: Vec<(f32, usize)>,
    field: Rect,
    view: Rect,
    band: (f32, f32),
    pressed: Option<usize>,
    press: Spring,
    launch: Option<Launch>,
    boom: Spring,

    /// File results for the query, best first. Selection positions past
    /// the apps (`res`) are rows here.
    files: Vec<index::Hit>,
    files_more: bool,
    files_gen: u64,
    /// A search is out for the current query.
    files_wait: bool,
    search: Mailbox<index::Ask>,
    /// Section top in grid coordinates, and its fade.
    files_y: Spring,
    files_a: Spring,
    /// Selection blob size: a tile's, or a file row's.
    blob_w: Spring,
    blob_h: Spring,
    iconset: Rc<IconSet>,
    out_w: f32,
    extents: Vec<(f32, f32)>,
    /// What the last Enter on a file did (the headless demo's record).
    opened: Option<(PathBuf, bool)>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn slot(p: usize) -> (f32, f32) {
    ((p % COLS) as f32 * TW, (p / COLS) as f32 * ROW_H)
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// `r` limited to the horizontal band `y0..y1`.
fn rows(r: Rect, y0: f32, y1: f32) -> Rect {
    intersect(r, Rect::new(r.x, y0, r.w, y1 - y0)).unwrap_or_default()
}

/// `r` limited to the vertical band `x0..x1`.
fn cols(r: Rect, x0: f32, x1: f32) -> Rect {
    intersect(r, Rect::new(x0, r.y, x1 - x0, r.h)).unwrap_or_default()
}

fn pill(w: f32) -> Rect {
    Rect::new((SW - w) * 0.5, M, w, FIELD_H)
}

pub fn open(ctx: &mut Ctx) -> Box<dyn WinScene> {
    Box::new(Launcher::new(ctx, false))
}

impl Launcher {
    fn new(ctx: &mut Ctx, dry: bool) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        let home = Rect::new(M, M, BW, BH);
        let session = SESSION.fetch_add(1, Ordering::Relaxed);
        let spring = |v: f32, c: SpringCfg| Spring::new(v, c);
        let mut s = Self {
            chrome: Chrome::new(pill(PILL_W), home, OPEN_W, OPEN_H),
            text,
            scale: ctx.scale,
            session,
            epoch: 0,
            fact_tx: ctx.fact_tx.clone(),
            dry,
            cat: None,
            pending_cat: None,
            freq: vec![],
            items: vec![],
            res: vec![],
            query: String::new(),
            sel: 0,
            start_r: PILL_R,
            scroll: spring(0.0, SCROLL),
            wheel_at: None,
            head: (spring(0.0, SEL_HEAD), spring(0.0, SEL_HEAD)),
            tail: (spring(0.0, SEL_TAIL), spring(0.0, SEL_TAIL)),
            hl: spring(0.0, FADE),
            caret: spring(-5.0, CARET),
            ghost: None,
            holder: spring(1.0, FADE),
            count: (String::new(), String::new()),
            count_t: spring(1.0, SWAP),
            foot: (None, None),
            foot_t: spring(1.0, SWAP),
            empty_t: spring(0.0, FADE),
            last_input: f32::NAN,
            wipe: spring(1.0, WIPE),
            wiping: false,
            icons: HashMap::new(),
            uploads: vec![],
            frees: vec![],
            icon_tx: None,
            alive: Arc::new(AtomicBool::new(true)),
            stale: 0,
            pointer: None,
            hover_from: None,
            hover_live: false,
            hits: vec![],
            want: vec![],
            order: vec![],
            field: Rect::default(),
            view: Rect::default(),
            band: (0.0, 0.0),
            pressed: None,
            press: spring(0.0, presets::SNAPPY),
            launch: None,
            boom: spring(0.0, BOOM),
            files: vec![],
            files_more: false,
            files_gen: 0,
            files_wait: false,
            search: {
                let tx = ctx.fact_tx.clone();
                Mailbox::new("nimbus-filesearch", move |a| index::answer(a, &tx, &mut files::fuzzy::Scratch::default()))
            },
            files_y: spring(0.0, SLIDE),
            files_a: spring(0.0, FADE),
            blob_w: spring(TW, SEL_HEAD),
            blob_h: spring(ROW_H, SEL_HEAD),
            iconset: ctx.icons.clone(),
            out_w: ctx.out_w,
            extents: vec![],
            opened: None,
        };
        if !dry {
            index::refresh(ctx.fact_tx.clone());
        }
        // Show the cached catalog right away; a worker checks the disk and
        // only reports back if something changed (or nothing was cached).
        let have = apps::cached();
        if let Some(c) = &have {
            s.set_catalog(c.clone(), true);
        }
        let tx = ctx.fact_tx.clone();
        let _ = std::thread::Builder::new().name("nimbus-apps".into()).spawn(move || {
            if let Some(c) = apps::fresh(have.as_ref()) {
                let _ = tx.send(Fact::Custom(Arc::new(Msg::Catalog(session, Mutex::new(Some(c))))));
            }
        });
        s
    }

    // ── model ───────────────────────────────────────────────────────────

    /// Take a (new) catalog. Tiles and icons of apps that survive carry
    /// over by desktop id, so a rescan glides instead of restarting.
    fn set_catalog(&mut self, c: Arc<Catalog>, cascade: bool) {
        // Icons in flight were asked for by old indices: stop the worker
        // and ignore anything it already sent.
        self.stop_icons();
        self.epoch = self.epoch.wrapping_add(1);
        self.foot = (None, None);
        self.foot_t.snap(1.0);
        self.pressed = None;
        self.press.snap(0.0);
        self.finish_wipe();
        let mut old_items = std::mem::take(&mut self.items);
        let mut old_icons = std::mem::take(&mut self.icons);
        let mut by_id: HashMap<&str, usize> = HashMap::new();
        if let Some(old) = &self.cat {
            for (i, a) in old.apps.iter().enumerate() {
                by_id.insert(a.id.as_str(), i);
            }
        }
        let mut items = Vec::with_capacity(c.apps.len());
        let mut icons = HashMap::new();
        for (ni, a) in c.apps.iter().enumerate() {
            match by_id.remove(a.id.as_str()) {
                Some(oi) if oi < old_items.len() => {
                    items.push(std::mem::replace(&mut old_items[oi], Item::new()));
                    if let Some(t) = old_icons.remove(&(oi as u32)) {
                        icons.insert(ni as u32, t);
                    }
                }
                _ => items.push(Item::new()),
            }
        }
        // Whatever did not survive gives its texture back (or never
        // uploads it).
        for t in old_icons.values() {
            if let Tex::Ready { key, .. } = t {
                self.uploads.retain(|(k, _)| k != key);
                self.frees.push(*key);
            }
        }
        self.items = items;
        self.icons = icons;
        self.freq = apps::frecency(&c.apps);
        self.cat = Some(c);
        self.res.clear();
        self.refilter(cascade);
    }

    fn finish_wipe(&mut self) {
        if self.wiping {
            self.wiping = false;
            self.wipe.snap(1.0);
            for it in &mut self.items {
                it.ghost = None;
                it.gv.snap(0.0);
            }
        }
    }

    fn refilter(&mut self, cascade: bool) {
        let Some(cat) = &self.cat else { return };
        let q = self.query.trim().to_lowercase();
        let new = if q.is_empty() { fuzzy::browse(&self.freq) } else { fuzzy::rank(&cat.apps, &self.freq, &q) };
        let mut pos = vec![usize::MAX; self.items.len()];
        for (p, &i) in new.iter().enumerate() {
            pos[i as usize] = p;
        }
        // Results restart at the top. Shift everything into the unscrolled
        // frame first so nothing jumps.
        let scrolled = self.scroll.get();
        self.scroll.snap(0.0);
        self.wheel_at = None;
        for s in [&mut self.head.1, &mut self.tail.1] {
            s.pos -= scrolled;
            s.target -= scrolled;
        }
        self.finish_wipe();
        for it in &mut self.items {
            it.y.pos -= scrolled;
            it.y.target -= scrolled;
            if let Some(g) = &mut it.ghost {
                g.1 -= scrolled;
            }
        }
        let shown = |it: &Item| {
            let oy = it.y.get();
            it.live && it.vis.get() > 0.08 && oy > -ROW_H && oy < VIEW_H
        };
        // Close movers glide (same or next row, ~one slot away); far ones
        // get out of the way where they were and pop in where they land.
        let near = |it: &Item, p: usize| {
            let (sx, sy) = slot(p);
            let (ox, oy) = (it.x.get(), it.y.get());
            let rows = ((oy / ROW_H).round() - (p / COLS) as f32).abs();
            rows <= 1.0 && ((ox - sx) / TW).hypot((oy - sy) / ROW_H) <= 1.3
        };
        // When most of what is on screen would leave or jump, swap grids
        // with a wipe from the field down instead of a storm of pops (which
        // would leave the grid half empty for a few frames).
        let (mut seen, mut far) = (0usize, 0usize);
        for (i, it) in self.items.iter().enumerate() {
            if shown(it) {
                seen += 1;
                if pos[i] == usize::MAX || !near(it, pos[i]) {
                    far += 1;
                }
            }
        }
        let wipe = !cascade && seen >= 6 && far * 2 > seen;
        let mut top_glides = false;
        for (i, it) in self.items.iter_mut().enumerate() {
            let was = shown(it);
            let (ox, oy) = (it.x.get(), it.y.get());
            let p = pos[i];
            if p == usize::MAX {
                if wipe && was {
                    it.ghost = Some((ox, oy));
                    it.gv.snap(it.vis.get());
                    it.vis.snap(0.0);
                } else {
                    it.vis.cfg = LEAVE;
                    it.vis.set(0.0);
                }
                it.live = false;
                it.want = 0.0;
                it.delay = 0.0;
                continue;
            }
            it.vis.cfg = POP;
            let (sx, sy) = slot(p);
            let row = p / COLS;
            let lands_on_screen = row < VIEW_ROWS + 1;
            if wipe {
                if was {
                    it.ghost = Some((ox, oy));
                    it.gv.snap(it.vis.get());
                }
                it.x.snap(sx);
                it.y.snap(sy);
                it.vis.snap(1.0);
                it.delay = 0.0;
            } else if was && near(it, p) && !cascade {
                it.x.set(sx);
                it.y.set(sy);
                it.delay = 0.0;
                top_glides |= p == 0;
            } else {
                if was {
                    it.ghost = Some((ox, oy));
                    it.gv.snap(it.vis.get());
                    it.gv.set(0.0);
                }
                it.x.snap(sx);
                it.y.snap(sy);
                it.vis.snap(0.0);
                it.delay = match (lands_on_screen, cascade) {
                    (false, _) => 0.0,
                    (true, true) => 0.02 + row as f32 * 0.018 + (p % COLS) as f32 * 0.006,
                    (true, false) => (p.min(24)) as f32 * 0.003,
                };
            }
            it.live = true;
            it.want = 1.0;
            if it.delay <= 0.0 && !wipe {
                if lands_on_screen {
                    it.vis.set(1.0);
                } else {
                    it.vis.snap(1.0);
                }
            }
        }
        if wipe {
            self.wiping = true;
            self.wipe.snap(0.0);
            self.wipe.set(1.0);
        }
        self.res = new;
        let n = self.res.len();
        self.ask_files();
        self.place_files();
        self.update_count();
        self.update_empty();
        self.select(0, false);
        if n == 0 {
            if self.files.is_empty() {
                self.hl.set(0.0);
            }
        } else if top_glides {
            // The blob rides along with the tile it belongs to.
            let it = &self.items[self.res[0] as usize];
            self.head = (it.x, it.y);
            self.tail = (it.x, it.y);
            self.hl.set(1.0);
        } else {
            // A new top result: the blob pops in on it, never flies over
            // a grid that is rearranging.
            let (x, y) = slot(0);
            for s in [&mut self.head.0, &mut self.tail.0] {
                s.snap(x);
            }
            for s in [&mut self.head.1, &mut self.tail.1] {
                s.snap(y);
            }
            self.hl.snap(0.0);
            self.hl.set(1.0);
        }
    }

    fn total(&self) -> usize {
        self.res.len() + self.files.len()
    }

    /// Is position `p` a file row (rather than an app tile)?
    fn is_file(&self, p: usize) -> bool {
        p >= self.res.len() && p < self.total()
    }

    fn update_count(&mut self) {
        let q = self.query.trim();
        let n = self.total();
        let count = match (q.is_empty(), n) {
            (true, n) => format!("{n} apps"),
            (false, 0) => "No matches".to_string(),
            (false, 1) => "1 result".to_string(),
            (false, n) if self.files_more => format!("{n}+ results"),
            (false, n) => format!("{n} results"),
        };
        if count != self.count.1 {
            self.count.0 = std::mem::replace(&mut self.count.1, count);
            self.count_t.snap(0.0);
            self.count_t.set(1.0);
        }
    }

    /// "No matches" waits for the file search, or it would flash on every
    /// key that leaves no app.
    fn update_empty(&mut self) {
        let n = self.total();
        if n > 0 {
            self.empty_t.set(0.0);
        } else if !self.files_wait {
            self.empty_t.set(1.0);
        }
    }

    /// Search files for the query (no query: no section at all).
    fn ask_files(&mut self) {
        self.files_gen += 1;
        let q = self.query.trim();
        if q.is_empty() {
            self.files.clear();
            self.files_more = false;
            self.files_wait = false;
            self.files_a.snap(0.0);
            return;
        }
        self.files_wait = true;
        let ask = index::Ask { owner: self.session, generation: self.files_gen, query: q.to_string() };
        self.search.push(ask);
    }

    fn apps_h(&self) -> f32 {
        self.res.len().div_ceil(COLS) as f32 * ROW_H
    }

    fn place_files(&mut self) {
        let y = if self.res.is_empty() { 0.0 } else { self.apps_h() + FILE_GAP };
        if self.files_a.get() < 0.02 {
            self.files_y.snap(y);
        } else {
            self.files_y.set(y);
        }
    }

    fn file_y(&self, i: usize) -> f32 {
        self.files_y.target + FILE_HEAD + i as f32 * FILE_ROW
    }

    /// New results from the search thread.
    fn set_files(&mut self, hits: Vec<index::Hit>, more: bool) {
        let keep = self.is_file(self.sel).then(|| self.files[self.sel - self.res.len()].path.clone());
        let first = self.files.is_empty();
        self.files = hits;
        self.files_more = more;
        self.files_wait = false;
        if first && !self.files.is_empty() {
            self.files_a.snap(0.0);
            self.place_files();
            self.files_a.set(1.0);
        } else if self.files.is_empty() {
            self.files_a.set(0.0);
        }
        self.update_count();
        self.update_empty();
        if self.res.is_empty() || keep.is_some() {
            // The selection was on a file (or there are only files): it
            // stays on the same one if it survived, else the first.
            let at = keep.and_then(|k| self.files.iter().position(|f| f.path == k)).unwrap_or(0);
            let appeared = self.hl.get() < 0.5 || self.hl.target < 0.5;
            if self.files.is_empty() {
                self.hl.set(if self.res.is_empty() { 0.0 } else { 1.0 });
                self.select(self.res.len().saturating_sub(1), false);
            } else {
                if appeared {
                    self.hl.snap(0.0);
                }
                self.select(self.res.len() + at, false);
                self.hl.set(1.0);
            }
        }
    }

    /// Enter, or a click, on position `p`.
    fn activate(&mut self, p: usize, default_app: bool) {
        if !self.is_file(p) {
            self.launch(p);
            return;
        }
        if self.launch.is_some() || self.chrome.closing {
            return;
        }
        let i = p - self.res.len();
        let Some(hit) = self.files.get(i) else { return };
        let path = hit.path.clone();
        if !self.dry {
            if default_app {
                files::open_default(&path);
            } else {
                // The browser grows out of this row.
                let (gx, gy) = self.grid_origin(self.chrome.morph());
                let (ox, oy) = (((self.out_w - SW) * 0.5).round() + self.chrome.surf_origin.0 as f32, TOP_Y + self.chrome.surf_origin.1 as f32);
                let y = self.files_y.get() + FILE_HEAD + i as f32 * FILE_ROW;
                let from = Rect::new(ox + gx, oy + gy + y - self.scroll.get(), BW - 2.0 * PAD, FILE_ROW);
                let _ = self.fact_tx.send(Fact::Command(files::reveal_line(&path, Some(from))));
            }
        }
        self.opened = Some((path, default_app));
        WinScene::close(self);
    }

    fn select(&mut self, p: usize, reveal: bool) {
        let n = self.total();
        if n == 0 {
            self.set_foot(None);
            return;
        }
        let p = p.min(n - 1);
        self.sel = p;
        let file = self.is_file(p);
        let (x, y) = if file { (0.0, self.file_y(p - self.res.len())) } else { slot(p) };
        let (bw, bh) = if file { (BW - 2.0 * PAD, FILE_ROW) } else { (TW, ROW_H) };
        self.blob_w.set(bw);
        self.blob_h.set(bh);
        self.head.0.cfg = SEL_HEAD;
        self.head.1.cfg = SEL_HEAD;
        self.tail.0.cfg = SEL_TAIL;
        self.tail.1.cfg = SEL_TAIL;
        self.head.0.set(x);
        self.head.1.set(y);
        self.tail.0.set(x);
        self.tail.1.set(y);
        if self.hl.get() < 0.05 {
            self.head.0.snap(x);
            self.head.1.snap(y);
            self.tail.0.snap(x);
            self.tail.1.snap(y);
            self.blob_w.snap(bw);
            self.blob_h.snap(bh);
        }
        if reveal {
            let t = self.scroll.target;
            // The first row brings the section's label along.
            let top = if file && p == self.res.len() { y - FILE_HEAD } else { y };
            if top < t {
                self.scroll.set(top);
            } else if y + bh > t + VIEW_H {
                self.scroll.set(y + bh - VIEW_H);
            }
        }
        self.set_foot(Some(if file { FILE_FOOT + (p - self.res.len()) as u32 } else { self.res[p] }));
    }

    fn set_foot(&mut self, f: Option<u32>) {
        if f != self.foot.1 {
            self.foot.0 = std::mem::replace(&mut self.foot.1, f);
            self.foot_t.snap(0.0);
            self.foot_t.set(1.0);
        }
    }

    fn max_scroll(&self) -> f32 {
        let end = if self.files.is_empty() { self.apps_h() } else { self.file_y(self.files.len()) + FILE_GAP };
        (end - VIEW_H).max(0.0)
    }

    /// Where the wheel comes to rest near `t`: a whole row of either kind.
    fn rest_on(&self, t: f32) -> f32 {
        let fy = self.files_y.target;
        if !self.files.is_empty() && t >= fy { fy + ((t - fy) / FILE_ROW).round() * FILE_ROW } else { (t / ROW_H).round() * ROW_H }
    }

    fn typed(&mut self, old: String) {
        if old.starts_with(&self.query) && old.len() > self.query.len() {
            // Deleting: the old tail is wiped away behind the caret.
            if self.ghost.as_ref().is_none_or(|g| g.len() < old.len()) {
                self.ghost = Some(old);
            }
        } else {
            self.ghost = None;
        }
        self.last_input = self.chrome.time;
        self.holder.set(if self.query.is_empty() { 1.0 } else { 0.0 });
        self.refilter(false);
    }

    /// Keys own the selection until the pointer really moves again.
    fn disarm_hover(&mut self) {
        self.hover_live = false;
        self.hover_from = self.pointer;
    }

    fn idle(&self) -> f32 {
        if self.last_input.is_nan() { 0.0 } else { self.chrome.time - self.last_input }
    }

    // ── launching ───────────────────────────────────────────────────────

    fn launch(&mut self, p: usize) {
        if self.launch.is_some() || self.chrome.closing {
            return;
        }
        let (Some(cat), Some(&idx)) = (&self.cat, self.res.get(p)) else { return };
        let Some(app) = cat.apps.get(idx as usize) else { return };
        if !self.dry {
            let (id, argv) = (app.id.clone(), apps::argv(app));
            let _ = std::thread::Builder::new().name("nimbus-launch".into()).spawn(move || {
                apps::spawn(argv);
                apps::record(&id);
            });
        }
        // Freeze the layout where it is now and fold from there into the
        // icon: content is contracted into it, then the icon bursts.
        let r = self.chrome.morph();
        let (gx, gy) = self.grid_origin(r);
        let it = &self.items[idx as usize];
        let x = gx + it.x.get() + (TW - ICON) * 0.5;
        let y = gy + it.y.get() - self.scroll.get() + ICON_Y;
        let rect = Rect::new(x, y, ICON, ICON);
        self.launch = Some(Launch { idx, rect });
        let c = &mut self.chrome;
        c.home = r;
        c.open_w.snap(1.0);
        c.open_h.snap(1.0);
        c.open_w.cfg = FOLD;
        c.open_h.cfg = FOLD;
        c.start = rect.inflate(4.0);
        self.start_r = 16.0;
        c.close();
        self.boom.snap(0.0);
        self.sel = p;
        self.pressed = None;
        self.stop_icons();
    }

    // ── icons ───────────────────────────────────────────────────────────

    fn icon_px(&self) -> u32 {
        (ICON * self.scale).round() as u32
    }

    fn key_of(&self, idx: u32) -> u64 {
        // "lnch" | session | generation | app index: unique among all GPU
        // textures (a newer generation never reuses a key still in use).
        0x6c6e_6368_0000_0000 | ((self.session & 0xff) << 24) | ((self.epoch as u64 & 0xff) << 16) | (idx as u64 & 0xffff)
    }

    fn request_icon(&mut self, idx: u32) {
        if self.icons.contains_key(&idx) || self.chrome.closing {
            return;
        }
        let Some(cat) = &self.cat else { return };
        let Some(app) = cat.apps.get(idx as usize) else { return };
        let name = app.icon.clone();
        if name.is_empty() {
            self.icons.insert(idx, Tex::Missing);
            return;
        }
        if self.icon_tx.is_none() {
            let (tx, rx) = mpsc::channel::<IconReq>();
            let alive = Arc::new(AtomicBool::new(true));
            self.alive = alive.clone();
            let (out, session, epoch, cat) = (self.fact_tx.clone(), self.session, self.epoch, cat.clone());
            let ok = std::thread::Builder::new()
                .name("nimbus-icons".into())
                .spawn(move || icon_worker(rx, out, session, epoch, cat, alive))
                .is_ok();
            if !ok {
                return;
            }
            self.icon_tx = Some(tx);
        }
        let px = self.icon_px();
        if let Some(tx) = &self.icon_tx
            && tx.send(IconReq { idx, name, px }).is_ok()
        {
            self.icons.insert(idx, Tex::Pending);
        }
    }

    fn stop_icons(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        self.icon_tx = None;
        // Requests in flight are lost: ask again if we reopen.
        self.icons.retain(|_, t| !matches!(t, Tex::Pending));
    }

    fn free_textures(&mut self) {
        for t in self.icons.values() {
            if let Tex::Ready { key, .. } = t {
                self.frees.push(*key);
            }
        }
        self.icons.clear();
        self.uploads.clear();
    }

    // ── drawing helpers ─────────────────────────────────────────────────

    fn grid_origin(&self, r: Rect) -> (f32, f32) {
        (r.x + r.w * 0.5 - BW * 0.5 + PAD, r.y + GRID_TOP)
    }

    fn px(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [(r.x * s).floor() as i32, (r.y * s).floor() as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32]
    }

    /// Fade for something spanning `top..bot` near the viewport edges.
    fn edge_a(&self, top: f32, bot: f32) -> f32 {
        let (t, b) = self.band;
        smooth((top - t + 1.0) / EDGE) * smooth((b - bot + 1.0) / EDGE)
    }

    fn text_at(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect) {
        if c.3 <= 0.003 || clip.w <= 0.5 || clip.h <= 0.5 {
            return;
        }
        let s = self.scale;
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: self.px(clip) });
    }

    /// Text with its vertical centre at `cy`; returns its width.
    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text_at(out, k, x, cy - lh * 0.5, c, clip);
        w
    }

    /// A texture cropped to `clip` (uv follows), pixel-snapped when drawn
    /// at its native size so icons stay crisp at rest.
    fn image(&self, out: &mut DrawList, key: u64, r: Rect, clip: Rect, alpha: f32) {
        if alpha <= 0.003 {
            return;
        }
        let s = self.scale;
        let mut r = r;
        let native = self.icon_px() as f32 / s;
        if (r.w - native).abs() < 0.01 {
            r.x = (r.x * s).round() / s;
            r.y = (r.y * s).round() / s;
        }
        let Some(c) = intersect(r, clip) else { return };
        let mut d = ImageDraw::new(key, [c.x * s, c.y * s, c.w * s, c.h * s]);
        d.uv = [(c.x - r.x) / r.w, (c.y - r.y) / r.h, (c.x + c.w - r.x) / r.w, (c.y + c.h - r.y) / r.h];
        d.alpha = alpha.min(1.0);
        out.images.push(d);
    }

    /// App icon (or its stand-in while loading / when missing) in `r`.
    fn app_icon(&mut self, fs: &mut FontSystem, out: &mut DrawList, idx: u32, r: Rect, clip: Rect, a: f32) {
        if a <= 0.003 {
            return;
        }
        let sc = self.scale;
        let (ready, fade) = match self.icons.get(&idx) {
            Some(Tex::Ready { key, fade }) => (Some(*key), fade.get().clamp(0.0, 1.0)),
            Some(Tex::Missing) => (None, 0.0),
            _ => (None, -1.0),
        };
        let k = r.w / ICON;
        if fade < 1.0 {
            let ba = a * (1.0 - fade.max(0.0));
            if let Some(cr) = intersect(r.inflate(-3.0 * k), clip) {
                if fade < 0.0 {
                    // Loading: a quiet tile.
                    out.shapes.push(paint::rect(sc, cr, 13.0 * k).fill(paint::FAINT).alpha(ba * 0.8).done());
                } else {
                    // No icon: the initial on a faintly iridescent tile.
                    out.shapes.push(
                        paint::rect(sc, cr, 13.0 * k).on_mix(paint::FAINT, 0.32, paint::IRID.0, paint::IRID.1, 0.0).alpha(ba).done(),
                    );
                    if let Some(app) = self.cat.as_ref().and_then(|c| c.apps.get(idx as usize)) {
                        let ch: String = app.name.chars().take(1).flat_map(char::to_uppercase).collect();
                        let f = if k < 0.7 { F_LETTER_S } else { F_LETTER };
                        let (tk, tw) = self.text.get(fs, &ch, f);
                        let lh = self.text.line_h(f);
                        self.text_at(out, tk, r.x + (r.w - tw) * 0.5, r.y + (r.h - lh) * 0.5, TEXT.fade(ba), clip);
                    }
                }
            }
        }
        if let Some(key) = ready {
            self.image(out, key, r, clip, a * fade);
        }
    }

    /// One grid tile (icon + name).
    fn tile(&mut self, fs: &mut FontSystem, out: &mut DrawList, i: u32, t: TileArgs) {
        let v = t.v;
        let a = v.clamp(0.0, 1.0) * t.a;
        if a <= 0.003 || t.clip.w <= 0.5 || t.clip.h <= 0.5 {
            return;
        }
        let is_sel = t.hit.is_some() && t.hit == Some(self.sel);
        let p = if is_sel && self.pressed == Some(self.sel) { self.press.get() } else { 0.0 };
        let k = (0.62 + 0.38 * v) * (1.0 - 0.07 * p) * t.shrink;
        let isz = ICON * k;
        let cx = t.x + TW * 0.5;
        let rise = (1.0 - v.min(1.0)) * 10.0;
        let ir = Rect::new(cx - isz * 0.5, t.y + ICON_Y + (ICON - isz) * 0.5 + rise, isz, isz);
        let ia = a * self.edge_a(ir.y, ir.y + ir.h);
        self.app_icon(fs, out, i, ir, t.clip, ia);
        let ly = t.y + ICON_Y + ICON + 8.0 + rise * 0.6;
        let la = a * (0.75 + 0.25 * v.min(1.0)) * t.label * self.edge_a(ly, ly + self.text.line_h(F_NAME));
        if la > 0.003
            && let Some(app) = self.cat.as_ref().and_then(|c| c.apps.get(i as usize))
        {
            let (nk, nw) = self.text.get_fit(fs, &app.name, F_NAME, TW - 14.0);
            let col = if is_sel { TEXT.mix(theme::accent().0, 0.16) } else { SOFT };
            self.text_at(out, nk, cx - nw * 0.5, ly, col.fade(la), t.clip);
        }
        if let Some(rp) = t.hit
            && let Some(hr) = intersect(Rect::new(t.x + 2.0, t.y + 2.0, TW - 4.0, ROW_H - 4.0), t.clip)
        {
            self.hits.push((hr, rp));
        }
    }

    fn search_glass(&self, out: &mut DrawList, cx: f32, cy: f32, k: f32, c: Rgba) {
        let sc = self.scale;
        let (ox, oy) = (cx - 1.5 * k, cy - 1.5 * k);
        out.shapes.push(paint::circle(sc, ox, oy, 6.5 * k).fill(Rgba::TRANSPARENT).rim(1.9 * k.sqrt(), c, c).done());
        let n = (6.0 * k).ceil() as usize;
        for i in 0..n {
            let t = (4.6 + i as f32 * 0.85 * 6.0 / n as f32) * k;
            out.shapes.push(paint::circle(sc, ox + t, oy + t, 1.05 * k.sqrt()).fill(c).done());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn keycap(&mut self, fs: &mut FontSystem, out: &mut DrawList, cap: &str, label: &str, right: f32, cy: f32, a: f32, clip: Rect) -> f32 {
        let (lk, lw) = self.text.get(fs, label, F_SMALL);
        let (ck, cw) = self.text.get(fs, cap, F_KEY);
        let x = right - lw;
        let llh = self.text.line_h(F_SMALL);
        self.text_at(out, lk, x, cy - llh * 0.5, MUTED.fade(a), clip);
        let kw = cw + 12.0;
        let kr = Rect::new(x - 7.0 - kw, cy - 10.0, kw, 20.0);
        let Some(kc) = intersect(kr, clip) else { return kr.x };
        out.shapes.push(
            paint::rect(self.scale, kc, 6.0).fill(Rgba::hex(0xffffff, 0.07)).alpha(a).done(),
        );
        let clh = self.text.line_h(F_KEY);
        self.text_at(out, ck, kr.x + 6.0, cy - clh * 0.5 - 0.5, SOFT.fade(a), clip);
        kr.x
    }

    fn file_icon(&self, out: &mut DrawList, k: kind::Kind, x: f32, y: f32, size: f32, c: Rgba) {
        if c.3 > 0.003 {
            let s = self.scale;
            out.icons.push(IconDraw { id: kind::icon(k, &self.iconset), x: (x * s).round(), y: (y * s).round(), size: size * s, color: c });
        }
    }

    /// Text with its matched characters overdrawn in `hi`.
    #[allow(clippy::too_many_arguments)]
    fn marked(&mut self, out: &mut DrawList, key: u64, spans: &[files::fuzzy::Span], x: f32, y: f32, base: Rgba, hi: Rgba, clip: Rect) {
        self.text_at(out, key, x, y, base, clip);
        if spans.is_empty() {
            return;
        }
        let mut ext = std::mem::take(&mut self.extents);
        ext.clear();
        self.text.extents(key, spans, &mut ext);
        for &(a, b) in &ext {
            if let Some(c) = intersect(Rect::new(x + a - 0.5, clip.y, b - a + 1.0, clip.h), clip) {
                self.text_at(out, key, x, y, hi, c);
            }
        }
        self.extents = ext;
    }

    /// The section under the apps: only rows on screen are drawn.
    fn draw_files(&mut self, fs: &mut FontSystem, out: &mut DrawList, gx: f32, gy: f32, clip: Rect, a: f32) {
        let fa = self.files_a.get().clamp(0.0, 1.0) * a;
        if fa <= 0.003 || self.files.is_empty() || clip.h <= 0.5 {
            return;
        }
        let top = gy + self.files_y.get() - self.scroll.get();
        let hy = top + FILE_HEAD * 0.5 + 3.0;
        let ha = fa * self.edge_a(hy - 7.0, hy + 7.0);
        let (k, _) = self.text.get(fs, "FILES", F_SECTION);
        let lh = self.text.line_h(F_SECTION);
        self.text_at(out, k, gx + 16.0, hy - lh * 0.5, MUTED.fade(ha), clip);
        let accent = theme::accent().0;
        let right = gx + BW - 2.0 * PAD - 16.0;
        let name_x = gx + 16.0 + FILE_ICON + 12.0;
        let width = right - name_x;
        let apps = self.res.len();
        let files = std::mem::take(&mut self.files);
        for (i, f) in files.iter().enumerate() {
            let y = top + FILE_HEAD + i as f32 * FILE_ROW;
            if y + FILE_ROW < clip.y {
                continue;
            }
            if y > clip.y + clip.h {
                break;
            }
            let row = Rect::new(gx, y, BW - 2.0 * PAD, FILE_ROW);
            if let Some(hr) = intersect(row.inflate(-1.0), clip) {
                self.hits.push((hr, apps + i));
            }
            let ra = fa * self.edge_a(y, y + FILE_ROW);
            if ra <= 0.003 {
                continue;
            }
            let cy = y + FILE_ROW * 0.5;
            let is_sel = self.sel == apps + i;
            self.file_icon(out, f.kind, gx + 16.0, cy - FILE_ICON * 0.5, FILE_ICON, (if f.is_dir { SOFT } else { MUTED }).fade(ra));
            let (nk, nw) = self.text.get_fit(fs, &f.name, F_FILE, width * 0.62);
            let nlh = self.text.line_h(F_FILE);
            let base = if is_sel { TEXT } else { SOFT };
            self.marked(out, nk, &f.name_hl, name_x, cy - nlh * 0.5, base.fade(ra), TEXT.mix(accent, 0.55).fade(ra), clip);
            // The folder, right aligned, muted, and whatever matched in it.
            let room = width - nw - 24.0;
            if room > 30.0 {
                let (dk, dw) = self.text.get_fit(fs, &f.dir, F_SMALL, room);
                let dlh = self.text.line_h(F_SMALL);
                let spans: Vec<files::fuzzy::Span> = f.dir_hl.iter().map(|s| (s.0 + 2, s.1 + 2)).collect();
                self.marked(out, dk, &spans, right - dw, cy - dlh * 0.5 + 0.5, META_DIR.fade(ra), TEXT.mix(accent, 0.4).fade(ra), clip);
            }
        }
        self.files = files;
    }

    /// Width of the footer line for one app (for the swap wipe).
    fn foot_span(&mut self, fs: &mut FontSystem, idx: Option<u32>, maxw: f32) -> f32 {
        if let Some(hit) = idx.and_then(|i| i.checked_sub(FILE_FOOT)).and_then(|i| self.files.get(i as usize)) {
            let (name, dir) = (hit.name.clone(), hit.dir.clone());
            let (_, nw) = self.text.get_fit(fs, &name, F_LABEL, maxw * 0.55);
            let room = maxw - 32.0 - nw - 12.0;
            let (_, cw) = self.text.get_fit(fs, &dir, F_SMALL, room.max(40.0));
            return 32.0 + nw + 12.0 + cw;
        }
        let Some(app) = idx.and_then(|i| self.cat.as_ref()?.apps.get(i as usize)) else { return 0.0 };
        let (_, nw) = self.text.get_fit(fs, &app.name, F_LABEL, maxw * 0.55);
        let sub = if !app.comment.is_empty() { app.comment.as_str() } else { app.generic.as_str() };
        let room = maxw - 32.0 - nw - 12.0;
        if !sub.is_empty() && room > 40.0 {
            let (_, cw) = self.text.get_fit(fs, sub, F_SMALL, room);
            32.0 + nw + 12.0 + cw
        } else {
            32.0 + nw
        }
    }

    /// Footer line for one app: icon, name, comment.
    #[allow(clippy::too_many_arguments)]
    fn foot_line(&mut self, fs: &mut FontSystem, out: &mut DrawList, idx: u32, x: f32, cy: f32, maxw: f32, a: f32, clip: Rect) {
        if a <= 0.003 || clip.w <= 0.5 {
            return;
        }
        if let Some(i) = idx.checked_sub(FILE_FOOT) {
            let Some(hit) = self.files.get(i as usize) else { return };
            let (name, dir, k) = (hit.name.clone(), hit.dir.clone(), hit.kind);
            self.file_icon(out, k, x + 2.0, cy - 9.0, 18.0, SOFT.fade(a));
            let tx = x + 32.0;
            let (nk, nw) = self.text.get_fit(fs, &name, F_LABEL, maxw * 0.55);
            let lh = self.text.line_h(F_LABEL);
            self.text_at(out, nk, tx, cy - lh * 0.5, TEXT.fade(a), clip);
            let room = maxw - 32.0 - nw - 12.0;
            if room > 40.0 {
                let (ck, _) = self.text.get_fit(fs, &dir, F_SMALL, room);
                let slh = self.text.line_h(F_SMALL);
                self.text_at(out, ck, tx + nw + 12.0, cy - slh * 0.5 + 0.5, MUTED.fade(a), clip);
            }
            return;
        }
        let Some(cat) = self.cat.clone() else { return };
        let Some(app) = cat.apps.get(idx as usize) else { return };
        self.app_icon(fs, out, idx, Rect::new(x, cy - 11.0, 22.0, 22.0), clip, a);
        let tx = x + 32.0;
        let (nk, nw) = self.text.get_fit(fs, &app.name, F_LABEL, maxw * 0.55);
        let lh = self.text.line_h(F_LABEL);
        self.text_at(out, nk, tx, cy - lh * 0.5, TEXT.fade(a), clip);
        let sub = if !app.comment.is_empty() { app.comment.as_str() } else { app.generic.as_str() };
        let room = maxw - 32.0 - nw - 12.0;
        if !sub.is_empty() && room > 40.0 {
            let (ck, _) = self.text.get_fit(fs, sub, F_SMALL, room);
            let slh = self.text.line_h(F_SMALL);
            self.text_at(out, ck, tx + nw + 12.0, cy - slh * 0.5 + 0.5, MUTED.fade(a), clip);
        }
    }
}

/// Contract everything drawn since `from` out of `home` into `r` (both
/// logical), fading shapes and images by `a` and text by `ta`.
fn contract(out: &mut DrawList, from: (usize, usize, usize), home: Rect, r: Rect, sc: f32, a: f32, ta: f32) {
    let (kx, ky) = (r.w / home.w, r.h / home.h);
    let k = kx.min(ky);
    let map = |x: f32, y: f32| ((r.x + (x / sc - home.x) * kx) * sc, (r.y + (y / sc - home.y) * ky) * sc);
    for s in &mut out.shapes[from.0..] {
        let (cx, cy) = map(s.rect[0] + s.rect[2] * 0.5, s.rect[1] + s.rect[3] * 0.5);
        let (w, h) = (s.rect[2] * k, s.rect[3] * k);
        s.rect = [cx - w * 0.5, cy - h * 0.5, w, h];
        s.geom[0] *= k;
        s.geom[2] *= k;
        s.geom[3] *= k;
        s.grad[3] *= k;
        if s.spot[3] > 0.0 {
            let (x, y) = map(s.spot[0], s.spot[1]);
            s.spot = [x, y, s.spot[2] * k, s.spot[3] * a];
        }
        for c in [&mut s.fill_a, &mut s.fill_b, &mut s.rim_a, &mut s.rim_b, &mut s.shadow, &mut s.bloom] {
            c[3] *= a;
        }
        s.prism[0] *= a;
    }
    for i in &mut out.images[from.1..] {
        let (cx, cy) = map(i.rect[0] + i.rect[2] * 0.5, i.rect[1] + i.rect[3] * 0.5);
        let (w, h) = (i.rect[2] * k, i.rect[3] * k);
        i.rect = [cx - w * 0.5, cy - h * 0.5, w, h];
        i.radius *= k;
        if i.reveal[2] >= 0.0 {
            let (x, y) = map(i.reveal[0], i.reveal[1]);
            i.reveal = [x, y, i.reveal[2] * k, i.reveal[3] * k];
        }
        i.alpha *= a;
    }
    for t in &mut out.texts[from.2..] {
        let (x, y) = map(t.x, t.y);
        t.x = x.round();
        t.y = y.round();
        let (x0, y0) = map(t.clip[0] as f32, t.clip[1] as f32);
        let (x1, y1) = map(t.clip[2] as f32, t.clip[3] as f32);
        t.clip = [x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32];
        t.color.3 *= ta;
    }
}

#[allow(clippy::type_complexity)]
fn icon_worker(rx: mpsc::Receiver<IconReq>, out: calloop::channel::Sender<Fact>, session: u64, epoch: u32, cat: Arc<Catalog>, alive: Arc<AtomicBool>) {
    // Every message wakes the whole host (bar frames included), so batch:
    // flush when the queue drains or a batch has waited ~20 ms, so one
    // burst of requests costs one or two messages.
    const HOLD: Duration = Duration::from_millis(20);
    let mut batch: Vec<(u32, Option<ImageData>)> = Vec::new();
    let mut since: Option<Instant> = None;
    let flush = |b: &mut Vec<(u32, Option<ImageData>)>, since: &mut Option<Instant>| -> bool {
        *since = None;
        if b.is_empty() {
            return true;
        }
        let v = std::mem::take(b);
        out.send(Fact::Custom(Arc::new(Msg::Icons(session, epoch, Mutex::new(v))))).is_ok()
    };
    let mut paths: HashMap<String, Option<PathBuf>> = HashMap::new();
    loop {
        let req = if batch.is_empty() {
            match rx.recv() {
                Ok(r) => r,
                Err(_) => break,
            }
        } else {
            match rx.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) => {
                    if !flush(&mut batch, &mut since) {
                        break;
                    }
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        };
        if !alive.load(Ordering::Relaxed) {
            break;
        }
        let path = paths.entry(req.name.clone()).or_insert_with(|| cat.icon_path(&req.name)).clone();
        let img = path.and_then(|p| crate::appicons::load(&p, req.px));
        batch.push((req.idx, img));
        let t0 = *since.get_or_insert_with(Instant::now);
        if t0.elapsed() >= HOLD && !flush(&mut batch, &mut since) {
            break;
        }
    }
    if alive.load(Ordering::Relaxed) {
        flush(&mut batch, &mut since);
    }
}

impl WinScene for Launcher {
    fn size(&self) -> (f32, f32) {
        (SW, SH)
    }
    fn placement(&self) -> Placement {
        Placement::Top(TOP_Y)
    }
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
        let Fact::Custom(any) = f else { return };
        if let Some(h) = any.downcast_ref::<index::Hits>() {
            if h.owner == self.session && h.generation == self.files_gen {
                let hits = h.hits.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
                self.set_files(hits, h.more);
            }
            return;
        }
        if any.downcast_ref::<index::Refreshed>().is_some() {
            // A walk finished: what is on screen may have new matches.
            if !self.query.trim().is_empty() && !self.chrome.closing {
                self.ask_files();
            }
            return;
        }
        let Some(msg) = any.downcast_ref::<Msg>() else { return };
        match msg {
            Msg::Catalog(s, c) if *s == self.session => {
                if let Some(c) = c.lock().ok().and_then(|mut g| g.take()) {
                    if self.launch.is_some() || self.chrome.closing {
                        // Indices are in use by the fold: keep it for later.
                        self.pending_cat = Some(c);
                    } else {
                        let first = self.cat.is_none();
                        self.set_catalog(c, first);
                    }
                }
            }
            Msg::Icons(s, g, v) if *s == self.session => {
                let v = v.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
                if *g != self.epoch {
                    self.stale += v.len() as u32;
                    return;
                }
                for (idx, img) in v {
                    if !matches!(self.icons.get(&idx), Some(Tex::Pending)) {
                        continue;
                    }
                    match img {
                        Some(img) => {
                            let key = self.key_of(idx);
                            self.uploads.push((key, img));
                            let mut fade = Spring::new(0.0, FADE);
                            fade.set(1.0);
                            self.icons.insert(idx, Tex::Ready { key, fade });
                        }
                        None => {
                            self.icons.insert(idx, Tex::Missing);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn set_scale(&mut self, s: f32) {
        if s != self.scale {
            self.scale = s;
            self.text.set_scale(s);
            // Icons were rasterised for the old scale, and any in flight
            // still are.
            self.stop_icons();
            self.free_textures();
            self.epoch = self.epoch.wrapping_add(1);
        }
    }

    fn step(&mut self, dt: f32) -> bool {
        self.chrome.wake_at = None;
        let mut m = self.chrome.step(dt);
        let t = self.chrome.time;
        if self.last_input.is_nan() {
            // First frame: the host has synced the clock by now.
            self.last_input = t;
        }
        // The fold lands on the icon: its burst starts.
        if let Some(l) = &self.launch
            && self.boom.target == 0.0
        {
            let r = self.chrome.morph();
            if r.w < l.rect.w * 1.7 && r.h < l.rect.h * 1.7 {
                self.boom.set(1.0);
            }
            m = true;
        }
        // Folded and invisible: stop the tail of the spring right there
        // (a launch waits for its burst to play out).
        let c = &mut self.chrome;
        if c.closing
            && c.open_w.get().abs() < 0.02
            && c.open_h.get().abs() < 0.02
            && (self.launch.is_none() || self.boom.get() > 0.97)
        {
            c.open_w.snap(0.0);
            c.open_h.snap(0.0);
        }
        for it in &mut self.items {
            if it.delay > 0.0 {
                it.delay -= dt;
                if it.delay <= 0.0 {
                    it.vis.set(it.want);
                }
                m = true;
            }
            m |= it.x.step(dt) | it.y.step(dt) | it.vis.step(dt);
            if it.ghost.is_some() {
                m |= it.gv.step(dt);
                if !self.wiping && it.gv.at_rest() {
                    it.ghost = None;
                }
            }
        }
        if self.wiping {
            self.wipe.step(dt);
            // Done once the edge is past the bottom (the spring overshoots).
            if self.wipe.get() >= 1.0 || self.wipe.at_rest() {
                self.finish_wipe();
            }
            m = true;
        }
        for tx in self.icons.values_mut() {
            if let Tex::Ready { fade, .. } = tx {
                m |= fade.step(dt);
            }
        }
        let max = self.max_scroll();
        if self.scroll.target > max {
            self.scroll.set(max);
        }
        // Wheel stopped: come to rest on a whole row.
        if let Some(w) = self.wheel_at {
            if t >= w + WHEEL_REST {
                self.wheel_at = None;
                let snapped = self.rest_on(self.scroll.target).clamp(0.0, max);
                self.scroll.set(snapped);
            } else {
                self.chrome.wake(w + WHEEL_REST);
            }
        }
        for s in [
            &mut self.scroll,
            &mut self.head.0,
            &mut self.head.1,
            &mut self.tail.0,
            &mut self.tail.1,
            &mut self.hl,
            &mut self.caret,
            &mut self.holder,
            &mut self.count_t,
            &mut self.foot_t,
            &mut self.empty_t,
            &mut self.press,
            &mut self.boom,
            &mut self.files_y,
            &mut self.files_a,
            &mut self.blob_w,
            &mut self.blob_h,
        ] {
            m |= s.step(dt);
        }
        if self.ghost.is_some() && self.caret.at_rest() {
            self.ghost = None;
        }
        // Caret blink: two wake-ups a second for a while, then solid.
        if !self.chrome.closing {
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
            // Last frame before the surface goes: hand every texture back.
            self.free_textures();
            out.frees.append(&mut self.frees);
            self.text.end_frame();
            return;
        }
        out.frees.append(&mut self.frees);
        out.uploads.append(&mut self.uploads);

        let r = self.chrome.morph();
        self.chrome.body = r;
        let (ow, oh) = (self.chrome.open_w.get(), self.chrome.open_h.get());
        let open = ow.min(oh);
        let closing = self.chrome.closing;
        let launch = self.launch.as_ref().map(|l| (l.idx, l.rect));
        // While folding into a launched app the layout stays put and is
        // contracted into the icon afterwards.
        let lay = if launch.is_some() { self.chrome.home } else { r };
        let o = if launch.is_some() { 1.0 } else { oh.clamp(0.0, 1.0) };
        // The body: a search pill from the very first frame. Closing, it
        // stays solid until it is back to the pill (or on the icon), then
        // goes.
        let body_a = if let Some((_, lr)) = launch {
            let s = lr.inflate(4.0);
            smooth(((r.w - s.w) / s.w).max((r.h - s.h) / s.h) / 0.3)
        } else if closing {
            smooth((ow.max(oh) - 0.02) / 0.1)
        } else {
            0.9 + 0.1 * smooth(open / 0.2)
        };
        let rad = lerp(self.start_r, BODY_R, oh.clamp(0.0, 1.0));
        out.shapes.push(
            paint::ink(sc, r, rad)
                .shadow(SHADOW, 10.0 + 16.0 * oh.clamp(0.0, 1.0), 4.0 + 8.0 * oh.clamp(0.0, 1.0))
                .alpha(body_a)
                .done(),
        );
        out.glass(r, rad, body_a);
        let marks = (out.shapes.len(), out.images.len(), out.texts.len());
        let inner = lay.inflate(-1.0);
        // Field content (glass, text) is the pill: it is there throughout.
        let ca = if launch.is_some() { 1.0 } else { body_a };
        // Side content (count, grid) goes first on close.
        let grid_a = if launch.is_some() || !closing { 1.0 } else { smooth((oh - 0.4) / 0.45) };
        let side_a = if launch.is_some() {
            1.0
        } else if closing {
            grid_a
        } else {
            smooth((ow - 0.3) / 0.4)
        };
        // ── grid ────────────────────────────────────────────────────────
        let (gx, gy) = self.grid_origin(lay);
        let vp = Rect::new(gx, gy, BW - 2.0 * PAD, VIEW_H);
        let view = intersect(vp, inner.inflate(-1.0)).unwrap_or_default();
        self.view = view;
        self.band = (vp.y, vp.y + vp.h);
        let scroll = self.scroll.get();
        // A wipe: the new grid above the edge, the old one below it.
        let we = if self.wiping { vp.y - 8.0 + self.wipe.get().clamp(0.0, 1.0) * (VIEW_H + 16.0) } else { FAR };
        let new_clip = rows(view, -FAR, we);
        let old_clip = rows(view, we, FAR);
        let launched = launch.map(|l| l.0);

        // Selection: one blob between a bouncy head and a lagging tail.
        let hl_raw = self.hl.get().clamp(0.0, 1.0);
        let hl = hl_raw * grid_a;
        if hl > 0.003 && self.total() > 0 {
            let (hx, hy, tx, ty) = (self.head.0.get(), self.head.1.get(), self.tail.0.get(), self.tail.1.get());
            let (bw, bh) = (self.blob_w.get(), self.blob_h.get());
            // 0 on a tile, 1 on a file row.
            let wide = smooth((bw - TW) / (BW - 2.0 * PAD - TW));
            let (lx, ux) = (hx.min(tx), hx.max(tx));
            let (ly, uy) = (hy.min(ty), hy.max(ty));
            let stretch = ((ux - lx) / TW + (uy - ly) / ROW_H).min(1.0);
            let sq = 3.0 * stretch;
            let mut blob = Rect::new(gx + lx + 4.0, gy + ly - scroll + 3.0 + sq, ux - lx + bw - 8.0, uy - ly + bh - 6.0 - 2.0 * sq);
            // Pops in when it lands on a new top result.
            let k = 0.9 + 0.1 * hl_raw;
            blob = Rect::new(blob.x + blob.w * (1.0 - k) * 0.5, blob.y + blob.h * (1.0 - k) * 0.5, blob.w * k, blob.h * k);
            // Past the viewport edges it fades instead of being squashed.
            let over = (vp.y - blob.y).max(blob.y + blob.h - (vp.y + vp.h)).max(0.0);
            let ba = hl * (1.0 - smooth(over / 22.0));
            if ba > 0.003
                && let Some(b) = intersect(blob, new_clip)
            {
                let p = self.press.get().clamp(0.0, 1.0);
                // Faint white, edged by the prism hairline; an iridescent
                // pill under the name says "this one".
                out.shapes.push(paint::rect(sc, b, lerp(16.0, 12.0, wide)).fill(SEL.fade(1.0 + 0.6 * p)).prism(paint::HAIR, 1.5).alpha(ba).done());
                let pw = (b.w * 0.22).clamp(14.0, 22.0);
                let pill = Rect::new(b.x + (b.w - pw) * 0.5, b.y + b.h - 7.0, pw, 3.0);
                if let Some(pr) = intersect(pill, new_clip) {
                    out.shapes.push(paint::rect(sc, pr, 1.5).on_fill(1.0, 0.0, 0.0).alpha(ba * (1.0 - wide)).done());
                }
            }
        }
        let names_a = grid_a;
        let mut want_icons = std::mem::take(&mut self.want);
        want_icons.clear();
        if self.cat.is_some() {
            let vis_rows = |y: f32| y + ROW_H > view.y - 2.0 && y < view.y + view.h + 2.0;
            // Leavers and far movers' old selves first (beneath)...
            for i in 0..self.items.len() {
                if Some(i as u32) == launched {
                    continue;
                }
                let it = &self.items[i];
                let (ghost, gv, live, v) = (it.ghost, it.gv.get(), it.live, it.vis.get());
                let (x, y) = (it.x.get(), it.y.get());
                if let Some((ox, oy)) = ghost
                    && gv > 0.003
                    && vis_rows(gy + oy - scroll)
                {
                    let (clip, label) = if self.wiping { (old_clip, 1.0) } else { (view, self.slot_free(ox, oy)) };
                    let args = TileArgs { x: gx + ox, y: gy + oy - scroll, v: gv, a: names_a, label, shrink: 1.0, clip, hit: None };
                    self.tile(fs, out, i as u32, args);
                }
                if !live && v > 0.003 && vis_rows(gy + y - scroll) {
                    let label = self.slot_free(x, y);
                    let args = TileArgs { x: gx + x, y: gy + y - scroll, v, a: names_a, label, shrink: 1.0, clip: view, hit: None };
                    self.tile(fs, out, i as u32, args);
                }
            }
            // ...then the results, which take hits; movers furthest from
            // home first, so the one arriving sits on top.
            let mut order = std::mem::take(&mut self.order);
            order.clear();
            for rp in 0..self.res.len() {
                let i = self.res[rp];
                if Some(i) == launched {
                    continue;
                }
                let it = &self.items[i as usize];
                if !vis_rows(gy + it.y.get() - scroll) {
                    continue;
                }
                // Warm icons even for tiles still waiting for their cascade.
                want_icons.push(i);
                if it.vis.get() > 0.003 {
                    order.push(((it.x.target - it.x.get()).hypot(it.y.target - it.y.get()), rp));
                }
            }
            if order.iter().any(|o| o.0 > 0.5) {
                order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            for &(dist, rp) in &order {
                let i = self.res[rp];
                let it = &self.items[i as usize];
                let args = TileArgs {
                    x: gx + it.x.get(),
                    y: gy + it.y.get() - scroll,
                    v: it.vis.get(),
                    a: names_a,
                    label: 1.0 - smooth(dist / (0.35 * TW)),
                    shrink: 1.0 - 0.1 * smooth(dist / (0.5 * TW)),
                    clip: new_clip,
                    hit: Some(rp),
                };
                self.tile(fs, out, i, args);
            }
            self.order = order;
        }
        for &i in &want_icons {
            self.request_icon(i);
        }
        self.want = want_icons;
        if launch.is_none() {
            self.draw_files(fs, out, gx, gy, new_clip, names_a);
        }
        // The wipe's leading edge: a faint accent light sweeping down.
        if self.wiping && we < vp.y + vp.h {
            let w = self.wipe.get().clamp(0.0, 1.0);
            let la = (w * std::f32::consts::PI).sin() * 0.8 * grid_a;
            if let Some(e) = intersect(Rect::new(vp.x + 24.0, we - 0.75, vp.w - 48.0, 1.5), view) {
                out.shapes.push(paint::rect(sc, e, 0.75).on_fill(1.0, 0.0, 0.0).alpha(la * 0.28).done());
            }
        }

        // Nothing matched: say so where the tiles were.
        let ea = self.empty_t.get().clamp(0.0, 1.0) * grid_a;
        if ea > 0.003 {
            self.search_glass(out, vp.x + vp.w * 0.5, vp.y + VIEW_H * 0.44 - 44.0, 2.2, MUTED.fade(ea * 0.6));
            let msg = format!("No apps or files match \u{201c}{}\u{201d}", self.query.trim());
            let (k, w) = self.text.get_fit(fs, &msg, F_LABEL, vp.w - 80.0);
            let lh = self.text.line_h(F_LABEL);
            self.text_at(out, k, vp.x + (vp.w - w) * 0.5, vp.y + VIEW_H * 0.44 - lh * 0.5, SOFT.fade(ea), view);
            let (k2, w2) = self.text.get(fs, "Try fewer letters, or a different name", F_SMALL);
            let lh2 = self.text.line_h(F_SMALL);
            self.text_at(out, k2, vp.x + (vp.w - w2) * 0.5, vp.y + VIEW_H * 0.44 + 24.0 - lh2 * 0.5, MUTED.fade(ea), view);
        }

        // ── search field (drawn over the grid's top edge) ───────────────
        // It is the pill the window grows out of and folds back into: at
        // o = 0 it is the whole body, then it separates as the body opens.
        let field = Rect::new(lay.x + PAD * o, lay.y + PAD * o, lay.w - 2.0 * PAD * o, FIELD_H);
        self.field = field;
        out.shapes.push(
            paint::rect(sc, field, lerp(self.start_r, BODY_R - PAD, o)).fill(FIELD).alpha(ca).done(),
        );
        let fcy = field.y + FIELD_H * 0.5;
        let fclip = intersect(field.inflate(-2.0), inner).unwrap_or_default();
        if ca > 0.003 {
            self.search_glass(out, field.x + 24.0, fcy, 1.0, SOFT.fade(ca));
            let tx = field.x + 46.0;
            let (qk, qw) = self.text.get(fs, &self.query, F_QUERY);
            let empty = self.query.is_empty();
            let ct = if empty { -5.0 } else { qw + 2.0 };
            if self.caret.target != ct {
                self.caret.set(ct);
            }
            let cx = tx + self.caret.get();
            let lh = self.text.line_h(F_QUERY);
            let ty = fcy - lh * 0.5;
            // The placeholder is pushed away by the caret, never overlaps.
            let ha = ca * self.holder.get().clamp(0.0, 1.0);
            if ha > 0.003 {
                let c = cols(fclip, cx + 3.0, FAR);
                self.label(fs, out, "Search apps", F_QUERY, tx + 2.0, fcy, MUTED.fade(ha * 0.9), c);
            }
            // The caret wipes typed text in, and wipes deleted text away.
            let reveal = Rect::new(tx - 2.0, field.y, (cx + 0.5 - tx + 2.0).max(0.0), FIELD_H);
            if !empty && let Some(c) = intersect(reveal, fclip) {
                self.text_at(out, qk, tx, ty, TEXT.fade(ca), c);
            }
            if let Some(g) = self.ghost.clone() {
                let (gk, _) = self.text.get(fs, &g, F_QUERY);
                let wipe = Rect::new(tx + qw, field.y, (cx - tx - qw).max(0.0), FIELD_H);
                if let Some(c) = intersect(wipe, fclip) {
                    self.text_at(out, gk, tx, ty, TEXT.fade(ca * 0.45), c);
                }
            }
            // Caret: solid while typing, then a calm blink (on wake-ups).
            let idle = self.idle();
            let on = idle < 0.6 || idle >= BLINK_FOR || (((idle - 0.6) / 0.5).floor() as i64) % 2 == 1;
            if on && !closing && launch.is_none() {
                out.shapes.push(
                    paint::rect(sc, Rect::new(cx, fcy - 11.0, 2.0, 22.0), 1.0).on_fill(0.0, 1.0, 0.0).alpha(ca).done(),
                );
            }
            // Count: the new one wipes in from the right over the old.
            let ct = self.count_t.get().clamp(0.0, 1.0);
            let right = field.x + field.w - 18.0;
            let sa = ca * side_a;
            if sa > 0.003 {
                let (ok, ow_) = self.text.get(fs, &self.count.0, F_SMALL);
                let (nk, nw) = self.text.get(fs, &self.count.1, F_SMALL);
                let slh = self.text.line_h(F_SMALL);
                let edge = right + 4.0 - ct * (ow_.max(nw) + 8.0);
                if ct < 1.0 && !self.count.0.is_empty() {
                    self.text_at(out, ok, right - ow_, fcy - slh * 0.5, MUTED.fade(sa), cols(fclip, -FAR, edge));
                }
                if !self.count.1.is_empty() {
                    self.text_at(out, nk, right - nw, fcy - slh * 0.5, MUTED.fade(sa), cols(fclip, edge, FAR));
                }
            }
        }

        // ── footer ──────────────────────────────────────────────────────
        let fy = lay.y + FOOT_Y;
        let foot_a = ca * smooth((lay.h - BH + 40.0) / 30.0);
        if foot_a > 0.003 {
            let hair = Rect::new(lay.x + PAD + 4.0, fy + 1.0, lay.w - 2.0 * PAD - 8.0, 1.0);
            if let Some(h) = intersect(hair, inner) {
                paint::divider(&mut out.shapes, sc, h.x, h.y, h.w, foot_a);
            }
            let cy = fy + (BH - FOOT_Y) * 0.5 + 1.0;
            let clip = intersect(Rect::new(lay.x, fy + 2.0, lay.w, BH - FOOT_Y - 2.0), inner).unwrap_or_default();
            let right = lay.x + lay.w - PAD - 10.0;
            let k1 = self.keycap(fs, out, "Esc", "Close", right, cy, foot_a, clip);
            let k2 = self.keycap(fs, out, "Enter", "Open", k1 - 16.0, cy, foot_a, clip);
            let k2 = if self.is_file(self.sel) { self.keycap(fs, out, "Ctrl+Enter", "Default app", k2 - 16.0, cy, foot_a, clip) } else { k2 };
            let maxw = k2 - 24.0 - (lay.x + PAD + 10.0);
            let lx = lay.x + PAD + 10.0;
            // Selection changes wipe the new line in over the old one.
            let ft = self.foot_t.get().clamp(0.0, 1.0);
            let (f0, f1) = self.foot;
            let span = self.foot_span(fs, f0, maxw).max(self.foot_span(fs, f1, maxw)) + 8.0;
            let edge = lx - 4.0 + ft * (span + 4.0);
            if let Some(p) = f0
                && ft < 1.0
            {
                self.foot_line(fs, out, p, lx, cy, maxw, foot_a, cols(clip, edge, FAR));
            }
            if let Some(c) = f1 {
                self.foot_line(fs, out, c, lx, cy, maxw, foot_a, cols(clip, -FAR, edge));
            }
        }

        // ── launching: content contracts into the icon, which bursts ────
        if let Some((idx, lr)) = launch {
            let ka = smooth((open - 0.08) / 0.5);
            let kt = smooth((open - 0.5) / 0.35);
            contract(out, marks, lay, r, sc, ka, kt);
            let b = self.boom.get().clamp(0.0, 1.0);
            let fold = 1.0 - open.clamp(0.0, 1.0);
            let (cx, cy) = (lr.x + lr.w * 0.5, lr.y + lr.h * 0.5);
            // A thin prism ring expands off the icon (the accent's under
            // another accent); no glow...
            if b > 0.003 && b < 0.999 {
                let rr = ICON * 0.5 * (0.9 + 0.95 * b);
                let ra = (1.0 - b).powf(0.6) * smooth(b / 0.08);
                let ring = paint::circle(sc, cx, cy, rr).fill(Rgba::TRANSPARENT);
                let ring = if theme::prism() {
                    ring.prism(2.0, 4.5)
                } else {
                    let (aa, ab) = theme::accent();
                    ring.rim(2.0, aa, ab)
                };
                out.shapes.push(ring.alpha(ra).done());
            }
            // ...as the icon swells to ~1.6x and goes.
            let k = (1.0 + 0.1 * smooth(fold)) * (1.0 + 0.45 * b);
            let sz = ICON * k;
            let ia = 1.0 - smooth((b - 0.15) / 0.85);
            let ir = Rect::new(cx - sz * 0.5, cy - sz * 0.5, sz, sz);
            let all = Rect::new(-1e4, -1e4, 2e4, 2e4);
            self.app_icon(fs, out, idx, ir, all, ia);
        }

        self.text.end_frame();
        let (dx, dy) = self.chrome.translate(out, sc);
        for (h, _) in &mut self.hits {
            h.x += dx;
            h.y += dy;
        }
        self.field.x += dx;
        self.field.y += dy;
        self.view.x += dx;
        self.view.y += dy;
    }

    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pointer = Some((x, y));
        // The pointer often rests where the window opens: hover takes the
        // selection only after it has actually moved.
        let (ax, ay) = *self.hover_from.get_or_insert((x, y));
        if !self.hover_live && (x - ax).hypot(y - ay) > HOVER_SLOP {
            self.hover_live = true;
        }
        if self.hover_live
            && !self.chrome.grabbing
            && self.launch.is_none()
            && let Some(p) = self.tile_at(x, y)
            && p != self.sel
        {
            self.select(p, false);
        }
        None
    }

    fn pointer_leave(&mut self) {
        self.pointer = None;
        self.hover_from = None;
        self.hover_live = false;
    }

    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        if self.launch.is_some() {
            return None;
        }
        if let Some(p) = self.tile_at(x, y) {
            if p != self.sel {
                self.select(p, false);
            }
            self.pressed = Some(p);
            self.press.set(1.0);
        } else if self.chrome.body.contains(x, y) {
            self.chrome.start_grab();
        }
        None
    }

    fn release(&mut self) -> (Option<Action>, bool) {
        self.chrome.end_grab();
        self.press.set(0.0);
        if let Some(p) = self.pressed.take()
            && let Some((x, y)) = self.pointer
            && self.tile_at(x, y) == Some(p)
        {
            self.activate(p, false);
        }
        (None, false)
    }

    fn scroll(&mut self, dy: f32) -> Option<Action> {
        if self.launch.is_none() {
            let t = (self.scroll.target + dy * 64.0).clamp(0.0, self.max_scroll());
            self.scroll.set(t);
            self.wheel_at = Some(self.chrome.time);
        }
        None
    }

    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if self.launch.is_some() {
            return (None, false);
        }
        self.disarm_hover();
        // Up, down and paging work on the apps' grid and then, a row at a
        // time, on the files below it.
        let apps = self.res.len();
        let n = self.total();
        let s = self.sel;
        let in_files = self.is_file(s);
        let page = if in_files { FILE_PAGE } else { COLS * VIEW_ROWS };
        match k.sym {
            keys::ESCAPE => return (None, true),
            keys::RETURN | keys::KP_ENTER => self.activate(s, k.ctrl),
            keys::LEFT => self.select(s.saturating_sub(1), true),
            keys::RIGHT => self.select(s + 1, true),
            keys::UP => {
                if in_files {
                    if s > 0 {
                        self.select(s - 1, true)
                    }
                } else if s >= COLS {
                    self.select(s - COLS, true)
                }
            }
            keys::DOWN => {
                if in_files {
                    self.select(s + 1, true)
                } else if s + COLS < apps {
                    self.select(s + COLS, true)
                } else if s / COLS + 1 < apps.div_ceil(COLS) {
                    self.select(apps - 1, true)
                } else if n > apps {
                    self.select(apps, true)
                }
            }
            0xfe20 => self.select(if s == 0 { n.saturating_sub(1) } else { s - 1 }, true),
            keys::TAB if k.shift => self.select(if s == 0 { n.saturating_sub(1) } else { s - 1 }, true),
            keys::TAB => self.select(if s + 1 >= n { 0 } else { s + 1 }, true),
            keys::PAGE_DOWN => self.select((s + page).min(n.saturating_sub(1)), true),
            keys::PAGE_UP => self.select(s.saturating_sub(page), true),
            keys::HOME => self.select(0, true),
            keys::END => self.select(n.saturating_sub(1), true),
            keys::BACKSPACE => {
                if !self.query.is_empty() {
                    let old = self.query.clone();
                    if k.ctrl {
                        delete_word(&mut self.query);
                    } else {
                        self.query.pop();
                    }
                    self.typed(old);
                }
            }
            _ if k.ctrl => {
                let old = self.query.clone();
                match k.text.as_deref().map(|t| t.to_ascii_lowercase()).as_deref() {
                    Some("u") | Some("\u{15}") => self.query.clear(),
                    Some("w") | Some("\u{17}") => delete_word(&mut self.query),
                    _ => match k.sym {
                        0x75 | 0x55 => self.query.clear(),
                        0x77 | 0x57 => delete_word(&mut self.query),
                        _ => return (None, false),
                    },
                }
                if old != self.query {
                    self.typed(old);
                }
            }
            _ => {
                if let Some(t) = &k.text
                    && !k.alt
                    && !t.chars().any(char::is_control)
                    && !t.is_empty()
                    && self.query.chars().count() < 64
                {
                    let old = self.query.clone();
                    self.query.push_str(t);
                    self.typed(old);
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
            Some((x, y)) if self.tile_at(x, y).is_some() => Cursor::Pointer,
            Some((x, y)) if self.field.contains(x, y) => Cursor::Text,
            _ => Cursor::Default,
        }
    }

    fn close(&mut self) {
        let c = &mut self.chrome;
        if !c.closing {
            c.open_w.cfg = CLOSE_W;
            c.open_h.cfg = CLOSE_H;
        }
        c.close();
        self.stop_icons();
    }

    fn reopen(&mut self) {
        if self.launch.take().is_some() {
            // A fold into a launched app: come back from the pill instead.
            self.chrome.start = pill(PILL_W);
            self.chrome.home = Rect::new(M, M, BW, BH);
            self.start_r = PILL_R;
            self.boom.snap(0.0);
        }
        self.chrome.open_w.cfg = OPEN_W;
        self.chrome.open_h.cfg = OPEN_H;
        self.chrome.reopen();
        self.last_input = self.chrome.time;
        if !self.dry {
            index::refresh(self.fact_tx.clone());
        }
        if let Some(c) = self.pending_cat.take() {
            let first = self.cat.is_none();
            self.set_catalog(c, first);
        }
    }
}

impl Launcher {
    fn tile_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.view.contains(x, y) {
            return None;
        }
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, p)| *p)
    }

    /// Label alpha for an outgoing tile at `(x, y)`: gone as soon as the
    /// slot's new occupant shows, so two names never share a slot.
    fn slot_free(&self, x: f32, y: f32) -> f32 {
        let (c, r) = ((x / TW).round(), (y / ROW_H).round());
        if c < 0.0 || r < 0.0 || c >= COLS as f32 {
            return 1.0;
        }
        let p = r as usize * COLS + c as usize;
        match self.res.get(p).and_then(|&i| self.items.get(i as usize)) {
            Some(it) => 1.0 - smooth((it.vis.get() - 0.05) / 0.1),
            None => 1.0,
        }
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        // Dropped without its last frame (surface closed, output gone):
        // the next render anywhere frees what is still on the GPU.
        let mut keys = std::mem::take(&mut self.frees);
        keys.extend(self.icons.values().filter_map(|t| if let Tex::Ready { key, .. } = t { Some(*key) } else { None }));
        if !keys.is_empty() {
            gfx::free_later(keys);
        }
    }
}

fn delete_word(s: &mut String) {
    let t = s.trim_end().len();
    s.truncate(t);
    while let Some(c) = s.chars().last() {
        if c.is_whitespace() || c == '-' || c == '.' {
            break;
        }
        s.pop();
    }
}

// ── headless demo ───────────────────────────────────────────────────────

/// The file section, over the index `files::demo` installed.
pub fn demo_files(d: &mut crate::snapshot::Demo) {
    let _ = apps::fresh(None);
    let mut l = Launcher::new(&mut d.ctx(), true);
    l.chrome.time = 1234.5;
    let key = |sym: u32, text: Option<&str>| KeyInput { sym, text: text.map(String::from), ctrl: false, shift: false, alt: false };
    fn tick(d: &mut crate::snapshot::Demo, l: &mut Launcher, secs: f32) {
        const DT: f32 = 1.0 / 120.0;
        let mut list = DrawList::default();
        for _ in 0..(secs / DT).round() as u32 {
            l.draw(&mut d.gpu.fonts, &mut list);
            if !list.uploads.is_empty() || !list.frees.is_empty() {
                let only = DrawList { uploads: std::mem::take(&mut list.uploads), frees: std::mem::take(&mut list.frees), ..Default::default() };
                d.render(&only, l.text_buffers(), (4.0, 4.0), "_launcher_scratch");
            }
            l.step(DT);
        }
    }
    fn settle(d: &mut crate::snapshot::Demo, l: &mut Launcher, secs: f32) {
        for _ in 0..(secs / 0.05).round() as u32 {
            d.pump(l, 0.01);
            tick(d, l, 0.05);
        }
    }
    fn typed(d: &mut crate::snapshot::Demo, l: &mut Launcher, s: &str) {
        for c in s.chars() {
            let t = c.to_string();
            l.key(&KeyInput { sym: c as u32, text: Some(t), ctrl: false, shift: false, alt: false });
            tick(d, l, 0.03);
        }
    }
    settle(d, &mut l, 1.6);
    d.shoot(&mut l, "lf_browse");
    eprintln!("lf demo: empty query, file rows={} sel-blob-w={} (want 0, {TW})", l.files.len(), l.blob_w.get());
    let apps = l.res.len();
    let ctrl_bs = KeyInput { sym: keys::BACKSPACE, text: None, ctrl: true, shift: false, alt: false };
    // The real catalog: one Files, ours, however it is asked for.
    let cat = l.cat.clone();
    let named = |n: &str| cat.iter().flat_map(|c| c.apps.iter()).filter(|a| a.name == n).map(|a| a.id.clone()).collect::<Vec<_>>();
    let gnome = cat.iter().flat_map(|c| c.apps.iter()).filter(|a| a.id.to_lowercase().contains("nautilus")).count();
    eprintln!("lf demo: catalog apps named Files={:?} nautilus-ids={gnome}", named("Files"));
    for q in ["files", "file", "nautilus", "explorer", "finder"] {
        typed(d, &mut l, q);
        settle(d, &mut l, 0.15);
        let top = l.res.first().and_then(|&i| l.cat.as_ref()?.apps.get(i as usize)).map(|a| a.id.clone());
        eprintln!("lf demo: query {q:?} top app={top:?} apps={}", l.res.len());
        l.key(&ctrl_bs);
        settle(d, &mut l, 0.15);
    }
    typed(d, &mut l, "report");
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "lf_report");
    eprintln!("lf demo: report apps={} files={} more={} sel={}", l.res.len(), l.files.len(), l.files_more, l.sel);
    l.key(&ctrl_bs);
    settle(d, &mut l, 0.4);
    // Apps and files together; Down crosses from the grid to the rows, and
    // the blob morphs from a tile to a row.
    typed(d, &mut l, "files");
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "lf_mixed");
    eprintln!("lf demo: mixed apps={} files={} sel={} is_file={}", l.res.len(), l.files.len(), l.sel, l.is_file(l.sel));
    l.key(&key(keys::DOWN, None));
    for i in 0..4 {
        d.shoot(&mut l, &format!("lf_morph_{i}"));
        tick(d, &mut l, 0.05);
    }
    settle(d, &mut l, 0.6);
    d.shoot(&mut l, "lf_row");
    eprintln!("lf demo: down -> sel={} is_file={}", l.sel, l.is_file(l.sel));
    l.key(&key(keys::UP, None));
    settle(d, &mut l, 0.5);
    eprintln!("lf demo: up -> sel={} is_file={}", l.sel, l.is_file(l.sel));
    // Back to no query: exactly the browse grid again.
    l.key(&ctrl_bs);
    settle(d, &mut l, 1.0);
    d.shoot(&mut l, "lf_cleared");
    eprintln!("lf demo: cleared files={} apps {apps}->{} blob-w={} moving={}", l.files.len(), l.res.len(), l.blob_w.get(), l.step(1.0 / 120.0));
    // No app matches, only files: the section moves up, nothing flashes.
    typed(d, &mut l, "invoice");
    let mut flashed = l.empty_t.target > 0.5;
    for _ in 0..6 {
        d.pump(&mut l, 0.01);
        tick(d, &mut l, 0.03);
        flashed |= l.empty_t.target > 0.5;
    }
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "lf_only_files");
    eprintln!("lf demo: invoice apps={} files={} sel={} no-match-flash={flashed}", l.res.len(), l.files.len(), l.sel);
    // A long list scrolls; a deep match highlights in the folder.
    l.key(&ctrl_bs);
    settle(d, &mut l, 0.4);
    typed(d, &mut l, "photo");
    settle(d, &mut l, 0.8);
    l.scroll(4.0);
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "lf_scrolled");
    // Nothing at all.
    l.key(&ctrl_bs);
    settle(d, &mut l, 0.4);
    typed(d, &mut l, "qzxqzx");
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "lf_nothing");
    l.key(&ctrl_bs);
    settle(d, &mut l, 0.4);
    // Enter reveals; Ctrl+Enter asks for the default app (dry here).
    typed(d, &mut l, "invoice");
    settle(d, &mut l, 0.5);
    let ctrl_enter = KeyInput { sym: keys::RETURN, text: None, ctrl: true, shift: false, alt: false };
    l.key(&ctrl_enter);
    eprintln!("lf demo: ctrl+enter default-app={:?} closing={}", l.opened.as_ref().map(|o| o.1), l.chrome.closing);
    drop(l);
    // Files never open with no query, and Enter still launches apps.
    let mut l = Launcher::new(&mut d.ctx(), true);
    l.chrome.time = 2000.0;
    settle(d, &mut l, 1.0);
    let moving = l.step(1.0 / 120.0);
    eprintln!("lf demo: at rest moving={moving} wake_in={:?}", l.chrome.wake_at.map(|t| t - l.chrome.time));
}

/// Headless demo for `nimbus --snapshot` (NIMBUS_SNAPSHOT_ONLY=launcher).
pub fn demo(d: &mut crate::snapshot::Demo) {
    // Warm the catalog cache like an earlier open would have.
    let _ = apps::fresh(None);
    let mut l = Launcher::new(&mut d.ctx(), true);
    // The live host syncs the window clock to time-since-start.
    l.chrome.time = 1234.5;

    let key = |sym: u32, text: Option<&str>| KeyInput { sym, text: text.map(String::from), ctrl: false, shift: false, alt: false };
    // `Demo::tick` renders uploads without a text-buffer map (it panics on
    // the first texture), so step here and push uploads through `render`.
    fn tick(d: &mut crate::snapshot::Demo, l: &mut Launcher, secs: f32) {
        const DT: f32 = 1.0 / 120.0;
        let mut list = DrawList::default();
        for _ in 0..(secs / DT).round() as u32 {
            l.draw(&mut d.gpu.fonts, &mut list);
            if !list.uploads.is_empty() || !list.frees.is_empty() {
                let only = DrawList { uploads: std::mem::take(&mut list.uploads), frees: std::mem::take(&mut list.frees), ..Default::default() };
                d.render(&only, l.text_buffers(), (4.0, 4.0), "_launcher_scratch");
            }
            l.step(DT);
        }
    }
    // Frames every other tick (60 fps) for strips.
    fn film(d: &mut crate::snapshot::Demo, l: &mut Launcher, name: &str, secs: f32) {
        let n = (secs * 60.0).round() as u32;
        for i in 0..n {
            d.pump(l, 0.0);
            d.shoot(l, &format!("{name}_{i:03}"));
            tick(d, l, 1.0 / 60.0);
        }
    }
    fn settle(d: &mut crate::snapshot::Demo, l: &mut Launcher, secs: f32) {
        let n = (secs / 0.05).round() as u32;
        for _ in 0..n {
            d.pump(l, 0.01);
            tick(d, l, 0.05);
        }
    }

    film(d, &mut l, "launcher_open", 0.75);
    settle(d, &mut l, 1.5);
    d.shoot(&mut l, "launcher_browse");
    // At rest: no animation frames, only the caret's blink wake-up.
    let moving = l.step(1.0 / 120.0);
    eprintln!("launcher demo: at rest moving={moving} wake_in={:?}", l.chrome.wake_at.map(|t| t - l.chrome.time));

    // The pointer is parked over a tile as the window opens: no takeover.
    if let Some((r, _)) = l.hits.iter().find(|(_, p)| *p == COLS + 2).copied() {
        let (x, y) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        l.pointer_motion(x, y);
        eprintln!("launcher demo: parked pointer keeps sel={} (want 0)", l.sel);
        // Then it really moves: hover selects.
        l.pointer_motion(x + 6.0, y + 2.0);
    }
    film(d, &mut l, "launcher_hover", 0.3);
    settle(d, &mut l, 0.6);
    d.shoot(&mut l, "launcher_hovered");
    l.pointer_leave();

    // Wheel down two notches; it comes to rest on a row.
    l.scroll(2.0);
    tick(d, &mut l, 0.12);
    d.pump(&mut l, 0.05);
    d.shoot(&mut l, "launcher_scrolling");
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "launcher_scrolled");
    eprintln!("launcher demo: wheel rest scroll={} (row {})", l.scroll.get(), l.scroll.get() / ROW_H);

    // Type a query: results reflow on springs.
    l.key(&key(0x66, Some("f")));
    film(d, &mut l, "launcher_type_f", 0.5);
    l.key(&key(0x69, Some("i")));
    film(d, &mut l, "launcher_type_fi", 0.5);
    settle(d, &mut l, 1.0);
    d.shoot(&mut l, "launcher_query");

    // Move the selection: liquid highlight.
    l.key(&key(keys::RIGHT, None));
    film(d, &mut l, "launcher_sel_right", 0.4);
    l.key(&key(keys::DOWN, None));
    film(d, &mut l, "launcher_sel_down", 0.4);
    settle(d, &mut l, 0.6);
    d.shoot(&mut l, "launcher_moved");

    // Backspace back to browsing.
    l.key(&key(keys::BACKSPACE, None));
    film(d, &mut l, "launcher_back", 0.3);
    l.key(&key(keys::BACKSPACE, None));
    film(d, &mut l, "launcher_back2", 0.6);
    settle(d, &mut l, 1.0);
    d.shoot(&mut l, "launcher_browse2");

    // A query with no matches.
    for c in ["q", "z", "x"] {
        l.key(&key(0x71, Some(c)));
        tick(d, &mut l, 0.06);
    }
    settle(d, &mut l, 0.8);
    d.shoot(&mut l, "launcher_empty");
    let ctrl_bs = KeyInput { sym: keys::BACKSPACE, text: None, ctrl: true, shift: false, alt: false };
    l.key(&ctrl_bs);
    settle(d, &mut l, 0.8);

    // Launch: the window folds into the icon, which bursts (dry run).
    for c in ["t", "e", "r", "m"] {
        l.key(&key(0x74, Some(c)));
        tick(d, &mut l, 0.05);
    }
    settle(d, &mut l, 1.0);
    d.shoot(&mut l, "launcher_term");
    l.key(&key(keys::RETURN, None));
    film(d, &mut l, "launcher_launch", 0.8);
    let mut list = DrawList::default();
    l.draw(&mut d.gpu.fonts, &mut list);
    eprintln!(
        "launcher demo: done={} frees={} textures-left={}",
        l.chrome.done(),
        list.frees.len(),
        l.icons.values().filter(|t| matches!(t, Tex::Ready { .. })).count()
    );

    // A rescan that drops apps lands while icons are in flight and the
    // last app is the footer's: nothing stale, nothing out of bounds.
    let mut l = Launcher::new(&mut d.ctx(), true);
    l.chrome.time = 2000.0;
    tick(d, &mut l, 0.05);
    if let Some(c) = l.cat.clone() {
        let last = c.apps.len() as u32 - 1;
        l.foot = (Some(0), Some(last));
        let msg = Msg::Catalog(l.session, Mutex::new(Some(Arc::new(c.trimmed()))));
        l.apply(&Fact::Custom(Arc::new(msg)));
        settle(d, &mut l, 1.2);
        let ok = l.icons.keys().all(|&i| (i as usize) < l.cat.as_ref().map_or(0, |c| c.apps.len()));
        eprintln!("launcher demo: rescan apps {}->{} stale-dropped={} indices-ok={ok}", c.apps.len(), l.items.len(), l.stale);
        d.shoot(&mut l, "launcher_rescan");
    }
    // Escape: everything folds back into the pill it came from.
    let (_, close) = l.key(&key(keys::ESCAPE, None));
    if close {
        l.close();
    }
    film(d, &mut l, "launcher_close", 0.6);
    eprintln!("launcher demo: closed done={}", l.chrome.done());
    // Dropped mid-open (surface gone): textures go to the orphan list.
    let mut l = Launcher::new(&mut d.ctx(), true);
    settle(d, &mut l, 0.6);
    let n = l.icons.values().filter(|t| matches!(t, Tex::Ready { .. })).count();
    drop(l);
    eprintln!("launcher demo: dropped with {n} textures (handed to gfx::free_later)");
}
