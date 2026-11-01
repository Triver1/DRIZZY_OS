//! Notifications: a freedesktop notification daemon (`notify/daemon.rs`)
//! and the toast window.
//!
//! Toasts grow out of the bar's right tab (a shared-element morph from a
//! sliver of its flat bottom), drop into a stack under it, and the stack
//! reflows on springs; cards past the fourth tuck into a "+N more" pill.
//! Each card dismisses itself after its timeout (paused while the stack is
//! hovered); a waiting toast costs no frames, and only the last stretch
//! shows an accent line sweeping out. Cards can be thrown sideways; the top
//! card retracts into the tab when it expires, lower ones fold shut.
//!
//! The `notifications` command opens the same window in history mode: a
//! panel that morphs out of the tab, live toasts morphing into their rows.
//! There is one such window for all outputs. Toasts never take keyboard
//! focus; the history panel does (Escape folds it).
//!
//! The surface only takes input on what is drawn and closes itself (and
//! frees its textures) once nothing is left.

mod daemon;

use std::{
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::Instant,
};

use calloop::channel::Sender;
use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    gfx::{DrawList, IconDraw, ImageData, ImageDraw, TextDraw},
    icons::IconSet,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    window::{Chrome, Ctx, Cursor, KeyInput, Keyboard, Placement, WinScene, keys},
};

use daemon::{Job, reason};

// ── model ───────────────────────────────────────────────────────────────

pub const DEFAULT_TIMEOUT: f32 = 5.0;
const HISTORY_MAX: usize = 50;

/// One notification as the UI sees it (decoded by the daemon thread).
pub struct Note {
    pub id: u32,
    /// Unique per Notify call (a replacement gets a new one).
    pub seq: u64,
    /// Display forms, prepared once: the app name in caps, its initial
    /// (for the lettered tile) and the body on one line (history rows).
    pub label: String,
    pub initial: String,
    pub line: String,
    pub summary: String,
    pub body: String,
    /// (key, label), without "default".
    pub actions: Vec<(String, String)>,
    pub default_action: bool,
    pub critical: bool,
    /// Seconds; None = stays until dismissed.
    pub timeout: Option<f32>,
    pub image: Option<ImageData>,
    /// Local "HH:MM" of arrival.
    pub time: String,
    pub transient: bool,
    pub resident: bool,
    /// Still open at the daemon (actions can be invoked).
    pub live: AtomicBool,
}

/// Payload of `Fact::Custom` for this module.
pub enum Msg {
    New(Arc<Note>),
    /// Closed by the sending app.
    Closed(u32),
}

static HISTORY: Mutex<VecDeque<Arc<Note>>> = Mutex::new(VecDeque::new());

/// Record `n`; returns the ids of entries pushed out of the history.
fn history_push(n: &Arc<Note>) -> Vec<u32> {
    if n.transient {
        return vec![];
    }
    if let Ok(mut h) = HISTORY.lock() {
        h.retain(|o| o.id != n.id);
        h.push_front(n.clone());
        if h.len() > HISTORY_MAX {
            return h.drain(HISTORY_MAX..).map(|o| o.id).collect();
        }
    }
    vec![]
}

fn history_remove(id: u32) {
    if let Ok(mut h) = HISTORY.lock() {
        h.retain(|o| o.id != id);
    }
}

fn history_snapshot() -> Vec<Arc<Note>> {
    HISTORY.lock().map(|h| h.iter().cloned().collect()).unwrap_or_default()
}

/// Output scale of the last toast window, so the daemon decodes icons at
/// the size they are drawn.
static SCALE_BITS: AtomicU32 = AtomicU32::new(0x4000_0000); // 2.0
/// A notification fact is about to open the window (vs. the command).
static PENDING: AtomicBool = AtomicBool::new(false);

fn icon_px() -> u32 {
    (ICON * f32::from_bits(SCALE_BITS.load(Ordering::Relaxed))).ceil() as u32
}

/// An action the user clicked, waiting for an xdg-activation token so the
/// app may raise its window (`take_invoke` / `send_invoke`, main.rs).
#[derive(Clone)]
pub struct Invoke {
    id: u32,
    seq: u64,
    key: String,
}

static INVOKE: Mutex<Option<Invoke>> = Mutex::new(None);

fn invoke(n: &Note, key: &str) {
    if let Ok(mut g) = INVOKE.lock() {
        // An unclaimed earlier click goes out without a token.
        if let Some(i) = g.replace(Invoke { id: n.id, seq: n.seq, key: key.into() }) {
            send_invoke(i, None);
        }
    }
}

/// The click that just happened invoked an action: main.rs asks the
/// compositor for an activation token, then calls `send_invoke`.
pub fn take_invoke() -> Option<Invoke> {
    INVOKE.lock().ok()?.take()
}

pub fn send_invoke(i: Invoke, token: Option<String>) {
    daemon::send(Job::Invoke(i.id, i.seq, i.key, token));
}

/// Start the org.freedesktop.Notifications server (called once at startup).
pub fn start_daemon(tx: Sender<Fact>) {
    daemon::start(tx);
}

fn msg(f: &Fact) -> Option<&Msg> {
    match f {
        Fact::Custom(a) => a.downcast_ref::<Msg>(),
        _ => None,
    }
}

/// Should this fact open the toast window (a new notification)?
pub fn wants_window(f: &Fact) -> bool {
    let yes = matches!(msg(f), Some(Msg::New(_)));
    if yes {
        PENDING.store(true, Ordering::Relaxed);
    }
    yes
}

/// Opened by a notification: toasts. Opened by the command: history.
pub fn open(ctx: &mut Ctx) -> Box<dyn WinScene> {
    let toasts = PENDING.swap(false, Ordering::Relaxed);
    Box::new(Toasts::new(ctx, !toasts))
}

// ── look ────────────────────────────────────────────────────────────────

/// Surface (logical px). Flush with the output's right edge so card
/// shadows aren't cut; cards line up with the tab's right edge.
const MARGIN: f32 = 0.0;
const SW: f32 = 460.0;
const SH: f32 = 760.0;
const EDGE: f32 = 9.0;
const CW: f32 = 372.0;
const CX: f32 = SW - EDGE - CW;
const HW: f32 = 404.0;
const TOP: f32 = 44.0;
const GAP: f32 = 10.0;
const MAX_VIS: usize = 4;
const ICON: f32 = 36.0;
const TX: f32 = 62.0;
const ROW_GAP: f32 = 6.0;
const LIST_TOP: f32 = 58.0;
const PANEL_MAX: f32 = SH - TOP - 44.0;
/// The time-left line only shows for the last stretch, sweeping to 0 as
/// an honest "about to go" cue (a waiting toast draws no frames).
const DRAIN: f32 = 1.4;

const F_APP: Font = Font::new(11.5, 600).family("Geist").track(0.02);
const F_TIME: Font = Font::new(11.5, 500).family("Geist");
const F_SUM: Font = Font::new(14.0, 600).family("Geist");
const F_BODY: Font = Font::new(13.0, 450).family("Geist");
const F_BTN: Font = Font::new(12.5, 600).family("Geist");
const F_TITLE: Font = Font::new(15.5, 650).family("Geist");
const F_COUNT: Font = Font::new(12.0, 600).family("Geist");
const F_RSUM: Font = Font::new(13.5, 600).family("Geist");
const F_RBODY: Font = Font::new(12.5, 450).family("Geist");
const F_INITIAL: Font = Font::new(17.0, 700).family("Geist");
const F_MORE: Font = Font::new(12.0, 600).family("Geist");

/// Prism Ink, a touch lifted under the pointer.
const INK_HOVER: Rgba = Rgba::hex(0x16161d, 0.62);
/// History rows: no boxes, just a faint white wash (stronger on hover).
const ROW: Rgba = Rgba::hex(0xffffff, 0.03);
const ROW_HOVER: Rgba = Rgba::hex(0xffffff, 0.065);
/// Faint hover circles / pills behind bare buttons.
const HOVER: Rgba = Rgba::hex(0xffffff, 0.10);
const HAIR: Rgba = Rgba::hex(0xffffff, 0.08);
const SHADOW: Rgba = Rgba::hex(0x000000, 0.55);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
const TRACK: Rgba = Rgba::hex(0xffffff, 0.07);
const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);

const MORPH_W: SpringCfg = SpringCfg::new(0.5, 0.84);
const MORPH_H: SpringCfg = SpringCfg::new(0.62, 0.8);
const SLOT: SpringCfg = SpringCfg::new(0.5, 0.88);
const FLY: SpringCfg = SpringCfg::new(0.55, 1.0);
const XFADE: SpringCfg = SpringCfg::new(0.42, 1.0);
const WIPE: SpringCfg = SpringCfg::new(0.55, 1.0);
const PANEL_W: SpringCfg = SpringCfg::new(0.48, 0.84);
const PANEL_H: SpringCfg = SpringCfg::new(0.6, 0.8);
const ROW_MORPH: SpringCfg = SpringCfg::new(0.52, 0.86);
const ENTER: SpringCfg = SpringCfg::new(0.42, 0.9);
const BADGE: SpringCfg = SpringCfg::new(0.38, 0.8);

/// Texture keys: a namespace of our own on the shared GPU texture map,
/// then the window instance (bits 32-47) and the note's seq, so two
/// windows can never free each other's textures.
const TEX: u64 = 0x6e6f_0000_0000_0000;
static INSTANCE: AtomicU64 = AtomicU64::new(0);

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rect(a: Rect, b: Rect, tw: f32, th: f32) -> Rect {
    Rect::new(lerp(a.x, b.x, tw), lerp(a.y, b.y, th), lerp(a.w, b.w, tw), lerp(a.h, b.h, th))
}

/// Intersection, or None when (nearly) empty.
fn crop(r: Rect, c: Rect) -> Option<Rect> {
    let (x0, y0) = (r.x.max(c.x), r.y.max(c.y));
    let (x1, y1) = ((r.x + r.w).min(c.x + c.w), (r.y + r.h).min(c.y + c.h));
    (x1 - x0 > 0.25 && y1 - y0 > 0.25).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The second line is ellipsised; this is plenty to fill it.
const L2_MAX: usize = 200;

/// Body text broken into the card's two lines (second one ellipsised
/// when drawn).
fn wrap2(fs: &mut FontSystem, s: &str, f: Font, max: f32, scale: f32) -> (String, String) {
    if s.is_empty() {
        return (String::new(), String::new());
    }
    let (first, rest) = match s.split_once('\n') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    // Two lines need a few hundred chars at most; never shape more.
    let first = daemon::clip_chars(first, 400);
    let mut b = Buffer::new(fs, Metrics::new(f.size * scale, (f.size * 1.3).round() * scale));
    b.set_wrap(Wrap::WordOrGlyph);
    b.set_size(Some(max * scale), None);
    let attrs = Attrs::new().family(Family::Name(f.family.unwrap_or("Geist"))).weight(Weight(f.weight));
    b.set_text(first, &attrs, Shaping::Advanced, None);
    b.shape_until_scroll(fs, false);
    let end = b.layout_runs().next().and_then(|r| r.glyphs.iter().map(|g| g.end).max()).unwrap_or(first.len()).min(first.len());
    let end = (end..=first.len()).find(|i| first.is_char_boundary(*i)).unwrap_or(first.len());
    let l1 = first[..end].trim_end().to_string();
    let mut l2 = daemon::clip_chars(first[end..].trim_start(), L2_MAX).to_string();
    if let Some(r) = rest {
        let room = L2_MAX.saturating_sub(l2.chars().count());
        if room > 0 {
            if !l2.is_empty() {
                l2.push(' ');
            }
            l2.push_str(&daemon::clip_chars(r, room).replace('\n', " "));
        }
    }
    (l1, l2)
}

// ── state ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ctl {
    /// Toast keys (stable across replacement).
    Card(u64),
    Close(u64),
    Action(u64, u8),
    More,
    /// The history panel's body (takes clicks, does nothing).
    Panel,
    Clear,
    Shut,
    /// History rows by note seq.
    Row(u64),
    RowClose(u64),
}

impl Ctl {
    fn toast(self) -> Option<u64> {
        match self {
            Ctl::Card(k) | Ctl::Close(k) | Ctl::Action(k, _) => Some(k),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leave {
    /// Thrown sideways off the stack (a user action).
    Fly,
    /// Back into the tab it came from (top card expiring).
    Retract,
    /// Folds shut in its slot, the cards below closing the gap (a lower
    /// card expiring, or closed by its app): a passive, quiet exit.
    Collapse,
}

#[derive(Clone, Default)]
struct Lines {
    summary: String,
    l1: String,
    l2: String,
}

struct Toast {
    key: u64,
    note: Arc<Note>,
    lines: Option<Lines>,
    /// Previous content while a replacement wipes in place.
    old: Option<(Arc<Note>, Lines)>,
    xf: Spring,
    /// Natural height (animated when replaced) and where it is headed:
    /// a shrinking replacement waits for the wipe to pass first.
    h: Spring,
    h_goal: f32,
    /// Morph out of the tab, per axis.
    ew: Spring,
    eh: Spring,
    /// Share of its slot in the stack (0 collapses the gap).
    lh: Spring,
    /// 1 in the stack, 0 tucked into the overflow pill.
    vis: Spring,
    dx: Spring,
    hover: Spring,
    press: Spring,
    leave: Option<Leave>,
    /// Timer: seconds left when last (re)started, and since when it runs.
    left: Option<f32>,
    since: Option<f32>,
    start_pending: bool,
    rect: Rect,
    alpha: f32,
}

impl Toast {
    fn new(key: u64, note: Arc<Note>) -> Self {
        let s = |v: f32, c: SpringCfg| Spring::new(v, c);
        let h = estimate_h(&note);
        let mut t = Self {
            key,
            lines: None,
            old: None,
            xf: s(1.0, WIPE),
            h: s(h, SLOT),
            h_goal: h,
            ew: s(0.0, MORPH_W),
            eh: s(0.0, MORPH_H),
            lh: s(0.0, SLOT),
            vis: s(1.0, SLOT),
            dx: s(0.0, FLY),
            hover: s(0.0, presets::SNAPPY),
            press: s(0.0, presets::SNAPPY),
            leave: None,
            left: note.timeout,
            since: None,
            start_pending: true,
            rect: Rect::default(),
            alpha: 0.0,
            note,
        };
        t.ew.set(1.0);
        t.eh.set(1.0);
        t.lh.set(1.0);
        t
    }

    fn replace(&mut self, note: Arc<Note>) {
        if let Some(l) = self.lines.take() {
            self.old = Some((self.note.clone(), l));
            self.xf.snap(0.0);
            self.xf.set(1.0);
        }
        self.h_goal = estimate_h(&note);
        self.left = note.timeout;
        self.since = None;
        self.start_pending = true;
        self.note = note;
    }

    fn remaining(&self, now: f32) -> Option<f32> {
        let l = self.left?;
        Some(match self.since {
            Some(s) => l - (now - s),
            None => l,
        })
    }

    fn deadline(&self) -> Option<f32> {
        if self.leave.is_some() {
            return None;
        }
        Some(self.since? + self.left?)
    }

    fn pause(&mut self, now: f32) {
        if let Some(r) = self.remaining(now) {
            self.left = Some(r.max(0.0));
        }
        self.since = None;
    }

    fn resume(&mut self, now: f32) {
        if self.left.is_some() && self.leave.is_none() {
            self.since = Some(now);
        }
    }

    fn springs(&mut self, dt: f32) -> bool {
        // Grow at once (new content must fit); shrink once the wipe has
        // passed, so the old buttons aren't cut by the bottom edge.
        if self.h.target != self.h_goal && (self.old.is_none() || self.h_goal > self.h.target || self.xf.get() > 0.75) {
            self.h.set(self.h_goal);
        }
        let mut m = false;
        for s in [&mut self.xf, &mut self.h, &mut self.ew, &mut self.eh, &mut self.lh, &mut self.vis, &mut self.dx, &mut self.hover, &mut self.press] {
            m |= s.step(dt);
        }
        if self.xf.at_rest() && self.old.is_some() {
            self.old = None;
        }
        m
    }

    fn finished(&self) -> bool {
        let Some(l) = self.leave else { return false };
        let slot = self.lh.at_rest();
        match l {
            Leave::Fly => slot && (self.dx.at_rest() || self.dx.get().abs() > CW + 40.0),
            Leave::Retract => slot && self.ew.at_rest() && self.eh.at_rest(),
            Leave::Collapse => slot,
        }
    }
}

/// Height before the text is shaped (refined on first draw).
fn estimate_h(n: &Note) -> f32 {
    let body_lines = if n.body.is_empty() {
        0
    } else if n.body.len() > 44 {
        2
    } else {
        1
    };
    card_h(n, body_lines)
}

fn card_h(n: &Note, body_lines: usize) -> f32 {
    let text_bottom: f32 = match body_lines {
        0 => 49.0,
        1 => 68.0,
        _ => 86.0,
    };
    let mut h = text_bottom.max(50.0) + 14.0;
    if !n.actions.is_empty() {
        h += 40.0;
    }
    h
}

fn row_h(n: &Note) -> f32 {
    if n.body.is_empty() { 58.0 } else { 74.0 }
}

struct Row {
    note: Arc<Note>,
    /// Previous content while a replacement wipes in place.
    old: Option<Arc<Note>>,
    xf: Spring,
    h: Spring,
    lh: Spring,
    dx: Spring,
    hover: Spring,
    leaving: bool,
    delay: f32,
    /// Shared-element morph: the toast (or "+N more" pill) it came from,
    /// and how far it has travelled into its slot.
    from: Option<Rect>,
    m: Spring,
    /// The card's own text and height, cross-fading into the row's.
    card: Option<Lines>,
    /// Rows without a source wipe in top-down, staggered.
    enter: Spring,
    enter_delay: f32,
    rect: Rect,
}

impl Row {
    fn new(note: Arc<Note>) -> Self {
        Self {
            h: Spring::new(row_h(&note), SLOT),
            old: None,
            xf: Spring::new(1.0, WIPE),
            note,
            lh: Spring::new(1.0, SLOT),
            dx: Spring::new(0.0, FLY),
            hover: Spring::new(0.0, presets::SNAPPY),
            leaving: false,
            delay: 0.0,
            from: None,
            m: Spring::new(1.0, ROW_MORPH),
            card: None,
            enter: Spring::new(1.0, ENTER),
            enter_delay: 0.0,
            rect: Rect::default(),
        }
    }

    /// Wipe in (top-down) after `delay` seconds.
    fn wipe_in(mut self, delay: f32) -> Self {
        self.enter.snap(0.0);
        self.enter_delay = delay;
        self
    }

    fn replace(&mut self, note: Arc<Note>) {
        self.old = Some(std::mem::replace(&mut self.note, note));
        self.xf.snap(0.0);
        self.xf.set(1.0);
        self.h.set(row_h(&self.note));
    }

    fn springs(&mut self, dt: f32) -> bool {
        let mut m = self.lh.step(dt) | self.dx.step(dt) | self.hover.step(dt) | self.h.step(dt) | self.xf.step(dt) | self.m.step(dt);
        if self.enter_delay > 0.0 {
            self.enter_delay -= dt;
            m = true;
        } else {
            self.enter.set(1.0);
            m |= self.enter.step(dt);
        }
        if self.xf.at_rest() {
            self.old = None;
        }
        if self.m.at_rest() {
            self.from = None;
            self.card = None;
        }
        m
    }
}

struct Press {
    ctl: Option<Ctl>,
    toast: Option<u64>,
    x0: f32,
    y0: f32,
    last: (Instant, f32),
    vel: f32,
    dragging: bool,
}

pub struct Toasts {
    chrome: Chrome,
    text: TextCache,
    icons: Rc<IconSet>,
    scale: f32,
    tab: Rect,
    /// Where cards and the panel grow from: a sliver of the tab's flat bottom.
    start: Rect,
    toasts: Vec<Toast>,
    next_key: u64,
    hits: Vec<(Rect, Ctl)>,
    hover: Option<Ctl>,
    /// Last pointer position over the surface (hover is re-hit-tested as
    /// the layout moves under a still pointer).
    ptr: Option<(f32, f32)>,
    hovs: Vec<(Ctl, Spring)>,
    press: Option<Press>,
    paused: bool,
    more_a: Spring,
    more_n: usize,
    more_prev: usize,
    more_xf: Spring,
    more_rect: Rect,
    more_label: (usize, String),
    history: bool,
    pw: Spring,
    ph: Spring,
    panel_h: Spring,
    rows: Vec<Row>,
    scroll: Spring,
    empty_a: Spring,
    clearing: bool,
    panel: Rect,
    /// Header count: shown number, the one cross-fading out, badge
    /// presence and width.
    count_n: usize,
    count_prev: usize,
    count_xf: Spring,
    count_a: Spring,
    count_w: Spring,
    /// This window's texture key prefix.
    tex: u64,

    uploaded: Vec<u64>,
    inuse: Vec<u64>,
}

impl Toasts {
    fn new(ctx: &mut Ctx, history: bool) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        SCALE_BITS.store(ctx.scale.to_bits(), Ordering::Relaxed);
        let left = ctx.out_w - SW - MARGIN;
        let t = ctx.tabs[2];
        let tab = if t.w > 1.0 { Rect::new(t.x - left, t.y, t.w, t.h) } else { Rect::new(SW - EDGE - 180.0, -15.0, 180.0, 47.0) };
        let start = Rect::new(tab.x + 12.0, tab.y + tab.h - 8.0, (tab.w - 24.0).max(24.0), 8.0);
        let mut chrome = Chrome::new(start, start, MORPH_W, MORPH_H);
        chrome.open_w.snap(1.0);
        chrome.open_h.snap(1.0);
        let inst = INSTANCE.fetch_add(1, Ordering::Relaxed);
        let mut s = Self {
            chrome,
            text,
            icons: ctx.icons.clone(),
            scale: ctx.scale,
            tab,
            start,
            toasts: vec![],
            next_key: 1,
            hits: vec![],
            hover: None,
            ptr: None,
            hovs: vec![],
            press: None,
            paused: false,
            more_a: Spring::new(0.0, SLOT),
            more_n: 0,
            more_prev: 0,
            more_xf: Spring::new(1.0, XFADE),
            more_rect: Rect::default(),
            more_label: (0, String::new()),
            history: false,
            pw: Spring::new(0.0, PANEL_W),
            ph: Spring::new(0.0, PANEL_H),
            panel_h: Spring::new(170.0, SLOT),
            rows: vec![],
            scroll: Spring::new(0.0, presets::SMOOTH),
            empty_a: Spring::new(0.0, presets::SMOOTH),
            clearing: false,
            panel: Rect::default(),
            count_n: 0,
            count_prev: 0,
            count_xf: Spring::new(1.0, XFADE),
            count_a: Spring::new(0.0, BADGE),
            count_w: Spring::new(0.0, BADGE),
            tex: TEX | ((inst & 0xffff) << 32),

            uploaded: vec![],
            inuse: vec![],
        };
        if history {
            s.enter_history();
        }
        s
    }

    fn tex_key(&self, n: &Note) -> u64 {
        self.tex | (n.seq & 0xffff_ffff)
    }

    fn tab_bottom(&self) -> f32 {
        self.tab.y + self.tab.h
    }

    // ── state changes ──

    fn add(&mut self, note: Arc<Note>) {
        if self.history {
            if let Some(r) = self.rows.iter_mut().find(|r| r.note.id == note.id && !r.leaving) {
                r.replace(note);
            } else if note.transient {
                // Not kept and nobody sees it: done right away.
                daemon::send(Job::UiClose(note.id, note.seq, reason::EXPIRED));
            } else {
                let mut r = Row::new(note).wipe_in(0.08);
                r.lh.snap(0.0);
                r.lh.set(1.0);
                self.rows.insert(0, r);
                self.scroll.set(0.0);
            }
            return;
        }
        if let Some(t) = self.toasts.iter_mut().find(|t| t.note.id == note.id && t.leave.is_none()) {
            t.replace(note);
            return;
        }
        let key = self.next_key;
        self.next_key += 1;
        self.toasts.insert(0, Toast::new(key, note));
        // A window that was folding away comes back.
        self.chrome.closing = false;
        self.chrome.open_w.snap(1.0);
        self.chrome.open_h.snap(1.0);
    }

    fn dismiss(&mut self, i: usize, why: Option<u32>, how: Leave) {
        let first_live = self.toasts.iter().position(|t| t.leave.is_none());
        let t = &mut self.toasts[i];
        if t.leave.is_some() {
            return;
        }
        if let Some(r) = why {
            daemon::send(Job::UiClose(t.note.id, t.note.seq, r));
        }
        // Only the top card retracts into the tab; lower ones fold shut.
        let how = if how == Leave::Retract && (first_live != Some(i) || t.vis.get() < 0.5) { Leave::Collapse } else { how };
        t.leave = Some(how);
        t.lh.set(0.0);
        match how {
            Leave::Fly => {
                let dir = if t.dx.get() < -1.0 || t.dx.vel < -50.0 { -1.0 } else { 1.0 };
                t.dx.set(dir * (CW + 60.0));
                if t.dx.vel.abs() < 600.0 {
                    t.dx.vel = dir * 600.0;
                }
            }
            Leave::Retract => {
                t.ew.set(0.0);
                t.eh.set(0.0);
            }
            Leave::Collapse => {}
        }
    }

    fn dismiss_all(&mut self) {
        for i in 0..self.toasts.len() {
            self.dismiss(i, Some(reason::DISMISSED), Leave::Fly);
        }
    }

    fn enter_history(&mut self) {
        if self.history {
            return;
        }
        self.history = true;
        let fresh = self.pw.get() < 0.05;
        // Rows already on screen (panel folding back) keep their motion.
        let mut keep = std::mem::take(&mut self.rows);
        let mut stagger = 0.0;
        let more = self.more_rect;
        let mut rows = vec![];
        for n in history_snapshot() {
            if let Some(i) = keep.iter().position(|r| r.note.seq == n.seq && !r.leaving) {
                rows.push(keep.swap_remove(i));
                continue;
            }
            // A live toast becomes its row (shared element): the row starts
            // at the card's rect, or at the "+N more" pill it was tucked in.
            let src = self.toasts.iter().find(|t| t.note.seq == n.seq && t.leave.is_none()).and_then(|t| {
                if t.alpha > 0.05 && t.vis.get() > 0.5 {
                    Some((t.rect, t.lines.clone()))
                } else if more.w > 0.0 {
                    Some((more, None))
                } else {
                    None
                }
            });
            let mut r = Row::new(n);
            match src {
                Some((from, card)) => {
                    r.from = Some(from);
                    r.card = card;
                    r.m.snap(0.0);
                    r.m.set(1.0);
                }
                None => {
                    r = r.wipe_in(0.06 + stagger);
                    stagger = (stagger + 0.035_f32).min(0.3);
                }
            }
            rows.push(r);
        }
        self.rows = rows;
        if fresh {
            self.empty_a.snap(if self.rows.is_empty() { 1.0 } else { 0.0 });
            self.scroll.snap(0.0);
            self.panel_h.snap(self.panel_target());
        }
        self.pw.set(1.0);
        self.ph.set(1.0);
        // Toasts that turned into rows stop drawing; the rest (transient,
        // not kept) fold away and are closed at the daemon.
        let rows = &self.rows;
        self.toasts.retain(|t| t.leave.is_some() || !rows.iter().any(|r| r.note.seq == t.note.seq && r.from.is_some()));
        for i in 0..self.toasts.len() {
            let t = &self.toasts[i];
            if t.leave.is_none() {
                let kept = self.rows.iter().any(|r| r.note.seq == t.note.seq);
                self.dismiss(i, (!kept).then_some(reason::EXPIRED), Leave::Collapse);
            }
        }
        self.more_rect = Rect::default();
        self.chrome.closing = false;
    }

    fn leave_history(&mut self) {
        if !self.history {
            return;
        }
        self.history = false;
        self.pw.set(0.0);
        self.ph.set(0.0);
    }

    fn remove_row(&mut self, seq: u64, delay: f32) {
        if let Some(r) = self.rows.iter_mut().find(|r| r.note.seq == seq && !r.leaving) {
            r.leaving = true;
            r.delay = delay;
            r.hover.set(0.0);
            history_remove(r.note.id);
            if r.note.live.load(Ordering::Relaxed) {
                daemon::send(Job::UiClose(r.note.id, r.note.seq, reason::DISMISSED));
            }
        }
    }

    fn clear_all(&mut self) {
        let seqs: Vec<u64> = self.rows.iter().filter(|r| !r.leaving).map(|r| r.note.seq).collect();
        if seqs.is_empty() {
            return;
        }
        self.clearing = true;
        for (i, s) in seqs.into_iter().enumerate() {
            self.remove_row(s, i.min(10) as f32 * 0.035);
        }
        if let Ok(mut h) = HISTORY.lock() {
            h.clear();
        }
    }

    fn panel_target(&self) -> f32 {
        let content: f32 = self.rows.iter().map(|r| (r.h.target + ROW_GAP) * r.lh.target).sum();
        (LIST_TOP + content + 4.0).clamp(170.0, PANEL_MAX)
    }

    fn content_h(&self) -> f32 {
        self.rows.iter().map(|r| (r.h.get() + ROW_GAP) * r.lh.get()).sum()
    }

    fn hover_of(&self, c: Ctl) -> f32 {
        self.hovs.iter().find(|(k, _)| *k == c).map_or(0.0, |(_, s)| s.get().clamp(0.0, 1.0))
    }

    fn hit(&self, x: f32, y: f32) -> Option<Ctl> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, c)| *c)
    }

    fn toast_index(&self, key: u64) -> Option<usize> {
        self.toasts.iter().position(|t| t.key == key)
    }

    fn click(&mut self, c: Ctl) {
        match c {
            Ctl::Card(k) => {
                if let Some(i) = self.toast_index(k) {
                    let n = self.toasts[i].note.clone();
                    // An invoked action closes it at the daemon (after the
                    // ActionInvoked signal), so the UI sends no close.
                    let why = if n.default_action {
                        invoke(&n, "default");
                        None
                    } else {
                        Some(reason::DISMISSED)
                    };
                    self.dismiss(i, why, Leave::Fly);
                }
            }
            Ctl::Close(k) => {
                if let Some(i) = self.toast_index(k) {
                    self.dismiss(i, Some(reason::DISMISSED), Leave::Fly);
                }
            }
            Ctl::Action(k, a) => {
                if let Some(i) = self.toast_index(k) {
                    let n = self.toasts[i].note.clone();
                    if let Some((key, _)) = n.actions.get(a as usize) {
                        invoke(&n, key);
                    }
                    if !n.resident {
                        self.dismiss(i, None, Leave::Fly);
                    }
                }
            }
            Ctl::More => self.enter_history(),
            Ctl::Panel => {}
            Ctl::Clear => self.clear_all(),
            Ctl::Shut => self.leave_history(),
            Ctl::Row(s) => {
                if let Some(n) = self.rows.iter().find(|r| r.note.seq == s).map(|r| r.note.clone())
                    && n.default_action
                    && n.live.load(Ordering::Relaxed)
                {
                    invoke(&n, "default");
                    self.leave_history();
                }
            }
            Ctl::RowClose(s) => self.remove_row(s, 0.0),
        }
    }

    // ── drawing helpers ──

    fn px(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [(r.x * s).floor() as i32, (r.y * s).floor() as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32]
    }

    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, max: f32, c: Rgba, clip: Rect) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let (k, w) = self.text.get_fit(fs, s, f, max);
        if c.3 > 0.003 && clip.w > 0.5 && clip.h > 0.5 {
            let lh = self.text.line_h(f);
            let sc = self.scale;
            out.texts.push(TextDraw { key: k, x: (x * sc).round(), y: ((cy - lh * 0.5) * sc).round(), color: c, clip: self.px(clip) });
        }
        w
    }

    fn measure(&mut self, fs: &mut FontSystem, s: &str, f: Font, max: f32) -> f32 {
        self.text.get_fit(fs, s, f, max).1
    }

    fn shape(&self, out: &mut DrawList, r: Rect, clip: Rect, radius: f32, f: impl FnOnce(paint::ShapeB) -> paint::ShapeB) {
        if let Some(c) = crop(r, clip) {
            out.shapes.push(f(paint::rect(self.scale, c, radius)).done());
        }
    }

    fn image(&self, out: &mut DrawList, key: u64, r: Rect, radius: f32, clip: Rect, alpha: f32) {
        let Some(c) = crop(r, clip) else { return };
        if alpha < 0.003 {
            return;
        }
        let s = self.scale;
        let mut d = ImageDraw::new(key, [c.x * s, c.y * s, c.w * s, c.h * s]);
        d.radius = radius * s;
        d.uv = [(c.x - r.x) / r.w, (c.y - r.y) / r.h, (c.x + c.w - r.x) / r.w, (c.y + c.h - r.y) / r.h];
        d.alpha = alpha;
        out.images.push(d);
    }

    fn icon(&self, out: &mut DrawList, name: &str, x: f32, y: f32, size: f32, c: Rgba) {
        if c.3 <= 0.003 {
            return;
        }
        let s = self.scale;
        let id = self.icons.id(&[name, "window-close-symbolic"]);
        out.icons.push(IconDraw { id, x: x * s, y: y * s, size: (size * s).round(), color: c });
    }

    fn texture(&mut self, out: &mut DrawList, n: &Note) -> Option<u64> {
        let img = n.image.as_ref()?;
        let key = self.tex_key(n);
        if !self.uploaded.contains(&key) {
            self.uploaded.push(key);
            out.uploads.push((key, img.clone()));
        }
        self.inuse.push(key);
        Some(key)
    }

    /// App icon (or a lettered accent tile) at `r`.
    #[allow(clippy::too_many_arguments)]
    fn avatar(&mut self, fs: &mut FontSystem, out: &mut DrawList, n: &Note, r: Rect, radius: f32, clip: Rect, a: f32) {
        if let Some(key) = self.texture(out, n) {
            self.image(out, key, r, radius, clip, a);
            return;
        }
        // Lettered tile: iridescent (the accent under another one); a
        // critical one is a flat soft red.
        let critical = n.critical;
        self.shape(out, r, clip, radius, |s| if critical { s.fill(BAD) } else { s.on_fill(paint::IRID.0, paint::IRID.1, 0.1) }.alpha(a));
        let ink = if critical { Rgba::hex(0x1a0a0e, 1.0) } else { theme::on_ink() };
        let f = Font::new(F_INITIAL.size * r.w / ICON, F_INITIAL.weight).family("Geist");
        let w = self.measure(fs, &n.initial, f, r.w);
        self.label(fs, out, &n.initial, f, r.x + (r.w - w) * 0.5, r.y + r.h * 0.5, r.w, ink.fade(a), clip);
    }

    fn lines(&mut self, fs: &mut FontSystem, n: &Note) -> Lines {
        let (summary, body) = if n.summary.is_empty() {
            match n.body.split_once('\n') {
                Some((a, b)) => (a.to_string(), b.to_string()),
                None => (n.body.clone(), String::new()),
            }
        } else {
            (n.summary.clone(), n.body.clone())
        };
        let (l1, l2) = wrap2(fs, &body, F_BODY, CW - TX - 16.0, self.scale);
        Lines { summary, l1, l2 }
    }

    /// One card's content, anchored at (ox, oy) (its natural top-left).
    #[allow(clippy::too_many_arguments)]
    fn card_content(&mut self, fs: &mut FontSystem, out: &mut DrawList, key: u64, n: &Note, l: &Lines, ox: f32, oy: f32, hh: f32, hover: f32, clip: Rect, a: f32, interactive: bool) {
        if a <= 0.003 {
            return;
        }
        if n.critical {
            self.shape(out, Rect::new(ox + 5.0, oy + 16.0, 3.0, (hh - 32.0).max(8.0)), clip, 1.5, |s| s.fill(BAD).alpha(a));
        }
        self.avatar(fs, out, n, Rect::new(ox + 14.0, oy + 14.0, ICON, ICON), 10.0, clip, a);
        let tx = ox + TX;
        let app_c = if n.critical { BAD.mix(TEXT, 0.25) } else { MUTED };
        self.label(fs, out, &n.label, F_APP, tx, oy + 21.0, CW - TX - 70.0, app_c.fade(a), clip);
        // Time on the right; the close button takes its place on hover.
        let tw = self.measure(fs, &n.time, F_TIME, 60.0);
        self.label(fs, out, &n.time, F_TIME, ox + CW - 16.0 - tw, oy + 21.0, 60.0, MUTED.fade(a * (1.0 - hover)), clip);
        let close = Rect::new(ox + CW - 12.0 - 24.0, oy + 9.0, 24.0, 24.0);
        if interactive {
            self.hits.push((close, Ctl::Close(key)));
        }
        let ch = self.hover_of(Ctl::Close(key));
        if hover > 0.003 {
            // A bare icon; the faint circle only under the pointer.
            self.shape(out, close, clip, 12.0, |s| s.fill(HOVER).alpha(a * hover * ch));
            if crop(close, clip).is_some_and(|c| c.h > 20.0) {
                self.icon(out, "window-close-symbolic", close.x + 5.0, close.y + 5.0, 14.0, SOFT.mix(TEXT, ch).fade(a * hover));
            }
        }
        let max = CW - TX - 16.0;
        self.label(fs, out, &l.summary, F_SUM, tx, oy + 40.0, max, TEXT.fade(a), clip);
        self.label(fs, out, &l.l1, F_BODY, tx, oy + 59.0, max, SOFT.fade(a), clip);
        self.label(fs, out, &l.l2, F_BODY, tx, oy + 77.0, max, SOFT.fade(a), clip);
        self.card_buttons(fs, out, key, n, l, ox, oy, clip, a, interactive);
    }

    #[allow(clippy::too_many_arguments)]
    fn card_buttons(&mut self, fs: &mut FontSystem, out: &mut DrawList, key: u64, n: &Note, l: &Lines, ox: f32, oy: f32, clip: Rect, a: f32, interactive: bool) {
        let tx = ox + TX;
        if !n.actions.is_empty() {
            let lines = if !l.l2.is_empty() { 2 } else if !l.l1.is_empty() { 1 } else { 0 };
            let by = oy + card_h(n, lines) - 14.0 - 30.0 - 2.0;
            let k = n.actions.len() as f32;
            let bw = (CW - TX - 14.0 - 8.0 * (k - 1.0)) / k;
            for (i, (_, label)) in n.actions.iter().enumerate() {
                let r = Rect::new(tx + i as f32 * (bw + 8.0), by, bw, 30.0);
                let c = Ctl::Action(key, i as u8);
                if interactive {
                    self.hits.push((r, c));
                }
                let h = self.hover_of(c);
                // Flat faint pills, no bevel.
                self.shape(out, r, clip, 15.0, |s| s.fill(Rgba::hex(0xffffff, 0.055 + 0.06 * h)).alpha(a));
                let w = self.measure(fs, label, F_BTN, bw - 16.0);
                self.label(fs, out, label, F_BTN, r.x + (r.w - w) * 0.5, r.y + 15.0, bw - 16.0, SOFT.mix(TEXT, h).fade(a), clip);
            }
        }
    }

    fn draw_toasts(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        let sc = self.scale;
        let tab_bottom = self.tab_bottom();
        let now = self.chrome.time;
        let mut y = TOP;
        let y_end = TOP + self.toasts.iter().map(|t| (t.h.get() + GAP) * t.lh.get().max(0.0) * t.vis.get().clamp(0.0, 1.0)).sum::<f32>();
        self.draw_more(fs, out, y_end);
        // The overflow pill's size, for cards tucking into / out of it.
        let pill_w = if self.more_rect.w > 0.0 { self.more_rect.w } else { 96.0 };
        let mut toasts = std::mem::take(&mut self.toasts);
        for t in toasts.iter_mut() {
            if t.lines.is_none() {
                let l = self.lines(fs, &t.note);
                let n = if !l.l2.is_empty() { 2 } else if !l.l1.is_empty() { 1 } else { 0 };
                t.h_goal = card_h(&t.note, n);
                if t.old.is_none() {
                    t.h.set(t.h_goal);
                }
                t.lines = Some(l);
            }
            let hh = t.h.get();
            let slot_y = y;
            let v = t.vis.get().clamp(0.0, 1.0);
            let lh = t.lh.get().clamp(0.0, 1.0);
            y += (hh + GAP) * t.lh.get().max(0.0) * v;
            let fin = Rect::new(CX + t.dx.get(), slot_y, CW, hh);
            let (ew, eh) = (t.ew.get(), t.eh.get());
            let mut r = lerp_rect(self.start, fin, ew, eh);
            let mut radius = lerp(4.0, 16.0, eh.clamp(0.0, 1.0));
            // Tucking into the "+N more" pill (or coming back out of it):
            // the opaque card shrinks into the pill's rect, content clipped.
            if v < 1.0 {
                let pill = Rect::new(SW - EDGE - pill_w, slot_y + 2.0, pill_w, 28.0);
                let k = 1.0 - v;
                r = lerp_rect(r, pill, k, k);
                radius = lerp(radius, 10.0, k);
            }
            // Folding shut in place: the bottom edge rises to the top.
            if t.leave == Some(Leave::Collapse) {
                let k = 1.0 - lh;
                r = Rect::new(r.x + 10.0 * k, r.y, r.w - 20.0 * k, r.h * lh);
            }
            let mut a = 1.0;
            if t.leave == Some(Leave::Fly) {
                a *= 1.0 - smooth(0.15, 1.0, t.dx.get().abs() / (CW + 40.0));
            }
            // Alpha only for the very last sliver of a tuck / collapse /
            // morph; bodies stay at full strength otherwise.
            a *= smooth(0.0, 0.1, v);
            if t.leave == Some(Leave::Collapse) {
                a *= smooth(0.0, 0.12, lh);
            }
            // The sliver overlapping the tab is drawn in the tab's colours.
            let grow = eh.clamp(0.0, 1.0);
            a *= smooth(0.0, 0.12, eh.min(ew));
            let press = 1.0 - 0.02 * t.press.get().clamp(0.0, 1.0);
            if press < 1.0 {
                let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
                r = Rect::new(cx - r.w * press * 0.5, cy - r.h * press * 0.5, r.w * press, r.h * press);
            }
            t.rect = r;
            t.alpha = a;
            if a <= 0.003 || r.h < 0.5 || r.w < 0.5 {
                continue;
            }
            let hv = t.hover.get().clamp(0.0, 1.0);
            // Under the history panel nothing shows: the stack slides up
            // beneath its bottom edge as the panel grows down.
            let pfloor = if self.panel.h > 0.0 { self.panel.y + self.panel.h } else { f32::MIN };
            let body = if r.y < pfloor { crop(r, Rect::new(r.x - 50.0, pfloor, r.w + 100.0, SH)) } else { Some(r) };
            let Some(body) = body else { continue };
            out.shapes.push(
                // Prism Ink: flat matte ink, the prism hairline (stronger
                // under the pointer).
                paint::rect(sc, body, radius.min(body.h * 0.5))
                    .fill(paint::INK.mix(INK_HOVER, hv))
                    .prism(paint::HAIR, grow * (1.0 + 0.4 * hv))
                    .shadow(SHADOW.fade(grow * a), 18.0 + 4.0 * hv, 7.0 + 2.0 * hv)
                    .alpha(a)
                    .done(),
            );
            out.glass(body, radius.min(body.h * 0.5), a);
            let interactive = t.leave.is_none() && v > 0.5;
            if interactive {
                self.hits.push((r, Ctl::Card(t.key)));
            }
            // Content keeps its natural layout, anchored to the right edge
            // (the tab's edge) and the top, and is revealed by the body.
            let mut clip = r.inflate(-1.0);
            // Revealed below the tab while growing out of it, and hidden
            // under the history panel as it slides down over the stack.
            let floor = if eh < 1.0 { tab_bottom } else { f32::MIN }.max(pfloor);
            if clip.y < floor {
                clip = Rect::new(clip.x, floor, clip.w, (clip.y + clip.h - floor).max(0.0));
            }
            let (ox, oy) = (r.x + r.w - CW, r.y);
            // Text leaves a tucking card in its first half (and arrives in
            // the second half coming out), before it overlaps the pill.
            let ca = a * smooth(0.12, 0.4, eh.min(ew)) * smooth(0.5, 1.0, v);
            // A replacement wipes in left to right behind an accent caret;
            // both versions stay at full brightness.
            let mut new_clip = clip;
            if let Some((on, ol)) = &t.old {
                let xf = t.xf.get().clamp(0.0, 1.0);
                let wx = lerp(r.x - 4.0, r.x + r.w + 4.0, xf);
                if let Some(oc) = crop(clip, Rect::new(wx, clip.y, r.w + 8.0, clip.h)) {
                    self.card_content(fs, out, t.key, on, ol, ox, oy, hh, hv, oc, ca, false);
                }
                new_clip = crop(clip, Rect::new(r.x - 8.0, clip.y, wx - r.x + 8.0, clip.h)).unwrap_or(Rect::new(clip.x, clip.y, 0.0, 0.0));
                let k = (xf * std::f32::consts::PI).sin();
                self.shape(out, Rect::new(wx - 1.0, oy + 12.0, 2.0, hh - 24.0), clip, 1.0, |s| s.on_fill(0.0, 1.0, 0.0).alpha(ca * k));
            }
            let lines = t.lines.take().unwrap_or_default();
            let note = t.note.clone();
            let n_hits = self.hits.len();
            self.card_content(fs, out, t.key, &note, &lines, ox, oy, hh, hv, new_clip, ca, interactive);
            t.lines = Some(lines);
            if !interactive {
                self.hits.truncate(n_hits);
            }
            // Time left: only for the last stretch, an accent line sweeping
            // from full to nothing (frozen, dimmed, while paused).
            if !note.critical
                && t.old.is_none()
                && t.leave.is_none()
                && let Some(rem) = t.remaining(now)
                && rem < DRAIN
            {
                let f = (rem / DRAIN).clamp(0.0, 1.0);
                let la = ca * smooth(0.0, 0.1, 1.0 - f) * if self.paused { 0.6 } else { 1.0 };
                let track = Rect::new(ox + 16.0, oy + hh - 6.0, CW - 32.0, 2.0);
                self.shape(out, track, clip, 1.0, |s| s.fill(TRACK).alpha(la));
                if f > 0.002 {
                    self.shape(out, Rect::new(track.x, track.y, track.w * f, 2.0), clip, 1.0, |s| s.on_fill(1.0, 0.0, 0.0).alpha(la));
                }
            }
        }
        self.toasts = toasts;
        self.draw_more_text(fs, out);
    }

    /// Overflow: "+N more" under the stack's end, cross-fading its count in
    /// place. Drawn beneath the cards: it slides out from under the last
    /// one (and back under it), and cards tuck into it from above.
    fn draw_more(&mut self, fs: &mut FontSystem, out: &mut DrawList, y_end: f32) {
        let ma = self.more_a.get();
        if ma <= 0.003 {
            self.more_rect = Rect::default();
            return;
        }
        let n = self.more_n.max(1);
        if self.more_label.0 != n {
            self.more_label = (n, format!("+{n} more"));
        }
        let label = std::mem::take(&mut self.more_label.1);
        let w = self.measure(fs, &label, F_MORE, 200.0);
        let pw = w + 26.0;
        let r = Rect::new(SW - EDGE - pw, y_end + 2.0 - 34.0 * (1.0 - ma), pw, 28.0);
        self.more_rect = r;
        let pa = smooth(0.0, 0.15, ma);
        let h = self.hover_of(Ctl::More);
        out.shapes.push(
            paint::rect(self.scale, r, 14.0)
                .fill(paint::INK.mix(INK_HOVER, h))
                .prism(paint::HAIR, 1.0)
                .shadow(SHADOW.fade(pa), 12.0, 4.0)
                .alpha(pa)
                .done(),
        );
        out.glass(r, 14.0, pa);
        self.hits.push((r, Ctl::More));
        self.more_label.1 = label;
    }

    /// The pill's text, after the cards: text draws above every shape, so
    /// it is clipped to what no card covers.
    fn draw_more_text(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        let r = self.more_rect;
        let ma = self.more_a.get();
        if r.w <= 0.0 || ma <= 0.003 {
            return;
        }
        let mut top = r.y;
        for t in &self.toasts {
            let c = t.rect;
            if t.alpha > 0.02 && c.x < r.x + r.w && c.x + c.w > r.x && c.y < r.y + r.h {
                top = top.max(c.y + c.h);
            }
        }
        let Some(clip) = crop(r.inflate(-1.0), Rect::new(r.x, top, r.w, SH)) else { return };
        let label = std::mem::take(&mut self.more_label.1);
        let w = self.measure(fs, &label, F_MORE, 200.0);
        let pa = smooth(0.0, 0.15, ma);
        let h = self.hover_of(Ctl::More);
        let xf = self.more_xf.get().clamp(0.0, 1.0);
        if xf < 1.0 && self.more_prev > 0 {
            let old = format!("+{} more", self.more_prev);
            let ow = self.measure(fs, &old, F_MORE, 200.0);
            self.label(fs, out, &old, F_MORE, r.x + r.w - 13.0 - ow, r.y + 14.0, 200.0, SOFT.fade(pa * (1.0 - xf)), clip);
        }
        self.label(fs, out, &label, F_MORE, r.x + r.w - 13.0 - w, r.y + 14.0, 200.0, SOFT.mix(TEXT, h).fade(pa * xf), clip);
        self.more_label.1 = label;
    }

    /// A history row's content at `rr`. `m` < 1 while it still morphs
    /// from its toast (avatar and text column travel from the card's).
    #[allow(clippy::too_many_arguments)]
    fn row_content(&mut self, fs: &mut FontSystem, out: &mut DrawList, n: &Note, rr: Rect, clip: Rect, ra: f32, h: f32, m: f32, interactive: bool) {
        if ra <= 0.003 {
            return;
        }
        if n.critical {
            self.shape(out, Rect::new(rr.x + 4.0, rr.y + 14.0, 3.0, rr.h - 28.0), clip, 1.5, |s| s.fill(BAD).alpha(ra));
        }
        let ic = lerp(ICON, 32.0, m);
        self.avatar(fs, out, n, Rect::new(rr.x + lerp(14.0, 12.0, m), rr.y + lerp(14.0, 13.0, m), ic, ic), lerp(10.0, 9.0, m), clip, ra);
        let tx = rr.x + lerp(TX, 56.0, m);
        let max = rr.w - (tx - rr.x) - 14.0;
        self.label(fs, out, &n.label, F_APP, tx, rr.y + 20.0, max - 60.0, MUTED.fade(ra), clip);
        let tw = self.measure(fs, &n.time, F_TIME, 60.0);
        self.label(fs, out, &n.time, F_TIME, rr.x + rr.w - 14.0 - tw, rr.y + 20.0, 60.0, MUTED.fade(ra * (1.0 - h)), clip);
        let x = Rect::new(rr.x + rr.w - 10.0 - 22.0, rr.y + 9.0, 22.0, 22.0);
        if interactive && h > 0.2 {
            self.hits.push((x, Ctl::RowClose(n.seq)));
        }
        if h > 0.003 {
            let xh = self.hover_of(Ctl::RowClose(n.seq));
            self.shape(out, x, clip, 11.0, |s| s.fill(HOVER).alpha(ra * h * xh));
            if crop(x, clip).is_some_and(|c| c.h > 18.0) {
                self.icon(out, "window-close-symbolic", x.x + 4.0, x.y + 4.0, 14.0, SOFT.mix(TEXT, xh).fade(ra * h));
            }
        }
        let (summary, body) = if n.summary.is_empty() { (n.line.as_str(), "") } else { (n.summary.as_str(), n.line.as_str()) };
        self.label(fs, out, summary, F_RSUM, tx, rr.y + 39.0, max, TEXT.fade(ra), clip);
        if !body.is_empty() {
            self.label(fs, out, body, F_RBODY, tx, rr.y + 57.0, max, SOFT.fade(ra), clip);
        }
    }

    /// One history row: body (a toast turning into a box-less row while it
    /// morphs), content, and a replacement's wipe.
    #[allow(clippy::too_many_arguments)]
    fn draw_row(&mut self, fs: &mut FontSystem, out: &mut DrawList, row: &Row, rr: Rect, bclip: Rect, clip: Rect, ra: f32, interactive: bool) {
        let sc = self.scale;
        let h = row.hover.get().clamp(0.0, 1.0);
        let m = if row.from.is_some() { row.m.get() } else { 1.0 };
        let k = m.clamp(0.0, 1.0);
        if let Some(b) = crop(rr, bclip) {
            let rad = 16.0f32.min(b.h * 0.5);
            // A toast (Prism Ink) turning into a box-less row: the ink and
            // its hairline fade as a faint wash takes over.
            if k < 1.0 {
                out.shapes.push(
                    paint::rect(sc, b, rad)
                        .fill(paint::INK)
                        .prism(paint::HAIR, 1.0)
                        .shadow(SHADOW, 18.0, 7.0)
                        .alpha(ra * (1.0 - k))
                        .done(),
                );
            }
            out.shapes.push(paint::rect(sc, b, rad).fill(ROW.mix(ROW_HOVER, h)).alpha(ra * k).done());
        }
        let live = interactive && !row.leaving;
        if live && let Some(hr) = crop(rr, bclip) {
            self.hits.push((hr, Ctl::Row(row.note.seq)));
        }
        let Some(clip) = crop(clip, rr.inflate(-1.0)) else { return };
        // Morphing from a card: the shared text (app, time, summary, first
        // line) is the row's own, travelling with it; what only the card
        // has (second line, buttons) is cut by the shrinking body and goes.
        let wa = ra;
        if let Some(l) = &row.card
            && m < 1.0
        {
            let ca = ra * (1.0 - smooth(0.0, 0.45, m));
            if ca > 0.003 {
                let tx = rr.x + lerp(TX, 56.0, k);
                self.label(fs, out, &l.l2, F_BODY, tx, rr.y + 77.0, rr.w - (tx - rr.x) - 14.0, SOFT.fade(ca), clip);
                self.card_buttons(fs, out, 0, &row.note, l, rr.x, rr.y, clip, ca, false);
            }
        }
        let n = row.note.clone();
        match &row.old {
            Some(old) => {
                let xf = row.xf.get().clamp(0.0, 1.0);
                let wx = lerp(rr.x - 4.0, rr.x + rr.w + 4.0, xf);
                if let Some(oc) = crop(clip, Rect::new(wx, clip.y, rr.w + 8.0, clip.h)) {
                    let old = old.clone();
                    self.row_content(fs, out, &old, rr, oc, wa, h, k, false);
                }
                if let Some(nc) = crop(clip, Rect::new(rr.x - 8.0, clip.y, wx - rr.x + 8.0, clip.h)) {
                    self.row_content(fs, out, &n, rr, nc, wa, h, k, live);
                }
                let s = (xf * std::f32::consts::PI).sin();
                self.shape(out, Rect::new(wx - 1.0, rr.y + 10.0, 2.0, rr.h - 20.0), clip, 1.0, |b| b.on_fill(0.0, 1.0, 0.0).alpha(wa * s));
            }
            None => self.row_content(fs, out, &n, rr, clip, wa, h, k, live),
        }
    }

    fn draw_history(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        let (pw, ph) = (self.pw.get(), self.ph.get());
        if pw <= 0.001 && ph <= 0.001 {
            self.panel = Rect::default();
            for r in &mut self.rows {
                r.rect = Rect::default();
            }
            return;
        }
        let sc = self.scale;
        let r = self.panel;
        let grow = ph.clamp(0.0, 1.0);
        // Only the first sliver out of the tab fades; after that the
        // content is at full brightness and revealed by the growing clip.
        let a = smooth(0.0, 0.1, pw.min(ph));
        out.shapes.push(
            paint::rect(sc, r, lerp(4.0, 24.0, grow).min(r.h * 0.5))
                .fill(paint::INK)
                .prism(paint::HAIR, grow)
                .shadow(SHADOW.fade(grow), 18.0 + 10.0 * grow, 6.0 + 6.0 * grow)
                .alpha(a)
                .done(),
        );
        out.glass(r, lerp(4.0, 24.0, grow).min(r.h * 0.5), a);
        if self.history {
            self.hits.push((r, Ctl::Panel));
        }
        let mut clip = r.inflate(-1.5);
        let tb = self.tab_bottom();
        if ph < 1.0 {
            let top = clip.y.max(tb);
            clip = Rect::new(clip.x, top, clip.w, (clip.y + clip.h - top).max(0.0));
        }
        // Content anchored to the panel's right and top edges (the tab's).
        let (ox, oy) = (r.x + r.w - HW, r.y);
        let interactive = self.history;

        // Header.
        let tw = self.label(fs, out, "Notifications", F_TITLE, ox + 20.0, oy + 29.0, 200.0, TEXT.fade(a), clip);
        // Count badge: the number cross-fades in place, the width springs,
        // and it shrinks away (with its last number) at zero.
        let shown = if self.count_n > 0 { self.count_n } else { self.count_prev };
        let ka = self.count_a.get().max(0.0);
        if shown > 0 && ka > 0.003 {
            let s = shown.to_string();
            let cw = self.measure(fs, &s, F_COUNT, 60.0);
            let target = cw + 14.0;
            if self.count_w.target != target {
                if self.count_w.get() < 1.0 {
                    self.count_w.snap(target);
                } else {
                    self.count_w.set(target);
                }
            }
            let (bw, bh) = (self.count_w.get() * ka, 20.0 * lerp(0.4, 1.0, ka.min(1.0)));
            let badge = Rect::new(ox + 20.0 + tw + 8.0, oy + 29.0 - bh * 0.5, bw, bh);
            let ba = a * smooth(0.0, 0.3, ka);
            self.shape(out, badge, clip, 7.0_f32.min(bh * 0.5), |b| b.fill(Rgba::hex(0xffffff, 0.07)).alpha(ba));
            if let Some(bc) = crop(badge, clip) {
                let xf = self.count_xf.get().clamp(0.0, 1.0);
                let mid = badge.x + badge.w * 0.5;
                if xf < 1.0 && self.count_prev > 0 && self.count_n > 0 {
                    let o = self.count_prev.to_string();
                    let ow = self.measure(fs, &o, F_COUNT, 60.0);
                    self.label(fs, out, &o, F_COUNT, mid - ow * 0.5, oy + 29.0, 60.0, SOFT.fade(ba * (1.0 - xf)), bc);
                }
                self.label(fs, out, &s, F_COUNT, mid - cw * 0.5, oy + 29.0, 60.0, SOFT.fade(ba * xf), bc);
            }
        }
        let shut = Rect::new(ox + HW - 14.0 - 30.0, oy + 14.0, 30.0, 30.0);
        if interactive {
            self.hits.push((shut, Ctl::Shut));
        }
        let sh = self.hover_of(Ctl::Shut);
        self.shape(out, shut, clip, 15.0, |s| s.fill(HOVER).alpha(a * sh));
        if crop(shut, clip).is_some_and(|c| c.h > 26.0) {
            self.icon(out, "window-close-symbolic", shut.x + 7.0, shut.y + 7.0, 16.0, SOFT.mix(TEXT, sh).fade(a));
        }
        let ca = self.empty_a.get();
        let clear_a = a * (1.0 - ca.clamp(0.0, 1.0));
        let cw = self.measure(fs, "Clear all", F_BTN, 120.0);
        let clear = Rect::new(shut.x - 8.0 - (cw + 24.0), oy + 14.0, cw + 24.0, 30.0);
        if clear_a > 0.05 {
            if interactive {
                self.hits.push((clear, Ctl::Clear));
            }
            let h = self.hover_of(Ctl::Clear);
            self.shape(out, clear, clip, 15.0, |s| s.fill(HOVER).alpha(clear_a * h));
            self.label(fs, out, "Clear all", F_BTN, clear.x + 12.0, clear.y + 15.0, 120.0, SOFT.mix(TEXT, h).fade(clear_a), clip);
        }
        let hair = Rect::new(ox + 16.0, oy + LIST_TOP - 6.0, HW - 32.0, 1.0);
        self.shape(out, hair, clip, 0.5, |s| s.fill(HAIR).alpha(a));

        // Rows.
        let list_top = oy + LIST_TOP;
        let lc = crop(Rect::new(r.x + 1.0, list_top - 4.0, r.w - 2.0, r.y + r.h - 6.0 - (list_top - 4.0)), clip).unwrap_or_default();
        let mut y = list_top - self.scroll.get();
        let rows = std::mem::take(&mut self.rows);
        let mut rows = rows;
        let mut flying = vec![];
        for (i, row) in rows.iter_mut().enumerate() {
            let k = row.lh.get().max(0.0);
            let rh = row.h.get();
            let slot = Rect::new(ox + 8.0 + row.dx.get(), y, HW - 16.0, rh);
            y += (rh + ROW_GAP) * k;
            // Rows without a source drop in a touch and wipe top-down.
            let e = row.enter.get().clamp(0.0, 1.0);
            let slot = Rect::new(slot.x, slot.y - 10.0 * (1.0 - e), slot.w, slot.h);
            if let Some(f) = row.from {
                let m = row.m.get();
                row.rect = lerp_rect(f, slot, m, m);
                flying.push(i);
                continue;
            }
            row.rect = slot;
            let ra = a * if row.leaving { 1.0 - smooth(0.1, 0.9, row.dx.get() / (HW * 0.8)) } else { 1.0 };
            if ra <= 0.003 || slot.y > lc.y + lc.h || slot.y + slot.h < lc.y {
                continue;
            }
            // Revealed by the entrance wipe; collapsing rows shrink with
            // their slot.
            let reveal = Rect::new(slot.x - 2.0, slot.y, slot.w + 4.0, rh * k.min(1.0) * e);
            let bclip = if e < 1.0 { crop(lc, reveal).unwrap_or_default() } else { lc };
            let cclip = crop(lc, reveal).unwrap_or_default();
            self.draw_row(fs, out, row, slot, bclip, cclip, ra, interactive);
        }
        // Rows still travelling from their toast fly over everything.
        let all = Rect::new(0.0, 0.0, SW, SH);
        for i in flying {
            let rr = rows[i].rect;
            let landed = rows[i].m.get() > 0.9;
            self.draw_row(fs, out, &rows[i], rr, all, all, 1.0, interactive && landed);
        }
        self.rows = rows;

        // Empty state, centred in the panel as it is now (it moves with
        // the fold instead of being cut by it).
        let ea = a * self.empty_a.get().clamp(0.0, 1.0);
        if ea > 0.003 {
            let cy = oy + LIST_TOP + (r.h - LIST_TOP).max(0.0) * 0.5 - 4.0;
            // Never over the header: cut at the hairline as the fold ends.
            let clip = crop(clip, Rect::new(clip.x, oy + LIST_TOP - 5.0, clip.w, SH)).unwrap_or_default();
            let s1 = "All caught up";
            let w1 = self.measure(fs, s1, F_RSUM, 300.0);
            self.label(fs, out, s1, F_RSUM, ox + (HW - w1) * 0.5, cy - 10.0, 300.0, SOFT.fade(ea), clip);
            let s2 = "New notifications will show up here";
            let w2 = self.measure(fs, s2, F_RBODY, 340.0);
            self.label(fs, out, s2, F_RBODY, ox + (HW - w2) * 0.5, cy + 11.0, 340.0, MUTED.fade(ea), clip);
        }
    }
}

impl WinScene for Toasts {
    fn size(&self) -> (f32, f32) {
        (SW, SH)
    }
    fn placement(&self) -> Placement {
        Placement::TopRight(MARGIN)
    }
    /// Toasts never take the keyboard (a click must not swallow what the
    /// user types next); the history panel does, for Escape.
    fn keyboard(&self) -> Keyboard {
        if self.history { Keyboard::Exclusive } else { Keyboard::None }
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }

    fn apply(&mut self, f: &Fact) {
        match f {
            Fact::Command(c) if c == "notifications" => {
                if self.history {
                    self.leave_history();
                } else {
                    self.enter_history();
                }
            }
            _ => match msg(f) {
                Some(Msg::New(n)) => {
                    PENDING.store(false, Ordering::Relaxed);
                    self.add(n.clone());
                }
                Some(Msg::Closed(id)) => {
                    if let Some(i) = self.toasts.iter().position(|t| t.note.id == *id && t.leave.is_none()) {
                        self.dismiss(i, None, Leave::Collapse);
                    }
                    if let Some(r) = self.rows.iter_mut().find(|r| r.note.id == *id && !r.leaving) {
                        r.leaving = true;
                    }
                }
                None => {}
            },
        }
    }

    fn set_scale(&mut self, s: f32) {
        self.scale = s;
        self.text.set_scale(s);
        SCALE_BITS.store(s.to_bits(), Ordering::Relaxed);
        for t in &mut self.toasts {
            t.lines = None;
        }
    }

    fn step(&mut self, dt: f32) -> bool {
        self.chrome.wake_at = None;
        let mut anim = self.chrome.step(dt);
        let now = self.chrome.time;

        // The layout moves under a still pointer (a card flies off, the
        // next one springs into its place): hover follows what is there.
        if self.press.is_none()
            && let Some((x, y)) = self.ptr
        {
            self.hover = self.hit(x, y);
        }

        // Pause while the stack is hovered or a card is held.
        let pause = !self.history && (self.hover.is_some() || self.press.is_some());
        if pause != self.paused {
            self.paused = pause;
            for t in &mut self.toasts {
                if pause { t.pause(now) } else { t.resume(now) }
            }
        }
        for t in &mut self.toasts {
            if t.start_pending {
                t.start_pending = false;
                if !self.paused {
                    t.resume(now);
                }
            }
        }
        for i in 0..self.toasts.len() {
            if self.toasts[i].deadline().is_some_and(|d| now >= d) {
                self.dismiss(i, Some(reason::EXPIRED), Leave::Retract);
            }
        }

        // Stack: the newest MAX_VIS cards show, the rest fold into "+N".
        let mut n = 0;
        for t in &mut self.toasts {
            if t.leave.is_none() {
                t.vis.set(if n < MAX_VIS { 1.0 } else { 0.0 });
                n += 1;
            }
        }
        let over = n.saturating_sub(MAX_VIS);
        if over > 0 && over != self.more_n {
            if self.more_n > 0 && self.more_a.get() > 0.5 {
                self.more_prev = self.more_n;
                self.more_xf.snap(0.0);
                self.more_xf.set(1.0);
            }
            self.more_n = over;
        }
        self.more_a.set(if over > 0 && !self.history { 1.0 } else { 0.0 });
        anim |= self.more_a.step(dt) | self.more_xf.step(dt);
        if self.more_a.at_rest() && self.more_a.get() == 0.0 {
            self.more_n = 0;
        }

        // Hovers.
        let hov_toast = self.hover.and_then(Ctl::toast);
        let hov_row = match self.hover {
            Some(Ctl::Row(s) | Ctl::RowClose(s)) => Some(s),
            _ => None,
        };
        if let Some(h) = self.hover
            && !self.hovs.iter().any(|(c, _)| *c == h)
        {
            self.hovs.push((h, Spring::new(0.0, presets::SNAPPY)));
        }
        for (c, s) in &mut self.hovs {
            s.set(if Some(*c) == self.hover { 1.0 } else { 0.0 });
            anim |= s.step(dt);
        }
        self.hovs.retain(|(_, s)| !(s.at_rest() && s.get() == 0.0));

        let pressed = self.press.as_ref().and_then(|p| if p.dragging { None } else { p.toast });
        let mut wake: Option<f32> = None;
        for t in &mut self.toasts {
            t.hover.set(if hov_toast == Some(t.key) && t.leave.is_none() { 1.0 } else { 0.0 });
            t.press.set(if pressed == Some(t.key) { 1.0 } else { 0.0 });
            anim |= t.springs(dt);
            // Frames only for the last stretch (the time line sweeping out);
            // until then a single wake-up.
            if let Some(d) = t.deadline() {
                if now >= d - DRAIN {
                    anim = true;
                } else {
                    wake = Some(wake.map_or(d - DRAIN, |w: f32| w.min(d - DRAIN)));
                }
            }
        }

        // History panel.
        for r in &mut self.rows {
            if r.leaving {
                if r.delay > 0.0 {
                    r.delay -= dt;
                    continue;
                }
                r.dx.set(HW);
                if !self.clearing {
                    r.lh.set(0.0);
                }
            }
        }
        // Clear all: rows keep their slots while they fly, then the
        // panel closes the gap in one move.
        if self.clearing && self.rows.iter().all(|r| r.leaving && r.delay <= 0.0 && r.dx.get() > HW * 0.85) {
            self.clearing = false;
            for r in &mut self.rows {
                r.lh.snap(0.0);
            }
        }
        for r in &mut self.rows {
            r.hover.set(if hov_row == Some(r.note.seq) && !r.leaving { 1.0 } else { 0.0 });
            anim |= r.springs(dt) | (r.delay > 0.0);
        }
        self.rows.retain(|r| !(r.leaving && r.delay <= 0.0 && r.lh.at_rest() && r.lh.get() == 0.0));
        let target = self.panel_target();
        if self.panel_h.target != target {
            self.panel_h.set(target);
        }
        if self.history {
            self.empty_a.set(if self.rows.is_empty() { 1.0 } else { 0.0 });
        }
        // Header count follows the rows as they start leaving.
        let live = self.rows.iter().filter(|r| !r.leaving).count();
        if live != self.count_n {
            if self.count_n > 0 {
                self.count_prev = self.count_n;
            }
            if self.count_n > 0 && live > 0 {
                self.count_xf.snap(0.0);
                self.count_xf.set(1.0);
            } else {
                self.count_xf.snap(1.0);
            }
            self.count_n = live;
        }
        self.count_a.set(if live > 0 { 1.0 } else { 0.0 });
        anim |= self.count_a.step(dt) | self.count_xf.step(dt) | self.count_w.step(dt);
        let max_scroll = (self.rows.iter().map(|r| if r.leaving { 0.0 } else { r.h.target + ROW_GAP }).sum::<f32>() + 4.0 - (self.panel_h.target - LIST_TOP)).max(0.0);
        if self.scroll.target > max_scroll {
            self.scroll.set(max_scroll);
        }
        anim |= self.pw.step(dt) | self.ph.step(dt) | self.panel_h.step(dt) | self.scroll.step(dt) | self.empty_a.step(dt);
        let panel_gone = !self.history && self.pw.at_rest() && self.ph.at_rest() && self.pw.get() == 0.0;
        if panel_gone && !self.rows.is_empty() {
            self.rows.clear();
        }
        if panel_gone {
            self.count_a.snap(0.0);
            self.count_n = 0;
            self.count_prev = 0;
        }

        let before = self.toasts.len();
        self.toasts.retain(|t| !t.finished());
        if self.toasts.len() != before {
            anim = true;
        }

        // Nothing left: fold the window away (the surface is destroyed
        // after this frame, which also frees the textures).
        if self.toasts.is_empty() && panel_gone && self.press.is_none() {
            self.chrome.closing = true;
            self.chrome.open_w.snap(0.0);
            self.chrome.open_h.snap(0.0);
            return false;
        }
        if !anim && let Some(w) = wake {
            self.chrome.wake(w);
        }
        anim
    }

    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        self.inuse.clear();
        if self.chrome.done() {
            out.frees.append(&mut self.uploaded);
            self.text.end_frame();
            return;
        }
        // The panel is drawn over the stack (it slides down over toasts).
        let (pw, ph) = (self.pw.get(), self.ph.get());
        self.panel = if pw <= 0.001 && ph <= 0.001 {
            Rect::default()
        } else {
            lerp_rect(self.start, Rect::new(SW - EDGE - HW, TOP, HW, self.panel_h.get()), pw, ph)
        };
        self.draw_toasts(fs, out);
        self.draw_history(fs, out);
        // Textures no longer shown go back.
        let mut i = 0;
        while i < self.uploaded.len() {
            let k = self.uploaded[i];
            let keep = self.inuse.contains(&k) || self.toasts.iter().any(|t| self.tex_key(&t.note) == k) || self.rows.iter().any(|r| self.tex_key(&r.note) == k);
            if keep {
                i += 1;
            } else {
                out.frees.push(self.uploaded.swap_remove(i));
            }
        }
        let mut b = self.panel;
        for t in &self.toasts {
            if t.alpha > 0.0 {
                b = if b.w <= 0.0 { t.rect } else { union(b, t.rect) };
            }
        }
        self.chrome.body = b;
        self.text.end_frame();
    }

    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    fn release_textures(&mut self, frees: &mut Vec<u64>) {
        frees.append(&mut self.uploaded);
    }

    fn input_region(&self) -> Vec<Rect> {
        let mut v: Vec<Rect> = self.toasts.iter().filter(|t| t.alpha > 0.05 && t.leave.is_none()).map(|t| t.rect.inflate(2.0)).collect();
        if self.more_rect.w > 0.0 {
            v.push(self.more_rect.inflate(2.0));
        }
        if self.panel.w > 0.0 && self.history {
            v.push(self.panel.inflate(2.0));
        }
        v
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.ptr = Some((x, y));
        if let Some(p) = &mut self.press {
            let dx = x - p.x0;
            if !p.dragging && p.toast.is_some() && dx.abs() > 6.0 && dx.abs() > (y - p.y0).abs() {
                p.dragging = true;
                p.x0 = x;
            }
            if p.dragging {
                let now = Instant::now();
                let dt = now.duration_since(p.last.0).as_secs_f32().max(1.0 / 240.0);
                p.vel = p.vel * 0.4 + (x - p.last.1) / dt * 0.6;
                p.last = (now, x);
                let off = x - p.x0;
                if let Some(t) = p.toast.and_then(|k| self.toasts.iter_mut().find(|t| t.key == k)) {
                    // Rubber band towards the screen's inside.
                    let off = if off < 0.0 { -(-off).powf(0.85) } else { off };
                    t.dx.snap(off);
                }
                return None;
            }
        }
        self.hover = self.hit(x, y);
        None
    }

    /// A leave while the button is held means the compositor broke the
    /// implicit grab (overview, keybind): the press is cancelled, so the
    /// timers run again and a dragged card springs home.
    fn pointer_leave(&mut self) {
        self.ptr = None;
        self.hover = None;
        if let Some(p) = self.press.take()
            && p.dragging
            && let Some(t) = p.toast.and_then(|k| self.toasts.iter_mut().find(|t| t.key == k))
        {
            t.dx.set(0.0);
        }
    }

    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        self.ptr = Some((x, y));
        let ctl = self.hit(x, y);
        // What is pressed is what is hovered, whatever moved since.
        self.hover = ctl;
        let toast = ctl.and_then(Ctl::toast);
        self.press = Some(Press { ctl, toast, x0: x, y0: y, last: (Instant::now(), x), vel: 0.0, dragging: false });
        None
    }

    fn release(&mut self) -> (Option<Action>, bool) {
        let Some(p) = self.press.take() else { return (None, false) };
        if p.dragging {
            if let Some(i) = p.toast.and_then(|k| self.toast_index(k)) {
                let t = &mut self.toasts[i];
                let off = t.dx.get();
                // Stale velocity (held still before letting go) doesn't count.
                let v = if p.last.0.elapsed().as_secs_f32() > 0.08 { 0.0 } else { p.vel };
                t.dx.vel = v;
                let thrown = (v.abs() > 650.0 && v.signum() == off.signum()) || off.abs() > CW * 0.38;
                if thrown {
                    self.dismiss(i, Some(reason::DISMISSED), Leave::Fly);
                } else {
                    t.dx.set(0.0);
                }
            }
            return (None, false);
        }
        if let Some(c) = p.ctl
            && self.hover == Some(c)
        {
            self.click(c);
        }
        (None, false)
    }

    fn scroll(&mut self, dy: f32) -> Option<Action> {
        if self.history {
            let max = (self.content_h() + 4.0 - (self.panel_h.target - LIST_TOP)).max(0.0);
            self.scroll.set((self.scroll.target + dy * 64.0).clamp(0.0, max));
        }
        None
    }

    /// Escape folds the history; toasts never see keys (no focus).
    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if k.sym == keys::ESCAPE && self.history {
            self.leave_history();
        }
        (None, false)
    }

    fn cursor(&self) -> Cursor {
        match (&self.press, self.hover) {
            (Some(p), _) if p.dragging => Cursor::Grabbing,
            (_, Some(Ctl::Row(s))) => {
                if self.rows.iter().any(|r| r.note.seq == s && r.note.default_action && r.note.live.load(Ordering::Relaxed)) {
                    Cursor::Pointer
                } else {
                    Cursor::Default
                }
            }
            (_, Some(Ctl::Panel)) => Cursor::Default,
            (_, Some(_)) => Cursor::Pointer,
            _ => Cursor::Default,
        }
    }

    fn close(&mut self) {
        if self.history {
            self.leave_history();
        } else {
            self.dismiss_all();
        }
    }

    fn reopen(&mut self) {
        self.chrome.closing = false;
        self.chrome.open_w.snap(1.0);
        self.chrome.open_h.snap(1.0);
        self.enter_history();
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

// ── demo ────────────────────────────────────────────────────────────────

/// Headless demo for `nimbus --snapshot` (NIMBUS_SNAPSHOT_ONLY=notify).
pub fn demo(d: &mut crate::snapshot::Demo) {
    demo::run(d);
}

mod demo {
    use super::*;
    use daemon::{Raw, RawImage};

    fn raw(id: u32, app: &str, icon: &str, summary: &str, body: &str) -> Raw {
        Raw {
            id,
            app_name: app.into(),
            app_icon: icon.into(),
            summary: summary.into(),
            body: body.into(),
            actions: vec![],
            urgency: 1,
            timeout: 20_000,
            image_data: None,
            image_path: None,
            desktop_entry: None,
            transient: false,
            resident: false,
        }
    }

    /// A soft gradient "avatar" delivered as image-data.
    fn avatar(w: i32, h: i32) -> RawImage {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let d = ((fx - 0.5).powi(2) + (fy - 0.38).powi(2)).sqrt();
                let head = d < 0.2;
                let body = ((fx - 0.5).powi(2) / 0.12 + (fy - 1.0).powi(2) / 0.2) < 1.0;
                let (r, g, b) = if head || body {
                    (250.0, 236.0, 226.0)
                } else {
                    (120.0 + 120.0 * fx, 140.0 + 40.0 * fy, 230.0 - 60.0 * fx)
                };
                data.extend_from_slice(&[r as u8, g as u8, b as u8]);
            }
        }
        RawImage { w, h, stride: w * 3, alpha: false, bps: 8, channels: 3, data }
    }

    fn note(r: Raw, seq: u64, time: &str) -> Arc<Note> {
        let mut n = daemon::build_demo(r, seq);
        n.time = time.into();
        Arc::new(n)
    }

    fn feed(w: &mut dyn WinScene, n: &Arc<Note>) {
        let _ = history_push(n);
        let f = Fact::Custom(Arc::new(Msg::New(n.clone())));
        wants_window(&f);
        w.apply(&f);
    }

    fn center(w: &Toasts, c: Ctl) -> Option<(f32, f32)> {
        w.hits.iter().rev().find(|(_, k)| *k == c).map(|(r, _)| (r.x + r.w * 0.5, r.y + r.h * 0.5))
    }

    /// Draw + step like the live loop. Texture uploads/frees go to the GPU
    /// with this window's text buffers.
    fn frame(d: &mut crate::snapshot::Demo, w: &mut Toasts, l: &mut DrawList) -> bool {
        w.draw(&mut d.gpu.fonts, l);
        if !l.uploads.is_empty() || !l.frees.is_empty() {
            let only = DrawList { uploads: std::mem::take(&mut l.uploads), frees: std::mem::take(&mut l.frees), ..Default::default() };
            d.render(&only, w.text_buffers(), (1.0, 1.0), "notify_scratch");
        }
        w.step(1.0 / 120.0)
    }

    fn tick(d: &mut crate::snapshot::Demo, w: &mut Toasts, secs: f32) {
        let mut l = DrawList::default();
        for _ in 0..(secs * 120.0).round() as u32 {
            frame(d, w, &mut l);
        }
    }

    /// Render the window over a stand-in for the bar's right tab.
    fn shot(d: &mut crate::snapshot::Demo, w: &mut Toasts, name: &str) {
        let mut l = DrawList::default();
        w.draw(&mut d.gpu.fonts, &mut l);
        let tab = paint::ink(w.scale, w.tab, 13.0)
            .shadow(Rgba::hex(0x000000, 0.5), 10.0, 4.0)
            .done();
        l.shapes.insert(0, tab);
        d.render(&l, w.text_buffers(), (SW, SH), name);
    }

    /// Run until nothing moves; returns the frame count.
    fn settle(d: &mut crate::snapshot::Demo, w: &mut Toasts, max: f32) -> u32 {
        let mut l = DrawList::default();
        let mut frames = 0;
        while frames < (max * 120.0) as u32 {
            frames += 1;
            if !frame(d, w, &mut l) {
                break;
            }
        }
        frames
    }

    pub fn run(d: &mut crate::snapshot::Demo) {
        if let Ok(mut h) = HISTORY.lock() {
            h.clear();
        }
        PENDING.store(true, Ordering::Relaxed);
        let mut w = {
            let mut ctx = d.ctx();
            let toasts = PENDING.swap(false, Ordering::Relaxed);
            Toasts::new(&mut ctx, !toasts)
        };
        SCALE_BITS.store(d.scale.to_bits(), Ordering::Relaxed);

        // 1. A first toast grows out of the right tab.
        let mut a = raw(1, "Firefox", "firefox", "Download complete", "nimbus-0.1.0.tar.gz (4.2 MB) finished downloading");
        a.actions = vec!["default".into(), "".into(), "open".into(), "Open".into(), "show".into(), "Show in folder".into()];
        feed(&mut w, &note(a, 1, "11:52"));
        for (i, t) in [0.0f32, 0.05, 0.05, 0.05, 0.07, 0.1, 0.15, 0.3].iter().enumerate() {
            tick(d, &mut w, *t);
            shot(d, &mut w, &format!("notify_morph_{i:02}"));
        }
        let f = settle(d, &mut w, 3.0);
        eprintln!("notify: first toast settled after {f} frames");

        // 2. More arrive: avatar via image-data, a critical one without an
        // icon, plain text, and one too many for the stack.
        let mut b = raw(2, "Signal", "", "Maya Lindqvist", "Are we still on for tonight? I booked the table at 8, the one by the window — bring the hard drive if you can");
        b.image_data = Some(avatar(96, 96));
        b.actions = vec!["default".into(), "".into(), "reply".into(), "Reply".into(), "read".into(), "Mark as read".into()];
        feed(&mut w, &note(b, 2, "11:53"));
        tick(d, &mut w, 0.12);
        shot(d, &mut w, "notify_second_mid");
        tick(d, &mut w, 0.6);
        let mut c = raw(3, "Power", "", "Battery critically low", "4% remaining. Plug in now or the system will suspend.");
        c.urgency = 2;
        feed(&mut w, &note(c, 3, "11:54"));
        tick(d, &mut w, 0.7);
        let mut k = raw(4, "kitty", "kitty", "cargo build finished", "nimbus (release) in 2m 48s");
        k.timeout = -1;
        feed(&mut w, &note(k, 4, "11:55"));
        tick(d, &mut w, 0.7);
        feed(&mut w, &note(raw(5, "Calendar", "org.gnome.Calendar", "Standup in 5 minutes", ""), 5, "11:55"));
        for i in 0..4 {
            tick(d, &mut w, 0.07);
            shot(d, &mut w, &format!("notify_overflow_{i:02}"));
        }
        tick(d, &mut w, 0.8);
        shot(d, &mut w, "notify_stack");
        let f = settle(d, &mut w, 3.0);
        eprintln!("notify: stack settled after {f} frames; wake at {:?} (now {:.2})", w.chrome.wake_at, w.chrome.time);

        // 3. Hover the second card: the stack pauses, × replaces the time.
        if let Some((x, y)) = w.toasts.get(1).map(|t| (t.rect.x + t.rect.w * 0.6, t.rect.y + t.rect.h * 0.5)) {
            w.pointer_motion(x, y);
        }
        tick(d, &mut w, 0.4);
        shot(d, &mut w, "notify_hover");
        if let Some((x, y)) = center(&w, Ctl::Close(w.toasts[1].key)) {
            w.pointer_motion(x, y);
        }
        tick(d, &mut w, 0.3);
        shot(d, &mut w, "notify_hover_close");

        // 4. Throw the top card sideways.
        let t0 = &w.toasts[0];
        let (x0, y0) = (t0.rect.x + 120.0, t0.rect.y + t0.rect.h * 0.5);
        w.pointer_motion(x0, y0);
        w.press(x0, y0);
        for i in 1..=8 {
            std::thread::sleep(std::time::Duration::from_millis(8));
            w.pointer_motion(x0 + i as f32 * 14.0, y0 + i as f32 * 0.5);
            tick(d, &mut w, 1.0 / 120.0);
        }
        shot(d, &mut w, "notify_drag");
        w.release();
        for i in 0..5 {
            tick(d, &mut w, 0.06);
            shot(d, &mut w, &format!("notify_fling_{i:02}"));
        }
        w.pointer_leave();
        let f = settle(d, &mut w, 3.0);
        eprintln!("notify: after fling settled in {f} frames, {} toasts", w.toasts.len());
        shot(d, &mut w, "notify_after_fling");

        // 5. Let the timers run out: the top card drains and retracts.
        let wake = w.chrome.wake_at.unwrap_or(w.chrome.time);
        let skip = (wake - w.chrome.time).max(0.0);
        // Idle until the wake (no frames), as the live loop would.
        w.chrome.time += skip;
        tick(d, &mut w, DRAIN * 0.6);
        shot(d, &mut w, "notify_drain");
        tick(d, &mut w, DRAIN * 0.4 + 0.12);
        shot(d, &mut w, "notify_retract_a");
        tick(d, &mut w, 0.12);
        shot(d, &mut w, "notify_retract_b");
        let f = settle(d, &mut w, 3.0);
        eprintln!("notify: after expiry settled in {f} frames, {} toasts", w.toasts.len());

        // A replacement updates a card in place (cross-fade).
        let mut b2 = raw(2, "Signal", "", "Maya Lindqvist", "Never mind, found it. See you at 8!");
        b2.image_data = Some(avatar(96, 96));
        feed(&mut w, &note(b2, 6, "11:56"));
        for i in 0..3 {
            tick(d, &mut w, 0.08);
            shot(d, &mut w, &format!("notify_replace_{i:02}"));
        }
        settle(d, &mut w, 2.0);

        // A lower card expiring folds shut in place (no throw).
        let mut x = raw(7, "Screenshot", "applets-screenshooter", "Screenshot saved", "~/Pictures/Screenshots/2026-10-25.png");
        x.timeout = 1500;
        feed(&mut w, &note(x, 7, "11:57"));
        tick(d, &mut w, 0.3);
        feed(&mut w, &note(raw(8, "Mail", "thunderbird", "Invoice #4411", "Your October invoice is ready"), 8, "11:57"));
        settle(d, &mut w, 2.0);
        if let Some(wk) = w.chrome.wake_at {
            w.chrome.time = w.chrome.time.max(wk);
        }
        tick(d, &mut w, DRAIN * 0.5);
        shot(d, &mut w, "notify_drain_low");
        tick(d, &mut w, DRAIN * 0.5 + 0.05);
        for i in 0..3 {
            tick(d, &mut w, 0.07);
            shot(d, &mut w, &format!("notify_collapse_{i:02}"));
        }
        settle(d, &mut w, 2.0);

        // × twice under a still pointer: the second click hits the card
        // that sprang into the first one's place.
        let before = w.toasts.len();
        if let Some(k) = w.toasts.first().map(|t| t.key) {
            if let Some((x, y)) = w.toasts.first().map(|t| (t.rect.x + 200.0, t.rect.y + 30.0)) {
                w.pointer_motion(x, y);
            }
            tick(d, &mut w, 0.3);
            if let Some((x, y)) = center(&w, Ctl::Close(k)) {
                w.pointer_motion(x, y);
                tick(d, &mut w, 0.2);
                w.press(x, y);
                w.release();
                settle(d, &mut w, 1.5);
                shot(d, &mut w, "notify_close_again");
                w.press(x, y);
                w.release();
            }
        }
        settle(d, &mut w, 1.5);
        eprintln!("notify: two × clicks in place: {before} -> {} toasts", w.toasts.len());
        w.pointer_leave();
        feed(&mut w, &note(raw(9, "Calendar", "org.gnome.Calendar", "Lunch with Sam", "12:30 at Kafé Lindt"), 9, "11:58"));
        settle(d, &mut w, 2.0);

        // 6. History: the panel grows from the tab, toasts morph into rows.
        w.apply(&Fact::Command("notifications".into()));
        for i in 0..6 {
            tick(d, &mut w, 0.05);
            shot(d, &mut w, &format!("notify_hist_open_{i:02}"));
        }
        let f = settle(d, &mut w, 3.0);
        eprintln!("notify: history settled after {f} frames, wake {:?}", w.chrome.wake_at);
        shot(d, &mut w, "notify_history");
        if let Some((x, y)) = w.rows.get(1).map(|r| (r.rect.x + r.rect.w * 0.5, r.rect.y + r.rect.h * 0.5)) {
            w.pointer_motion(x, y);
        }
        tick(d, &mut w, 0.4);
        shot(d, &mut w, "notify_history_hover");
        if let Some(s) = w.rows.get(1).map(|r| r.note.seq)
            && let Some((x, y)) = center(&w, Ctl::RowClose(s))
        {
            w.pointer_motion(x, y);
            w.press(x, y);
            w.release();
        }
        tick(d, &mut w, 0.15);
        shot(d, &mut w, "notify_history_dismiss");
        settle(d, &mut w, 2.0);
        // A replacement while the history is open wipes its row in place.
        let mut c2 = raw(3, "Power", "", "Battery low", "12% remaining");
        c2.urgency = 2;
        feed(&mut w, &note(c2, 10, "11:59"));
        for i in 0..3 {
            tick(d, &mut w, 0.08);
            shot(d, &mut w, &format!("notify_row_replace_{i:02}"));
        }
        settle(d, &mut w, 2.0);
        if let Some((x, y)) = center(&w, Ctl::Clear) {
            w.pointer_motion(x, y);
            w.press(x, y);
            w.release();
        }
        w.pointer_leave();
        tick(d, &mut w, 0.12);
        shot(d, &mut w, "notify_clear_mid");
        settle(d, &mut w, 3.0);
        shot(d, &mut w, "notify_history_empty");
        w.apply(&Fact::Command("notifications".into()));
        for i in 0..3 {
            tick(d, &mut w, 0.06);
            shot(d, &mut w, &format!("notify_hist_close_{i:02}"));
        }
        let f = settle(d, &mut w, 3.0);
        let mut l = DrawList::default();
        w.draw(&mut d.gpu.fonts, &mut l);
        eprintln!("notify: closed after {f} frames, done={} frees={}", w.chrome.done(), l.frees.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daemon::tests::Rng;

    /// The card's two lines from hostile bodies (huge, combining marks,
    /// bidi controls, emoji, NULs, newlines everywhere): never panics and
    /// never hands the UI thread more than it can shape cheaply.
    #[test]
    fn fuzz_wrap2() {
        let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), glyphon::fontdb::Database::new());
        crate::fonts::load(fs.db_mut(), &["Geist"]);
        let mut r = Rng(0x5eed_1234_abcd_0042);
        let mut bodies: Vec<String> = (0..400).map(|_| r.text(300)).collect();
        bodies.push("x".repeat(1 << 20));
        bodies.push("\n".repeat(5000));
        bodies.push("\u{301}".repeat(20_000));
        bodies.push("🎉".repeat(30_000));
        bodies.push(format!("{}\n{}", "a ".repeat(100_000), "b".repeat(100_000)));
        for (i, b) in bodies.iter().enumerate() {
            let scale = [1.0, 1.25, 2.0, 3.0][i % 4];
            let (l1, l2) = wrap2(&mut fs, b, F_BODY, CW - TX - 16.0, scale);
            assert!(l1.chars().count() <= 400, "l1 {}", l1.len());
            assert!(l2.chars().count() <= L2_MAX + 1, "l2 {}", l2.chars().count());
            assert!(!l2.contains('\n'));
        }
    }
}
