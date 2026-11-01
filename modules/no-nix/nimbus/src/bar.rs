//! Shared bar state plus the default design ("islands"):
//!
//!   [ spaces ]            [ window title ]            [ status · clock ]
//!
//! Facts set targets; `step` integrates springs; `draw` emits shapes, text
//! and icons in physical pixels. Other designs live in `designs/` and draw
//! from the same state (facts, springs, text cache, hit list).

use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight};

use crate::{
    anim::{ColorSpring, Spring, SpringCfg, presets},
    designs::{self, Design},
    gfx::{DrawList, IconDraw, Shape, TextDraw},
    icons::IconSet,
    services::{BatteryInfo, ClockInfo, Fact, NetInfo, VolumeInfo, WorkspaceInfo},
    theme::{self, Look, Rgba},
};

pub const SURFACE_H: u32 = 46;
pub const EXCLUSIVE: i32 = 40;
/// Vertical centre line of the islands (logical px).
const ISLAND_CY: f32 = 22.0;
pub(crate) const MARGIN: f32 = 9.0;

const SLOT_W: f32 = 17.0;
const ACTIVE_W: f32 = 36.0;
const SPACES_PAD: f32 = 8.0;
const DOT: f32 = 6.0;
const IND_H: f32 = 8.0;
const IND_INSET: f32 = 5.0;

pub(crate) const ICON: f32 = 16.0;
const TITLE_MAX: f32 = 420.0;

const F_TITLE: Font = Font::new(13.0, 500);
const F_CLOCK: Font = Font::new(14.0, 600);
const F_DATE: Font = Font::new(13.0, 500);
const F_SMALL: Font = Font::new(12.0, 600);

const ENTER: SpringCfg = SpringCfg::new(0.62, 0.68);
const POINTER: SpringCfg = SpringCfg::new(0.14, 1.0);
pub(crate) const DIGIT: SpringCfg = SpringCfg::new(0.38, 0.74);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    None,
    Spaces,
    Workspace(u64),
    Title,
    Volume,
    Clock,
    Status,
    /// A design's main body (hover target, scroll switches workspace).
    Panel,
    /// The now-playing segment (click: play/pause, scroll: seek).
    Media,
    /// Transport buttons: 0 previous, 1 play/pause, 2 next.
    MediaCtl(u8),
    /// A system tray icon (index into `Bar::tray`).
    Tray(u8),
    /// The pulsing dot while the screen is being recorded.
    Record,
}

impl Hit {
    pub fn clickable(self) -> bool {
        matches!(self, Hit::Workspace(_) | Hit::Volume | Hit::Title | Hit::Media | Hit::MediaCtl(_) | Hit::Tray(_) | Hit::Record)
    }
}

pub enum Cmd {
    FocusWorkspace(u64),
    WorkspaceUp,
    WorkspaceDown,
    Volume(i32),
    Mute,
    /// Open or close the settings panel (tabs design: click the centre tab).
    ToggleSettings,
    Media(crate::media::Ctl),
    /// Tray item (id, title) clicked, at output-logical x.
    Tray(String, String, crate::tray::Click, f32),
    /// The recording dot was clicked.
    StopRecording,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn inflate(&self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
}

// ── text cache ──────────────────────────────────────────────────────────

/// A text style. `family: None` means the look's UI face (with the look's
/// size/weight tuning); designs can name any loaded family directly.
#[derive(Clone, Copy)]
pub(crate) struct Font {
    pub size: f32,
    pub weight: u16,
    pub family: Option<&'static str>,
    /// Letter spacing in em.
    pub track: f32,
    pub italic: bool,
}

impl Font {
    pub const fn new(size: f32, weight: u16) -> Self {
        Self { size, weight, family: None, track: 0.0, italic: false }
    }
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub const fn family(mut self, f: &'static str) -> Self {
        self.family = Some(f);
        self
    }
    pub const fn track(mut self, em: f32) -> Self {
        self.track = em;
        self
    }
}

impl Hash for Font {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.size.to_bits().hash(h);
        self.weight.hash(h);
        self.family.hash(h);
        self.track.to_bits().hash(h);
        self.italic.hash(h);
    }
}


pub struct TextCache {
    family: &'static str,
    fscale: f32,
    wshift: i16,
    pub bufs: HashMap<u64, Buffer>,
    widths: HashMap<u64, f32>,
    used: HashSet<u64>,
    scale: f32,
}

impl TextCache {
    pub(crate) fn new(fs: &mut FontSystem, look: &Look) -> Self {
        let mut bufs = HashMap::new();
        bufs.insert(0, Buffer::new(fs, Metrics::new(12.0, 14.0)));
        Self {
            family: look.font,
            fscale: look.font_scale,
            wshift: look.weight_shift,
            bufs,
            widths: HashMap::new(),
            used: HashSet::new(),
            scale: 1.0,
        }
    }

    fn tuned(&self, f: Font) -> (f32, u16, &'static str) {
        match f.family {
            Some(fam) => (f.size, f.weight, fam),
            None => (
                f.size * self.fscale,
                (f.weight as i16 + self.wshift).clamp(100, 900) as u16,
                self.family,
            ),
        }
    }

    pub(crate) fn line_h(&self, f: Font) -> f32 {
        (self.tuned(f).0 * 1.3).round()
    }

    pub(crate) fn set_scale(&mut self, s: f32) {
        if s != self.scale {
            self.scale = s;
            self.bufs.retain(|k, _| *k == 0);
            self.widths.clear();
        }
    }

    fn key(s: &str, f: Font) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        s.hash(&mut h);
        f.hash(&mut h);
        h.finish() | 1
    }

    fn shape(&self, fs: &mut FontSystem, s: &str, f: Font) -> (Buffer, f32) {
        let sc = self.scale;
        let (size, weight, family) = self.tuned(f);
        let mut b = Buffer::new(fs, Metrics::new(size * sc, self.line_h(f) * sc));
        b.set_size(None, None);
        let mut attrs = Attrs::new().family(Family::Name(family)).weight(Weight(weight));
        if f.track != 0.0 {
            attrs = attrs.letter_spacing(f.track);
        }
        if f.italic {
            attrs = attrs.style(glyphon::Style::Italic);
        }
        b.set_text(s, &attrs, Shaping::Advanced, None);
        b.shape_until_scroll(fs, false);
        let w = b.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
        (b, w / sc)
    }

    /// Returns (key, logical width). Shapes once, then it's a hash lookup.
    pub(crate) fn get(&mut self, fs: &mut FontSystem, s: &str, f: Font) -> (u64, f32) {
        let k = Self::key(s, f);
        if !self.bufs.contains_key(&k) {
            let (b, w) = self.shape(fs, s, f);
            self.bufs.insert(k, b);
            self.widths.insert(k, w);
        }
        self.used.insert(k);
        (k, self.widths[&k])
    }

    /// Shape `s`, truncating with an ellipsis to fit `max` logical px.
    pub(crate) fn get_fit(&mut self, fs: &mut FontSystem, s: &str, f: Font, max: f32) -> (u64, f32) {
        let k = Self::key(s, f) ^ (max.to_bits() as u64) << 1;
        if !self.bufs.contains_key(&k) {
            let (mut b, mut w) = self.shape(fs, s, f);
            if w > max {
                let chars: Vec<char> = s.chars().collect();
                let (mut lo, mut hi) = (0usize, chars.len());
                while lo < hi {
                    let mid = (lo + hi).div_ceil(2);
                    let t: String = chars[..mid].iter().collect::<String>().trim_end().to_string() + "…";
                    if self.shape(fs, &t, f).1 <= max { lo = mid } else { hi = mid - 1 }
                }
                let t: String = chars[..lo].iter().collect::<String>().trim_end().to_string() + "…";
                (b, w) = self.shape(fs, &t, f);
            }
            self.bufs.insert(k, b);
            self.widths.insert(k, w);
        }
        self.used.insert(k);
        (k, self.widths[&k])
    }

    /// Logical x extents of the glyphs covering the byte ranges `spans` of
    /// a shaped text (`key` from `get`/`get_fit`): for drawing matched
    /// characters over it in another colour.
    pub(crate) fn extents(&self, key: u64, spans: &[(u16, u16)], out: &mut Vec<(f32, f32)>) {
        let Some(b) = self.bufs.get(&key) else { return };
        for run in b.layout_runs() {
            for g in run.glyphs {
                if !spans.iter().any(|s| (g.start as u16) < s.1 && (g.end as u16) > s.0) {
                    continue;
                }
                let (x0, x1) = (g.x / self.scale, (g.x + g.w) / self.scale);
                match out.last_mut() {
                    Some(l) if (x0 - l.1).abs() < 0.5 => l.1 = x1,
                    _ => out.push((x0, x1)),
                }
            }
        }
    }

    pub(crate) fn end_frame(&mut self) {
        if self.bufs.len() > 128 {
            let used = &self.used;
            self.bufs.retain(|k, _| *k == 0 || used.contains(k));
            self.widths.retain(|k, _| used.contains(k));
        }
        self.used.clear();
    }
}

// ── animated parts ──────────────────────────────────────────────────────

struct Island {
    enter: Spring,
    delay: f32,
    lift: Spring,
    press: Spring,
    show: Spring,
}

impl Island {
    fn new(delay: f32) -> Self {
        let mut enter = Spring::new(0.0, ENTER);
        if delay <= 0.0 {
            enter.set(1.0);
        }
        Self {
            enter,
            delay,
            lift: Spring::new(0.0, presets::SNAPPY),
            press: Spring::new(0.0, presets::SNAPPY),
            show: Spring::new(1.0, presets::SMOOTH),
        }
    }
    fn step(&mut self, dt: f32) -> bool {
        let mut m = false;
        if self.delay > 0.0 {
            self.delay -= dt;
            if self.delay <= 0.0 {
                self.enter.set(1.0);
            }
            m = true;
        }
        m |= self.enter.step(dt);
        m |= self.lift.step(dt);
        m |= self.press.step(dt);
        m |= self.show.step(dt);
        m
    }
    fn alpha(&self) -> f32 {
        (self.enter.get().clamp(0.0, 1.0) * self.show.get().clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }
    fn dy(&self) -> f32 {
        -(1.0 - self.enter.get()) * 12.0
    }
}

struct Slot {
    id: u64,
    w: Spring,
    alive: Spring,
    dot: Spring,
    fill: ColorSpring,
    /// 0 = hollow ring (empty workspace), 1 = filled dot.
    solid: Spring,
    gone: bool,
}

pub(crate) struct Digit {
    pub cur: char,
    pub prev: char,
    pub p: Spring,
    delay: f32,
}

impl Digit {
    fn new() -> Self {
        Self { cur: ' ', prev: ' ', p: Spring::new(1.0, DIGIT), delay: 0.0 }
    }
    fn set(&mut self, c: char, delay: f32, animate: bool) {
        if c == self.cur {
            return;
        }
        self.prev = self.cur;
        self.cur = c;
        if animate {
            self.p.snap(0.0);
            self.delay = delay;
            if delay <= 0.0 {
                self.p.set(1.0);
            }
        } else {
            self.p.snap(1.0);
        }
    }
    fn step(&mut self, dt: f32) -> bool {
        if self.delay > 0.0 {
            self.delay -= dt;
            if self.delay <= 0.0 {
                self.p.set(1.0);
            }
            return true;
        }
        self.p.step(dt)
    }
}

/// Cross-fades between two icons.
pub(crate) struct IconSwap {
    pub cur: u16,
    pub prev: u16,
    pub t: Spring,
}

impl IconSwap {
    pub(crate) fn new(id: u16) -> Self {
        Self { cur: id, prev: id, t: Spring::new(1.0, presets::SMOOTH) }
    }
    fn set(&mut self, id: u16) {
        if id != self.cur {
            self.prev = self.cur;
            self.cur = id;
            self.t.snap(0.0);
            self.t.set(1.0);
        }
    }
}

pub(crate) struct TitleState {
    pub cur: Option<String>,
    pub prev: Option<String>,
    /// Cross-fade progress from `prev` to `cur`, 0 → 1.
    pub xf: Spring,
    pub w: Spring,
}

// ── the bar ─────────────────────────────────────────────────────────────

pub struct Bar {
    pub look: &'static Look,
    pub output: String,
    pub width: f32,
    pub scale: f32,
    pub text: TextCache,
    pub(crate) icons: std::rc::Rc<IconSet>,

    pub(crate) workspaces: Vec<WorkspaceInfo>,
    pub(crate) clock: Option<ClockInfo>,
    pub(crate) battery: Option<BatteryInfo>,
    pub(crate) net: NetInfo,
    pub(crate) volume: Option<VolumeInfo>,
    /// Focused window's app id (this output only) and its colour icon.
    pub(crate) app_id: Option<String>,
    /// (app id, its icon or None when it has none).
    pub(crate) app_icon: Option<(String, Option<crate::gfx::ImageData>)>,
    /// Now playing, and the bar-clock time at which `media.position` held.
    pub(crate) media: Option<crate::media::MediaInfo>,
    pub(crate) media_t0: f32,
    pub(crate) media_art: Option<std::sync::Arc<crate::media::Art>>,
    pub(crate) tray: std::sync::Arc<Vec<crate::tray::TrayItem>>,
    pub(crate) toggles: crate::toggles::ToggleState,
    pub(crate) weather: Option<crate::weather::Weather>,

    pub(crate) spaces: Island,
    pub(crate) center: Island,
    pub(crate) status: Island,
    pub(crate) slots: Vec<Slot>,
    pub(crate) ind_l: Spring,
    pub(crate) ind_r: Spring,
    pub(crate) ind_alpha: Spring,
    pub(crate) have_ws: bool,
    pub(crate) title: TitleState,
    pub(crate) digits: [Digit; 4],
    pub(crate) date_reveal: Spring,
    pub(crate) vol_reveal: Spring,
    /// Render-clock time at which the peeked volume level tucks away.
    pub(crate) vol_hide_at: f32,
    pub(crate) vol_icon: IconSwap,
    pub(crate) net_icon: IconSwap,
    pub(crate) bat_icon: IconSwap,

    pub(crate) pointer: Option<(f32, f32)>,
    pub(crate) spot_x: Spring,
    pub(crate) spot_y: Spring,
    pub hovered: Hit,
    pub(crate) pressed: Hit,
    pub(crate) scroll_acc: f32,
    pub(crate) hits: Vec<(Rect, Hit)>,
    /// Bodies in logical px, for the blur and input regions.
    pub glass: Vec<Rect>,

    // ── shared with designs ─────────────────────────────────────────────
    pub(crate) design: Design,
    pub(crate) ds: designs::States,
    /// Seconds since start (the render clock).
    pub(crate) time: f32,
    /// When things last changed, on the render clock; designs use these to
    /// stage transient "peek" animations.
    pub(crate) ws_changed: f32,
    pub(crate) title_changed: f32,
    /// When a different window took focus (title edits don't count).
    pub(crate) focus_changed: f32,
    pub(crate) focus_id: Option<u64>,
    focus_seen: bool,
    /// One of nimbus's own windows has the keyboard: niri then reports no
    /// focused window, but the app the user was in should stay on the bar.
    hold_focus: bool,
    /// The last focused window seen here (title, app id, id), restored when
    /// a nimbus window takes the keyboard just after niri reported "none".
    last_focus: Option<(Option<String>, Option<String>, Option<u64>)>,
    pub(crate) vol_changed: f32,
    /// Active workspace as a float slot index; head is snappy, tail trails.
    pub(crate) ws_head: Spring,
    pub(crate) ws_tail: Spring,
    /// Earliest render-clock time something wants a frame while idle.
    pub wake_at: Option<f32>,
    /// Where the centre tab is (tabs design), so the settings panel can
    /// grow out of exactly that shape.
    pub center_tab: Rect,
    /// All three tabs (left, centre, right), tabs design only.
    pub tabs: [Rect; 3],
    /// 0..1: how much the settings panel currently covers the centre tab.
    pub popup_cover: f32,
}

impl Bar {
    pub fn new(output: String, fs: &mut FontSystem, icons: std::rc::Rc<IconSet>) -> Self {
        let vol = icons.id(&["audio-volume-high-symbolic"]);
        let net = icons.id(&["network-offline-symbolic"]);
        let bat = icons.id(&["battery-missing-symbolic"]);
        let look = theme::current();
        Self {
            look,
            output,
            width: 0.0,
            scale: 1.0,
            text: TextCache::new(fs, look),
            icons,
            workspaces: vec![],
            clock: None,
            battery: None,
            net: NetInfo::Offline,
            volume: None,
            app_id: None,
            app_icon: None,
            media: None,
            media_t0: 0.0,
            media_art: None,
            tray: Default::default(),
            toggles: Default::default(),
            weather: None,
            spaces: Island::new(0.0),
            center: Island::new(0.07),
            status: Island::new(0.14),
            slots: vec![],
            ind_l: Spring::new(0.0, presets::BOUNCY),
            ind_r: Spring::new(0.0, presets::TAIL),
            ind_alpha: Spring::new(0.0, presets::SMOOTH),
            have_ws: false,
            title: TitleState {
                cur: None,
                prev: None,
                xf: Spring::new(1.0, presets::SMOOTH),
                w: Spring::new(0.0, presets::SMOOTH),
            },
            digits: [Digit::new(), Digit::new(), Digit::new(), Digit::new()],
            date_reveal: Spring::new(0.0, presets::SMOOTH),
            vol_reveal: Spring::new(0.0, presets::SMOOTH),
            vol_hide_at: -1.0,
            vol_icon: IconSwap::new(vol),
            net_icon: IconSwap::new(net),
            bat_icon: IconSwap::new(bat),
            pointer: None,
            spot_x: Spring::new(0.0, POINTER),
            spot_y: Spring::new(0.0, POINTER),
            hovered: Hit::None,
            pressed: Hit::None,
            scroll_acc: 0.0,
            hits: vec![],
            glass: vec![],
            design: Design::current(),
            ds: designs::States::default(),
            time: 0.0,
            ws_changed: -100.0,
            title_changed: -100.0,
            focus_changed: -100.0,
            focus_id: None,
            focus_seen: false,
            hold_focus: false,
            last_focus: None,
            vol_changed: -100.0,
            ws_head: Spring::new(0.0, presets::BOUNCY),
            ws_tail: Spring::new(0.0, presets::TAIL),
            wake_at: None,
            center_tab: Rect::default(),
            tabs: [Rect::default(); 3],
            popup_cover: 0.0,
        }
    }

    /// Accent changed: nothing to retarget, just draw again.
    pub fn dirty_all(&mut self) {}

    /// Switch design live (settings window). Fresh design state, so the new
    /// one plays its entrance.
    pub fn set_design(&mut self, d: Design) {
        self.design = d;
        self.ds = designs::States::default();
        self.popup_cover = 0.0;
    }

    pub fn resize(&mut self, width: f32, scale: f32) {
        self.width = width;
        self.scale = scale;
        self.text.set_scale(scale);
    }

    // ── facts → targets ─────────────────────────────────────────────────

    pub fn apply(&mut self, fact: &Fact) {
        match fact {
            Fact::Niri(s) => {
                let mine: Vec<WorkspaceInfo> =
                    s.workspaces.iter().filter(|w| w.output == self.output).cloned().collect();
                self.set_workspaces(mine);
                let here = s.focused_output.as_deref() == Some(self.output.as_str());
                if here && s.focused_id.is_none() && self.hold_focus {
                    // A nimbus window took the keyboard: keep the app.
                    return;
                }
                let title = if here {
                    s.title.clone().filter(|t| !t.trim().is_empty()).or_else(|| s.app_id.clone())
                } else {
                    None
                };
                self.app_id = if here { s.app_id.clone() } else { None };
                let focus = if here { s.focused_id } else { None };
                if focus.is_some() {
                    self.last_focus = Some((title.clone(), self.app_id.clone(), focus));
                }
                if focus != self.focus_id {
                    if self.focus_seen {
                        self.focus_changed = self.time;
                    }
                    self.focus_id = focus;
                }
                self.focus_seen = true;
                self.set_title(title);
            }
            Fact::Clock(c) => self.set_clock(*c),
            Fact::Media(m) => {
                self.media_t0 = self.time - m.as_ref().map_or(0.0, |m| m.stamp.elapsed().as_secs_f32());
                self.media = m.clone();
            }
            Fact::MediaArt(a) => self.media_art = Some(a.clone()),
            Fact::AppIcon(id, img) => self.app_icon = Some((id.clone(), img.clone())),
            Fact::Tray(v) => self.tray = v.clone(),
            Fact::Toggles(t) => self.toggles = *t,
            Fact::Weather(w) => self.weather = w.clone(),
            Fact::System(_) | Fact::WifiList(_) | Fact::BtList(_) | Fact::Audio(_) | Fact::Outputs(_) | Fact::Power(_) | Fact::About(_) | Fact::Command(_) | Fact::Custom(_) => {}
            Fact::Battery(b) => {
                self.battery = *b;
                let id = match b {
                    Some(b) => self.icons.battery(b.percent, b.charging),
                    None => self.icons.id(&["battery-missing-symbolic"]),
                };
                self.bat_icon.set(id);
            }
            Fact::Net(n) => {
                self.net = *n;
                let name = match n {
                    NetInfo::Offline => "network-offline-symbolic",
                    NetInfo::Ethernet => "network-wired-symbolic",
                    NetInfo::Wifi(q) => match q {
                        0..=20 => "network-wireless-signal-weak-symbolic",
                        21..=45 => "network-wireless-signal-ok-symbolic",
                        46..=70 => "network-wireless-signal-good-symbolic",
                        _ => "network-wireless-signal-excellent-symbolic",
                    },
                };
                self.net_icon.set(self.icons.id(&[name]));
            }
            Fact::Volume(v) => {
                let changed = self.volume.is_some_and(|o| o != *v);
                self.volume = Some(*v);
                let name = if v.muted || v.percent == 0 {
                    "audio-volume-muted-symbolic"
                } else if v.percent < 34 {
                    "audio-volume-low-symbolic"
                } else if v.percent < 67 {
                    "audio-volume-medium-symbolic"
                } else {
                    "audio-volume-high-symbolic"
                };
                self.vol_icon.set(self.icons.id(&[name]));
                if changed {
                    self.vol_changed = self.time;
                    // Volume keys: peek the level, then tuck it away again.
                    self.vol_reveal.set(1.0);
                    self.vol_hide_at = self.time + 1.4;
                }
            }
        }
    }

    fn set_workspaces(&mut self, ws: Vec<WorkspaceInfo>) {
        let first = !self.have_ws;
        self.have_ws = true;
        let mut old = std::mem::take(&mut self.slots);
        let mut next: Vec<Slot> = Vec::with_capacity(ws.len());
        for w in &ws {
            let slot = match old.iter().position(|s| s.id == w.id && !s.gone) {
                Some(i) => old.remove(i),
                None => Slot {
                    id: w.id,
                    w: Spring::new(if first { SLOT_W } else { 0.0 }, presets::SMOOTH),
                    alive: Spring::new(if first { 1.0 } else { 0.0 }, presets::SMOOTH),
                    dot: Spring::new(DOT, presets::SNAPPY),
                    fill: ColorSpring::new(self.look.dot_empty, presets::SMOOTH),
                    solid: Spring::new(0.0, presets::SMOOTH),
                    gone: false,
                },
            };
            next.push(slot);
        }
        // Vanished workspaces shrink out from where they were.
        for (i, mut s) in old.into_iter().enumerate() {
            s.gone = true;
            s.w.set(0.0);
            s.alive.set(0.0);
            next.insert(i.min(next.len()), s);
        }
        for s in &mut next {
            let Some(info) = ws.iter().find(|w| w.id == s.id) else { continue };
            s.alive.set(1.0);
            s.w.set(if info.active { ACTIVE_W } else { SLOT_W });
            s.solid.set(if info.windows > 0 { 1.0 } else { 0.0 });
            let c = if info.urgent {
                theme::BAD
            } else if info.windows > 0 {
                self.look.dot_used
            } else {
                self.look.dot_empty
            };
            s.fill.set(c);
            if first {
                s.w.snap(s.w.target);
                s.solid.snap(s.solid.target);
                s.fill = ColorSpring::new(c, presets::SMOOTH);
            }
        }
        self.slots = next;
        self.workspaces = ws;

        // Indicator: target edges from the *target* layout.
        let mut x = SPACES_PAD;
        let mut target = None;
        for s in &self.slots {
            let active = self.workspaces.iter().any(|w| w.id == s.id && w.active);
            if active {
                target = Some((x + IND_INSET, x + ACTIVE_W - IND_INSET));
            }
            x += s.w.target;
        }
        let live: Vec<&WorkspaceInfo> = self.workspaces.iter().collect();
        if let Some(ai) = live.iter().position(|w| w.active) {
            let ai = ai as f32;
            if first || self.ind_alpha.target == 0.0 {
                self.ws_head.snap(ai);
                self.ws_tail.snap(ai);
            } else if ai != self.ws_head.target {
                self.ws_head.set(ai);
                self.ws_tail.set(ai);
                self.ws_changed = self.time;
            }
        }
        if let Some((l, r)) = target {
            if first || self.ind_alpha.target == 0.0 {
                self.ind_l.snap(l);
                self.ind_r.snap(r);
            } else {
                // The edge in the direction of travel leads; the other trails
                // behind on a softer spring: a liquid stretch.
                let moving_right = (l + r) > (self.ind_l.target + self.ind_r.target);
                let (head, tail) = (presets::BOUNCY, presets::TAIL);
                self.ind_l.cfg = if moving_right { tail } else { head };
                self.ind_r.cfg = if moving_right { head } else { tail };
                self.ind_l.set(l);
                self.ind_r.set(r);
            }
            self.ind_alpha.set(1.0);
        } else {
            self.ind_alpha.set(0.0);
        }
    }

    /// A nimbus window gained (true) or lost the keyboard. On gaining it,
    /// undo a "nothing focused" that niri reported just before.
    pub fn hold_focus(&mut self, on: bool) -> bool {
        self.hold_focus = on;
        if on
            && self.focus_id.is_none()
            && let Some((title, app, id)) = self.last_focus.clone()
        {
            self.app_id = app;
            self.focus_id = id;
            self.set_title(title);
            return true;
        }
        false
    }

    fn set_title(&mut self, t: Option<String>) {
        // Terminals and editors prefix titles with spinners and status dots
        // (`◐ claude`, `● main.rs`) that change many times a second; they
        // would keep the title cross-fading forever.
        let t = t.map(|t| {
            let s = t.trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_ascii() || c.is_whitespace());
            if s.is_empty() { t } else { s.to_string() }
        });
        if t == self.title.cur {
            return;
        }
        let was_none = self.title.cur.is_none();
        self.title_changed = self.time;
        self.title.prev = self.title.cur.take();
        self.title.cur = t;
        self.title.xf.snap(0.0);
        self.title.xf.set(1.0);
        self.center.show.set(if self.title.cur.is_some() { 1.0 } else { 0.0 });
        if was_none && self.title.cur.is_some() {
            // Appearing from nothing: don't cross-fade from an old title.
            self.title.prev = None;
        }
    }

    fn set_clock(&mut self, c: ClockInfo) {
        let animate = self.clock.is_some();
        self.clock = Some(c);
        let s = format!("{:02}{:02}", c.hour, c.minute);
        // Right-most digit rolls first; the others follow like an odometer.
        for (i, ch) in s.chars().enumerate() {
            self.digits[i].set(ch, (3 - i) as f32 * 0.06, animate);
        }
    }

    // ── input ───────────────────────────────────────────────────────────

    /// Centre of the most specific hit area for `h` (used by snapshots).
    pub fn hit_center(&self, h: Hit) -> Option<(f32, f32)> {
        self.hits.iter().rev().find(|(_, x)| *x == h).map(|(r, _)| (r.x + r.w * 0.5, r.y + r.h * 0.5))
    }

    fn hit(&self, x: f32, y: f32) -> Hit {
        // Later entries are more specific.
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Hit::None, |(_, h)| *h)
    }

    /// Returns true if the hovered target changed.
    pub fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
        if self.pointer.is_none() {
            self.spot_x.snap(x);
            self.spot_y.snap(y);
        }
        self.pointer = Some((x, y));
        self.spot_x.set(x);
        self.spot_y.set(y);
        let h = self.hit(x, y);
        let changed = h != self.hovered;
        self.hovered = h;
        self.update_hover();
        changed
    }

    pub fn pointer_leave(&mut self) {
        self.pointer = None;
        self.hovered = Hit::None;
        self.pressed = Hit::None;
        self.update_hover();
    }

    fn update_hover(&mut self) {
        let h = self.hovered;
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        self.spaces.lift.set(on(matches!(h, Hit::Spaces | Hit::Workspace(_))));
        self.center.lift.set(on(h == Hit::Title));
        self.status.lift.set(on(matches!(h, Hit::Volume | Hit::Clock | Hit::Status)));
        self.date_reveal.set(on(h == Hit::Clock));
        if h == Hit::Volume {
            self.vol_reveal.set(1.0);
        } else if self.time >= self.vol_hide_at {
            self.vol_reveal.set(0.0);
        }
        for s in &mut self.slots {
            s.dot.set(if h == Hit::Workspace(s.id) { DOT + 3.0 } else { DOT });
        }
    }

    pub fn press(&mut self) {
        self.pressed = self.hovered;
        match self.hovered {
            Hit::Spaces | Hit::Workspace(_) => self.spaces.press.set(1.0),
            Hit::Volume | Hit::Clock | Hit::Status => self.status.press.set(1.0),
            Hit::Title => self.center.press.set(1.0),
            Hit::Panel | Hit::None | Hit::Media | Hit::MediaCtl(_) | Hit::Tray(_) | Hit::Record => {}
        }
    }

    pub fn release(&mut self) -> Option<Cmd> {
        self.spaces.press.set(0.0);
        self.center.press.set(0.0);
        self.status.press.set(0.0);
        let p = std::mem::replace(&mut self.pressed, Hit::None);
        if p != self.hovered {
            return None;
        }
        match p {
            Hit::Workspace(id) => Some(Cmd::FocusWorkspace(id)),
            Hit::Volume => Some(Cmd::Mute),
            Hit::Title if matches!(self.design, Design::Tabs | Design::Deck) => Some(Cmd::ToggleSettings),
            Hit::Tray(i) => self.tray_cmd(i, crate::tray::Click::Primary),
            Hit::Record => Some(Cmd::StopRecording),
            Hit::Media => Some(Cmd::Media(crate::media::Ctl::PlayPause)),
            Hit::MediaCtl(0) => Some(Cmd::Media(crate::media::Ctl::Prev)),
            Hit::MediaCtl(1) => Some(Cmd::Media(crate::media::Ctl::PlayPause)),
            Hit::MediaCtl(2) => Some(Cmd::Media(crate::media::Ctl::Next)),
            _ => None,
        }
    }

    /// Right (menu) / middle (secondary) click on a tray icon.
    pub fn release_button(&mut self, button: u32) -> Option<Cmd> {
        let Hit::Tray(i) = self.hovered else { return None };
        match button {
            0x111 => self.tray_cmd(i, crate::tray::Click::Menu),
            0x112 => self.tray_cmd(i, crate::tray::Click::Secondary),
            _ => None,
        }
    }

    fn tray_cmd(&self, i: u8, c: crate::tray::Click) -> Option<Cmd> {
        let it = self.tray.get(i as usize)?;
        let x = self.hit_center(Hit::Tray(i)).map_or(self.width - 120.0, |p| p.0);
        Some(Cmd::Tray(it.id.clone(), it.title.clone(), c, x))
    }

    /// `dy` in wheel steps (positive = down). Fractional for touchpads.
    pub fn scroll(&mut self, dy: f32) -> Option<Cmd> {
        self.scroll_acc += dy;
        if self.scroll_acc.abs() < 1.0 {
            return None;
        }
        let dir = self.scroll_acc.signum();
        self.scroll_acc = 0.0;
        match self.hovered {
            Hit::Spaces | Hit::Workspace(_) | Hit::Panel => {
                Some(if dir > 0.0 { Cmd::WorkspaceDown } else { Cmd::WorkspaceUp })
            }
            Hit::Media | Hit::MediaCtl(_) if self.media.as_ref().is_some_and(|m| m.can_seek) => {
                Some(Cmd::Media(crate::media::Ctl::Seek(if dir > 0.0 { -5.0 } else { 5.0 })))
            }
            Hit::Volume => {
                self.vol_hide_at = self.time + 1.4;
                Some(Cmd::Volume(if dir > 0.0 { -5 } else { 5 }))
            }
            _ => None,
        }
    }

    /// Estimated playback position now (seconds).
    pub(crate) fn media_pos(&self) -> f32 {
        let Some(m) = &self.media else { return 0.0 };
        let p = if m.playing { m.position + (self.time - self.media_t0).max(0.0) * m.rate } else { m.position };
        if m.length > 0.0 { p.clamp(0.0, m.length) } else { p.max(0.0) }
    }

    // ── time ────────────────────────────────────────────────────────────

    /// Advance all springs. Returns true while anything still moves.
    /// Advance the render clock to wall time (call before applying events
    /// and before stepping, so change stamps and deadlines are honest even
    /// after the bar sat idle).
    pub fn sync_time(&mut self, t: f32) {
        if t > self.time {
            self.time = t;
        }
    }

    /// Ask for a frame at render-clock time `t` without animating until then.
    pub(crate) fn wake(&mut self, t: f32) {
        self.wake_at = Some(self.wake_at.map_or(t, |w| w.min(t)));
    }

    pub fn step(&mut self, dt: f32) -> bool {
        self.time += dt;
        self.wake_at = None;
        let mut m = designs::step(self, dt);
        m |= self.ws_head.step(dt) | self.ws_tail.step(dt);
        m |= self.spaces.step(dt);
        m |= self.center.step(dt);
        m |= self.status.step(dt);
        for s in &mut self.slots {
            m |= s.w.step(dt) | s.alive.step(dt) | s.dot.step(dt) | s.fill.step(dt) | s.solid.step(dt);
        }
        self.slots.retain(|s| !(s.gone && s.alive.at_rest() && s.w.at_rest()));
        m |= self.ind_l.step(dt) | self.ind_r.step(dt) | self.ind_alpha.step(dt);
        m |= self.title.xf.step(dt) | self.title.w.step(dt);
        for d in &mut self.digits {
            m |= d.step(dt);
        }
        // Waiting is not animating: sleep until the deadline instead.
        if self.vol_reveal.target > 0.0 && self.hovered != Hit::Volume {
            if self.time >= self.vol_hide_at {
                self.vol_reveal.set(0.0);
            } else {
                self.wake(self.vol_hide_at);
            }
        }
        m |= self.date_reveal.step(dt) | self.vol_reveal.step(dt);
        m |= self.vol_icon.t.step(dt) | self.net_icon.t.step(dt) | self.bat_icon.t.step(dt);
        m |= self.spot_x.step(dt) | self.spot_y.step(dt);
        m
    }

    // ── drawing ─────────────────────────────────────────────────────────

    pub fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        self.glass.clear();
        if designs::draw(self, fs, out) {
            self.text.end_frame();
            return;
        }
        if let Some((bg, line)) = self.look.strip {
            let a = self.spaces.alpha();
            let s = self.scale;
            let h = ISLAND_CY * 2.0;
            out.shapes.push(Shape {
                rect: self.px(Rect::new(0.0, 0.0, self.width, h)),
                fill_a: bg.fade(a).arr(),
                fill_b: bg.fade(a).arr(),
                grad: [0.0, 1.0, 0.0, 0.0],
                ..Default::default()
            });
            out.shapes.push(Shape {
                rect: self.px(Rect::new(0.0, h - 1.0, self.width, 1.0)),
                fill_a: line.fade(a).arr(),
                fill_b: line.fade(a).arr(),
                grad: [0.0, 1.0, 0.0, 0.0],
                geom: [0.0, 0.0, 0.0, 0.0],
                ..Default::default()
            });
            let _ = s;
        }
        self.draw_spaces(out);
        self.draw_title(fs, out);
        self.draw_status(fs, out);
        self.text.end_frame();
    }

    /// Top and height of an island's visible body.
    pub(crate) fn iy(&self) -> f32 {
        if self.look.hang { 0.0 } else { ISLAND_CY - self.look.island_h * 0.5 }
    }
    pub(crate) fn ih(&self) -> f32 {
        self.look.island_h
    }
    /// Centre line for island content.
    pub(crate) fn ccy(&self) -> f32 {
        self.iy() + self.ih() * 0.5
    }

    pub(crate) fn px(&self, r: Rect) -> [f32; 4] {
        let s = self.scale;
        [r.x * s, r.y * s, r.w * s, r.h * s]
    }

    pub(crate) fn clip(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [
            (r.x * s).floor() as i32,
            (r.y * s).floor() as i32,
            ((r.x + r.w) * s).ceil() as i32,
            ((r.y + r.h) * s).ceil() as i32,
        ]
    }

    /// The body of an island, styled by the current look.
    fn glass(&mut self, out: &mut DrawList, r: Rect, isl: &IslandLook) {
        let s = self.scale;
        let lk = self.look;
        let a = isl.alpha;
        if a <= 0.002 {
            return;
        }
        let l = isl.lift;
        let mut r = r.inflate(l * 1.0 - isl.press * 1.5);
        if lk.hang {
            r.h += r.y + 24.0;
            r.y = -24.0;
        }
        if a > 0.05 {
            self.glass.push(r);
        }
        // Chip looks only show a body while hovered.
        let ba = if lk.chips { a * l.clamp(0.0, 1.0) } else { a };
        if ba <= 0.002 {
            return;
        }
        let (sx, sy) = (self.spot_x.get(), self.spot_y.get());
        out.shapes.push(Shape {
            rect: self.px(r),
            geom: [lk.radius.min(r.h * 0.5) * s, lk.rim_w * s, (lk.shadow_sigma + 5.0 * l) * s, (lk.shadow_dy + 2.0 * l) * s],
            fill_a: lk.body_top.mix(lk.hover_top, l).fade(ba).arr(),
            fill_b: lk.body_bot.mix(lk.hover_bot, l).fade(ba).arr(),
            rim_a: lk.rim_top.fade(ba * (1.0 + 0.35 * l)).arr(),
            rim_b: lk.rim_bot.fade(ba).arr(),
            shadow: lk.shadow.fade(ba * (0.75 + 0.25 * l)).arr(),
            grad: [0.0, 1.0, lk.inner, 0.0],
            spot: [sx * s, sy * s, 70.0 * s, l * ba * lk.specular],
            bloom: [0.0; 4],
            ..Default::default()
        });
    }

    pub(crate) fn text_at(&mut self, out: &mut DrawList, key: u64, x: f32, y: f32, color: Rgba, clip: Rect) {
        let s = self.scale;
        if let Some(sh) = self.look.text_shadow {
            let c = sh.fade(color.3);
            out.texts.push(TextDraw { key, x: (x * s).round(), y: ((y + 0.75) * s).round(), color: c, clip: self.clip(clip.inflate(1.0)) });
        }
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color, clip: self.clip(clip) });
    }

    pub(crate) fn icon_at(&self, out: &mut DrawList, id: u16, x: f32, y: f32, size: f32, color: Rgba) {
        let s = self.scale;
        if color.3 <= 0.002 {
            return;
        }
        out.icons.push(IconDraw { id, x: x * s, y: y * s, size: (size * s).round(), color });
    }

    pub(crate) fn icon_swap(&self, out: &mut DrawList, sw: &IconSwap, x: f32, y: f32, color: Rgba) {
        let t = sw.t.get().clamp(0.0, 1.0);
        if t < 1.0 {
            self.icon_at(out, sw.prev, x, y + t * 3.0, ICON, color.fade(1.0 - t));
        }
        self.icon_at(out, sw.cur, x, y - (1.0 - t) * 3.0, ICON, color.fade(t));
    }

    fn draw_spaces(&mut self, out: &mut DrawList) {
        let isl = IslandLook::of(&self.spaces);
        let content: f32 = self.slots.iter().map(|s| s.w.get().max(0.0)).sum();
        let w = SPACES_PAD * 2.0 + content.max(SLOT_W);
        let y = self.iy() + isl.dy;
        let body = Rect::new(MARGIN, y, w, self.ih());
        self.hits.push((body, Hit::Spaces));
        self.glass(out, body, &isl);
        let a = isl.alpha;
        let s = self.scale;
        let cy = self.ccy() + isl.dy;

        // Indicator span, absolute. Dots it passes over are absorbed into it.
        let ia = self.ind_alpha.get().clamp(0.0, 1.0);
        let (il, ir) = {
            let (l, r) = (self.ind_l.get(), self.ind_r.get());
            (MARGIN + l.min(r), MARGIN + l.max(r))
        };

        let mut x = MARGIN + SPACES_PAD;
        for slot in &self.slots {
            let sw = slot.w.get().max(0.0);
            let cx = x + sw * 0.5;
            let alive = slot.alive.get().clamp(0.0, 1.0);
            // Signed distance from the dot centre to the indicator span.
            let dist = (il - cx).max(cx - ir);
            let free = 1.0 - ia * (1.0 - ((dist + 1.0) / 7.0).clamp(0.0, 1.0));
            let d = slot.dot.get() * (0.4 + 0.6 * alive) * (0.4 + 0.6 * free);
            let c = slot.fill.get();
            let alpha = a * alive * free;
            let solid = slot.solid.get().clamp(0.0, 1.0);
            if alpha > 0.002 {
                let r = Rect::new(cx - d * 0.5, cy - d * 0.5, d, d);
                out.shapes.push(Shape {
                    rect: self.px(r),
                    geom: [d * 0.5 * s * self.look.dot_round, 1.2 * s, 0.0, 0.0],
                    fill_a: c.fade(alpha * solid).arr(),
                    fill_b: c.fade(alpha * solid).arr(),
                    rim_a: c.fade(alpha * (1.0 - solid * 0.6)).arr(),
                    rim_b: c.fade(alpha * (1.0 - solid * 0.6)).arr(),
                    grad: [0.0, 1.0, 0.0, 0.0],
                    ..Default::default()
                });
            }
            if !slot.gone {
                self.hits.push((Rect::new(x, self.iy(), sw, self.ih()), Hit::Workspace(slot.id)));
            }
            x += sw;
        }

        // Liquid indicator: two springs for the edges; squash vertically as
        // it stretches so it reads as a single elastic body.
        let ia = ia * a;
        if ia > 0.002 {
            let (l, r) = (self.ind_l.get(), self.ind_r.get());
            let (l, r) = (l.min(r), l.max(r));
            let rest = ACTIVE_W - 2.0 * IND_INSET;
            let stretch = ((r - l) - rest) / rest;
            let h = IND_H * (1.0 - 0.32 * stretch.clamp(0.0, 1.2)) * (1.0 + 0.18 * (-stretch).clamp(0.0, 1.0));
            let rect = Rect::new(MARGIN + l, cy - h * 0.5, r - l, h);
            out.shapes.push(Shape {
                rect: self.px(rect),
                geom: [h * 0.5 * s * self.look.ind_round, 1.0 * s, 0.0, 0.0],
                fill_a: self.look.accent_a.fade(ia).arr(),
                fill_b: self.look.accent_b.fade(ia).arr(),
                rim_a: Rgba::hex(0xffffff, 0.55 * ia).arr(),
                rim_b: Rgba::hex(0xffffff, 0.08 * ia).arr(),
                grad: [1.0, 0.0, 0.35, 5.0 * s],
                bloom: self.look.accent_a.mix(self.look.accent_b, 0.5).fade(self.look.bloom * ia).arr(),
                ..Default::default()
            });
        }
    }

    fn draw_title(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        let cur = self.title.cur.clone().map(|t| self.text.get_fit(fs, &t, F_TITLE, TITLE_MAX));
        let prev = self.title.prev.clone().map(|t| self.text.get_fit(fs, &t, F_TITLE, TITLE_MAX));
        let pad = 16.0;
        if let Some((_, w)) = cur {
            let target = w + pad * 2.0;
            if self.title.w.target != target {
                if self.title.w.get() < 1.0 {
                    self.title.w.snap(target * 0.7);
                }
                self.title.w.set(target);
            }
        }
        let isl = IslandLook::of(&self.center);
        if isl.alpha <= 0.002 {
            return;
        }
        let w = self.title.w.get().max(pad * 2.0) * (0.9 + 0.1 * self.center.show.get().clamp(0.0, 1.0));
        let y = self.iy() + isl.dy;
        let body = Rect::new((self.width - w) * 0.5, y, w, self.ih());
        self.hits.push((body, Hit::Title));
        self.glass(out, body, &isl);
        let a = isl.alpha;
        let lh = self.text.line_h(F_TITLE);
        let ty = self.ccy() + isl.dy - lh * 0.5;
        let clip = body.inflate(-2.0);
        let xf = self.title.xf.get().clamp(0.0, 1.0);
        let color = self.look.text_muted.mix(self.look.text, 0.35 + 0.65 * self.center.lift.get().clamp(0.0, 1.0));
        if let Some((k, tw)) = prev
            && xf < 1.0
        {
            self.text_at(out, k, self.width * 0.5 - tw * 0.5, ty - xf * 7.0, color.fade(a * (1.0 - xf)), clip);
        }
        if let Some((k, tw)) = cur {
            self.text_at(out, k, self.width * 0.5 - tw * 0.5, ty + (1.0 - xf) * 7.0, color.fade(a * xf), clip);
        }
    }

    fn draw_status(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        let isl = IslandLook::of(&self.status);
        let a = isl.alpha;
        let lift = self.status.lift.get().clamp(0.0, 1.0);
        let icon_c = self.look.text_muted.mix(self.look.text, lift).fade(a);

        // Measure.
        let vol_txt = self.volume.map(|v| if v.muted { "Muted".to_string() } else { format!("{}%", v.percent) });
        let vol = vol_txt.as_deref().map(|t| self.text.get(fs, t, F_SMALL));
        let bat = self.battery.map(|b| self.text.get(fs, &format!("{}%", b.percent), F_SMALL));
        let date = self.clock.map(|c| {
            const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let s = format!("{} {} {}", WD[c.weekday as usize % 7], c.day, MO[c.month as usize % 12]);
            self.text.get(fs, &s, F_DATE)
        });
        let digit_w = ('0'..='9').map(|c| self.text.get(fs, &c.to_string(), F_CLOCK).1).fold(0.0, f32::max);
        let colon = self.text.get(fs, ":", F_CLOCK);

        let vr = self.vol_reveal.get().clamp(0.0, 1.2);
        let dr = self.date_reveal.get().clamp(0.0, 1.2);
        let (pl, pr, gap) = (13.0, 15.0, 12.0);
        let vol_w = ICON + vol.map_or(0.0, |(_, w)| (5.0 + w) * vr);
        let bat_w = ICON + bat.map_or(0.0, |(_, w)| 4.0 + w);
        let date_w = date.map_or(0.0, |(_, w)| (w + 9.0) * dr);
        let clock_w = digit_w * 4.0 + colon.1 + 2.0;
        let total = pl + vol_w + gap + ICON + gap + bat_w + gap + 1.0 + gap + date_w + clock_w + pr;

        let y = self.iy() + isl.dy;
        let body = Rect::new(self.width - MARGIN - total, y, total, self.ih());
        self.hits.push((body, Hit::Status));
        self.glass(out, body, &isl);
        if a <= 0.002 {
            return;
        }
        let cy = self.ccy() + isl.dy;
        let iy = cy - ICON * 0.5;
        let s_lh = self.text.line_h(F_SMALL);
        let mut x = body.x + pl;

        // Volume (+ revealed level).
        let vol_x = x;
        let vs = std::mem::replace(&mut self.vol_icon, IconSwap::new(0));
        self.icon_swap(out, &vs, x, iy, icon_c);
        self.vol_icon = vs;
        if let Some((k, w)) = vol {
            let clip = Rect::new(x + ICON, y, (5.0 + w) * vr, self.ih());
            let c = self.look.text.fade(a * vr.min(1.0));
            self.text_at(out, k, x + ICON + 5.0 - (1.0 - vr.min(1.0)) * 6.0, cy - s_lh * 0.5, c, clip);
        }
        self.hits.push((Rect::new(vol_x - 6.0, y, vol_w + 12.0, self.ih()), Hit::Volume));
        x += vol_w + gap;

        // Network.
        let ns = std::mem::replace(&mut self.net_icon, IconSwap::new(0));
        self.icon_swap(out, &ns, x, iy, icon_c);
        self.net_icon = ns;
        x += ICON + gap;

        // Battery.
        let bc = match self.battery {
            Some(b) if b.charging => theme::GOOD,
            Some(b) if b.percent <= 15 => theme::BAD,
            Some(b) if b.percent <= 30 => theme::WARN,
            _ => self.look.text_muted.mix(self.look.text, lift),
        };
        let bs = std::mem::replace(&mut self.bat_icon, IconSwap::new(0));
        self.icon_swap(out, &bs, x, iy, bc.fade(a));
        self.bat_icon = bs;
        if let Some((k, _)) = bat {
            self.text_at(out, k, x + ICON + 4.0, cy - s_lh * 0.5, bc.fade(a), body);
        }
        x += bat_w + gap;

        // Hairline divider with a soft vertical fade.
        let s = self.scale;
        out.shapes.push(Shape {
            rect: self.px(Rect::new(x, cy - 7.0, 1.0, 14.0)),
            geom: [0.5 * s, 0.0, 0.0, 0.0],
            fill_a: self.look.divider.fade(0.2 * a).arr(),
            fill_b: self.look.divider.fade(a).arr(),
            grad: [0.0, 1.0, 0.0, 0.0],
            ..Default::default()
        });
        x += 1.0 + gap;

        // Date slides out from behind the clock on hover.
        let clock_hit_x = x;
        if let Some((k, w)) = date {
            let clip = Rect::new(x, y, date_w, self.ih());
            let c = self.look.text_muted.fade(a * dr.min(1.0));
            self.text_at(out, k, x + date_w - w - 9.0 + (1.0 - dr.min(1.0)) * 10.0, cy - self.text.line_h(F_DATE) * 0.5, c, clip);
        }
        x += date_w;

        // Odometer clock.
        let lh = self.text.line_h(F_CLOCK);
        let ty = cy - lh * 0.5;
        // Digits roll inside a window just taller than their cap height.
        let clip = Rect::new(x - 2.0, cy - 8.5, clock_w + 4.0, 17.0);
        let roll = 13.0;
        for i in 0..4 {
            if i == 2 {
                let c = self.look.text_muted.mix(self.look.text, 0.5).fade(a);
                self.text_at(out, colon.0, x + 1.0, ty - 1.0, c, clip);
                x += colon.1 + 2.0;
            }
            let d = &self.digits[i];
            let p = d.p.get();
            let (cur, prev) = (d.cur, d.prev);
            let ck = self.text.get(fs, &cur.to_string(), F_CLOCK);
            let col = self.look.text.fade(a);
            if p < 1.0 && prev != ' ' {
                let pk = self.text.get(fs, &prev.to_string(), F_CLOCK);
                let fade = (1.0 - p * 1.7).clamp(0.0, 1.0);
                self.text_at(out, pk.0, x + (digit_w - pk.1) * 0.5, ty - p * roll, col.fade(fade), clip);
            }
            let fade = ((p - 0.2) / 0.8).clamp(0.0, 1.0);
            self.text_at(out, ck.0, x + (digit_w - ck.1) * 0.5, ty + (1.0 - p) * roll, col.fade(fade), clip);
            x += digit_w;
        }
        self.hits.push((Rect::new(clock_hit_x - 6.0, y, x - clock_hit_x + pr, self.ih()), Hit::Clock));
    }
}

struct IslandLook {
    alpha: f32,
    dy: f32,
    lift: f32,
    press: f32,
}

impl IslandLook {
    fn of(i: &Island) -> Self {
        Self { alpha: i.alpha(), dy: i.dy(), lift: i.lift.get().clamp(0.0, 1.2), press: i.press.get() }
    }
}
