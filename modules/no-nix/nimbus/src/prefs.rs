//! The full settings window: a sidebar of categories on the left, the
//! selected page on the right. It grows out of the quick-settings panel
//! (the gear button) and lives on its own layer surface while open.
//!
//! Motion follows the rest of nimbus: the window morphs from the panel's
//! exact shape, the sidebar selection is a liquid indicator (snappy head,
//! trailing tail), pages cross-fade in place, and every control has springs
//! for hover and state. It is draggable like the quick panel.
//!
//! Prism Ink, box-less: flat matte ink edged by the prism hairline; the
//! sidebar is set off by a hairline, groups by dividers rather than cards.
//! Switched-on things (switches, slider fills, the selected scale, the
//! nav marker) are iridescent under the Prism accent.

mod profile_page;

use std::{collections::HashMap, rc::Rc};

use glyphon::FontSystem;

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    designs::Design,
    gfx::{DrawList, IconDraw, TextDraw},
    icons::IconSet,
    paint,
    services::{AboutInfo, AudioState, BtDev, Fact, OutputSummary, PowerDetail, SystemState, VolumeInfo, WifiNet},
    settings::Action,
    theme::{self, Rgba},
    window::{KeyInput, keys},
};

/// Window size (logical) and the shadow margin around it on the surface.
pub const FW: f32 = 880.0;
pub const FH: f32 = 560.0;
pub const M: f32 = 40.0;
pub const SW: f32 = FW + 2.0 * M;
pub const SH: f32 = FH + 2.0 * M;
const SIDEBAR: f32 = 212.0;
const NAV_H: f32 = 38.0;

const F_TITLE: Font = Font::new(17.0, 650).family("Geist");
const F_PAGE: Font = Font::new(24.0, 650).family("Geist");
const F_SUB: Font = Font::new(13.0, 450).family("Geist");
const F_NAV: Font = Font::new(13.5, 550).family("Geist");
const F_LABEL: Font = Font::new(13.5, 600).family("Geist");
const F_BODY: Font = Font::new(13.0, 500).family("Geist");
const F_SMALL: Font = Font::new(12.0, 500).family("Geist");
const F_HUGE: Font = Font::new(44.0, 650).family("Geist");
const F_SECTION: Font = Font::new(11.0, 650).family("Geist").track(0.08);

/// Window rounding when open; inner radii are concentric with it.
const R_OPEN: f32 = 28.0;
/// The quick panel's rounding, where the window grows from.
const R_FROM: f32 = 30.0;
/// Faint white: selectable tiles at rest, hover circles and rows.
const TILE: Rgba = Rgba::hex(0xffffff, 0.035);
const HOVER: Rgba = Rgba::hex(0xffffff, 0.08);
const HAIR: Rgba = Rgba::hex(0xffffff, 0.07);
/// Restart / power off armed: a soft red tint and its glyph colour.
const DANGER: Rgba = Rgba::hex(0xff5a6e, 0.32);
const DANGER_FG: Rgba = Rgba::hex(0xffb8c1, 1.0);
const SHADOW: Rgba = Rgba::hex(0x000000, 0.6);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
const TRACK: Rgba = Rgba::hex(0xffffff, 0.14);
const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);
const GOOD: Rgba = Rgba::hex(0x7ee2b8, 1.0);

const OPEN_W: SpringCfg = SpringCfg::new(0.48, 0.84);
const OPEN_H: SpringCfg = SpringCfg::new(0.6, 0.8);
const GLIDE: SpringCfg = SpringCfg::new(0.45, 0.9);
const PAGE_X: SpringCfg = SpringCfg::new(0.38, 1.0);
const ARM_TIMEOUT: f32 = 3.0;
const SCALES: [f64; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Profile,
    Appearance,
    Sound,
    Display,
    Network,
    Power,
    About,
}

const PAGES: [(Page, &str, &str, &str); 7] = [
    (Page::Profile, "Profile", "avatar-default-symbolic", "You and this machine"),
    (Page::Appearance, "Appearance", "preferences-desktop-appearance-symbolic", "Bar design and accent colour"),
    (Page::Sound, "Sound", "audio-speakers-symbolic", "Output, input and devices"),
    (Page::Display, "Display", "video-display-symbolic", "Brightness and scaling"),
    (Page::Network, "Network", "network-wireless-symbolic", "Wi-Fi and Bluetooth"),
    (Page::Power, "Power", "battery-full-symbolic", "Battery and session"),
    (Page::About, "About", "help-about-symbolic", "nimbus and this machine"),
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ctl {
    None,
    Body,
    Grip,
    Close,
    Nav(u8),
    Design(u8),
    Accent(u8),
    /// 0 output volume, 1 microphone, 2 brightness
    Slider(u8),
    /// 0 mute, 1 mic mute, 2 wifi, 3 bluetooth
    Switch(u8),
    /// (section, index): 0 sinks, 1 wifi, 2 bluetooth
    Row(u8, u8),
    /// (output, scale option)
    Seg(u8, u8),
    /// 0 lock, 1 sleep, 2 restart, 3 power off
    Btn(u8),
    // Profile page
    Avatar,
    NameField,
    Friendly,
    ChangePic,
    RemovePic,
    Back,
    PathField,
    Thumb(u16),
    Stage,
    ZoomBar,
    CropSave,
    CropCancel,
}

pub struct Prefs {
    pub scale: f32,
    text: TextCache,
    icons: Rc<IconSet>,
    gpu_name: String,
    pub time: f32,

    start: Rect,
    pw: Spring,
    ph: Spring,
    fade: Spring,
    closing: bool,

    page: Page,
    prev_page: Page,
    page_t: Spring,
    nav_head: Spring,
    nav_tail: Spring,

    // data
    volume: Option<VolumeInfo>,
    system: SystemState,
    audio: AudioState,
    outputs: Vec<OutputSummary>,
    power: Option<PowerDetail>,
    about: Option<AboutInfo>,
    wifi_nets: Option<Vec<WifiNet>>,
    bt_devs: Option<Vec<BtDev>>,
    pending: Option<String>,
    design: Design,
    accent: usize,
    prof: profile_page::State,

    // controls
    sliders: [Spring; 3],
    switches: [Spring; 4],
    hover: HashMap<Ctl, Spring>,
    hovered: Ctl,
    pressed: Ctl,
    dragging: Ctl,
    armed: Option<(u8, f32)>,
    hits: Vec<(Rect, Ctl)>,
    slider_rects: [Rect; 3],

    // window drag (see settings.rs: relative motion, whole-pixel surface)
    off_x: Spring,
    off_y: Spring,
    pub surf_origin: (i32, i32),
    bounds: (f32, f32, f32, f32),
    grabbing: bool,
    fling: (f32, f32),
    last_move: Option<(f32, f32, f32)>,

    pub body: Rect,
    pub wake_at: Option<f32>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn spring0() -> Spring {
    Spring::new(0.0, presets::SNAPPY)
}

impl Prefs {
    pub fn new(fs: &mut FontSystem, icons: Rc<IconSet>, scale: f32, start: Rect, gpu_name: String, facts: &[Fact]) -> Self {
        let mut text = TextCache::new(fs, theme::current());
        text.set_scale(scale);
        let mut pw = Spring::new(0.0, OPEN_W);
        let mut ph = Spring::new(0.0, OPEN_H);
        pw.set(1.0);
        ph.set(1.0);
        let mut s = Self {
            scale,
            text,
            icons,
            gpu_name,
            time: 0.0,
            start,
            pw,
            ph,
            fade: Spring::new(1.0, presets::SMOOTH),
            closing: false,
            page: Page::Profile,
            prev_page: Page::Profile,
            page_t: Spring::new(1.0, PAGE_X),
            nav_head: Spring::new(0.0, presets::BOUNCY),
            nav_tail: Spring::new(0.0, presets::TAIL),
            volume: None,
            system: SystemState::default(),
            audio: AudioState::default(),
            outputs: vec![],
            power: None,
            about: None,
            wifi_nets: None,
            bt_devs: None,
            pending: None,
            design: Design::current(),
            accent: theme::accent_index(),
            prof: profile_page::State::new(),
            sliders: [spring0(), spring0(), spring0()],
            switches: [spring0(), spring0(), spring0(), spring0()],
            hover: HashMap::new(),
            hovered: Ctl::None,
            pressed: Ctl::None,
            dragging: Ctl::None,
            armed: None,
            hits: vec![],
            slider_rects: [Rect::default(); 3],
            off_x: Spring::new(0.0, GLIDE),
            off_y: Spring::new(0.0, GLIDE),
            surf_origin: (0, 0),
            bounds: (0.0, 0.0, 0.0, 0.0),
            grabbing: false,
            fling: (0.0, 0.0),
            last_move: None,
            body: start,
            wake_at: None,
        };
        for f in facts {
            s.apply(f);
        }
        for sl in &mut s.sliders {
            sl.snap(sl.target);
        }
        for sw in &mut s.switches {
            sw.snap(sw.target);
        }
        s
    }

    /// What to read when the window opens (and on page switches).
    pub fn initial_reads() -> Vec<Action> {
        vec![Action::ReadAudio, Action::ReadOutputs, Action::ReadPower, Action::ReadAbout, Action::ScanWifi, Action::ScanBluetooth]
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.text.set_scale(scale);
    }

    pub fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    pub fn apply(&mut self, f: &Fact) {
        match f {
            Fact::Volume(v) => {
                self.volume = Some(*v);
                if self.dragging != Ctl::Slider(0) {
                    self.sliders[0].set(v.percent.min(100) as f32 / 100.0);
                }
            }
            Fact::System(s) => {
                self.system = *s;
                if let Some(b) = s.brightness
                    && self.dragging != Ctl::Slider(2)
                {
                    self.sliders[2].set(b);
                }
            }
            Fact::Audio(a) => {
                self.audio = a.clone();
                if let Some(m) = a.mic
                    && self.dragging != Ctl::Slider(1)
                {
                    self.sliders[1].set(m.percent.min(100) as f32 / 100.0);
                }
            }
            Fact::Outputs(o) => self.outputs = o.clone(),
            Fact::Power(p) => self.power = Some(p.clone()),
            Fact::About(a) => self.about = Some(a.clone()),
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
            _ => {}
        }
    }

    // ── lifecycle ───────────────────────────────────────────────────────

    pub fn close(&mut self) {
        if !self.closing {
            self.commit_name();
            self.closing = true;
            self.grabbing = false;
            self.dragging = Ctl::None;
            self.fade.set(0.0);
        }
    }

    pub fn reopen(&mut self) {
        self.closing = false;
        self.fade.set(1.0);
    }

    pub fn escape(&mut self) {
        self.close();
    }

    /// A key while the window has focus. The Profile page's fields and
    /// views go first; Escape otherwise closes.
    pub fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if !self.closing && !self.profile_key(k) && k.sym == keys::ESCAPE {
            self.escape();
        }
        (None, false)
    }

    pub fn closing(&self) -> bool {
        self.closing
    }

    pub fn done(&self) -> bool {
        self.closing && self.fade.get() < 0.02
    }

    pub fn set_bounds(&mut self, min_x: f32, max_x: f32, min_y: f32, max_y: f32) {
        self.bounds = (min_x, max_x, min_y, max_y);
    }

    pub fn margin(&self) -> (i32, i32) {
        (self.off_x.get().floor() as i32, self.off_y.get().floor() as i32)
    }

    fn clamp_off(&self, x: f32, y: f32) -> (f32, f32) {
        let (lx, hx, ly, hy) = self.bounds;
        (x.clamp(lx, hx.max(lx)), y.clamp(ly, hy.max(ly)))
    }

    pub fn grabbing(&self) -> bool {
        self.grabbing
    }

    pub fn over_grip(&self) -> bool {
        matches!(self.hovered, Ctl::Grip | Ctl::Body)
    }

    pub fn clickable(&self) -> bool {
        !matches!(self.hovered, Ctl::None | Ctl::Body | Ctl::Grip)
    }

    pub fn drag_by(&mut self, dx: f32, dy: f32) {
        if !self.grabbing {
            return;
        }
        let (nx, ny) = self.clamp_off(self.off_x.get() + dx, self.off_y.get() + dy);
        if let Some((t, px, py)) = self.last_move {
            let dt = (self.time - t).max(1.0 / 240.0);
            self.fling.0 = self.fling.0 * 0.6 + (nx - px) / dt * 0.4;
            self.fling.1 = self.fling.1 * 0.6 + (ny - py) / dt * 0.4;
        }
        self.last_move = Some((self.time, nx, ny));
        self.off_x.snap(nx);
        self.off_y.snap(ny);
    }

    fn page_index(p: Page) -> usize {
        PAGES.iter().position(|x| x.0 == p).unwrap_or(0)
    }

    fn go(&mut self, p: Page) {
        if p == self.page {
            return;
        }
        if self.page == Page::Profile {
            self.profile_left();
        }
        self.prev_page = self.page;
        self.page = p;
        self.page_t.snap(0.0);
        self.page_t.set(1.0);
        let i = Self::page_index(p) as f32;
        self.nav_head.set(i);
        self.nav_tail.set(i);
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
        if self.hovered != Ctl::None {
            self.hover.entry(self.hovered).or_insert(spring0());
        }
        for (c, h) in self.hover.iter_mut() {
            h.set(if self.hovered == *c && !self.closing { 1.0 } else { 0.0 });
        }
        let muted = self.volume.is_some_and(|v| v.muted);
        let mic_muted = self.audio.mic.is_some_and(|m| m.muted);
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        self.switches[0].set(on(muted));
        self.switches[1].set(on(mic_muted));
        self.switches[2].set(on(self.system.wifi == Some(true)));
        self.switches[3].set(on(self.system.bluetooth == Some(true)));

        let mut m = false;
        for s in [
            &mut self.pw,
            &mut self.ph,
            &mut self.fade,
            &mut self.page_t,
            &mut self.nav_head,
            &mut self.nav_tail,
            &mut self.off_x,
            &mut self.off_y,
        ] {
            m |= s.step(dt);
        }
        for s in self.sliders.iter_mut().chain(self.switches.iter_mut()).chain(self.hover.values_mut()) {
            m |= s.step(dt);
        }
        m |= self.profile_step(dt);
        self.hover.retain(|c, s| !s.at_rest() || s.target > 0.0 || *c == self.hovered);
        m
    }

    // ── input ───────────────────────────────────────────────────────────

    fn hit(&self, x: f32, y: f32) -> Ctl {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Ctl::None, |(_, c)| *c)
    }

    fn slider_value(&self, i: u8, x: f32) -> f32 {
        let r = self.slider_rects[i as usize];
        ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0)
    }

    fn drag_to(&mut self, x: f32) -> Option<Action> {
        let Ctl::Slider(i) = self.dragging else { return None };
        let v = self.slider_value(i, x);
        self.sliders[i as usize].snap(v);
        let pct = (v * 100.0).round() as u8;
        Some(match i {
            0 => Action::Volume(pct),
            1 => Action::MicVolume(pct),
            _ => Action::Brightness(pct.max(2)),
        })
    }

    pub fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        if self.grabbing {
            return None;
        }
        if self.profile_motion(x, y) {
            return None;
        }
        if self.dragging != Ctl::None {
            return self.drag_to(x);
        }
        self.hovered = self.hit(x, y);
        None
    }

    pub fn pointer_leave(&mut self) {
        if self.dragging == Ctl::None && !self.grabbing {
            self.hovered = Ctl::None;
        }
    }

    pub fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pressed = self.hovered;
        if self.closing {
            return None;
        }
        if self.profile_press(x, y) {
            return None;
        }
        if self.over_grip() {
            self.grabbing = true;
            self.fling = (0.0, 0.0);
            self.last_move = None;
            return None;
        }
        if let Ctl::Slider(_) = self.hovered {
            self.dragging = self.hovered;
            return self.drag_to(x);
        }
        None
    }

    pub fn release(&mut self) -> (Option<Action>, bool) {
        if self.grabbing {
            self.grabbing = false;
            self.pressed = Ctl::None;
            let (tx, ty) = self.clamp_off(self.off_x.get() + self.fling.0 * 0.12, self.off_y.get() + self.fling.1 * 0.12);
            self.off_x.vel = self.fling.0;
            self.off_y.vel = self.fling.1;
            self.off_x.set(tx);
            self.off_y.set(ty);
            return (None, false);
        }
        if self.dragging != Ctl::None {
            self.dragging = Ctl::None;
            return (None, false);
        }
        let c = std::mem::replace(&mut self.pressed, Ctl::None);
        if c != self.hovered || self.closing {
            return (None, false);
        }
        match c {
            Ctl::Close => (None, true),
            Ctl::Nav(i) => {
                let p = PAGES[i as usize].0;
                self.go(p);
                let read = match p {
                    Page::Sound => Some(Action::ReadAudio),
                    Page::Display => Some(Action::ReadOutputs),
                    Page::Power => Some(Action::ReadPower),
                    Page::About => Some(Action::ReadAbout),
                    Page::Network => Some(Action::ScanWifi),
                    Page::Profile | Page::Appearance => None,
                };
                (read, false)
            }
            Ctl::Design(i) => {
                let d = Design::ALL[i as usize];
                self.design = d;
                (Some(Action::SetDesign(d)), false)
            }
            Ctl::Accent(i) => {
                self.accent = i as usize;
                (Some(Action::SetAccent(i as usize)), false)
            }
            Ctl::Switch(i) => match i {
                0 => {
                    if let Some(v) = &mut self.volume {
                        v.muted = !v.muted;
                    }
                    (Some(Action::Mute), false)
                }
                1 => {
                    if let Some(m) = &mut self.audio.mic {
                        m.muted = !m.muted;
                    }
                    (Some(Action::MicMute), false)
                }
                2 => {
                    let on = !self.system.wifi.unwrap_or(false);
                    self.system.wifi = Some(on);
                    (Some(Action::Wifi(on)), false)
                }
                _ => {
                    let on = !self.system.bluetooth.unwrap_or(false);
                    self.system.bluetooth = Some(on);
                    (Some(Action::Bluetooth(on)), false)
                }
            },
            Ctl::Row(sec, i) => {
                let i = i as usize;
                match sec {
                    0 => {
                        let Some(d) = self.audio.sinks.get(i) else { return (None, false) };
                        let id = d.id;
                        for s in &mut self.audio.sinks {
                            s.default = s.id == id;
                        }
                        (Some(Action::SetSink(id)), false)
                    }
                    1 => {
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
            Ctl::Seg(o, k) => {
                let Some(out) = self.outputs.get(o as usize) else { return (None, false) };
                let scale = SCALES[k as usize];
                let name = out.name.clone();
                self.outputs[o as usize].scale = scale;
                (Some(Action::SetScale(name, scale)), false)
            }
            Ctl::Btn(i) => match i {
                0 => (Some(Action::Lock), true),
                1 => (Some(Action::Suspend), true),
                _ => {
                    if self.armed.is_some_and(|(a, _)| a == i) {
                        self.armed = None;
                        (Some(if i == 2 { Action::Reboot } else { Action::PowerOff }), true)
                    } else {
                        self.armed = Some((i, self.time));
                        (None, false)
                    }
                }
            },
            Ctl::Avatar | Ctl::NameField | Ctl::Friendly | Ctl::ChangePic | Ctl::RemovePic | Ctl::Back | Ctl::Thumb(_) | Ctl::CropSave | Ctl::CropCancel => {
                self.profile_click(c);
                (None, false)
            }
            _ => (None, false),
        }
    }

    pub fn scroll(&mut self, dy: f32) -> Option<Action> {
        if self.profile_scroll(dy) {
            return None;
        }
        let Ctl::Slider(i) = self.hovered else { return None };
        let s = &mut self.sliders[i as usize];
        let v = (s.target - dy.signum() * 0.05).clamp(0.0, 1.0);
        s.set(v);
        let pct = (v * 100.0).round() as u8;
        Some(match i {
            0 => Action::Volume(pct),
            1 => Action::MicVolume(pct),
            _ => Action::Brightness(pct.max(2)),
        })
    }

    // ── drawing helpers ─────────────────────────────────────────────────

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
        if c.3 <= 0.002 || clip.w <= 0.5 {
            return;
        }
        let s = self.scale;
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: self.px(clip) });
    }

    /// Text with its vertical centre at `cy`.
    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text(out, k, x, cy - lh * 0.5, c, clip);
        w
    }

    fn icon(&self, out: &mut DrawList, name: &str, x: f32, y: f32, size: f32, c: Rgba) {
        if c.3 <= 0.002 {
            return;
        }
        let s = self.scale;
        let id = self.icons.id(&[name, "help-about-symbolic"]);
        out.icons.push(IconDraw { id, x: x * s, y: y * s, size: (size * s).round(), color: c });
    }

    /// A selectable tile: flat faint white, a touch brighter on hover. No
    /// rim, no gradient.
    fn tile(&self, out: &mut DrawList, r: Rect, radius: f32, a: f32, hover: f32) {
        out.shapes.push(paint::rect(self.scale, r, radius).fill(TILE.mix(HOVER, hover)).alpha(a).done());
    }

    /// A quiet hover surface: nothing at rest.
    fn hover_bg(&self, out: &mut DrawList, r: Rect, radius: f32, a: f32, h: f32) {
        if h * a > 0.003 {
            out.shapes.push(paint::rect(self.scale, r, radius).fill(HOVER).alpha(a * h).done());
        }
    }

    /// A horizontal hairline divider that fades in from both ends.
    fn divider(&self, out: &mut DrawList, x: f32, y: f32, w: f32, a: f32) {
        paint::divider(&mut out.shapes, self.scale, x, y, w, a);
    }

    #[allow(clippy::too_many_arguments)]
    fn switch(&mut self, out: &mut DrawList, id: u8, x: f32, cy: f32, a: f32) {
        let r = Rect::new(x, cy - 12.0, 42.0, 24.0);
        self.hits.push((r, Ctl::Switch(id)));
        let t = self.switches[id as usize].get();
        let tc = t.clamp(0.0, 1.0);
        out.shapes.push(paint::rect(self.scale, r, 12.0).on_mix(TRACK, tc, 1.0, 0.0, 0.0).alpha(a).done());
        let kx = lerp(r.x + 12.0, r.x + r.w - 12.0, t);
        out.shapes.push(
            paint::circle(self.scale, kx, cy, 9.0 + self.hover_of(Ctl::Switch(id)))
                .fill(Rgba::hex(0xffffff, 1.0))
                .shadow(Rgba::hex(0x000000, 0.4), 3.0, 1.0)
                .alpha(a)
                .done(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(&mut self, fs: &mut FontSystem, out: &mut DrawList, id: u8, icon: &str, x: f32, w: f32, cy: f32, active: bool, a: f32, clip: Rect) {
        let row = Rect::new(x, cy - 14.0, w, 28.0);
        self.hits.push((row, Ctl::Slider(id)));
        let h = self.hover_of(Ctl::Slider(id)).max(if self.dragging == Ctl::Slider(id) { 1.0 } else { 0.0 });
        // Box-less: icon, a thin track with the "on" fill and a clear
        // glass knob, the value muted on the right.
        self.icon(out, icon, x, cy - 8.0, 16.0, SOFT.mix(TEXT, h).fade(a * (0.8 + 0.2 * h)));
        let tl = x + 30.0;
        let tw = w - 30.0 - 52.0;
        self.slider_rects[id as usize] = Rect::new(tl, cy, tw, 1.0);
        let sc = self.scale;
        out.shapes.push(paint::rect(sc, Rect::new(tl, cy - 2.0, tw, 4.0), 2.0).fill(TRACK).alpha(a).done());
        let lv = self.sliders[id as usize].get().clamp(0.0, 1.0);
        let fa = if active { 1.0 } else { 0.4 };
        if tw * lv > 0.5 {
            out.shapes.push(paint::rect(sc, Rect::new(tl, cy - 2.0, (tw * lv).max(4.0), 4.0), 2.0).on_fill(1.0, 0.0, 0.0).alpha(a * fa).done());
        }
        out.shapes.push(paint::knob(sc, tl + tw * lv, cy, 11.0 + 0.5 * h).alpha(a).done());
        let (k, tw2) = self.text.get(fs, &format!("{}%", (lv * 100.0).round() as u32), F_SMALL);
        let lh = self.text.line_h(F_SMALL);
        self.text(out, k, x + w - tw2, cy - lh * 0.5, MUTED.mix(SOFT, 0.3 + 0.7 * h).fade(a), clip);
    }

    /// A selectable list row with icon, label, optional right text.
    #[allow(clippy::too_many_arguments)]
    fn list_row(
        &mut self,
        fs: &mut FontSystem,
        out: &mut DrawList,
        c: Ctl,
        r: Rect,
        icon: &str,
        name: &str,
        right: Option<(&str, Rgba)>,
        lock: bool,
        a: f32,
        clip: Rect,
    ) {
        self.hits.push((r, c));
        let h = self.hover_of(c);
        self.hover_bg(out, r, 12.0, a, h);
        let cy = r.y + r.h * 0.5;
        self.icon(out, icon, r.x + 12.0, cy - 8.0, 16.0, TEXT.fade(a));
        let mut rw = 0.0;
        if let Some((t, col)) = right {
            let (k, w) = self.text.get(fs, t, F_SMALL);
            rw = w + 12.0;
            let lh = self.text.line_h(F_SMALL);
            self.text(out, k, r.x + r.w - 12.0 - w, cy - lh * 0.5, col.fade(a), clip);
        } else if lock {
            rw = 26.0;
            self.icon(out, "network-wireless-encrypted-symbolic", r.x + r.w - 28.0, cy - 8.0, 16.0, MUTED.fade(a));
        }
        let (k, _) = self.text.get_fit(fs, name, F_LABEL, r.w - 52.0 - rw);
        let lh = self.text.line_h(F_LABEL);
        self.text(out, k, r.x + 38.0, cy - lh * 0.5, TEXT.fade(a), clip);
    }

    fn section(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, x: f32, y: f32, a: f32, clip: Rect) {
        self.label(fs, out, &s.to_uppercase(), F_SECTION, x, y, MUTED.fade(a), clip);
    }

    // ── frame ───────────────────────────────────────────────────────────

    pub fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        self.profile_sync(out);
        let sc = self.scale;
        let pw = self.pw.get();
        let ph = self.ph.get();
        let fade = self.fade.get().clamp(0.0, 1.0);
        // Closing: shrink slightly toward the centre while fading.
        let shrink = 1.0 - 0.06 * (1.0 - fade);
        let home = Rect::new(M, M, FW, FH);
        let mut r = Rect::new(
            lerp(self.start.x, home.x, pw),
            lerp(self.start.y, home.y, ph),
            lerp(self.start.w, home.w, pw),
            lerp(self.start.h, home.h, ph),
        );
        if shrink < 1.0 {
            let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            r = Rect::new(cx - r.w * shrink * 0.5, cy - r.h * shrink * 0.5, r.w * shrink, r.h * shrink);
        }
        self.body = r;
        // Prism Ink: flat matte ink, the prism hairline, a soft shadow.
        out.shapes.push(
            paint::ink(sc, r, lerp(R_FROM, R_OPEN, ph.clamp(0.0, 1.0)))
                .shadow(SHADOW, 18.0 + 12.0 * ph, 6.0 + 8.0 * ph)
                .alpha(fade)
                .done(),
        );
        out.glass(r, lerp(R_FROM, R_OPEN, ph.clamp(0.0, 1.0)), fade);
        self.hits.push((r, Ctl::Body));
        let clip = r.inflate(-1.5);
        // Content appears once the window has (nearly) reached its size.
        let a = fade * ((pw.min(ph) - 0.6) / 0.35).clamp(0.0, 1.0);
        if a <= 0.0 {
            self.text.end_frame();
            self.translate(out);
            return;
        }

        // Sidebar.
        let side = Rect::new(r.x + 8.0, r.y + 8.0, SIDEBAR, r.h - 16.0);
        // No sidebar card: a vertical hairline that fades at both ends.
        {
            let (x, y0, h) = (side.x + side.w, r.y + 24.0, r.h - 48.0);
            let hh = h * 0.5;
            out.shapes.push(paint::rect(sc, Rect::new(x, y0, 1.0, hh), 0.0).gradient(HAIR.fade(0.0), HAIR, 0.0, 1.0).alpha(a).done());
            out.shapes.push(paint::rect(sc, Rect::new(x, y0 + hh, 1.0, h - hh), 0.0).gradient(HAIR, HAIR.fade(0.0), 0.0, 1.0).alpha(a).done());
        }
        self.hits.push((Rect::new(side.x, side.y, side.w, 52.0), Ctl::Grip));
        self.label(fs, out, "Settings", F_TITLE, side.x + 18.0, side.y + 28.0, TEXT.fade(a), clip);
        let ny = side.y + 62.0;
        // Liquid selection: two springs for the edges of one pill.
        let (head, tail) = (self.nav_head.get(), self.nav_tail.get());
        let (lo, hi) = (head.min(tail), head.max(tail));
        let sel = Rect::new(side.x + 8.0, ny + lo * NAV_H, side.w - 16.0, (hi - lo) * NAV_H + NAV_H - 4.0);
        // Concentric with the window: inset 16 from a 28 corner.
        out.shapes.push(paint::rect(sc, sel, 12.0).fill(Rgba::hex(0xffffff, 0.06)).alpha(a).done());
        let bar_h = 16.0 * (1.0 - 0.3 * ((hi - lo) * 2.0).clamp(0.0, 1.0));
        out.shapes.push(
            paint::rect(sc, Rect::new(sel.x + 4.0, sel.y + sel.h * 0.5 - bar_h * 0.5, 3.0, bar_h), 1.5)
                .on_fill(0.0, 1.0, 0.0)
                .alpha(a)
                .done(),
        );
        for (i, (p, name, icon, _)) in PAGES.iter().enumerate() {
            let row = Rect::new(side.x + 8.0, ny + i as f32 * NAV_H, side.w - 16.0, NAV_H - 4.0);
            let c = Ctl::Nav(i as u8);
            self.hits.push((row, c));
            let h = self.hover_of(c);
            if *p != self.page {
                self.hover_bg(out, row, 12.0, a, h * 0.5);
            }
            let selected = *p == self.page;
            let col = if selected { TEXT } else { SOFT.mix(TEXT, h * 0.5) };
            self.icon(out, icon, row.x + 16.0, row.y + row.h * 0.5 - 8.0, 16.0, col.fade(a));
            self.label(fs, out, name, F_NAV, row.x + 42.0, row.y + row.h * 0.5, col.fade(a), clip);
        }
        let (k, _) = self.text.get(fs, "nimbus", F_SMALL);
        self.text(out, k, side.x + 18.0, side.y + side.h - 30.0, MUTED.fade(a * 0.8), clip);

        // Close button.
        let close = Rect::new(r.x + r.w - 44.0, r.y + 14.0, 30.0, 30.0);
        self.hits.push((close, Ctl::Close));
        let ch = self.hover_of(Ctl::Close);
        self.hover_bg(out, close, 15.0, a, ch);
        self.icon(out, "window-close-symbolic", close.x + 7.0, close.y + 7.0, 16.0, SOFT.mix(TEXT, ch).fade(a));
        // The right side is also a drag handle where nothing else is.
        self.hits.push((Rect::new(side.x + side.w, r.y, r.w - side.w - 60.0, 60.0), Ctl::Grip));

        // Pages cross-fade in place: the old one leaves fast, the new one
        // arrives a beat later. Nothing moves.
        let content = Rect::new(side.x + side.w + 28.0, r.y + 24.0, r.w - side.w - 8.0 - 28.0 - 28.0, r.h - 48.0);
        let pt = self.page_t.get().clamp(0.0, 1.0);
        let hits_before = self.hits.len();
        if pt < 1.0 && self.prev_page != self.page {
            let pa = a * (1.0 - pt * 2.2).clamp(0.0, 1.0);
            if pa > 0.0 {
                self.draw_page(fs, out, self.prev_page, content, pa, clip);
                // Leaving page: visible, not interactive.
                self.hits.truncate(hits_before);
            }
        }
        let na = a * ((pt - 0.3) / 0.7).clamp(0.0, 1.0);
        if na > 0.0 {
            self.draw_page(fs, out, self.page, content, na, clip);
        }
        self.profile_rehover();
        self.text.end_frame();
        self.translate(out);
    }

    fn draw_page(&mut self, fs: &mut FontSystem, out: &mut DrawList, p: Page, c: Rect, a: f32, clip: Rect) {
        if p == Page::Profile {
            return self.page_profile(fs, out, c, a, clip);
        }
        let (_, title, _, sub) = PAGES[Self::page_index(p)];
        self.label(fs, out, title, F_PAGE, c.x, c.y + 18.0, TEXT.fade(a), clip);
        self.label(fs, out, sub, F_SUB, c.x, c.y + 46.0, MUTED.fade(a), clip);
        let body = Rect::new(c.x, c.y + 72.0, c.w, c.h - 72.0);
        match p {
            Page::Profile => {}
            Page::Appearance => self.page_appearance(fs, out, body, a, clip),
            Page::Sound => self.page_sound(fs, out, body, a, clip),
            Page::Display => self.page_display(fs, out, body, a, clip),
            Page::Network => self.page_network(fs, out, body, a, clip),
            Page::Power => self.page_power(fs, out, body, a, clip),
            Page::About => self.page_about(fs, out, body, a, clip),
        }
    }

    // ── pages ───────────────────────────────────────────────────────────

    fn page_appearance(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        self.section(fs, out, "Bar design", b.x, b.y + 6.0, a, clip);
        let (cols, gap) = (3.0, 12.0);
        let rows = (Design::ALL.len() as f32 / cols).ceil();
        let cw = (b.w - gap * (cols - 1.0)) / cols;
        let ch = 104.0;
        for (i, d) in Design::ALL.iter().enumerate() {
            let (col, row) = ((i % cols as usize) as f32, (i / cols as usize) as f32);
            let r = Rect::new(b.x + col * (cw + gap), b.y + 22.0 + row * (ch + gap), cw, ch);
            let c = Ctl::Design(i as u8);
            self.hits.push((r, c));
            let h = self.hover_of(c);
            let sel = *d == self.design;
            self.tile(out, r, 14.0, a, h);
            if sel {
                // Selected: the prism hairline, turned up into a ring.
                out.shapes.push(paint::rect(sc, r.inflate(1.0), 15.0).prism(1.6, 3.6).alpha(a).done());
            }
            // Miniature of the design.
            let pv = Rect::new(r.x + 10.0, r.y + 10.0, r.w - 20.0, 46.0);
            self.preview(out, *d, pv, a);
            self.label(fs, out, design_title(*d), F_LABEL, r.x + 12.0, r.y + 72.0, TEXT.fade(a), clip);
            let (k, _) = self.text.get_fit(fs, design_blurb(*d), F_SMALL, r.w - 24.0);
            let lh = self.text.line_h(F_SMALL);
            self.text(out, k, r.x + 12.0, r.y + 90.0 - lh * 0.5, MUTED.fade(a), clip);
        }
        let ay = b.y + 22.0 + rows * (ch + gap) + 16.0;
        self.section(fs, out, "Accent", b.x, ay, a, clip);
        let n = theme::ACCENTS.len() as f32;
        let step = (b.w / n).min(92.0);
        // Shapes aren't clipped: keep the swatches inside the window while
        // it is still growing open.
        let body_b = self.body.y + self.body.h;
        let fit = ((body_b - 10.0 - (ay + 50.0)) / 12.0).clamp(0.0, 1.0);
        for (i, (name, ca, cb)) in theme::ACCENTS.iter().enumerate() {
            let x = b.x + i as f32 * step;
            let r = Rect::new(x, ay + 16.0, step - 6.0, 34.0);
            let c = Ctl::Accent(i as u8);
            self.hits.push((r, c));
            let h = self.hover_of(c);
            let sel = i == self.accent;
            // Prism (the first entry) is the iridescent fill itself.
            let sw = paint::circle(sc, x + 16.0, r.y + 17.0, 10.0 + 1.5 * h);
            let sw = if i == 0 { sw.irid(paint::IRID.0, paint::IRID.1) } else { sw.gradient(*ca, *cb, 1.0, 1.0) };
            out.shapes.push(sw.alpha(a * fit).done());
            if sel {
                out.shapes.push(paint::circle(sc, x + 16.0, r.y + 17.0, 14.0).rim(1.5, TEXT.fade(0.85), TEXT.fade(0.85)).alpha(a * fit).done());
            }
            self.label(fs, out, name, F_SMALL, x + 36.0, r.y + 17.0, if sel { TEXT } else { SOFT }.fade(a), clip);
        }
    }

    /// Small schematic of each design (same shape language as the bar).
    fn preview(&self, out: &mut DrawList, d: Design, r: Rect, a: f32) {
        let sc = self.scale;
        // The accent bits: iridescent under Prism, else the accent pair.
        let on = |rr: Rect, radius: f32, k: f32| paint::rect(sc, rr, radius).on_fill(1.0, 0.0, 0.0).alpha(a * k).done();
        out.shapes.push(paint::rect(sc, r, 9.0).gradient(Rgba::hex(0x3a2a55, 1.0), Rgba::hex(0x7a3a2a, 1.0), 1.0, 0.0).alpha(a).done());
        let ink = Rgba::hex(0x0c0c0e, 1.0);
        let dot = |x: f32, y: f32, c: Rgba| paint::circle(sc, x, y, 1.6).fill(c).alpha(a).done();
        match d {
            Design::Deck => {
                // Spaces, the now-playing drawer (cover + tint), centre, status.
                let mw = r.w * 0.26;
                for (x, w) in [(r.x + 6.0, 26.0), (r.x + 35.0, mw), (r.x + r.w * 0.5 + 4.0, r.w * 0.2), (r.x + r.w - 6.0 - r.w * 0.18, r.w * 0.18)] {
                    out.shapes.push(paint::rect(sc, Rect::new(x, r.y - 6.0, w, 18.0), 5.0).fill(ink).alpha(a).done());
                }
                out.shapes.push(paint::rect(sc, Rect::new(r.x + 38.0, r.y + 1.0, 7.0, 7.0), 2.0).fill(Rgba::hex(0xffffff, 0.22)).alpha(a).done());
                out.shapes.push(on(Rect::new(r.x + 48.0, r.y + 9.5, (mw - 18.0) * 0.6, 1.0), 0.5, 1.0));
                out.shapes.push(on(Rect::new(r.x + 10.0, r.y + 3.0, 8.0, 3.0), 1.5, 1.0));
            }
            Design::Tabs => {
                for (x, w) in [(r.x + 6.0, 34.0), (r.x + r.w * 0.5 - 30.0, 60.0), (r.x + r.w - 46.0, 40.0)] {
                    out.shapes.push(paint::rect(sc, Rect::new(x, r.y - 6.0, w, 18.0), 5.0).fill(ink).alpha(a).done());
                }
                out.shapes.push(on(Rect::new(r.x + 12.0, r.y + 4.0, 10.0, 3.0), 1.5, 1.0));
            }
            Design::Monolith => {
                let w = r.w * 0.62;
                out.shapes.push(paint::rect(sc, Rect::new(r.x + (r.w - w) * 0.5, r.y + 5.0, w, 14.0), 5.0).fill(ink).alpha(a).done());
                out.shapes.push(on(Rect::new(r.x + (r.w - w) * 0.5 + 6.0, r.y + 10.5, 9.0, 3.0), 1.5, 1.0));
            }
            Design::Islands => {
                for (x, w) in [(r.x + 6.0, 30.0), (r.x + r.w * 0.5 - 26.0, 52.0), (r.x + r.w - 42.0, 36.0)] {
                    out.shapes.push(paint::rect(sc, Rect::new(x, r.y + 5.0, w, 13.0), 6.5).fill(Rgba::hex(0x15161c, 0.95)).alpha(a).done());
                }
                out.shapes.push(on(Rect::new(r.x + 11.0, r.y + 10.0, 9.0, 3.0), 1.5, 1.0));
            }
            Design::Braun => {
                out.shapes.push(paint::rect(sc, Rect::new(r.x + 3.0, r.y + 4.0, r.w - 6.0, 15.0), 5.0).fill(Rgba::hex(0xece8e1, 1.0)).alpha(a).done());
                for k in 0..4 {
                    let c = if k == 1 { Rgba::hex(0xff5a1f, 1.0) } else { Rgba::hex(0x55524c, 1.0) };
                    out.shapes.push(dot(r.x + 12.0 + k as f32 * 6.0, r.y + 11.5, c));
                }
            }
            Design::Led => {
                for (x, w) in [(r.x + 4.0, 22.0), (r.x + r.w * 0.5 - 28.0, 56.0), (r.x + r.w - 40.0, 36.0)] {
                    out.shapes.push(paint::rect(sc, Rect::new(x, r.y + 4.0, w, 15.0), 4.0).fill(ink).alpha(a).done());
                    for k in 0..((w - 6.0) / 3.0) as i32 {
                        out.shapes.push(dot(x + 4.0 + k as f32 * 3.0, r.y + 11.5, Rgba::hex(0xffb21e, if k % 3 == 0 { 0.3 } else { 0.9 })));
                    }
                }
            }
            Design::Swiss => {
                out.shapes.push(paint::rect(sc, Rect::new(r.x, r.y, r.w, 18.0), 0.0).fill(Rgba::hex(0x0f0f10, 1.0)).alpha(a).done());
                out.shapes.push(paint::rect(sc, Rect::new(r.x + 10.0, r.y, 8.0, 18.0), 0.0).fill(Rgba::hex(0xff3b1f, 1.0)).alpha(a).done());
                out.shapes.push(paint::rect(sc, Rect::new(r.x + r.w - 22.0, r.y, 22.0, 18.0), 0.0).fill(Rgba::hex(0xf2f0eb, 1.0)).alpha(a).done());
            }
        }
    }

    fn page_sound(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        self.section(fs, out, "Output", b.x, b.y + 6.0, a, clip);
        let card = Rect::new(b.x, b.y + 20.0, b.w, 96.0);
        let muted = self.volume.is_some_and(|v| v.muted);
        let vi = if muted { "audio-volume-muted-symbolic" } else { "audio-volume-high-symbolic" };
        self.slider(fs, out, 0, vi, card.x + 2.0, card.w - 4.0, card.y + 28.0, !muted, a, clip);
        self.divider(out, card.x, card.y + 49.0, card.w, a);
        self.label(fs, out, "Mute", F_BODY, card.x + 2.0, card.y + 70.0, SOFT.fade(a), clip);
        self.switch(out, 0, card.x + card.w - 2.0 - 42.0, card.y + 70.0, a);

        let mut y = card.y + card.h + 18.0;
        self.section(fs, out, "Output device", b.x, y, a, clip);
        y += 14.0;
        let sinks = self.audio.sinks.clone();
        if sinks.is_empty() {
            self.label(fs, out, "Reading devices…", F_SMALL, b.x + 4.0, y + 18.0, MUTED.fade(a), clip);
            y += 36.0;
        }
        for (i, s) in sinks.iter().take(3).enumerate() {
            let r = Rect::new(b.x, y, b.w, 36.0);
            let right = s.default.then_some(("Default", theme::accent().0));
            self.list_row(fs, out, Ctl::Row(0, i as u8), r, "audio-speakers-symbolic", &s.name, right, false, a, clip);
            y += 38.0;
        }
        y += 12.0;
        self.section(fs, out, "Microphone", b.x, y, a, clip);
        let card = Rect::new(b.x, y + 14.0, b.w, 96.0);
        let mm = self.audio.mic.is_some_and(|m| m.muted);
        self.slider(fs, out, 1, "audio-input-microphone-symbolic", card.x + 2.0, card.w - 4.0, card.y + 28.0, !mm, a, clip);
        self.divider(out, card.x, card.y + 49.0, card.w, a);
        self.label(fs, out, "Mute microphone", F_BODY, card.x + 2.0, card.y + 70.0, SOFT.fade(a), clip);
        self.switch(out, 1, card.x + card.w - 2.0 - 42.0, card.y + 70.0, a);
    }

    fn page_display(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        self.section(fs, out, "Brightness", b.x, b.y + 6.0, a, clip);
        let card = Rect::new(b.x, b.y + 20.0, b.w, 56.0);
        if self.system.brightness.is_some() {
            self.slider(fs, out, 2, "display-brightness-symbolic", card.x + 2.0, card.w - 4.0, card.y + 28.0, true, a, clip);
        } else {
            self.label(fs, out, "No backlight on this output", F_SMALL, card.x + 2.0, card.y + 28.0, MUTED.fade(a), clip);
        }
        let mut y = card.y + card.h + 18.0;
        self.section(fs, out, "Displays", b.x, y, a, clip);
        y += 14.0;
        let outs = self.outputs.clone();
        if outs.is_empty() {
            self.label(fs, out, "Asking niri…", F_SMALL, b.x + 4.0, y + 18.0, MUTED.fade(a), clip);
        }
        for (oi, o) in outs.iter().take(3).enumerate() {
            let card = Rect::new(b.x, y, b.w, 104.0);
            self.icon(out, "video-display-symbolic", card.x + 2.0, card.y + 18.0, 18.0, TEXT.fade(a));
            self.label(fs, out, &o.name, F_LABEL, card.x + 32.0, card.y + 22.0, TEXT.fade(a), clip);
            let w = self.label(fs, out, &o.model, F_SMALL, card.x + 32.0, card.y + 40.0, MUTED.fade(a), clip);
            self.label(fs, out, &o.mode, F_SMALL, card.x + 42.0 + w, card.y + 40.0, SOFT.fade(a), clip);
            // Segmented scale control; the selection slides between options.
            let segs = Rect::new(card.x + 2.0, card.y + 60.0, card.w - 4.0, 30.0);
            out.shapes.push(paint::rect(sc, segs, segs.h * 0.5).fill(Rgba::hex(0xffffff, 0.05)).alpha(a).done());
            let sw = segs.w / SCALES.len() as f32;
            let cur = SCALES.iter().position(|s| (s - o.scale).abs() < 0.01);
            if let Some(k) = cur {
                let r = Rect::new(segs.x + k as f32 * sw + 2.0, segs.y + 2.0, sw - 4.0, segs.h - 4.0);
                out.shapes.push(paint::rect(sc, r, r.h * 0.5).on_fill(paint::IRID.0, paint::IRID.1, 0.1).alpha(a).done());
            }
            for (k, s) in SCALES.iter().enumerate() {
                let r = Rect::new(segs.x + k as f32 * sw, segs.y, sw, segs.h);
                let c = Ctl::Seg(oi as u8, k as u8);
                self.hits.push((r, c));
                let h = self.hover_of(c);
                let sel = cur == Some(k);
                if !sel {
                    self.hover_bg(out, r.inflate(-2.0), (segs.h - 4.0) * 0.5, a, h * 0.75);
                }
                let label = format!("{s}×");
                let (key, tw) = self.text.get(fs, &label, F_SMALL);
                let lh = self.text.line_h(F_SMALL);
                self.text(out, key, r.x + (r.w - tw) * 0.5, r.y + r.h * 0.5 - lh * 0.5, if sel { theme::on_ink() } else { SOFT.mix(TEXT, h) }.fade(a), clip);
            }
            y += card.h + 10.0;
        }
    }

    fn page_network(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        let half = (b.w - 16.0) * 0.5;
        for (k, (title, sw, sec)) in [("Wi-Fi", 2u8, 1u8), ("Bluetooth", 3, 2)].into_iter().enumerate() {
            let x = b.x + k as f32 * (half + 16.0);
            let card = Rect::new(x, b.y + 4.0, half, b.h - 8.0);
            // Two open columns: a hairline between them, one under the header.
            if k == 1 {
                let (lx, y0, h) = (x - 8.0, card.y + 4.0, card.h - 8.0);
                out.shapes.push(paint::rect(self.scale, Rect::new(lx, y0, 1.0, h * 0.5), 0.0).gradient(HAIR, HAIR, 0.0, 1.0).alpha(a).done());
                out.shapes.push(paint::rect(self.scale, Rect::new(lx, y0 + h * 0.5, 1.0, h * 0.5), 0.0).gradient(HAIR, HAIR.fade(0.0), 0.0, 1.0).alpha(a).done());
            }
            self.divider(out, card.x, card.y + 46.0, card.w, a);
            self.label(fs, out, title, F_LABEL, card.x + 2.0, card.y + 26.0, TEXT.fade(a), clip);
            self.switch(out, sw, card.x + card.w - 2.0 - 42.0, card.y + 26.0, a);
            let enabled = if sec == 1 { self.system.wifi } else { self.system.bluetooth };
            let mut y = card.y + 52.0;
            let rows: Option<Vec<(String, String, Option<(&'static str, Rgba)>, bool)>> = if enabled == Some(false) {
                Some(vec![])
            } else if sec == 1 {
                self.wifi_nets.as_ref().map(|v| {
                    v.iter()
                        .take(9)
                        .map(|n| {
                            let lvl = match n.signal {
                                75.. => "excellent",
                                50..=74 => "good",
                                25..=49 => "ok",
                                _ => "weak",
                            };
                            let pending = self.pending.as_deref() == Some(n.ssid.as_str());
                            let right = if pending {
                                Some(("Connecting…", MUTED))
                            } else if n.active {
                                Some(("Connected", theme::accent().0))
                            } else {
                                None
                            };
                            (n.ssid.clone(), format!("network-wireless-signal-{lvl}-symbolic"), right, n.secure && !n.active && !pending)
                        })
                        .collect()
                })
            } else {
                self.bt_devs.as_ref().map(|v| {
                    v.iter()
                        .take(9)
                        .map(|d| {
                            let pending = self.pending.as_deref() == Some(d.mac.as_str());
                            let right = if pending {
                                Some(("…", MUTED))
                            } else if d.connected {
                                Some(("Connected", theme::accent().0))
                            } else {
                                None
                            };
                            (d.name.clone(), d.kind.clone().map_or("bluetooth-symbolic".into(), |k| format!("{k}-symbolic")), right, false)
                        })
                        .collect()
                })
            };
            let empty = match (&rows, enabled) {
                (_, Some(false)) => Some(if sec == 1 { "Wi-Fi is off" } else { "Bluetooth is off" }),
                (None, _) => Some(if sec == 1 { "Scanning…" } else { "Looking for devices…" }),
                (Some(v), _) if v.is_empty() => Some(if sec == 1 { "No networks found" } else { "No paired devices" }),
                _ => None,
            };
            if let Some(msg) = empty {
                self.label(fs, out, msg, F_SMALL, card.x + 2.0, y + 16.0, MUTED.fade(a), clip);
                continue;
            }
            for (i, (name, icon, right, lock)) in rows.unwrap_or_default().into_iter().enumerate() {
                let r = Rect::new(card.x - 6.0, y, card.w + 4.0, 36.0);
                self.list_row(fs, out, Ctl::Row(sec, i as u8), r, &icon, &name, right, lock, a, clip);
                y += 38.0;
            }
        }
    }

    fn page_power(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let card = Rect::new(b.x, b.y + 4.0, b.w, 148.0);
        let p = self.power.clone().unwrap_or_default();
        let charging = p.status == "Charging" || p.status == "Full";
        let w = self.label(fs, out, &format!("{}%", p.percent), F_HUGE, card.x + 2.0, card.y + 40.0, TEXT.fade(a), clip);
        let state = match (p.status.as_str(), p.hours) {
            ("Charging", Some(h)) => format!("Charging · full in {}", fmt_hours(h)),
            ("Discharging", Some(h)) => format!("On battery · {} left", fmt_hours(h)),
            (s, _) if !s.is_empty() => s.to_string(),
            _ => "Reading battery…".into(),
        };
        self.label(fs, out, &state, F_BODY, card.x + 30.0 + w, card.y + 44.0, SOFT.fade(a), clip);
        // Battery bar: a thin track; iridescent, mint while charging, red when low.
        let track = Rect::new(card.x + 2.0, card.y + 78.0, card.w - 4.0, 6.0);
        out.shapes.push(paint::rect(sc, track, 3.0).fill(TRACK).alpha(a).done());
        let lvl = (p.percent as f32 / 100.0).clamp(0.0, 1.0);
        if track.w * lvl > 0.5 {
            let f = paint::rect(sc, Rect::new(track.x, track.y, (track.w * lvl).max(6.0), track.h), 3.0);
            let f = if charging {
                f.fill(GOOD)
            } else if p.percent <= 15 {
                f.fill(BAD)
            } else {
                f.on_fill(1.0, 0.0, 0.0)
            };
            out.shapes.push(f.alpha(a).done());
        }
        self.divider(out, card.x, card.y + 98.0, card.w, a);
        let stats = [
            ("Health", p.health.map_or("—".into(), |h| format!("{h}%"))),
            ("Cycles", p.cycles.map_or("—".into(), |c| c.to_string())),
            ("Charge limit", p.limit.map_or("—".into(), |l| format!("{l}%"))),
            ("Profile", p.profile.clone().unwrap_or_else(|| "—".into())),
        ];
        let sw = (card.w - 4.0) / 4.0;
        for (i, (k, v)) in stats.iter().enumerate() {
            let x = card.x + 2.0 + i as f32 * sw;
            self.label(fs, out, k, F_SMALL, x, card.y + 108.0, MUTED.fade(a), clip);
            self.label(fs, out, v, F_LABEL, x, card.y + 128.0, TEXT.fade(a), clip);
        }

        let y = card.y + card.h + 22.0;
        self.section(fs, out, "Session", b.x, y, a, clip);
        let buttons: [(&str, &str); 4] = [
            ("system-lock-screen-symbolic", "Lock"),
            ("weather-clear-night-symbolic", "Sleep"),
            ("system-reboot-symbolic", "Restart"),
            ("system-shutdown-symbolic", "Power off"),
        ];
        let bw = (b.w - 30.0) / 4.0;
        for (i, (icon, name)) in buttons.iter().enumerate() {
            let r = Rect::new(b.x + i as f32 * (bw + 10.0), y + 16.0, bw, 64.0);
            let c = Ctl::Btn(i as u8);
            self.hits.push((r, c));
            let h = self.hover_of(c);
            let armed = self.armed.is_some_and(|(x, _)| x == i as u8);
            // Bare icon over its label; a faint circle under the pointer
            // (soft red for power off), solid soft red once armed.
            let danger = i == 3;
            let (cx, cy) = (r.x + r.w * 0.5, r.y + 21.0);
            if armed {
                out.shapes.push(paint::circle(sc, cx, cy, 20.0).fill(DANGER.fade(1.0 + 0.25 * h)).alpha(a).done());
            } else if h > 0.003 {
                let hc = if danger { DANGER.fade(0.22 / DANGER.3) } else { HOVER };
                out.shapes.push(paint::circle(sc, cx, cy, 20.0).fill(hc).alpha(a * h).done());
            }
            let fg = if armed { DANGER_FG } else { SOFT.mix(TEXT, h).mix(DANGER_FG, if danger { h } else { 0.0 }) };
            self.icon(out, icon, cx - 9.0, cy - 9.0, 18.0, fg.fade(a));
            let label = if armed { "Confirm" } else { name };
            let (k, tw) = self.text.get(fs, label, F_SMALL);
            let lh = self.text.line_h(F_SMALL);
            let lc = if armed { DANGER_FG } else { MUTED.mix(SOFT, 0.4 + 0.6 * h) };
            self.text(out, k, r.x + (r.w - tw) * 0.5, r.y + 54.0 - lh * 0.5, lc.fade(a), clip);
        }
    }

    fn page_about(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        let card = Rect::new(b.x, b.y + 4.0, b.w, 110.0);
        let logo = Rect::new(card.x + 2.0, card.y + 26.0, 56.0, 56.0);
        out.shapes.push(paint::rect(sc, logo, 16.0).on_fill(paint::IRID.0, paint::IRID.1, 0.0).alpha(a).done());
        out.shapes.push(paint::rect(sc, Rect::new(logo.x + 10.0, logo.y + 10.0, 36.0, 12.0), 5.0).fill(theme::on_ink()).alpha(a).done());
        self.label(fs, out, "nimbus", F_PAGE, card.x + 76.0, card.y + 42.0, TEXT.fade(a), clip);
        self.label(fs, out, "A spring-animated bar for niri, written in Rust.", F_BODY, card.x + 76.0, card.y + 70.0, SOFT.fade(a), clip);
        let ab_ = self.about.clone().unwrap_or_default();
        let rows = [
            ("Version", env!("CARGO_PKG_VERSION").to_string()),
            ("Compositor", ab_.niri.clone().map_or("niri".into(), |v| format!("niri {v}"))),
            ("Renderer", format!("wgpu · Vulkan · {}", self.gpu_name)),
            ("Memory", if ab_.rss_kb > 0 { format!("{:.1} MiB resident", ab_.rss_kb as f32 / 1024.0) } else { "—".into() }),
            ("CPU time", format!("{:.2} s over {}", ab_.cpu_secs, fmt_hours(ab_.uptime_secs / 3600.0))),
            ("Threads", if ab_.threads > 0 { ab_.threads.to_string() } else { "—".into() }),
        ];
        let mut y = card.y + card.h + 16.0;
        let rc = Rect::new(b.x, y, b.w, rows.len() as f32 * 40.0 + 8.0);
        y += 4.0;
        for (i, (k, v)) in rows.iter().enumerate() {
            if i > 0 {
                self.divider(out, rc.x, y, rc.w, a);
            }
            self.label(fs, out, k, F_BODY, rc.x + 2.0, y + 20.0, MUTED.fade(a), clip);
            let (key, w) = self.text.get_fit(fs, v, F_LABEL, rc.w - 160.0);
            let lh = self.text.line_h(F_LABEL);
            self.text(out, key, rc.x + rc.w - 2.0 - w, y + 20.0 - lh * 0.5, TEXT.fade(a), clip);
            y += 40.0;
        }
    }

    /// Whole-pixel surface motion; draw the fractional remainder.
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
        let (ipx, ipy) = (px.round() as i32, py.round() as i32);
        for t in &mut out.texts {
            t.x = (t.x + px).round();
            t.y = (t.y + py).round();
            t.clip = [t.clip[0] + ipx, t.clip[1] + ipy, t.clip[2] + ipx, t.clip[3] + ipy];
        }
        for i in &mut out.icons {
            i.x += px;
            i.y += py;
        }
        for i in out.images.iter_mut().chain(out.backdrop.iter_mut()) {
            i.rect[0] += px;
            i.rect[1] += py;
            i.reveal[0] += px;
            i.reveal[1] += py;
        }
        for (r, _) in &mut self.hits {
            r.x += dx;
            r.y += dy;
        }
        for r in &mut self.slider_rects {
            r.x += dx;
        }
        self.body.x += dx;
        self.body.y += dy;
    }
}

fn design_title(d: Design) -> &'static str {
    match d {
        Design::Tabs => "Tabs",
        Design::Monolith => "Monolith",
        Design::Islands => "Islands",
        Design::Braun => "Braun",
        Design::Led => "LED",
        Design::Swiss => "Swiss",
        Design::Deck => "Deck",
    }
}

fn design_blurb(d: Design) -> &'static str {
    match d {
        Design::Tabs => "Matte tabs hanging from the edge",
        Design::Monolith => "One shape that morphs",
        Design::Islands => "Three floating islands",
        Design::Braun => "Warm hardware faceplate",
        Design::Led => "Dot-matrix display modules",
        Design::Swiss => "Typographic grid",
        Design::Deck => "Tabs with now playing",
    }
}

fn fmt_hours(h: f32) -> String {
    let m = (h * 60.0).round() as u32;
    if m >= 60 { format!("{}h {:02}m", m / 60, m % 60) } else { format!("{m}m") }
}
