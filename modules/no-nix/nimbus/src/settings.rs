//! Quick settings: the centre tab pops out into a control centre. It
//! detaches from the top edge and becomes a floating window at the top
//! middle of the screen, rounded on all sides; on close it tucks back up
//! into the edge. Lives on its own short-lived layer surface.
//!
//!   ╭──────────────────────────────────────────╮
//!   │ 11:56                  ▭ 60%   ▥   ▣   ⚙  │  clock · battery · monitor · wallpaper · settings
//!   │ Saturday, 25 October · ☀ 13°               │  (date opens the calendar; hover ☀: forecast)
//!   │ ─────────────────────────────────────────  │
//!   │ ▉▉▉  Title                                 │  now playing
//!   │ ▉▉▉  Artist                                │
//!   │ ▉▉▉  ⏮  ⏯  ⏭                               │
//!   │ 1:23 ━━━━━━━━━──────────────────── -2:07   │
//!   │ ─────────────────────────────────────────  │
//!   │ (◉) Wi-Fi     (◉) Bluetooth   (◉) Mute     │  tap Wi-Fi / Bluetooth: their page
//!   │  (◉)    (◉)    (◉)    (◉)    (◉)           │  night light, caffeine, airplane,
//!   │  Night  Caff.  Airpl. Shot   Record        │  screenshot, record
//!   │ ─────────────────────────────────────────  │
//!   │ 🔊 ━━━━━━━━━━━━━━━━━━━○─────────── 62%     │
//!   │ ☀  ━━━━━━━━━━━━━━━━━━━━━━━━○────── 60%     │
//!   │ ─────────────────────────────────────────  │
//!   │    🔒       ☾        ↻        ⏻            │  restart / power off arm, then confirm
//!   ╰──────────────────────────────────────────╯
//!
//! Prism Ink, box-less: the panel is flat matte ink edged by the prism
//! hairline; nothing inside sits in a box. Sections are split by hairline
//! dividers, toggles are round dots (faint white off, iridescent on),
//! sliders a thin track with an iridescent fill and a clear glass knob,
//! buttons bare icons with a faint circle only under the pointer. The
//! panel grows down out from under the centre tab (which stays on the
//! bar) and folds back into it on close: width leads, the drop follows
//! with a little overshoot, and modules appear in a quick cascade once
//! there is room. When nothing plays, the now-playing section folds down
//! to a calm one-liner and the window shortens with it. The playback
//! position advances on wake timers (once a second, only while open and
//! playing).

use std::{collections::HashMap, rc::Rc, sync::Arc};

use glyphon::FontSystem;

use crate::{
    anim::{ColorSpring, Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    gfx::{self, DrawList, IconDraw, ImageDraw, TextDraw},
    icons::IconSet,
    media::{Art, MediaInfo, TexSwap},
    paint,
    services::{BatteryInfo, BtDev, ClockInfo, Fact, NetInfo, SystemState, VolumeInfo, WifiNet},
    theme::{self, Rgba},
};

/// Popup surface size (logical); the panel plus room for its shadow.
pub const SW: f32 = 540.0;
pub const SH: f32 = 660.0;
/// The detached window floats just below the 32 px bar, so the centre tab
/// stays visible while it is open.
const DETACH_Y: f32 = 44.0;
const PW: f32 = 420.0;
/// Matches the deck's tab rounding, so the morph starts seamless.
const R_TAB: f32 = 13.0;
const R_OPEN: f32 = 30.0;
const PAD: f32 = 20.0;

// vertical layout (relative to the window's top edge)
/// Header block; the first hairline divider sits at its bottom.
const HEAD_H: f32 = 84.0;
/// Breathing room on either side of a divider.
const SEC: f32 = 16.0;
const MEDIA_H: f32 = 100.0;
const MEDIA_IDLE_H: f32 = 34.0;
const ART: f32 = 68.0;
const ART_R: f32 = 14.0;
/// Connectivity row and its round dots.
const CONN_H: f32 = 48.0;
const CONN_DOT: f32 = 38.0;
const SLIDER_H: f32 = 32.0;
const POWER_H: f32 = 40.0;
/// Quick toggles row (night light, caffeine, airplane, screenshot,
/// record): 42 px dots with their labels underneath.
const QUICK_H: f32 = 64.0;
const QUICK_DOT: f32 = 42.0;
const F_TINY: Font = Font::new(10.5, 500).family("Geist");
const F_CONN: Font = Font::new(12.5, 600).family("Geist");
const F_SUB: Font = Font::new(11.0, 500).family("Geist");
const F_BAT: Font = Font::new(12.0, 500).family("Geist");
const F_CAL_TITLE: Font = Font::new(15.0, 600).family("Geist");
const F_CAL: Font = Font::new(13.0, 500).family("Geist");

const F_CLOCK: Font = Font::new(32.0, 600).family("Geist");
const F_DATE: Font = Font::new(12.5, 500).family("Geist");
const F_LABEL: Font = Font::new(13.0, 600).family("Geist");
const F_SMALL: Font = Font::new(12.0, 500).family("Geist");
const F_SMALL_B: Font = Font::new(12.0, 600).family("Geist");
const F_NP_TITLE: Font = Font::new(14.0, 600).family("Geist");
const F_NP_ARTIST: Font = Font::new(12.5, 500).family("Geist");
const F_TIME: Font = Font::new(11.0, 500).family("Geist");
const F_VAL: Font = Font::new(11.5, 500).family("Geist");
const F_PAGE: Font = Font::new(15.0, 600).family("Geist");

const SHADOW: Rgba = Rgba::hex(0x000000, 0.55);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
/// Hover circles and rows: a faint white, nothing at rest.
const HOVER: Rgba = Rgba::hex(0xffffff, 0.08);
const TRACK: Rgba = Rgba::hex(0xffffff, 0.14);
const GOOD: Rgba = Rgba::hex(0x7ee2b8, 1.0);
const WARN: Rgba = Rgba::hex(0xffd37a, 1.0);
const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);
/// Recording "on": a soft red, never iridescent.
const REC_RED: Rgba = Rgba::hex(0xff6b7f, 1.0);
/// Shutdown hover / armed: a soft red tint and its glyph colour.
const DANGER: Rgba = Rgba::hex(0xff5a6e, 0.32);
const DANGER_FG: Rgba = Rgba::hex(0xffb8c1, 1.0);
/// A detail page's card.
const PAGE_BG: Rgba = Rgba::hex(0xffffff, 0.035);
const ROW_H: f32 = 38.0;
/// Detail page morph: the tile grows into the page.
const PAGE: SpringCfg = SpringCfg::new(0.5, 0.86);

/// Width opens first and quickly; height follows a beat later.
const OPEN_W: SpringCfg = SpringCfg::new(0.42, 0.82);
/// The drop: a touch springy so the window visibly pops loose.
const OPEN_H: SpringCfg = SpringCfg::new(0.58, 0.72);
const ARM_TIMEOUT: f32 = 3.0;
/// Where a flung window glides to, and how it flies home on close.
const GLIDE: SpringCfg = SpringCfg::new(0.45, 0.9);
/// Seconds of momentum carried by a fling.
const FLING: f32 = 0.12;
/// The now-playing module folding open / shut.
const FOLD: SpringCfg = SpringCfg::new(0.42, 0.9);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ctl {
    None,
    Tab,
    Body,
    Back,
    Switch,
    Row(u8),
    /// The drag handle at the top of the window.
    Grip,
    /// Opens the full settings window.
    Gear,
    /// Opens the wallpaper selector.
    Wallpaper,
    /// Opens the system monitor.
    Monitor,
    Volume,
    Brightness,
    Wifi,
    Bluetooth,
    Mute,
    Lock,
    Suspend,
    Reboot,
    PowerOff,
    /// The now-playing module (scroll: seek).
    NowPlaying,
    MediaPrev,
    MediaPlay,
    MediaNext,
    /// The playback position bar (drag: seek).
    Scrub,
    /// Clock and date (opens the calendar).
    Date,
    /// The temperature beside the clock (hover: forecast).
    Weather,
    /// Quick toggles.
    Night,
    Caffeine,
    Airplane,
    Shot,
    Record,
    /// Calendar: previous / next month, the grid (scroll or drag).
    CalPrev,
    CalNext,
    CalGrid,
}

/// Which view the panel shows. Wi-Fi and Bluetooth tiles open a page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Main,
    Wifi,
    Bluetooth,
    Calendar,
}

#[derive(Debug)]
pub enum Action {
    Volume(u8),
    Brightness(u8),
    Mute,
    Wifi(bool),
    Bluetooth(bool),
    ScanWifi,
    ScanBluetooth,
    WifiConnect(String),
    WifiDisconnect(String),
    BtConnect(String),
    BtDisconnect(String),
    Lock,
    Suspend,
    Reboot,
    PowerOff,
    // full settings window
    OpenPrefs,
    SetDesign(crate::designs::Design),
    SetAccent(usize),
    SetSink(u32),
    MicVolume(u8),
    MicMute,
    SetScale(String, f64),
    ReadAudio,
    ReadOutputs,
    ReadPower,
    ReadAbout,
    /// Open (or toggle) one of the generic windows by command name.
    Open(&'static str),
    // now playing (MPRIS)
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    /// Relative seek, seconds.
    MediaSeek(f32),
    // quick toggles
    NightLight(bool),
    Caffeine(bool),
    Airplane(bool),
    Screenshot,
    Record(bool),
}

pub struct Settings {
    pub scale: f32,
    text: TextCache,
    icons: Rc<IconSet>,
    /// Where the panel grows from: a sliver along the centre tab's bottom
    /// edge (this surface's coordinates), so it comes out from under the
    /// tab, which stays on the bar.
    start: Rect,
    pw: Spring,
    ph: Spring,
    closing: bool,
    pub time: f32,

    clock: Option<ClockInfo>,
    battery: Option<BatteryInfo>,
    net: NetInfo,
    volume: Option<VolumeInfo>,
    system: SystemState,
    vol: Spring,
    bri: Spring,

    // ── now playing ──
    media: Option<MediaInfo>,
    /// Panel-clock time at which `media.position` held.
    media_t0: f32,
    /// What the module shows (kept while it folds away).
    shown: Option<MediaInfo>,
    art: Option<Arc<Art>>,
    cover: TexSwap,
    media_k: Spring,
    playing: Spring,
    tint_a: ColorSpring,
    tint_b: ColorSpring,
    /// Track text cross-fade: previous (title, artist) and progress.
    prev_track: Option<(String, String)>,
    track_xf: Spring,
    /// Position (0..1) while the scrub bar is dragged.
    scrub: Option<f32>,
    btn_x: Spring,
    btn_a: Spring,

    hover: HashMap<Ctl, Spring>,
    hovered: Ctl,
    pressed: Ctl,
    dragging: Ctl,
    armed: Option<(Ctl, f32)>,
    arm: HashMap<Ctl, Spring>,

    /// Target page; `shown` lags until the morph back finishes.
    page: Page,
    shown_page: Page,
    page_t: Spring,
    /// The tile the page grew out of.
    origin: Rect,
    origin_on: bool,
    wifi_nets: Option<Vec<WifiNet>>,
    bt_devs: Option<Vec<BtDev>>,
    /// SSID / MAC currently being (dis)connected.
    pending: Option<String>,
    switch_t: Spring,

    toggles: crate::toggles::ToggleState,
    weather: Option<crate::weather::Weather>,
    /// Calendar: today (y, m 1..=12, d), the month shown, its slide-in
    /// (0 → 1, from `cal_dir`), and the live drag offset.
    today: (i32, u32, u32),
    cal: (i32, u32),
    cal_slide: Spring,
    cal_dir: f32,
    cal_drag: Option<(f32, f32)>,

    // Dragging: the window's offset from its home under the tab, in
    // logical px. The surface moves by whole pixels (layer margins); the
    // fraction is drawn inside it so motion stays sub-pixel smooth.
    off_x: Spring,
    off_y: Spring,
    /// Whole-pixel surface offset currently committed (set by main).
    pub surf_origin: (i32, i32),
    /// Allowed offset range: (min_x, max_x, max_y).
    bounds: (f32, f32, f32),
    grabbing: bool,
    fling: (f32, f32),
    last_move: Option<(f32, f32, f32)>,
    hits: Vec<(Rect, Ctl)>,
    /// Current body, for the input region.
    pub body: Rect,
    pub wake_at: Option<f32>,
}

impl Drop for Settings {
    fn drop(&mut self) {
        let k = self.cover.keys();
        if !k.is_empty() {
            gfx::free_later(k);
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mtime(s: f32) -> String {
    let s = s.max(0.0).round() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Rows of the open panel (offsets from its top edge).
struct Lay {
    div1: f32,
    media: f32,
    mh: f32,
    div2: f32,
    conn: f32,
    quick: f32,
    div3: f32,
    vol: f32,
    bri: Option<f32>,
    div4: f32,
    power: f32,
    bottom: f32,
}

/// A round toggle dot: faint white when off, iridescent (pastel) when on.
fn dot(sc: f32, cx: f32, cy: f32, r: f32, on: bool, h: f32) -> paint::ShapeB {
    let c = paint::circle(sc, cx, cy, r);
    if on {
        let lift = if theme::prism() { 0.2 } else { 0.0 } + 0.08 * h;
        c.on_fill(paint::IRID.0, paint::IRID.1, lift)
    } else {
        c.fill(paint::FAINT.fade(1.0 + 0.6 * h))
    }
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    (x1 - x0 > 0.3 && y1 - y0 > 0.3).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

impl Settings {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fs: &mut FontSystem,
        icons: Rc<IconSet>,
        scale: f32,
        start: Rect,
        title: Option<String>,
        facts: &[Fact],
    ) -> Self {
        let mut text = TextCache::new(fs, theme::current());
        text.set_scale(scale);
        let mut pw = Spring::new(0.0, OPEN_W);
        let mut ph = Spring::new(0.0, OPEN_H);
        pw.set(1.0);
        ph.set(1.0);
        let (aa, ab) = theme::accent();
        let _ = title;
        let start = Rect::new(start.x + 10.0, start.y + start.h, (start.w - 20.0).max(4.0), 4.0);
        let mut s = Self {
            scale,
            text,
            icons,
            start,
            pw,
            ph,
            closing: false,
            time: 0.0,
            clock: None,
            battery: None,
            net: NetInfo::Offline,
            volume: None,
            system: SystemState::default(),
            vol: Spring::new(0.0, presets::SNAPPY),
            bri: Spring::new(0.0, presets::SNAPPY),
            media: None,
            media_t0: 0.0,
            shown: None,
            art: None,
            cover: TexSwap::default(),
            media_k: Spring::new(0.0, FOLD),
            playing: Spring::new(0.0, presets::SMOOTH),
            tint_a: ColorSpring::new(aa, presets::SMOOTH),
            tint_b: ColorSpring::new(ab, presets::SMOOTH),
            prev_track: None,
            track_xf: Spring::new(1.0, presets::SMOOTH),
            scrub: None,
            btn_x: Spring::new(1.0, presets::SNAPPY),
            btn_a: Spring::new(0.0, presets::SNAPPY),
            hover: HashMap::new(),
            hovered: Ctl::None,
            pressed: Ctl::None,
            dragging: Ctl::None,
            armed: None,
            arm: HashMap::new(),
            page: Page::Main,
            shown_page: Page::Main,
            page_t: Spring::new(0.0, PAGE),
            origin: Rect::default(),
            origin_on: false,
            wifi_nets: None,
            bt_devs: None,
            pending: None,
            switch_t: Spring::new(0.0, presets::SNAPPY),
            toggles: Default::default(),
            weather: None,
            today: today(),
            cal: (today().0, today().1),
            cal_slide: Spring::new(1.0, presets::SNAPPY),
            cal_dir: 1.0,
            cal_drag: None,
            off_x: Spring::new(0.0, GLIDE),
            off_y: Spring::new(0.0, GLIDE),
            surf_origin: (0, 0),
            bounds: (0.0, 0.0, 0.0),
            grabbing: false,
            fling: (0.0, 0.0),
            last_move: None,
            hits: vec![],
            body: start,
            wake_at: None,
        };
        for f in facts {
            s.apply(f);
        }
        s.vol.snap(s.vol.target);
        s.bri.snap(s.bri.target);
        // Opens with the module already in its final shape.
        s.media_k.snap(s.media_k.target);
        s.playing.snap(s.playing.target);
        s.track_xf.snap(1.0);
        s.prev_track = None;
        s.settle_tint(true);
        s
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.text.set_scale(scale);
    }

    pub fn apply(&mut self, f: &Fact) {
        match f {
            Fact::Clock(c) => {
                self.clock = Some(*c);
                self.today = today();
            }
            Fact::Battery(b) => self.battery = *b,
            Fact::Net(n) => self.net = *n,
            Fact::Volume(v) => {
                self.volume = Some(*v);
                if self.dragging != Ctl::Volume {
                    self.vol.set(if v.muted { 0.0 } else { v.percent.min(100) as f32 / 100.0 });
                }
            }
            Fact::System(s) => {
                self.system = *s;
                if let Some(b) = s.brightness
                    && self.dragging != Ctl::Brightness
                {
                    self.bri.set(b);
                }
            }
            Fact::WifiList(n) => {
                if self.pending.as_ref().is_some_and(|p| n.iter().any(|x| &x.ssid == p && x.active)) {
                    self.pending = None;
                }
                self.wifi_nets = Some(n.clone());
            }
            Fact::BtList(d) => {
                self.pending = None;
                self.bt_devs = Some(d.clone());
            }
            Fact::Media(m) => {
                self.media_t0 = self.time - m.as_ref().map_or(0.0, |m| m.stamp.elapsed().as_secs_f32());
                if let (Some(new), Some(old)) = (m, &self.shown)
                    && (new.title != old.title || new.artist != old.artist)
                {
                    self.prev_track = Some((old.title.clone(), old.artist.clone()));
                    self.track_xf.snap(0.0);
                    self.track_xf.set(1.0);
                }
                if let Some(m) = m {
                    self.shown = Some(m.clone());
                    self.playing.set(if m.playing { 1.0 } else { 0.0 });
                }
                self.media_k.set(if m.is_some() { 1.0 } else { 0.0 });
                self.media = m.clone();
                self.settle_tint(false);
            }
            Fact::MediaArt(a) => {
                self.art = Some(a.clone());
                self.settle_tint(false);
            }
            Fact::Niri(_)
            | Fact::Audio(_)
            | Fact::Outputs(_)
            | Fact::Power(_)
            | Fact::About(_)
            | Fact::Command(_)
            | Fact::Custom(_)
            | Fact::AppIcon(..)
            | Fact::Tray(_) => {}
            Fact::Toggles(t) => self.toggles = *t,
            Fact::Weather(w) => self.weather = w.clone(),
        }
    }

    /// The cover's colours, or the accent while there are none.
    fn settle_tint(&mut self, snap: bool) {
        let tint = self.shown.as_ref().and_then(|s| s.art_url.as_ref()).and_then(|u| self.art.as_ref().filter(|a| &a.url == u)).and_then(|a| a.tint);
        let (a, b) = tint.unwrap_or_else(theme::accent);
        if snap {
            self.tint_a = ColorSpring::new(a, presets::SMOOTH);
            self.tint_b = ColorSpring::new(b, presets::SMOOTH);
        } else {
            self.tint_a.set(a);
            self.tint_b.set(b);
        }
    }

    /// Estimated playback position now (seconds).
    fn media_pos(&self) -> f32 {
        let Some(m) = self.media.as_ref().or(self.shown.as_ref()) else { return 0.0 };
        let p = if m.playing && self.media.is_some() { m.position + (self.time - self.media_t0).max(0.0) * m.rate } else { m.position };
        if m.length > 0.0 { p.clamp(0.0, m.length) } else { p.max(0.0) }
    }

    /// Escape: leave a detail page first, then close.
    pub fn escape(&mut self) {
        if self.page != Page::Main {
            self.page = Page::Main;
            self.page_t.set(0.0);
        } else {
            self.close();
        }
    }

    fn open_page(&mut self, page: Page, c: Ctl) -> Option<Action> {
        self.origin = self.hits.iter().find(|(_, h)| *h == c).map_or(self.origin, |(r, _)| *r);
        self.origin_on = match page {
            Page::Wifi => self.system.wifi == Some(true),
            _ => self.system.bluetooth == Some(true),
        };
        self.page = page;
        self.shown_page = page;
        self.page_t.set(1.0);
        match page {
            Page::Wifi => Some(Action::ScanWifi),
            Page::Bluetooth => Some(Action::ScanBluetooth),
            _ => None,
        }
    }

    pub fn close(&mut self) {
        if !self.closing {
            self.closing = true;
            self.pw.set(0.0);
            self.ph.set(0.0);
            self.grabbing = false;
            self.off_x.set(0.0);
            self.off_y.set(0.0);
            self.dragging = Ctl::None;
            self.scrub = None;
            self.armed = None;
        }
    }

    /// Clicked again mid-close: open back up from wherever it is.
    pub fn reopen(&mut self) {
        self.closing = false;
        self.pw.set(1.0);
        self.ph.set(1.0);
    }

    pub fn text_buffers(&self) -> &std::collections::HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    pub fn closing(&self) -> bool {
        self.closing
    }

    /// How much the bar's centre tab should be hidden: never, the panel
    /// grows out from under it and folds back into it.
    pub fn tab_cover(&self) -> f32 {
        0.0
    }

    /// Fully folded back into the tab: the surface can go.
    pub fn done(&self) -> bool {
        self.closing && self.pw.at_rest() && self.ph.at_rest() && self.off_x.at_rest() && self.off_y.at_rest()
    }

    /// Offset range that keeps the surface on the output.
    pub fn set_bounds(&mut self, min_x: f32, max_x: f32, max_y: f32) {
        self.bounds = (min_x, max_x, max_y.max(0.0));
    }

    /// Whole-pixel surface offset to commit for the current frame.
    pub fn margin(&self) -> (i32, i32) {
        (self.off_x.get().floor() as i32, self.off_y.get().floor() as i32)
    }

    fn clamp_off(&self, x: f32, y: f32) -> (f32, f32) {
        let (lo, hi, my) = self.bounds;
        (x.clamp(lo, hi), y.clamp(0.0, my))
    }

    /// Relative pointer motion while the window is grabbed.
    pub fn drag_by(&mut self, dx: f32, dy: f32) {
        if !self.grabbing {
            return;
        }
        let (nx, ny) = self.clamp_off(self.off_x.get() + dx, self.off_y.get() + dy);
        if let Some((t, px, py)) = self.last_move {
            let dt = (self.time - t).max(1.0 / 240.0);
            // Smoothed velocity for the fling.
            self.fling.0 = self.fling.0 * 0.6 + (nx - px) / dt * 0.4;
            self.fling.1 = self.fling.1 * 0.6 + (ny - py) / dt * 0.4;
        }
        self.last_move = Some((self.time, nx, ny));
        self.off_x.snap(nx);
        self.off_y.snap(ny);
    }

    pub fn grabbing(&self) -> bool {
        self.grabbing
    }

    /// Hovering something that drags the window rather than a control.
    pub fn over_grip(&self) -> bool {
        matches!(self.hovered, Ctl::Grip | Ctl::Body)
    }

    pub fn step(&mut self, dt: f32) -> bool {
        self.time += dt;
        self.wake_at = None;
        if let Some((_, at)) = self.armed {
            if self.time >= at + ARM_TIMEOUT {
                self.armed = None;
            } else {
                self.wake_at = Some(at + ARM_TIMEOUT);
            }
        }
        // Playing: tick once a second for the position (open panel only).
        if !self.closing
            && self.scrub.is_none()
            && let Some(m) = &self.media
            && m.playing
        {
            let pos = self.media_pos();
            if m.length <= 0.0 || pos < m.length {
                let next = self.time + ((pos.floor() + 1.0 - pos) / m.rate.max(0.1)).max(0.05);
                self.wake_at = Some(self.wake_at.map_or(next, |w| w.min(next)));
            }
        }
        if self.hovered != Ctl::None {
            self.hover.entry(self.hovered).or_insert(Spring::new(0.0, presets::SNAPPY));
        }
        if let Some((c, _)) = self.armed {
            self.arm.entry(c).or_insert(Spring::new(0.0, presets::SNAPPY));
        }
        for (c, h) in self.hover.iter_mut() {
            h.set(if self.hovered == *c && !self.closing { 1.0 } else { 0.0 });
        }
        for (c, a) in self.arm.iter_mut() {
            a.set(if self.armed.is_some_and(|(x, _)| x == *c) { 1.0 } else { 0.0 });
        }
        let on = match self.shown_page {
            Page::Wifi => self.system.wifi == Some(true),
            Page::Bluetooth => self.system.bluetooth == Some(true),
            Page::Main | Page::Calendar => false,
        };
        self.switch_t.set(if on { 1.0 } else { 0.0 });
        let btn = match self.hovered {
            Ctl::MediaPrev => Some(0.0),
            Ctl::MediaPlay => Some(1.0),
            Ctl::MediaNext => Some(2.0),
            _ => None,
        };
        match btn {
            Some(x) => {
                if self.btn_a.get() < 0.05 {
                    self.btn_x.snap(x);
                }
                self.btn_x.set(x);
                self.btn_a.set(1.0);
            }
            None => self.btn_a.set(0.0),
        }
        let mut m = self.pw.step(dt) | self.ph.step(dt) | self.vol.step(dt) | self.bri.step(dt);
        m |= self.page_t.step(dt) | self.switch_t.step(dt) | self.cal_slide.step(dt);
        m |= self.off_x.step(dt) | self.off_y.step(dt);
        m |= self.media_k.step(dt) | self.playing.step(dt) | self.track_xf.step(dt) | self.btn_x.step(dt) | self.btn_a.step(dt);
        m |= self.tint_a.step(dt) | self.tint_b.step(dt) | self.cover.t.step(dt);
        if self.page == Page::Main && self.page_t.at_rest() {
            self.shown_page = Page::Main;
        }
        for s in self.hover.values_mut().chain(self.arm.values_mut()) {
            m |= s.step(dt);
        }
        // Settled springs are dropped so the maps stay tiny.
        self.hover.retain(|c, s| !s.at_rest() || s.target > 0.0 || *c == self.hovered);
        // The player went and the module has folded: forget it.
        if self.media.is_none() && self.media_k.at_rest() && self.shown.is_some() {
            self.shown = None;
            self.prev_track = None;
            self.cover.clear();
        }
        m
    }

    // ── input ───────────────────────────────────────────────────────────

    fn hit(&self, x: f32, y: f32) -> Ctl {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Ctl::None, |(_, c)| *c)
    }

    pub fn clickable(&self) -> bool {
        !matches!(self.hovered, Ctl::None | Ctl::Body | Ctl::NowPlaying)
    }

    fn slider_value(&self, c: Ctl, x: f32) -> Option<f32> {
        let (r, _) = self.hits.iter().find(|(_, h)| *h == c)?;
        let (l, w) = slider_track(*r);
        Some(((x - l) / w).clamp(0.0, 1.0))
    }

    fn drag_to(&mut self, x: f32) -> Option<Action> {
        let c = self.dragging;
        let v = self.slider_value(c, x)?;
        match c {
            Ctl::Volume => {
                self.vol.snap(v);
                Some(Action::Volume((v * 100.0).round() as u8))
            }
            Ctl::Brightness => {
                let v = v.max(0.02);
                self.bri.snap(v);
                Some(Action::Brightness((v * 100.0).round() as u8))
            }
            Ctl::Scrub => {
                self.scrub = Some(v);
                None
            }
            Ctl::CalGrid => {
                if let Some((x0, _)) = self.cal_drag {
                    self.cal_drag = Some((x0, x - x0));
                }
                None
            }
            _ => None,
        }
    }

    pub fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        if self.grabbing {
            // Window drags come from relative motion (`drag_by`): absolute
            // positions are relative to a surface that is itself moving.
            return None;
        }
        if self.dragging != Ctl::None {
            return self.drag_to(x);
        }
        self.hovered = self.hit(x, y);
        None
    }

    pub fn pointer_leave(&mut self) {
        if self.dragging == Ctl::None {
            self.hovered = Ctl::None;
        }
    }

    pub fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pressed = self.hovered;
        if self.over_grip() && !self.closing {
            let _ = (x, y);
            self.grabbing = true;
            self.fling = (0.0, 0.0);
            self.last_move = None;
            return None;
        }
        if self.hovered == Ctl::CalGrid {
            self.dragging = Ctl::CalGrid;
            self.cal_drag = Some((x, 0.0));
            return None;
        }
        if matches!(self.hovered, Ctl::Volume | Ctl::Brightness | Ctl::Scrub) {
            self.dragging = self.hovered;
            return self.drag_to(x);
        }
        None
    }

    /// Returns (action, close-the-panel).
    pub fn release(&mut self) -> (Option<Action>, bool) {
        if self.grabbing {
            self.grabbing = false;
            self.pressed = Ctl::None;
            // Let go with momentum: glide on and settle.
            let (tx, ty) = self.clamp_off(self.off_x.get() + self.fling.0 * FLING, self.off_y.get() + self.fling.1 * FLING);
            self.off_x.vel = self.fling.0;
            self.off_y.vel = self.fling.1;
            self.off_x.set(tx);
            self.off_y.set(ty);
            return (None, false);
        }
        if self.dragging != Ctl::None {
            let c = std::mem::replace(&mut self.dragging, Ctl::None);
            if c == Ctl::CalGrid {
                // A decent swipe turns the page; a tap does nothing.
                if let Some((_, dx)) = self.cal_drag.take()
                    && dx.abs() > 50.0
                {
                    self.shift_month(if dx < 0.0 { 1 } else { -1 });
                }
                return (None, false);
            }
            let now = self.media_pos();
            if c == Ctl::Scrub
                && let Some(v) = self.scrub.take()
                && let Some(m) = &mut self.media
                && m.length > 0.0
            {
                // Jump there now; the player confirms with Seeked.
                let to = v * m.length;
                m.position = to;
                self.media_t0 = self.time;
                return (Some(Action::MediaSeek(to - now)), false);
            }
            return (None, false);
        }
        let c = std::mem::replace(&mut self.pressed, Ctl::None);
        if c != self.hovered || self.closing {
            return (None, false);
        }
        match c {
            Ctl::Tab => (None, true),
            Ctl::Wifi => (self.open_page(Page::Wifi, c), false),
            Ctl::Bluetooth => (self.open_page(Page::Bluetooth, c), false),
            Ctl::Date => {
                self.cal = (self.today.0, self.today.1);
                self.cal_slide.snap(1.0);
                (self.open_page(Page::Calendar, c), false)
            }
            Ctl::CalPrev => {
                self.shift_month(-1);
                (None, false)
            }
            Ctl::CalNext => {
                self.shift_month(1);
                (None, false)
            }
            Ctl::Night => {
                self.toggles.night = !self.toggles.night;
                (Some(Action::NightLight(self.toggles.night)), false)
            }
            Ctl::Caffeine => {
                self.toggles.caffeine = !self.toggles.caffeine;
                (Some(Action::Caffeine(self.toggles.caffeine)), false)
            }
            Ctl::Airplane => {
                let on = !self.airplane();
                self.system.wifi = Some(!on);
                self.system.bluetooth = Some(!on);
                (Some(Action::Airplane(on)), false)
            }
            // Folds away first so the panel isn't in the shot.
            Ctl::Shot => (Some(Action::Screenshot), true),
            Ctl::Record => {
                if !self.toggles.recorder {
                    return (None, false);
                }
                let on = !self.toggles.recording;
                self.toggles.recording = on;
                (Some(Action::Record(on)), on)
            }
            Ctl::Back => {
                self.page = Page::Main;
                self.page_t.set(0.0);
                (None, false)
            }
            Ctl::Switch => match self.shown_page {
                Page::Wifi => {
                    let on = !self.system.wifi.unwrap_or(false);
                    self.system.wifi = Some(on);
                    if !on {
                        self.wifi_nets = Some(vec![]);
                    } else {
                        self.wifi_nets = None;
                    }
                    (Some(Action::Wifi(on)), false)
                }
                _ => {
                    let on = !self.system.bluetooth.unwrap_or(false);
                    self.system.bluetooth = Some(on);
                    (Some(Action::Bluetooth(on)), false)
                }
            },
            Ctl::Row(i) => {
                let i = i as usize;
                match self.shown_page {
                    Page::Wifi => {
                        let Some(n) = self.wifi_nets.as_ref().and_then(|v| v.get(i)).cloned() else { return (None, false) };
                        self.pending = Some(n.ssid.clone());
                        (Some(if n.active { Action::WifiDisconnect(n.ssid) } else { Action::WifiConnect(n.ssid) }), false)
                    }
                    _ => {
                        let Some(d) = self.bt_devs.as_ref().and_then(|v| v.get(i)).cloned() else { return (None, false) };
                        self.pending = Some(d.mac.clone());
                        (Some(if d.connected { Action::BtDisconnect(d.mac) } else { Action::BtConnect(d.mac) }), false)
                    }
                }
            }
            Ctl::Mute => {
                if let Some(v) = &mut self.volume {
                    v.muted = !v.muted;
                }
                (Some(Action::Mute), false)
            }
            Ctl::MediaPlay => {
                // Flip at once; the player's PropertiesChanged confirms.
                let pos = self.media_pos();
                if let Some(m) = &mut self.media {
                    m.playing = !m.playing;
                    m.position = pos;
                    self.media_t0 = self.time;
                    self.playing.set(if m.playing { 1.0 } else { 0.0 });
                }
                (Some(Action::MediaPlayPause), false)
            }
            Ctl::MediaPrev => (Some(Action::MediaPrev), false),
            Ctl::MediaNext => (Some(Action::MediaNext), false),
            Ctl::Gear => (Some(Action::OpenPrefs), false),
            // The panel folds away as the selector opens.
            Ctl::Wallpaper => (Some(Action::Open("wallpapers")), true),
            Ctl::Monitor => (Some(Action::Open("monitor")), true),
            Ctl::Lock => (Some(Action::Lock), true),
            Ctl::Suspend => (Some(Action::Suspend), true),
            Ctl::Reboot | Ctl::PowerOff => {
                // Dangerous: the first click arms, the second confirms.
                if self.armed.is_some_and(|(a, _)| a == c) {
                    self.armed = None;
                    (Some(if c == Ctl::Reboot { Action::Reboot } else { Action::PowerOff }), true)
                } else {
                    self.armed = Some((c, self.time));
                    (None, false)
                }
            }
            _ => (None, false),
        }
    }

    pub fn scroll(&mut self, dy: f32) -> Option<Action> {
        let step = -dy.signum() * 0.05;
        match self.hovered {
            Ctl::Volume => {
                let v = (self.vol.target + step).clamp(0.0, 1.0);
                self.vol.set(v);
                Some(Action::Volume((v * 100.0).round() as u8))
            }
            Ctl::Brightness => {
                let v = (self.bri.target + step).clamp(0.02, 1.0);
                self.bri.set(v);
                Some(Action::Brightness((v * 100.0).round() as u8))
            }
            Ctl::CalGrid | Ctl::CalPrev | Ctl::CalNext => {
                self.shift_month(if dy > 0.0 { 1 } else { -1 });
                None
            }
            Ctl::NowPlaying | Ctl::Scrub | Ctl::MediaPrev | Ctl::MediaPlay | Ctl::MediaNext
                if self.media.as_ref().is_some_and(|m| m.can_seek) =>
            {
                Some(Action::MediaSeek(if dy > 0.0 { -5.0 } else { 5.0 }))
            }
            _ => None,
        }
    }

    // ── drawing ─────────────────────────────────────────────────────────

    /// Centre of a control's hit area from the last draw (snapshots).
    pub fn hit_center(&self, c: Ctl) -> Option<(f32, f32)> {
        self.hits.iter().rev().find(|(_, h)| *h == c).map(|(r, _)| (r.x + r.w * 0.5, r.y + r.h * 0.5))
    }

    fn hover_of(&self, c: Ctl) -> f32 {
        self.hover.get(&c).map_or(0.0, |s| s.get().clamp(0.0, 1.0))
    }

    fn px(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [(r.x * s).floor() as i32, (r.y * s).floor() as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32]
    }

    fn text(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect) {
        if c.3 <= 0.002 || clip.w <= 0.5 || clip.h <= 0.5 {
            return;
        }
        let s = self.scale;
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: self.px(clip) });
    }

    fn icon(&self, out: &mut DrawList, name: &str, x: f32, y: f32, size: f32, c: Rgba) {
        if c.3 <= 0.002 {
            return;
        }
        let s = self.scale;
        let id = self.icons.id(&[name]);
        out.icons.push(IconDraw { id, x: x * s, y: y * s, size: (size * s).round(), color: c });
    }

    /// A texture in `r` cropped to `clip`; `reveal` < 1 masks it to a
    /// circle growing from the centre.
    #[allow(clippy::too_many_arguments)]
    fn image(&self, out: &mut DrawList, key: u64, r: Rect, radius: f32, clip: Rect, alpha: f32, reveal: f32) {
        if alpha <= 0.003 {
            return;
        }
        let s = self.scale;
        let Some(c) = intersect(r, clip) else { return };
        let mut d = ImageDraw::new(key, [c.x * s, c.y * s, c.w * s, c.h * s]);
        d.uv = [(c.x - r.x) / r.w, (c.y - r.y) / r.h, (c.x + c.w - r.x) / r.w, (c.y + c.h - r.y) / r.h];
        d.radius = if (c.w - r.w).abs() < 0.5 && (c.h - r.h).abs() < 0.5 { radius * s } else { 2.0 * s };
        d.alpha = alpha.min(1.0);
        if reveal < 1.0 {
            let diag = (r.w * r.w + r.h * r.h).sqrt() * 0.5;
            d.reveal = [(r.x + r.w * 0.5) * s, (r.y + r.h * 0.5) * s, (diag + 4.0) * reveal.max(0.0) * s, 4.0 * s];
        }
        out.images.push(d);
    }

    /// A quiet hover surface: faint white, nothing at rest.
    fn hover_bg(&self, out: &mut DrawList, r: Rect, radius: f32, a: f32, h: f32) {
        if h * a > 0.003 {
            out.shapes.push(paint::rect(self.scale, r, radius).fill(HOVER).alpha(a * h).done());
        }
    }

    /// Vertical layout of the open panel, relative to its top edge.
    fn layout(&self) -> Lay {
        let mh = lerp(MEDIA_IDLE_H, MEDIA_H, self.media_k.get().clamp(0.0, 1.1));
        let div1 = HEAD_H;
        let media = div1 + 1.0 + SEC;
        let div2 = media + mh + SEC;
        let conn = div2 + 1.0 + SEC;
        let quick = conn + CONN_H + 10.0;
        let div3 = quick + QUICK_H + 12.0;
        let vol = div3 + 1.0 + 10.0;
        let mut y = vol + SLIDER_H;
        let bri = self.system.brightness.is_some().then(|| {
            y += 2.0;
            let b = y;
            y += SLIDER_H;
            b
        });
        let div4 = y + 10.0;
        let power = div4 + 1.0 + 10.0;
        Lay { div1, media, mh, div2, conn, quick, div3, vol, bri, div4, power, bottom: power + POWER_H + 14.0 }
    }

    /// Height of the open window for the current module states.
    fn full_h(&self) -> f32 {
        self.layout().bottom
    }

    pub fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        let sc = self.scale;
        let pw = self.pw.get();
        let ph = self.ph.get();

        // The body: tab → panel.
        let full_h = self.full_h();
        let target = Rect::new((SW - PW) * 0.5, DETACH_Y, PW, full_h);
        let r = Rect::new(
            lerp(self.start.x, target.x, pw),
            lerp(self.start.y, target.y, ph),
            lerp(self.start.w, target.w, pw),
            lerp(self.start.h, target.h, ph),
        );
        self.body = r;
        let radius = lerp(R_TAB, R_OPEN, ph.clamp(0.0, 1.0)).min(r.h * 0.5).min(r.w * 0.5);
        // Prism Ink: flat matte ink, the prism hairline, a soft shadow.
        out.shapes.push(paint::ink(sc, r, radius).shadow(SHADOW, 14.0 + 12.0 * ph, 4.0 + 8.0 * ph).done());
        out.glass(r, radius, 1.0);
        let clip = r.inflate(-1.5);
        self.hits.push((r, Ctl::Body));
        // Drag handle.
        let gh = self.hover_of(Ctl::Grip).max(if self.grabbing { 1.0 } else { 0.0 });
        let grip = Rect::new(r.x + r.w * 0.5 - 18.0 - 6.0 * gh, r.y + 6.0, 36.0 + 12.0 * gh, 4.0);
        self.hits.push((Rect::new(r.x + r.w * 0.5 - 40.0, r.y, 80.0, 16.0), Ctl::Grip));
        out.shapes.push(
            paint::rect(sc, grip, 2.0)
                .fill(Rgba::hex(0xffffff, 0.12 + 0.22 * gh))
                .alpha(((ph - 0.6) / 0.4).clamp(0.0, 1.0))
                .done(),
        );
        // Everything below moves with the window's top edge.
        let top = r.y;
        let lay = self.layout();

        // Modules cascade in once the body has room for them; they fade
        // quickly while a detail page grows over them.
        let main_a = (1.0 - self.page_t.get() * 1.8).clamp(0.0, 1.0);
        let bottom = r.y + r.h;
        let row = |i: usize, y_bottom: f32| {
            let fits = ((bottom - y_bottom) / 10.0).clamp(0.0, 1.0);
            main_a * fits * ((ph - 0.5 - 0.06 * i as f32) / 0.3).clamp(0.0, 1.0)
        };
        let l = r.x + PAD;
        let rr = r.x + r.w - PAD;
        let divider = |out: &mut DrawList, y: f32, a: f32| paint::divider(&mut out.shapes, sc, l - 4.0, top + y, rr - l + 8.0, a);

        // ── header: clock, date · weather; battery, monitor, wallpaper, settings
        let a0 = row(0, top + HEAD_H);
        if a0 > 0.0 {
            if let Some(c) = self.clock {
                const WD: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
                const MO: [&str; 12] = [
                    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
                    "November", "December",
                ];
                let (k, cw) = self.text.get(fs, &format!("{:02}:{:02}", c.hour, c.minute), F_CLOCK);
                let d = format!("{}, {} {}", WD[c.weekday as usize % 7], c.day, MO[c.month as usize % 12]);
                let (dk, dw) = self.text.get(fs, &d, F_DATE);
                let dlh = self.text.line_h(F_DATE);
                let dy = top + 58.0;
                // Clock + date open the calendar.
                let dr = Rect::new(l - 8.0, top + 12.0, cw.max(dw) + 16.0, 66.0);
                self.hits.push((dr, Ctl::Date));
                let h = self.hover_of(Ctl::Date);
                self.hover_bg(out, dr, 16.0, a0, h * 0.7);
                self.text(out, k, l, top + 14.0, TEXT.fade(a0), clip);
                self.text(out, dk, l, dy, MUTED.mix(SOFT, 0.35 + 0.65 * h).fade(a0), clip);
                // Weather rides the date line; hovering it lets the
                // forecast fade in after it (nothing moves).
                let wk = self.hover_of(Ctl::Weather);
                if let Some(w) = self.weather.clone() {
                    let (sk, sw) = self.text.get(fs, "·", F_DATE);
                    let x = l + dw + 8.0;
                    self.text(out, sk, x, dy, MUTED.fade(a0), clip);
                    let ix = x + sw + 8.0;
                    let cy = dy + dlh * 0.5;
                    self.icon(out, w.icon(), ix, cy - 7.0, 14.0, SOFT.mix(TEXT, wk).fade(a0));
                    let (tk, tw) = self.text.get(fs, &format!("{:.0}°", w.temp), F_DATE);
                    self.text(out, tk, ix + 19.0, dy, SOFT.mix(TEXT, wk).fade(a0), clip);
                    self.hits.push((Rect::new(ix - 5.0, dy - 5.0, 19.0 + tw + 10.0, dlh + 10.0), Ctl::Weather));
                    if wk > 0.01 {
                        let fx = ix + 19.0 + tw + 10.0;
                        let place = if w.place.is_empty() { String::new() } else { format!(" · {}", w.place) };
                        let sub = format!("{} · ↑{:.0}° ↓{:.0}°{place}", w.label(), w.hi, w.lo);
                        let (fk, _) = self.text.get_fit(fs, &sub, F_DATE, (rr - fx).max(10.0));
                        self.text(out, fk, fx, dy, MUTED.mix(SOFT, 0.5).fade(a0 * wk), clip);
                    }
                }
            }
            // Bare icon buttons: a faint circle only under the pointer.
            let by = top + 16.0;
            let gear = Rect::new(rr - 32.0 + 6.0, by, 32.0, 32.0);
            let wp = Rect::new(gear.x - 36.0, by, 32.0, 32.0);
            let mon = Rect::new(wp.x - 36.0, by, 32.0, 32.0);
            for (b, c, icon) in [(gear, Ctl::Gear, "emblem-system-symbolic"), (wp, Ctl::Wallpaper, "preferences-desktop-wallpaper-symbolic"), (mon, Ctl::Monitor, "")] {
                self.hits.push((b, c));
                let h = self.hover_of(c);
                self.hover_bg(out, b, 16.0, a0, h);
                let col = SOFT.mix(TEXT, h).fade(a0);
                if icon.is_empty() {
                    // System monitor: three activity bars (no stock icon).
                    for (i, bh) in [7.0f32, 12.0, 9.0].into_iter().enumerate() {
                        let x = b.x + 10.0 + i as f32 * 4.5;
                        out.shapes.push(paint::rect(sc, Rect::new(x, b.y + 22.5 - bh, 3.0, bh), 1.5).fill(col).done());
                    }
                } else {
                    self.icon(out, icon, b.x + 8.0, b.y + 8.0, 16.0, col);
                }
            }
            // Battery: a small cell and the percentage as text.
            if let Some(b) = self.battery {
                let (k, tw) = self.text.get(fs, &format!("{}%", b.percent), F_BAT);
                let col = if b.charging {
                    GOOD
                } else if b.percent <= 15 {
                    BAD
                } else if b.percent <= 30 {
                    WARN
                } else {
                    TEXT
                };
                let tx = mon.x - 8.0 - tw;
                let cy = by + 16.0;
                let cx = tx - 6.0 - 21.0;
                out.shapes.push(paint::rect(sc, Rect::new(cx, cy - 5.0, 19.0, 10.0), 3.0).rim(1.3, Rgba::hex(0xffffff, 0.5), Rgba::hex(0xffffff, 0.5)).alpha(a0).done());
                out.shapes.push(paint::rect(sc, Rect::new(cx + 19.6, cy - 2.0, 1.6, 4.0), 0.8).fill(Rgba::hex(0xffffff, 0.42)).alpha(a0).done());
                let lv = (b.percent.min(100) as f32 / 100.0).max(0.08);
                let fc = if b.charging || b.percent <= 30 { col } else { Rgba::hex(0xffffff, 0.92) };
                out.shapes.push(paint::rect(sc, Rect::new(cx + 2.3, cy - 2.7, 14.4 * lv, 5.4), 1.2).fill(fc).alpha(a0).done());
                let lh = self.text.line_h(F_BAT);
                let tc = if b.charging || b.percent <= 30 { col } else { SOFT };
                self.text(out, k, tx, cy - lh * 0.5, tc.fade(a0), clip);
            }
        }
        divider(out, lay.div1, a0);

        // ── now playing ─────────────────────────────────────────────────
        let my = top + lay.media;
        let m = Rect::new(l, my, rr - l, lay.mh);
        let a1 = row(1, my + lay.mh);
        self.draw_media(fs, out, m, a1, clip);
        divider(out, lay.div2, a1);

        // ── connectivity ────────────────────────────────────────────────
        let ty = top + lay.conn;
        let a2 = row(2, ty + CONN_H);
        self.draw_tiles(fs, out, Rect::new(l, ty, rr - l, CONN_H), a2, clip);

        // ── quick toggles ───────────────────────────────────────────────
        let qy = top + lay.quick;
        let aq = row(3, qy + QUICK_H);
        self.draw_quick(fs, out, Rect::new(l, qy, rr - l, QUICK_H), aq, clip);
        divider(out, lay.div3, aq);

        // ── sliders ─────────────────────────────────────────────────────
        let vol_level = self.vol.get();
        let bri_level = self.bri.get();
        let muted = self.volume.is_some_and(|v| v.muted);
        let vol_icon = if muted || vol_level < 0.01 {
            "audio-volume-muted-symbolic"
        } else if vol_level < 0.34 {
            "audio-volume-low-symbolic"
        } else if vol_level < 0.67 {
            "audio-volume-medium-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        let sy = top + lay.vol;
        let av = row(4, sy + SLIDER_H);
        self.slider(fs, out, Ctl::Volume, vol_icon, sy, vol_level, !muted, av, clip, l, rr);
        let mut last_a = av;
        if let Some(by) = lay.bri {
            let sy = top + by;
            last_a = row(5, sy + SLIDER_H);
            self.slider(fs, out, Ctl::Brightness, "display-brightness-symbolic", sy, bri_level, true, last_a, clip, l, rr);
        }
        divider(out, lay.div4, last_a);

        // ── power row: bare icons; restart / power off arm first (the
        // button widens into a soft red "Confirm"); a second click confirms
        let py = top + lay.power;
        let a4 = row(6, py + POWER_H);
        let buttons: [(Ctl, &str); 4] = [
            (Ctl::Lock, "system-lock-screen-symbolic"),
            (Ctl::Suspend, "weather-clear-night-symbolic"),
            (Ctl::Reboot, "system-reboot-symbolic"),
            (Ctl::PowerOff, "system-shutdown-symbolic"),
        ];
        let slot = (rr - l) / 4.0;
        for (i, (c, icon)) in buttons.into_iter().enumerate() {
            let arm = self.arm.get(&c).map_or(0.0, |s| s.get().clamp(0.0, 1.2));
            let (k, lw) = self.text.get(fs, "Confirm", F_SMALL_B);
            let w = POWER_H + (lw + 12.0) * arm.max(0.0);
            let cx = l + slot * (i as f32 + 0.5);
            let b = Rect::new(cx - w * 0.5, py, w, POWER_H);
            self.hits.push((b, c));
            if a4 <= 0.0 {
                continue;
            }
            let h = self.hover_of(c);
            let armk = arm.clamp(0.0, 1.0);
            let danger = c == Ctl::PowerOff;
            let hc = if danger { DANGER.fade(0.22 / DANGER.3) } else { HOVER };
            if h * (1.0 - armk) > 0.003 {
                out.shapes.push(paint::rect(sc, b, POWER_H * 0.5).fill(hc).alpha(a4 * h * (1.0 - armk)).done());
            }
            if armk > 0.003 {
                out.shapes.push(paint::rect(sc, b, POWER_H * 0.5).fill(DANGER.fade(1.0 + 0.25 * h)).alpha(a4 * armk).done());
            }
            let fg = SOFT.mix(TEXT, h).mix(DANGER_FG, if danger { h.max(armk) } else { armk });
            let content = 16.0 + (6.0 + lw) * armk;
            let ix = cx - content * 0.5;
            self.icon(out, icon, ix, py + 12.0, 16.0, fg.fade(a4));
            if armk > 0.01 {
                let lclip = Rect::new(ix + 16.0, py, (6.0 + lw) * armk, POWER_H);
                let lh = self.text.line_h(F_SMALL_B);
                self.text(out, k, ix + 22.0, py + POWER_H * 0.5 - lh * 0.5, DANGER_FG.fade(a4 * armk), lclip);
            }
        }
        if self.page_t.get() > 0.3 {
            self.hits.retain(|(_, c)| matches!(c, Ctl::Body | Ctl::Tab));
        }
        if self.shown_page == Page::Calendar {
            self.draw_calendar(fs, out, r, top, clip);
        } else if self.shown_page != Page::Main {
            self.draw_page(fs, out, r, top, clip);
        }
        self.text.end_frame();
        self.translate(out);
    }

    /// Now playing, box-less: a rounded cover, title / artist, bare
    /// transport and a thin iridescent scrub line. Folds to a calm
    /// "Not playing" line when nothing plays.
    fn draw_media(&mut self, fs: &mut FontSystem, out: &mut DrawList, m: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let mk = self.media_k.get().clamp(0.0, 1.0);
        // Cover texture follows the shown track's art.
        if let Some(s) = &self.shown {
            match (&s.art_url, &self.art) {
                (Some(u), Some(art)) if &art.url == u => {
                    let art = art.clone();
                    self.cover.want(&art.url, &art.img);
                }
                (None, _) => self.cover.clear(),
                _ => {}
            }
        }
        self.cover.flush(out);
        if a <= 0.0 {
            return;
        }
        self.hits.push((m, Ctl::NowPlaying));
        let (ta, tb) = (self.tint_a.get(), self.tint_b.get());
        let pl = self.playing.get().clamp(0.0, 1.0);
        let mclip = intersect(m.inflate(8.0), clip).unwrap_or(Rect::new(m.x, m.y, 0.0, 0.0));

        // Idle: a calm one-liner.
        let ia = a * (1.0 - mk * 2.2).clamp(0.0, 1.0);
        if ia > 0.003 {
            let d = 30.0;
            let (cx, cy) = (m.x + d * 0.5, m.y + MEDIA_IDLE_H * 0.5);
            out.shapes.push(paint::circle(sc, cx, cy, d * 0.5).fill(paint::FAINT).alpha(ia).done());
            self.icon(out, "audio-x-generic-symbolic", cx - 8.0, cy - 8.0, 16.0, SOFT.fade(ia));
            let (k, _) = self.text.get(fs, "Not playing", F_CONN);
            let lh = self.text.line_h(F_CONN);
            self.text(out, k, m.x + d + 12.0, cy - lh * 0.5, SOFT.fade(ia), mclip);
        }

        let Some(mi) = self.shown.clone() else { return };
        let ca = a * ((mk - 0.35) / 0.65).clamp(0.0, 1.0);
        if ca <= 0.003 {
            return;
        }

        // Cover, with a soft shadow under it.
        let ar = Rect::new(m.x, m.y, ART, ART);
        if let Some(r) = intersect(ar, mclip) {
            out.shapes.push(paint::rect(sc, r, ART_R).fill(paint::FAINT).shadow(Rgba::hex(0x000000, 0.42), 8.0, 5.0).alpha(ca).done());
        }
        let art_a = ca * (0.72 + 0.28 * pl);
        let rv = self.cover.t.get().clamp(0.0, 1.0);
        match (&self.cover.cur, &self.cover.prev) {
            (Some((k, _)), prev) => {
                if let Some((pk, _)) = prev {
                    self.image(out, *pk, ar, ART_R, mclip, art_a, 1.0);
                }
                self.image(out, *k, ar, ART_R, mclip, art_a, if prev.is_some() { rv } else { 1.0 });
            }
            (None, _) => self.icon(out, "audio-x-generic-symbolic", ar.x + 22.0, ar.y + 22.0, 24.0, SOFT.fade(ca * 0.8)),
        }

        // Title and artist (a new track cross-fades in place).
        let tx = ar.x + ART + 14.0;
        let tw = (m.x + m.w - tx).max(0.0);
        let tclip = intersect(Rect::new(tx - 1.0, m.y - 4.0, tw + 2.0, m.h + 8.0), mclip).unwrap_or(Rect::new(tx, m.y, 0.0, 0.0));
        let xf = self.track_xf.get().clamp(0.0, 1.0);
        let lines = |t: &str, ar_: &str| (t.to_string(), ar_.to_string());
        let cur = lines(&mi.title, if mi.artist.is_empty() { &mi.player } else { &mi.artist });
        let mut layers = vec![(cur, xf)];
        if let Some(p) = self.prev_track.clone().filter(|_| xf < 1.0) {
            layers.push((p, 1.0 - xf));
        }
        let tcol = SOFT.mix(TEXT, 0.5 + 0.5 * pl);
        for ((title, artist), k) in layers {
            let (tk, _) = self.text.get_fit(fs, &title, F_NP_TITLE, tw);
            let (ak, _) = self.text.get_fit(fs, &artist, F_NP_ARTIST, tw);
            self.text(out, tk, tx, m.y + 4.0, tcol.fade(ca * k), tclip);
            self.text(out, ak, tx, m.y + 24.0, MUTED.mix(SOFT, 0.3).fade(ca * k), tclip);
        }

        // Transport: bare icons; a faint circle slides between them.
        let cy = m.y + 54.0;
        let bw = 32.0;
        let bx = tx - 8.0;
        let hov = self.btn_a.get().clamp(0.0, 1.0);
        if hov > 0.01 {
            let hx = bx + self.btn_x.get() * bw;
            let pressed = matches!(self.pressed, Ctl::MediaPrev | Ctl::MediaPlay | Ctl::MediaNext);
            if let Some(t) = intersect(Rect::new(hx + 1.0, cy - 15.0, bw - 2.0, 30.0), mclip) {
                out.shapes.push(paint::rect(sc, t, 15.0).fill(Rgba::hex(0xffffff, if pressed { 0.15 } else { 0.10 })).alpha(ca * hov).done());
            }
        }
        let ctls = [(Ctl::MediaPrev, "media-skip-backward-symbolic", mi.can_prev), (Ctl::MediaPlay, "", true), (Ctl::MediaNext, "media-skip-forward-symbolic", mi.can_next)];
        for (i, (c, icon, enabled)) in ctls.into_iter().enumerate() {
            let b = Rect::new(bx + i as f32 * bw, cy - 15.0, bw, 30.0);
            let col = if !enabled {
                MUTED.fade(0.5)
            } else if self.hovered == c {
                TEXT
            } else {
                SOFT.mix(TEXT, 0.4)
            };
            let s = if i == 1 { 18.0 } else { 16.0 };
            let ix = b.x + (bw - s) * 0.5;
            if i == 1 {
                self.icon(out, "media-playback-pause-symbolic", ix, cy - s * 0.5, s, col.fade(ca * pl));
                self.icon(out, "media-playback-start-symbolic", ix, cy - s * 0.5, s, col.fade(ca * (1.0 - pl)));
            } else {
                self.icon(out, icon, ix, cy - s * 0.5, s, col.fade(ca));
            }
            if enabled && mk > 0.6 {
                self.hits.push((b, c));
            }
        }

        // Scrub line under the cover: elapsed, a thin track, remaining.
        let sy = m.y + ART + 19.0;
        if mi.length > 0.0 {
            let pos = self.scrub.map_or_else(|| self.media_pos(), |v| v * mi.length);
            let (ek, _) = self.text.get(fs, &mtime(pos), F_TIME);
            let (rk, rw) = self.text.get(fs, &format!("−{}", mtime(mi.length - pos)), F_TIME);
            let lh = self.text.line_h(F_TIME);
            let tr_l = m.x + 34.0;
            let tr_r = m.x + m.w - 42.0;
            self.text(out, ek, m.x, sy - lh * 0.5, MUTED.fade(ca), mclip);
            self.text(out, rk, m.x + m.w - rw, sy - lh * 0.5, MUTED.fade(ca), mclip);
            let tw = (tr_r - tr_l).max(1.0);
            let sh = self.hover_of(Ctl::Scrub).max(if self.dragging == Ctl::Scrub { 1.0 } else { 0.0 });
            let th = 4.0 + 2.0 * sh;
            let frac = (pos / mi.length).clamp(0.0, 1.0);
            if let Some(t) = intersect(Rect::new(tr_l, sy - th * 0.5, tw, th), mclip) {
                out.shapes.push(paint::rect(sc, t, th * 0.5).fill(TRACK).alpha(ca).done());
            }
            if frac * tw > 0.5
                && let Some(t) = intersect(Rect::new(tr_l, sy - th * 0.5, (frac * tw).max(th), th), mclip)
            {
                let f = paint::rect(sc, t, th * 0.5);
                // Iridescent while playing; the cover's colours under
                // another accent. Paused greys it out.
                let f = if theme::prism() {
                    f.on_mix(SOFT.fade(0.6), 0.45 + 0.55 * pl, 1.0, 0.0, 0.0)
                } else {
                    let (fa, fb) = (ta.mix(SOFT, 0.5 * (1.0 - pl)), tb.mix(SOFT, 0.5 * (1.0 - pl)));
                    f.gradient(fa.mix(TEXT, 0.25), fb.mix(TEXT, 0.15), 1.0, 0.0)
                };
                out.shapes.push(f.alpha(ca).done());
            }
            if sh > 0.01 {
                out.shapes.push(paint::knob(sc, tr_l + tw * frac, sy, 7.0 + sh).alpha(ca * sh).done());
            }
            if mi.can_seek && mk > 0.6 {
                self.hits.push((Rect::new(tr_l - 6.0, sy - 10.0, tw + 12.0, 20.0), Ctl::Scrub));
            }
        } else {
            let (k, _) = self.text.get(fs, "Live", F_TIME);
            let lh = self.text.line_h(F_TIME);
            self.text(out, k, m.x, sy - lh * 0.5, MUTED.fade(ca), mclip);
        }
    }

    /// Wi-Fi, Bluetooth, Mute: a round dot (iridescent when on) with a
    /// name and what it's connected to beside it. Wi-Fi / Bluetooth open
    /// their page.
    fn draw_tiles(&mut self, fs: &mut FontSystem, out: &mut DrawList, row: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let cwid = row.w / 3.0;
        let wifi = self.system.wifi;
        let bt = self.system.bluetooth;
        let muted = self.volume.is_some_and(|v| v.muted);
        let onoff = |s: Option<bool>| match s {
            Some(true) => "On".to_string(),
            Some(false) => "Off".to_string(),
            None => "…".to_string(),
        };
        let ssid = self.wifi_nets.as_ref().and_then(|v| v.iter().find(|n| n.active)).map(|n| n.ssid.clone());
        let wifi_sub = match (wifi, ssid, self.net) {
            (Some(true), Some(s), _) => s,
            (_, _, NetInfo::Ethernet) if wifi != Some(true) => "Ethernet".into(),
            _ => onoff(wifi),
        };
        let dev = self.bt_devs.as_ref().and_then(|v| v.iter().find(|d| d.connected)).map(|d| d.name.clone());
        let bt_sub = match (bt, dev) {
            (Some(true), Some(d)) => d,
            _ => onoff(bt),
        };
        let tiles: [(Ctl, &str, &str, Option<bool>, String); 3] = [
            (Ctl::Wifi, if wifi == Some(false) { "network-wireless-disabled-symbolic" } else { "network-wireless-symbolic" }, "Wi-Fi", wifi, wifi_sub),
            (Ctl::Bluetooth, if bt == Some(false) { "bluetooth-disabled-symbolic" } else { "bluetooth-active-symbolic" }, "Bluetooth", bt, bt_sub),
            (Ctl::Mute, "audio-volume-muted-symbolic", "Mute", Some(muted), onoff(Some(muted))),
        ];
        for (i, (c, icon, label, state, sub)) in tiles.into_iter().enumerate() {
            let t = Rect::new(row.x - 4.0 + i as f32 * cwid, row.y, cwid - 2.0, row.h);
            self.hits.push((t, c));
            if a <= 0.0 {
                continue;
            }
            let h = self.hover_of(c);
            self.hover_bg(out, t, 16.0, a, h * 0.75);
            let on = state == Some(true);
            let (cx, cy) = (t.x + 4.0 + CONN_DOT * 0.5, t.y + t.h * 0.5);
            out.shapes.push(dot(sc, cx, cy, CONN_DOT * 0.5, on, h).alpha(a).done());
            self.icon(out, icon, cx - 8.0, cy - 8.0, 16.0, if on { theme::on_ink() } else { TEXT }.fade(a));
            let x = cx + CONN_DOT * 0.5 + 10.0;
            let w = (t.x + t.w - 4.0 - x).max(0.0);
            let tc = Rect::new(x, t.y, w, t.h);
            let (k, _) = self.text.get_fit(fs, label, F_CONN, w);
            let lh = self.text.line_h(F_CONN);
            self.text(out, k, x, cy - lh + 1.0, TEXT.fade(a), tc);
            let (k, _) = self.text.get_fit(fs, &sub, F_SUB, w);
            self.text(out, k, x, cy + 1.5, MUTED.mix(SOFT, 0.2).fade(a), tc);
            let _ = clip;
        }
    }

    /// Airplane mode = every radio off (as far as we can tell).
    fn airplane(&self) -> bool {
        self.system.wifi == Some(false) && self.system.bluetooth != Some(true)
    }

    /// Show the previous / next month (slides in from that side).
    fn shift_month(&mut self, d: i32) {
        let (y, m) = self.cal;
        let i = y * 12 + m as i32 - 1 + d;
        self.cal = (i.div_euclid(12), i.rem_euclid(12) as u32 + 1);
        self.cal_dir = d.signum() as f32;
        self.cal_slide.snap(0.0);
        self.cal_slide.set(1.0);
    }

    /// Night light, caffeine, airplane, screenshot, record: round dots
    /// with their label underneath; "on" is iridescent (recording: a soft
    /// red).
    fn draw_quick(&mut self, fs: &mut FontSystem, out: &mut DrawList, row: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let n = 5.0;
        let cw = 62.0;
        let step = (row.w - cw) / (n - 1.0);
        let t = self.toggles;
        let rec_label = if !t.recorder { "Unavailable" } else if t.recording { "Stop" } else { "Record" };
        let tiles: [(Ctl, &str, &str, bool, bool); 5] = [
            (Ctl::Night, "night-light-symbolic", "Night light", t.night, true),
            (Ctl::Caffeine, "", "Caffeine", t.caffeine, true),
            (Ctl::Airplane, "airplane-mode-symbolic", "Airplane", self.airplane(), true),
            (Ctl::Shot, "camera-photo-symbolic", "Screenshot", false, true),
            (Ctl::Record, "media-record-symbolic", rec_label, t.recording, t.recorder),
        ];
        for (i, (c, icon, label, on, enabled)) in tiles.into_iter().enumerate() {
            let r = Rect::new(row.x + i as f32 * step, row.y, cw, row.h);
            if enabled {
                self.hits.push((r, c));
            }
            if a <= 0.0 {
                continue;
            }
            let h = if enabled { self.hover_of(c) } else { 0.0 };
            let ea = a * if enabled { 1.0 } else { 0.45 };
            let (cx, cy) = (r.x + cw * 0.5, r.y + QUICK_DOT * 0.5);
            let rec = c == Ctl::Record && on;
            let d = if rec {
                paint::circle(sc, cx, cy, QUICK_DOT * 0.5).fill(REC_RED.mix(Rgba::hex(0xffffff, 1.0), 0.08 * h))
            } else {
                dot(sc, cx, cy, QUICK_DOT * 0.5, on, h)
            };
            out.shapes.push(d.alpha(ea).done());
            let fg = if rec {
                Rgba::hex(0xffffff, 1.0)
            } else if on {
                theme::on_ink()
            } else {
                SOFT.mix(TEXT, 0.5 + 0.5 * h)
            };
            let (ix, iy) = (cx - 9.0, cy - 9.0);
            if icon.is_empty() {
                // A coffee cup (no stock icon): body, handle, two wisps.
                let col = fg.fade(ea);
                out.shapes.push(paint::rect(sc, Rect::new(ix + 2.0, iy + 7.0, 11.0, 10.0), 3.0).fill(col).done());
                out.shapes.push(paint::rect(sc, Rect::new(ix + 11.0, iy + 9.0, 6.0, 6.0), 3.0).rim(1.6, col, col).done());
                for k in 0..2 {
                    out.shapes.push(paint::rect(sc, Rect::new(ix + 5.0 + k as f32 * 4.0, iy + 1.0, 1.6, 4.5), 0.8).fill(col.fade(0.75)).done());
                }
            } else {
                self.icon(out, icon, ix, iy, 18.0, fg.fade(ea));
            }
            let (k, w) = self.text.get_fit(fs, label, F_TINY, cw + 4.0);
            self.text(out, k, cx - w * 0.5, r.y + QUICK_DOT + 6.0, MUTED.mix(SOFT, 0.35 + 0.65 * h).fade(ea), clip);
        }
    }

    /// The calendar page: grows from the date like the Wi-Fi page grows
    /// from its row; a Monday-first month grid with today iridescent.
    /// Arrows, scroll or a horizontal swipe change the month.
    fn draw_calendar(&mut self, fs: &mut FontSystem, out: &mut DrawList, r: Rect, top: f32, clip: Rect) {
        let sc = self.scale;
        let (aa, _) = theme::accent();
        let t = self.page_t.get();
        let e = t.clamp(0.0, 1.1);
        let d = Rect::new(r.x + 10.0, top + 10.0, r.w - 20.0, r.h - 20.0);
        self.page_bg(out, d, t);
        let a = ((t - 0.55) / 0.45).clamp(0.0, 1.0);
        let _ = e;
        if a <= 0.0 {
            return;
        }
        const MONTHS: [&str; 12] = [
            "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
        ];
        let (y, mo) = self.cal;
        // Header: back, month and year, arrows.
        let hy = d.y + 12.0;
        let back = Rect::new(d.x + 6.0, hy - 2.0, 32.0, 32.0);
        self.hits.push((back, Ctl::Back));
        let bh = self.hover_of(Ctl::Back);
        self.hover_bg(out, back, 16.0, a, bh);
        self.icon(out, "go-previous-symbolic", back.x + 8.0, back.y + 8.0, 16.0, TEXT.fade(a));
        let slide = self.cal_slide.get();
        let drag = self.cal_drag.map_or(0.0, |(_, dx)| dx.clamp(-120.0, 120.0));
        let dx = (1.0 - slide) * self.cal_dir * 48.0 + drag * 0.5;
        let ga = a * slide.clamp(0.0, 1.0) * (1.0 - drag.abs() / 240.0);
        let title = format!("{} {}", MONTHS[(mo - 1) as usize % 12], y);
        let (k, _) = self.text.get(fs, &title, F_CAL_TITLE);
        let lh = self.text.line_h(F_CAL_TITLE);
        let pclip = intersect(d, clip).unwrap_or(d);
        self.text(out, k, d.x + 46.0 + dx * 0.4, hy + 14.0 - lh * 0.5, TEXT.fade(ga), pclip);
        for (c, icon, x) in [(Ctl::CalPrev, "go-previous-symbolic", d.x + d.w - 84.0), (Ctl::CalNext, "go-next-symbolic", d.x + d.w - 44.0)] {
            let b = Rect::new(x, hy - 2.0, 32.0, 32.0);
            self.hits.push((b, c));
            let h = self.hover_of(c);
            self.hover_bg(out, b, 16.0, a, h);
            self.icon(out, icon, b.x + 8.0, b.y + 8.0, 16.0, SOFT.mix(TEXT, h).fade(a));
        }

        // Weekday header, Monday first.
        let gx = d.x + 14.0;
        let cw = (d.w - 28.0) / 7.0;
        let wy = hy + 46.0;
        for (i, wd) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate() {
            let (k, w) = self.text.get(fs, wd, F_TINY);
            self.text(out, k, gx + i as f32 * cw + (cw - w) * 0.5, wy, (if i >= 5 { MUTED.fade(0.7) } else { MUTED }).fade(a), pclip);
        }
        // The grid.
        let gy = wy + 24.0;
        let ch = ((d.y + d.h - 14.0 - gy) / 6.0).clamp(26.0, 40.0);
        let grid = Rect::new(gx, gy, cw * 7.0, ch * 6.0);
        self.hits.push((grid, Ctl::CalGrid));
        let first = weekday_mon0(y, mo, 1);
        let days = days_in_month(y, mo);
        let marks = day_marks(y, mo);
        for day in 1..=days {
            let cell = first + day - 1;
            let (col, row) = ((cell % 7) as f32, (cell / 7) as f32);
            let cx = gx + col * cw + cw * 0.5 + dx;
            let cy = gy + row * ch + ch * 0.5;
            let is_today = (y, mo, day) == self.today;
            if is_today {
                let s = (ch - 4.0).min(cw - 6.0).min(32.0);
                if let Some(rr) = intersect(Rect::new(cx - s * 0.5, cy - s * 0.5, s, s), pclip) {
                    out.shapes.push(paint::rect(sc, rr, s * 0.5).on_fill(paint::IRID.0, paint::IRID.1, 0.18).alpha(ga).done());
                }
            }
            let (k, w) = self.text.get(fs, &day.to_string(), F_CAL);
            let lh = self.text.line_h(F_CAL);
            let col_c = if is_today {
                theme::on_ink()
            } else if col >= 5.0 {
                SOFT.mix(MUTED, 0.4)
            } else {
                TEXT
            };
            self.text(out, k, cx - w * 0.5, cy - lh * 0.5, col_c.fade(ga), pclip);
            if marks.contains(&(day as u8)) {
                out.shapes.push(paint::circle(sc, cx, cy + ch * 0.5 - 5.0, 1.6).fill(aa).alpha(ga).done());
            }
        }
    }

    /// A page's surface: the row it came from grows into a faint card
    /// filling the panel (concentric with it).
    fn page_bg(&self, out: &mut DrawList, d: Rect, t: f32) {
        let e = t.clamp(0.0, 1.1);
        let o = self.origin;
        let m = Rect::new(lerp(o.x, d.x, e), lerp(o.y, d.y, e), lerp(o.w, d.w, e), lerp(o.h, d.h, e));
        let bg = HOVER.mix(PAGE_BG, (t * 1.6).clamp(0.0, 1.0));
        out.shapes.push(paint::rect(self.scale, m, lerp(16.0, R_OPEN - 10.0, e)).fill(bg).alpha((t * 5.0).clamp(0.0, 1.0)).done());
    }

    /// Shift the frame by the part of the drag offset the surface itself
    /// doesn't carry (it moves in whole pixels).
    fn translate(&mut self, out: &mut DrawList) {
        let dx = self.off_x.get() - self.surf_origin.0 as f32;
        let dy = self.off_y.get() - self.surf_origin.1 as f32;
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let (px, py) = (dx * self.scale, dy * self.scale);
        for s in &mut out.shapes {
            s.rect[0] += px;
            s.rect[1] += py;
            s.spot[0] += px;
            s.spot[1] += py;
        }
        for i in &mut out.images {
            i.rect[0] += px;
            i.rect[1] += py;
            i.reveal[0] += px;
            i.reveal[1] += py;
        }
        for t in &mut out.texts {
            t.x = (t.x + px).round();
            t.y = (t.y + py).round();
            t.clip = [t.clip[0] + px.round() as i32, t.clip[1] + py.round() as i32, t.clip[2] + px.round() as i32, t.clip[3] + py.round() as i32];
        }
        for i in &mut out.icons {
            i.x += px;
            i.y += py;
        }
        for (r, _) in &mut self.hits {
            r.x += dx;
            r.y += dy;
        }
        self.body.x += dx;
        self.body.y += dy;
    }

    /// A detail page (Wi-Fi / Bluetooth): the tile it came from grows to
    /// fill the panel, then its header, switch and list fade in.
    fn draw_page(&mut self, fs: &mut FontSystem, out: &mut DrawList, r: Rect, top: f32, clip: Rect) {
        let sc = self.scale;
        let t = self.page_t.get();
        let e = t.clamp(0.0, 1.1);
        let d = Rect::new(r.x + 10.0, top + 10.0, r.w - 20.0, r.h - 20.0);
        let _ = e;
        self.page_bg(out, d, t);
        let a = ((t - 0.55) / 0.45).clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let wifi = self.shown_page == Page::Wifi;

        // Header: back, title, switch.
        let hy = d.y + 12.0;
        let back = Rect::new(d.x + 6.0, hy - 2.0, 32.0, 32.0);
        self.hits.push((back, Ctl::Back));
        let bh = self.hover_of(Ctl::Back);
        self.hover_bg(out, back, 16.0, a, bh);
        self.icon(out, "go-previous-symbolic", back.x + 8.0, back.y + 8.0, 16.0, TEXT.fade(a));
        let (k, _) = self.text.get(fs, if wifi { "Wi-Fi" } else { "Bluetooth" }, F_PAGE);
        let lh = self.text.line_h(F_PAGE);
        self.text(out, k, d.x + 46.0, hy + 14.0 - lh * 0.5, TEXT.fade(a), clip);
        let sw = Rect::new(d.x + d.w - 14.0 - 42.0, hy + 3.0, 42.0, 24.0);
        self.hits.push((sw, Ctl::Switch));
        let st = self.switch_t.get();
        let stc = st.clamp(0.0, 1.0);
        out.shapes.push(paint::rect(sc, sw, 12.0).on_mix(TRACK, stc, 1.0, 0.0, 0.0).alpha(a).done());
        let kx = lerp(sw.x + 12.0, sw.x + sw.w - 12.0, st);
        out.shapes.push(
            paint::circle(sc, kx, sw.y + 12.0, 9.0 + self.hover_of(Ctl::Switch))
                .fill(Rgba::hex(0xffffff, 1.0))
                .shadow(Rgba::hex(0x000000, 0.4), 3.0, 1.0)
                .alpha(a)
                .done(),
        );

        // List.
        let ly = d.y + 54.0;
        let max_rows = ((d.h - 62.0) / ROW_H).floor() as usize;
        let enabled = if wifi { self.system.wifi } else { self.system.bluetooth };
        struct RowView {
            name: String,
            icon: String,
            right: Option<(&'static str, Rgba)>,
            lock: bool,
        }
        let rows: Option<Vec<RowView>> = if enabled == Some(false) {
            Some(vec![])
        } else if wifi {
            self.wifi_nets.as_ref().map(|v| {
                v.iter()
                    .take(max_rows)
                    .map(|n| {
                        let lvl = match n.signal {
                            75.. => "excellent",
                            50..=74 => "good",
                            25..=49 => "ok",
                            _ => "weak",
                        };
                        let pending = self.pending.as_deref() == Some(n.ssid.as_str());
                        RowView {
                            name: n.ssid.clone(),
                            icon: format!("network-wireless-signal-{lvl}-symbolic"),
                            right: if pending {
                                Some(("Connecting…", MUTED))
                            } else if n.active {
                                Some(("Connected", theme::accent().0))
                            } else {
                                None
                            },
                            lock: n.secure && !n.active && !pending,
                        }
                    })
                    .collect()
            })
        } else {
            self.bt_devs.as_ref().map(|v| {
                v.iter()
                    .take(max_rows)
                    .map(|dv| {
                        let pending = self.pending.as_deref() == Some(dv.mac.as_str());
                        RowView {
                            name: dv.name.clone(),
                            icon: dv.kind.clone().map_or("bluetooth-symbolic".into(), |k| format!("{k}-symbolic")),
                            right: if pending {
                                Some(("…", MUTED))
                            } else if dv.connected {
                                Some(("Connected", theme::accent().0))
                            } else {
                                None
                            },
                            lock: false,
                        }
                    })
                    .collect()
            })
        };
        let empty = match (&rows, enabled) {
            (_, Some(false)) => Some(if wifi { "Wi-Fi is off" } else { "Bluetooth is off" }),
            (None, _) => Some(if wifi { "Scanning…" } else { "Looking for devices…" }),
            (Some(v), _) if v.is_empty() => Some(if wifi { "No networks found" } else { "No paired devices" }),
            _ => None,
        };
        if let Some(msg) = empty {
            let (k, w) = self.text.get(fs, msg, F_SMALL);
            self.text(out, k, d.x + (d.w - w) * 0.5, ly + 40.0, MUTED.fade(a), clip);
            return;
        }
        for (i, rv) in rows.unwrap_or_default().into_iter().enumerate() {
            let rr = Rect::new(d.x + 6.0, ly + i as f32 * ROW_H, d.w - 12.0, ROW_H - 2.0);
            let c = Ctl::Row(i as u8);
            self.hits.push((rr, c));
            // Rows cascade in after the header.
            let ra = a * ((self.page_t.get() - 0.6 - 0.05 * i as f32) / 0.3).clamp(0.0, 1.0);
            let h = self.hover_of(c);
            self.hover_bg(out, rr, 12.0, ra, h);
            let icon = self.icons.id(&[rv.icon.as_str(), "bluetooth-symbolic"]);
            out.icons.push(IconDraw {
                id: icon,
                x: (rr.x + 10.0) * sc,
                y: (rr.y + rr.h * 0.5 - 8.0) * sc,
                size: (16.0 * sc).round(),
                color: TEXT.fade(ra),
            });
            let mut right_w = 0.0;
            if let Some((label, col)) = rv.right {
                let (k, w) = self.text.get(fs, label, F_SMALL);
                right_w = w + 10.0;
                let lh = self.text.line_h(F_SMALL);
                self.text(out, k, rr.x + rr.w - 12.0 - w, rr.y + rr.h * 0.5 - lh * 0.5, col.fade(ra), clip);
            } else if rv.lock {
                right_w = 26.0;
                self.icon(out, "network-wireless-encrypted-symbolic", rr.x + rr.w - 28.0, rr.y + rr.h * 0.5 - 8.0, 16.0, MUTED.fade(ra));
            }
            let (k, _) = self.text.get_fit(fs, &rv.name, F_LABEL, rr.w - 50.0 - right_w);
            let lh = self.text.line_h(F_LABEL);
            self.text(out, k, rr.x + 36.0, rr.y + rr.h * 0.5 - lh * 0.5, TEXT.mix(SOFT, 0.0).fade(ra), clip);
        }
    }

    /// A box-less slider: icon, a thin track with an iridescent fill and
    /// a clear glass knob, the value muted on the right.
    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        fs: &mut FontSystem,
        out: &mut DrawList,
        c: Ctl,
        icon: &str,
        y: f32,
        level: f32,
        active: bool,
        a: f32,
        clip: Rect,
        l: f32,
        rr: f32,
    ) {
        let sc = self.scale;
        let rail_l = l + 30.0;
        let track = Rect::new(rail_l, y, (rr - 42.0 - rail_l).max(1.0), SLIDER_H);
        self.hits.push((track, c));
        if a <= 0.0 {
            return;
        }
        let h = self.hover_of(c).max(if self.dragging == c { 1.0 } else { 0.0 });
        let cy = y + SLIDER_H * 0.5;
        self.icon(out, icon, l + 1.0, cy - 8.0, 16.0, SOFT.mix(TEXT, h).fade(a * (0.75 + 0.25 * h)));
        let lv = level.clamp(0.0, 1.0);
        out.shapes.push(paint::rect(sc, Rect::new(track.x, cy - 2.0, track.w, 4.0), 2.0).fill(TRACK).alpha(a).done());
        let fw = track.w * lv;
        if fw > 0.5 {
            let fill_a = if active { 1.0 } else { 0.4 };
            out.shapes.push(paint::rect(sc, Rect::new(track.x, cy - 2.0, fw.max(4.0), 4.0), 2.0).on_fill(1.0, 0.0, 0.0).alpha(a * fill_a).done());
        }
        out.shapes.push(paint::knob(sc, track.x + fw, cy, 11.0 + 0.5 * h).alpha(a).done());
        let (k, w) = self.text.get(fs, &format!("{}%", (lv * 100.0).round() as u32), F_VAL);
        let lh = self.text.line_h(F_VAL);
        self.text(out, k, rr - w, cy - lh * 0.5, MUTED.mix(SOFT, 0.3 + 0.7 * h).fade(a), clip);
    }
}

/// Track span (left, width) for a slider's hit rect.
fn slider_track(row: Rect) -> (f32, f32) {
    (row.x, row.w)
}

/// Today in local time: (year, month 1..=12, day).
fn today() -> (i32, u32, u32) {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32)
}

/// Days since 1970-01-01 (proleptic Gregorian).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Monday = 0 … Sunday = 6.
fn weekday_mon0(y: i32, m: u32, d: u32) -> u32 {
    // 1970-01-01 was a Thursday.
    (days_from_civil(y, m, d) + 3).rem_euclid(7) as u32
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

/// Days of a month that carry an event dot. Hook for a calendar source
/// (none yet).
fn day_marks(_y: i32, _m: u32) -> Vec<u8> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_math() {
        assert_eq!(weekday_mon0(2026, 10, 25), 6); // a Sunday
        assert_eq!(weekday_mon0(2024, 2, 29), 3); // a Thursday
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 12), 31);
    }
}
