//! The lock screen, one per output: state, springs and drawing. No Wayland
//! here, so the snapshot rig can drive it headless.
//!
//!            🔒
//!          12:34                ← huge clock, digits cross-fade in place
//!     Sunday, 25 October
//!
//!           (◉)                 ← avatar (primary output only)
//!          drizzy
//!     [ •  •  •  •  • | ]       ← the field: dots pop on springs
//!       Wrong password
//!                                          ▭ 62%
//!
//! Logic only moves spring targets; `step` integrates; `draw` emits shapes,
//! images, text and icons in physical pixels. At rest nothing moves and
//! the only wake-up is the caret blink (`wake_at`).

use std::{collections::HashMap, rc::Rc};

use glyphon::FontSystem;

use super::{assets::Backdrop, auth::User};
use crate::{
    anim::{Spring, SpringCfg},
    bar::{Font, Rect, TextCache},
    gfx::{DrawList, IconDraw, ImageData, ImageDraw, TextDraw},
    icons::IconSet,
    paint,
    services::{BatteryInfo, ClockInfo},
    theme::{self, Rgba},
};

// ── type ────────────────────────────────────────────────────────────────

const F_CLOCK: Font = Font::new(140.0, 300).family("Geist").track(-0.02);
const F_DATE: Font = Font::new(19.0, 450).family("Geist");
const F_NAME: Font = Font::new(17.0, 600).family("Geist");
const F_FIELD: Font = Font::new(15.0, 450).family("Geist");
const F_STATUS: Font = Font::new(13.5, 550).family("Geist");
const F_CAPS: Font = Font::new(10.0, 700).family("Geist").track(0.1);
const F_INITIAL: Font = Font::new(40.0, 500).family("Geist");
const F_BATT: Font = Font::new(13.0, 600).family("Geist");

// ── geometry (logical px) ───────────────────────────────────────────────

const PILL_W: f32 = 300.0;
const PILL_MAX: f32 = 440.0;
const PILL_H: f32 = 52.0;
const PILL_R: f32 = 17.0;
const DOT_R: f32 = 5.0;
const DOT_SP: f32 = 20.0;
const DOT_SP_MIN: f32 = 12.0;
/// Dots shown at most; older ones scroll off the left as more are typed.
const MAX_DOTS: usize = 28;
const AV_R: f32 = 44.0;
const CHECK: f32 = 26.0;

// ── motion ──────────────────────────────────────────────────────────────

const POP: SpringCfg = SpringCfg::new(0.34, 0.50);
const DIE: SpringCfg = SpringCfg::new(0.20, 1.0);
const SLIDE: SpringCfg = SpringCfg::new(0.30, 0.76);
const WIDTH: SpringCfg = SpringCfg::new(0.42, 0.70);
const SHAKE: SpringCfg = SpringCfg::new(0.15, 0.17);
const SHAKE_KICK: f32 = -560.0;
const FAIL_FADE: SpringCfg = SpringCfg::new(1.1, 1.0);
const ENTER: SpringCfg = SpringCfg::new(0.62, 0.72);
const FADE: SpringCfg = SpringCfg::new(0.28, 1.0);
const XFADE: SpringCfg = SpringCfg::new(0.55, 1.0);
const CARET: SpringCfg = SpringCfg::new(0.12, 1.0);
const MORPH: SpringCfg = SpringCfg::new(0.30, 0.68);
/// Dots gathering to the centre on success.
const GATHER: SpringCfg = SpringCfg::new(0.16, 0.95);
/// The accent core growing out of the gathered dots.
const CORE: SpringCfg = SpringCfg::new(0.26, 0.62);
const BG_IN: SpringCfg = SpringCfg::new(0.34, 1.0);
const FOCUS: SpringCfg = SpringCfg::new(0.85, 1.0);
const ZOOM: SpringCfg = SpringCfg::new(1.0, 1.0);

const BLINK: f32 = 0.53;
/// Stop blinking (no more wake-ups) this long after the last keystroke.
const BLINK_FOR: f32 = 15.0;

/// Success choreography, seconds after the verdict. The session unlocks at
/// `SUCCESS_TOTAL` no matter what was drawn. The dots gather (0–0.1), an
/// accent core grows out of them into the shrinking field (0.07–), the
/// check wipes in once the core is full, the iris opens from the field.
const CORE_AT: f32 = 0.07;
const CHECK_AT: (f32, f32) = (0.15, 0.31);
const IRIS_AT: (f32, f32) = (0.22, 0.56);
const OUT_AT: (f32, f32) = (0.42, 0.60);
pub const SUCCESS_TOTAL: f32 = 0.62;
/// Clock digit change: the old digit leaves fast, the new one arrives
/// tinted with the accent and settles to ink.
const DIGIT_OUT: f32 = 0.12;
const DIGIT_IN: (f32, f32) = (0.03, 0.42);

const INK: Rgba = Rgba::hex(0xf4f4f6, 1.0);
const DIM: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const RED: Rgba = Rgba::hex(0xff8a9a, 1.0);
const AMBER: Rgba = Rgba::hex(0xffc069, 1.0);
/// The avatar's iridescent hairline ring.
const RING_W: f32 = 1.6;

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn smooth(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}

fn span(t: f32, (a, b): (f32, f32)) -> f32 {
    clamp01((t - a) / (b - a))
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - clamp01(t)).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    let t = clamp01(t);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) * 0.5 }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A spring that starts `delay` s after the intro begins.
struct Enter {
    s: Spring,
    delay: f32,
}

impl Enter {
    fn new(delay: f32) -> Self {
        Self { s: Spring::new(0.0, ENTER), delay }
    }
    fn get(&self) -> f32 {
        self.s.get()
    }
    fn a(&self) -> f32 {
        clamp01(self.s.get())
    }
}

/// One clock character: changes in place, its cell width springs.
struct Glyph {
    cur: char,
    prev: char,
    /// Scene time of the last change (None: never changed).
    since: Option<f32>,
    w: Spring,
}

/// A line of text that cross-fades to its next value.
struct Swap {
    cur: String,
    cur_c: Rgba,
    prev: String,
    prev_c: Rgba,
    t: Spring,
}

impl Swap {
    fn new() -> Self {
        Self { cur: String::new(), cur_c: INK, prev: String::new(), prev_c: INK, t: Spring::new(1.0, XFADE) }
    }
    fn set(&mut self, s: &str, c: Rgba, animate: bool) {
        if s == self.cur && c == self.cur_c {
            return;
        }
        self.prev = std::mem::replace(&mut self.cur, s.to_string());
        self.prev_c = std::mem::replace(&mut self.cur_c, c);
        if animate {
            self.t.snap(0.0);
            self.t.set(1.0);
        } else {
            self.t.snap(1.0);
        }
    }
}

struct Dot {
    /// Offset from the field's centre.
    x: Spring,
    s: Spring,
    dying: bool,
    /// Seconds until a staggered pop-out starts.
    delay: f32,
    /// Success: converging on the centre, popped once there.
    gather: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Idle,
    Verifying,
    /// Since this scene time.
    Success(f32),
}

/// What the user just did to the password (the scene never sees it).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edit {
    Typed,
    Erased,
    Cleared,
}

pub struct Scene {
    pub scale: f32,
    pub time: f32,
    w: f32,
    h: f32,
    text: TextCache,
    icons: Rc<IconSet>,
    keys: u64,
    accent: (Rgba, Rgba),

    // background
    pending: Vec<(u64, ImageData)>,
    frees: Vec<u64>,
    have_bg: bool,
    /// Sharp-copy dimming and backdrop luminance (see `Backdrop`).
    bg_dim: f32,
    bg_luma: f32,
    bg_a: Spring,
    focus: Spring,
    zoom: Spring,
    focus_delay: f32,

    // intro
    created: f32,
    intro: Option<f32>,
    e_clock: Enter,
    e_date: Enter,
    e_avatar: Enter,
    e_name: Enter,
    e_field: Enter,
    e_corner: Enter,

    /// Presence of the password UI (primary output).
    primary: bool,
    ui: Spring,

    // header
    clock: Option<ClockInfo>,
    glyphs: Vec<Glyph>,
    date: Swap,
    battery: Option<BatteryInfo>,

    // identity
    name: String,
    initial: String,
    has_avatar: bool,

    // field
    count: usize,
    dots: Vec<Dot>,
    pill_w: Spring,
    shake: Spring,
    fail: Spring,
    verify: Spring,
    typed: Spring,
    caret_a: Spring,
    caret_x: Spring,
    caret_on: bool,
    blink_next: f32,
    blink_until: f32,
    caps_on: bool,
    caps: Spring,
    status: Swap,
    phase: Phase,
    morph: Spring,
    core: Spring,
    pop: Spring,
    /// "Password" placeholder presence (only once every dot is gone).
    place: Spring,
    fails: u32,
    popped: bool,
    /// Success: when the check started drawing (core full).
    check_t0: Option<f32>,
    /// "Wrong password" is showing (cleared by the next keystroke).
    wrong: bool,
    place_w: f32,
}

impl Scene {
    pub fn new(fs: &mut FontSystem, icons: Rc<IconSet>, user: &User, primary: bool, key_base: u64) -> Self {
        let text = TextCache::new(fs, &theme::MATTE);
        // The name from Settings' Profile page, else the account's.
        let shown = crate::profile::display_name().unwrap_or_else(|| user.name.clone());
        let initial = shown.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
        let mut caret_a = Spring::new(1.0, CARET);
        caret_a.snap(1.0);
        Self {
            scale: 1.0,
            time: 0.0,
            w: 1920.0,
            h: 1080.0,
            text,
            icons,
            keys: key_base << 8,
            accent: theme::accent(),
            pending: vec![],
            frees: vec![],
            have_bg: false,
            bg_dim: 1.0,
            bg_luma: 0.0,
            bg_a: Spring::new(0.0, BG_IN),
            focus: Spring::new(0.0, FOCUS),
            zoom: Spring::new(1.075, ZOOM),
            focus_delay: 0.0,
            created: 0.0,
            intro: None,
            e_clock: Enter::new(0.0),
            e_date: Enter::new(0.06),
            e_avatar: Enter::new(0.10),
            e_name: Enter::new(0.15),
            e_field: Enter::new(0.19),
            e_corner: Enter::new(0.30),
            primary,
            ui: Spring::new(if primary { 1.0 } else { 0.0 }, FADE),
            clock: None,
            glyphs: vec![],
            date: Swap::new(),
            battery: None,
            name: shown,
            initial,
            has_avatar: false,
            count: 0,
            dots: vec![],
            pill_w: Spring::new(PILL_W, WIDTH),
            shake: Spring::new(0.0, SHAKE),
            fail: Spring::new(0.0, FAIL_FADE),
            verify: Spring::new(0.0, FADE),
            typed: Spring::new(0.0, FADE),
            caret_a,
            caret_x: Spring::new(0.0, SLIDE),
            caret_on: true,
            blink_next: BLINK,
            blink_until: BLINK_FOR,
            caps_on: false,
            caps: Spring::new(0.0, POP),
            status: Swap::new(),
            phase: Phase::Idle,
            morph: Spring::new(0.0, MORPH),
            core: Spring::new(0.0, CORE),
            place: Spring::new(1.0, FADE),
            pop: Spring::new(0.0, SpringCfg::new(0.28, 0.4)),
            fails: 0,
            popped: false,
            check_t0: None,
            wrong: false,
            place_w: 64.0,
        }
    }

    fn key(&self, n: u64) -> u64 {
        self.keys | n
    }

    pub fn resize(&mut self, w: f32, h: f32, scale: f32) {
        self.w = w;
        self.h = h;
        if scale != self.scale {
            self.scale = scale;
            self.text.set_scale(scale);
            // The checkmark is rasterised at the physical size.
            let px = (CHECK * scale).round() as u32;
            if let Some(img) = super::assets::checkmark(px, theme::on_ink()) {
                self.pending.push((self.key(3), img));
            }
        }
    }

    pub fn set_backdrop(&mut self, b: Backdrop) {
        self.pending.push((self.key(1), b.blur));
        self.pending.push((self.key(2), b.sharp));
        self.bg_dim = b.dim;
        self.bg_luma = b.luma;
        if !self.have_bg {
            self.have_bg = true;
            self.bg_a.set(1.0);
            self.zoom.set(1.0);
            self.focus_delay = 0.10;
        }
    }

    /// How bright the backdrop is, 0 (dark) .. 1 (needs protecting).
    fn bright(&self) -> f32 {
        smooth((self.bg_luma - 0.06) / 0.12)
    }

    pub fn set_avatar(&mut self, img: ImageData) {
        self.pending.push((self.key(4), img));
        self.has_avatar = true;
    }

    fn begin_intro(&mut self) {
        if self.intro.is_none() {
            self.intro = Some(self.time);
        }
    }

    pub fn set_clock(&mut self, c: ClockInfo) {
        let animate = self.clock.is_some();
        self.clock = Some(c);
        let s = format!("{:02}:{:02}", c.hour, c.minute);
        if self.glyphs.is_empty() {
            self.glyphs = s
                .chars()
                .map(|ch| Glyph { cur: ch, prev: ch, since: None, w: Spring::new(0.0, SLIDE) })
                .collect();
        }
        for (i, ch) in s.chars().enumerate() {
            let g = &mut self.glyphs[i];
            if g.cur != ch {
                g.prev = g.cur;
                g.cur = ch;
                g.since = animate.then_some(self.time);
            }
        }
        let date = format!("{}, {} {}", WEEKDAYS[c.weekday as usize % 7], c.day, MONTHS[c.month as usize % 12]);
        self.date.set(&date, DIM, animate);
    }

    pub fn set_battery(&mut self, b: Option<BatteryInfo>) {
        self.battery = b;
    }

    /// Show (or hide) the password UI here. `count` is the current length.
    pub fn set_primary(&mut self, on: bool, count: usize) {
        if on == self.primary {
            return;
        }
        self.primary = on;
        self.ui.set(if on { 1.0 } else { 0.0 });
        if on {
            // Take over the field silently.
            self.dots.clear();
            self.count = 0;
            for _ in 0..count.min(MAX_DOTS) {
                self.count += 1;
                let mut d = Dot { x: Spring::new(0.0, SLIDE), s: Spring::new(1.0, POP), dying: false, delay: 0.0, gather: false };
                d.s.snap(1.0);
                self.dots.push(d);
            }
            self.count = count;
            self.layout(true);
            self.typed.snap(if count > 0 { 1.0 } else { 0.0 });
        }
        self.poke();
    }

    pub fn set_caps(&mut self, on: bool) {
        if on != self.caps_on {
            self.caps_on = on;
            self.caps.set(if on { 1.0 } else { 0.0 });
            self.layout(false);
        }
    }

    /// Restart the caret blink (any keystroke).
    fn poke(&mut self) {
        self.caret_on = true;
        self.caret_a.set(1.0);
        self.blink_next = self.time + BLINK;
        self.blink_until = self.time + BLINK_FOR;
    }

    fn alive(&self) -> usize {
        self.dots.iter().filter(|d| !d.dying).count()
    }

    /// Dot spacing and the field width for `n` dots.
    fn metrics(&self, n: usize) -> (f32, f32) {
        let pad = 34.0 + if self.caps_on { 44.0 } else { 0.0 };
        let want = n as f32 * DOT_SP + pad * 2.0;
        let w = want.clamp(PILL_W, PILL_MAX);
        let sp = if n > 0 { ((w - pad * 2.0) / n as f32).clamp(DOT_SP_MIN, DOT_SP) } else { DOT_SP };
        (sp, w)
    }

    /// Gap between the last dot and the caret.
    fn caret_gap(sp: f32) -> f32 {
        sp * 0.5 + 6.0
    }

    /// Offset of dot `i` of `n`: dots and caret centred as one group.
    fn dot_x(i: usize, n: usize, sp: f32) -> f32 {
        (i as f32 - (n as f32 - 1.0) * 0.5) * sp - Self::caret_gap(sp) * 0.5
    }

    /// Where the caret rests with `n` dots.
    fn caret_rest(&self, n: usize) -> f32 {
        if n == 0 {
            -self.placeholder_w() * 0.5 - 7.0
        } else {
            let (sp, _) = self.metrics(n);
            (n as f32 - 1.0) * 0.5 * sp + Self::caret_gap(sp) * 0.5
        }
    }

    /// Move the dots, caret and field width to where `count` puts them.
    fn layout(&mut self, snap: bool) {
        let n = self.alive();
        let (sp, w) = self.metrics(n);
        let mut i = 0;
        for d in self.dots.iter_mut().filter(|d| !d.dying) {
            let x = Self::dot_x(i, n, sp);
            if snap {
                d.x.snap(x);
            } else {
                d.x.set(x);
            }
            i += 1;
        }
        let caret = self.caret_rest(n);
        if snap {
            self.caret_x.snap(caret);
            self.pill_w.snap(w);
        } else {
            self.caret_x.set(caret);
            self.pill_w.set(w);
        }
    }

    fn placeholder_w(&self) -> f32 {
        // Measured on every draw; a close guess before the first.
        self.place_w
    }

    pub fn edit(&mut self, e: Edit, count: usize) {
        if !matches!(self.phase, Phase::Idle) {
            return;
        }
        self.poke();
        if self.wrong {
            self.wrong = false;
            self.status.set("", RED, true);
        }
        match e {
            Edit::Typed => {
                let n = self.alive();
                let (sp, _) = self.metrics(n + 1);
                let x = Self::dot_x(n, n + 1, sp);
                let mut s = Spring::new(0.0, POP);
                s.set(1.0);
                self.dots.push(Dot { x: Spring::new(x, SLIDE), s, dying: false, delay: 0.0, gather: false });
                if self.alive() > MAX_DOTS
                    && let Some(d) = self.dots.iter_mut().find(|d| !d.dying)
                {
                    d.dying = true;
                    d.s.cfg = DIE;
                    d.s.set(0.0);
                }
            }
            Edit::Erased => {
                if count < MAX_DOTS
                    && let Some(d) = self.dots.iter_mut().rev().find(|d| !d.dying)
                {
                    d.dying = true;
                    d.s.cfg = DIE;
                    d.s.set(0.0);
                }
            }
            Edit::Cleared => self.drop_all(0.018),
        }
        self.count = count;
        self.typed.set(if count > 0 { 1.0 } else { 0.0 });
        self.layout(false);
    }

    /// Pop every dot out, right to left, `stagger` s apart.
    fn drop_all(&mut self, stagger: f32) {
        let mut k = 0.0;
        for d in self.dots.iter_mut().rev().filter(|d| !d.dying) {
            d.dying = true;
            d.s.cfg = DIE;
            d.delay = k;
            k += stagger;
            if d.delay <= 0.0 {
                d.s.set(0.0);
            }
        }
    }

    pub fn verifying(&mut self) {
        self.phase = Phase::Verifying;
        self.verify.set(1.0);
        self.caret_a.set(0.0);
    }

    /// The attempt failed. `note` is what PAM said, if anything (shown
    /// instead of "Wrong password": a locked account, a timeout).
    pub fn failed(&mut self, note: Option<&str>) {
        self.phase = Phase::Idle;
        self.fails += 1;
        // The verify glow keeps fading, but everything it tints turns red
        // at once (`fail` snaps to 1), so nothing says "checking" anymore.
        self.verify.set(0.0);
        self.shake.kick(SHAKE_KICK);
        self.fail.snap(1.0);
        self.fail.set(0.0);
        self.drop_all(0.012);
        self.count = 0;
        self.typed.set(0.0);
        let msg = match note {
            Some(n) => n.to_string(),
            None if self.fails >= 3 => format!("Wrong password · {} attempts", self.fails),
            None => "Wrong password".into(),
        };
        self.status.set(&msg, RED, true);
        self.wrong = true;
        self.poke();
        self.layout(false);
        // The caret doesn't sweep across the field: it reappears at the
        // placeholder once the dots are gone.
        let rest = self.caret_rest(0);
        self.caret_x.snap(rest);
        self.caret_a.snap(0.0);
    }

    /// Enter on an empty field: a small shake, nothing else.
    pub fn nudge(&mut self) {
        if matches!(self.phase, Phase::Idle) {
            self.shake.kick(SHAKE_KICK * 0.45);
            self.poke();
        }
    }

    pub fn succeeded(&mut self) {
        self.phase = Phase::Success(self.time);
        self.verify.set(0.0);
        self.caret_a.set(0.0);
        self.typed.set(0.0);
        self.status.set("", INK, true);
        // Gather: every dot converges on the centre, and pops only once
        // it's there (`step`), so they read as merging, not fading.
        for d in &mut self.dots {
            if d.dying {
                d.s.set(0.0);
                continue;
            }
            d.x.cfg = GATHER;
            d.x.set(0.0);
            d.gather = true;
            d.dying = true;
        }
    }

    pub fn sync_time(&mut self, t: f32) {
        if self.time == 0.0 && self.created == 0.0 {
            self.created = t;
            self.blink_next = t + BLINK;
            self.blink_until = t + BLINK_FOR;
        }
        self.time = self.time.max(t);
    }

    /// Advance by `dt`. True while anything moves.
    pub fn step(&mut self, dt: f32) -> bool {
        self.time += dt;
        let t = self.time;
        let mut m = false;

        // Text arrives once the background is mostly in (so it never
        // shows over a half-faded wallpaper); without one, after a beat.
        if self.intro.is_none() && ((self.have_bg && self.bg_a.get() > 0.6) || t - self.created > 0.6) {
            self.begin_intro();
        }
        if self.intro.is_none() {
            m = true;
        }
        if let Some(t0) = self.intro {
            for e in [&mut self.e_clock, &mut self.e_date, &mut self.e_avatar, &mut self.e_name, &mut self.e_field, &mut self.e_corner] {
                if e.s.target == 0.0 {
                    if t - t0 >= e.delay {
                        e.s.set(1.0);
                    }
                    m = true;
                }
                m |= e.s.step(dt);
            }
        }
        if self.have_bg && self.focus.target == 0.0 {
            self.focus_delay -= dt;
            if self.focus_delay <= 0.0 {
                self.focus.set(1.0);
            }
            m = true;
        }
        for s in [&mut self.bg_a, &mut self.focus, &mut self.zoom, &mut self.ui] {
            m |= s.step(dt);
        }

        for g in &mut self.glyphs {
            m |= g.since.is_some_and(|t0| t - t0 < DIGIT_IN.1 + 0.02);
            m |= g.w.step(dt);
        }
        m |= self.date.t.step(dt);
        m |= self.status.t.step(dt);

        for d in &mut self.dots {
            if d.gather && d.x.get().abs() < 2.5 {
                d.gather = false;
                d.s.cfg = DIE;
                d.s.set(0.0);
            }
            if d.delay > 0.0 {
                d.delay -= dt;
                if d.delay <= 0.0 {
                    d.s.set(0.0);
                }
                m = true;
            }
            m |= d.x.step(dt);
            m |= d.s.step(dt);
        }
        let before = self.dots.len();
        self.dots.retain(|d| !(d.dying && !d.gather && d.delay <= 0.0 && d.s.at_rest()));
        m |= before != self.dots.len();
        // The placeholder returns only once every dot has gone.
        self.place.set(if self.count == 0 && self.dots.is_empty() && matches!(self.phase, Phase::Idle) { 1.0 } else { 0.0 });

        for s in [
            &mut self.pill_w,
            &mut self.shake,
            &mut self.fail,
            &mut self.verify,
            &mut self.typed,
            &mut self.caret_x,
            &mut self.caps,
            &mut self.morph,
            &mut self.core,
            &mut self.pop,
            &mut self.place,
        ] {
            m |= s.step(dt);
        }

        // Caret blink: a target flip on a timer, eased by a fast spring.
        if self.primary && matches!(self.phase, Phase::Idle) {
            if t < self.blink_until {
                while t >= self.blink_next {
                    self.caret_on = !self.caret_on;
                    self.blink_next += BLINK;
                }
            } else {
                self.caret_on = true;
            }
            self.caret_a.set(if self.caret_on { 1.0 } else { 0.0 });
        }
        m |= self.caret_a.step(dt);

        match self.phase {
            Phase::Verifying => m = true,
            Phase::Success(t0) => {
                if t - t0 >= 0.03 && self.morph.target == 0.0 {
                    self.morph.set(1.0);
                }
                if t - t0 >= CORE_AT && self.core.target == 0.0 {
                    self.core.set(1.0);
                }
                // The check waits for the core to fill the body (and for
                // the gathered dots to have merged into it).
                let ready = self.core.get() > 0.85 && self.dots.iter().all(|d| !d.gather);
                if self.check_t0.is_none() && (ready && t - t0 >= CHECK_AT.0 || t - t0 >= CHECK_AT.0 + 0.08) {
                    self.check_t0 = Some(t);
                }
                if self.check_t0.is_some() && !self.popped {
                    self.popped = true;
                    self.pop.kick(9.0);
                }
                m = true;
            }
            Phase::Idle => {}
        }
        m
    }

    /// Scene time of the next caret flip, when nothing else animates.
    pub fn wake_at(&self) -> Option<f32> {
        (self.primary && matches!(self.phase, Phase::Idle) && self.blink_next < self.blink_until).then_some(self.blink_next)
    }

    pub fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    // ── drawing ─────────────────────────────────────────────────────────

    fn px(&self, r: Rect) -> [f32; 4] {
        let s = self.scale;
        [r.x * s, r.y * s, r.w * s, r.h * s]
    }

    fn text(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, y: f32, c: Rgba) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        if c.3 > 0.003 {
            let sc = self.scale;
            let full = [0, 0, (self.w * sc).ceil() as i32, (self.h * sc).ceil() as i32];
            out.texts.push(TextDraw { key: k, x: (x * sc).round(), y: (y * sc).round(), color: c, clip: full });
        }
        w
    }

    /// Text centred on `cx`, its line box centred on `cy`.
    fn text_c(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, cx: f32, cy: f32, c: Rgba) -> f32 {
        let (_, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text(fs, out, s, f, cx - w * 0.5, cy - lh * 0.5, c)
    }

    pub fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        out.uploads.append(&mut self.pending);
        out.frees.append(&mut self.frees);
        let (w, h, s) = (self.w, self.h, self.scale);
        let (acc_a, _) = self.accent;
        let succ_t = match self.phase {
            Phase::Success(t0) => Some(self.time - t0),
            _ => None,
        };
        // Everything but the wallpaper fades at the very end of a success.
        let out_a = 1.0 - smooth(succ_t.map_or(0.0, |st| span(st, OUT_AT)));
        let ui = clamp01(self.ui.get()) * out_a;
        let bright = self.bright();

        // ── layout ──
        let cx = w * 0.5;
        let clock_lh = self.text.line_h(F_CLOCK);
        let primary_clock = h * 0.27;
        let lone_clock = h * 0.40;
        let clock_cy = lerp(lone_clock, primary_clock, smooth(self.ui.get()));
        let date_cy = clock_cy + clock_lh * 0.47;
        let av_cy = h * 0.53;
        let name_cy = av_cy + AV_R + 26.0;
        let pill_cy = name_cy + 50.0;

        // ── iris: where the opening front is (success only) ──
        let (ox, oy) = if self.primary { (cx, pill_cy) } else { (cx, h * 0.5) };
        let soft = 170.0;
        let reach = ((w * w + h * h).sqrt() + soft) * 1.02;
        let iris_p = succ_t.map_or(0.0, |st| ease_in_out(span(st, IRIS_AT)));
        let iris_r = iris_p * reach;
        // Text dissolves as the wavefront passes over it.
        let swept = |x: f32, y: f32| {
            if iris_p <= 0.0 {
                return 1.0;
            }
            let d = ((x - ox).powi(2) + (y - oy).powi(2)).sqrt();
            1.0 - smooth((iris_r - d + soft * 0.25) / (soft * 0.9))
        };

        // ── background ──
        let z = self.zoom.get();
        let full = |z: f32| [(w * s) * (1.0 - z) * 0.5, (h * s) * (1.0 - z) * 0.5, w * s * z, h * s * z];
        let bg_a = clamp01(self.bg_a.get());
        if self.have_bg {
            let mut blur = ImageDraw::new(self.key(1), full(z));
            blur.alpha = bg_a;
            blur.smooth = true;
            out.backdrop.push(blur);
            // The focus pull: the sharp copy, dimmed like the blur, so the
            // intro never flashes the bright desktop.
            let sharp_a = bg_a * (1.0 - clamp01(self.focus.get()));
            if sharp_a > 0.003 {
                let mut sharp = ImageDraw::new(self.key(2), full(z));
                sharp.alpha = sharp_a;
                sharp.bright = self.bg_dim;
                out.backdrop.push(sharp);
            }
        }
        // Covers the wallpaper until it has faded in (and the screen
        // before it arrives).
        if bg_a < 0.997 {
            out.shapes.push(
                paint::rect(s, Rect::new(0.0, 0.0, w, h), 0.0).fill(Rgba::hex(0x0b0b0e, 1.0 - bg_a)).done(),
            );
        }
        // Bright wallpapers: soft dark pools behind the text clusters.
        let scrim = 0.42 * bright * bg_a * out_a;
        if scrim > 0.003 {
            let pool = |x: f32, y: f32, pw: f32, ph: f32, a: f32| {
                paint::rect(s, Rect::new(x - pw * 0.5, y - ph * 0.5, pw, ph), ph * 0.5)
                    .fill(Rgba::hex(0x000000, a * 0.5))
                    .shadow(Rgba::hex(0x000000, a), 110.0, 0.0)
                    .done()
            };
            out.shapes.push(pool(cx, (clock_cy + date_cy) * 0.5, 380.0, 110.0, scrim * self.e_clock.a() * swept(cx, clock_cy)));
            if ui > 0.003 {
                out.shapes.push(pool(cx, (av_cy + pill_cy) * 0.5, 300.0, 150.0, scrim * ui));
            }
        }

        // ── lock glyph, top centre ──
        let lock_id = self.icons.id(&["system-lock-screen-symbolic"]);
        let la = lerp(0.5, 0.82, bright) * self.e_clock.a() * out_a * swept(cx, 39.0);
        if la > 0.003 {
            out.icons.push(IconDraw { id: lock_id, x: (cx - 9.0) * s, y: 30.0 * s, size: (18.0 * s).round(), color: INK.fade(la) });
        }

        // ── clock ──
        let ca = self.e_clock.a() * out_a * swept(cx, clock_cy);
        self.draw_clock(fs, out, cx, clock_cy, ca);
        let date = std::mem::take(&mut self.date.cur);
        let prev = std::mem::take(&mut self.date.prev);
        let dt_ = clamp01(self.date.t.get());
        let da = self.e_date.a() * out_a * swept(cx, date_cy) * lerp(0.82, 0.92, bright);
        if dt_ < 1.0 && !prev.is_empty() {
            self.text_c(fs, out, &prev, F_DATE, cx, date_cy, self.date.prev_c.fade(da * (1.0 - smooth(dt_ * 2.5))));
        }
        self.text_c(fs, out, &date, F_DATE, cx, date_cy, self.date.cur_c.fade(da * smooth(dt_)));
        self.date.cur = date;
        self.date.prev = prev;

        // ── avatar + name (password UI) ──
        if ui > 0.003 {
            self.draw_identity(fs, out, cx, av_cy, ui);
            let na = self.e_name.a() * ui;
            let name = self.name.clone();
            self.text_c(fs, out, &name, F_NAME, cx, name_cy, INK.fade(na * 0.94));
            self.draw_field(fs, out, cx, pill_cy, ui, succ_t);
        }

        // ── iris: the desktop's wallpaper opens from the field ──
        if iris_p > 0.0 && self.have_bg {
            let mut img = ImageDraw::new(self.key(2), full(1.0 + 0.045 * (1.0 - iris_p)));
            img.reveal = [ox * s, oy * s, iris_r * s, soft * s];
            out.backdrop.push(img);
            // A faint wavefront rides the opening edge.
            let rr = iris_r - soft * 0.55;
            let ka = 0.16 * (1.0 - iris_p) * smooth(iris_p * 6.0);
            if rr > 4.0 && ka > 0.003 {
                let c = Rgba::hex(0xffffff, ka);
                out.shapes.push(paint::circle(s, ox, oy, rr).rim(1.5, c, c).done());
            }
        }

        // ── battery, bottom right ──
        if let Some(b) = self.battery {
            let by = h - 34.0;
            let a = self.e_corner.a() * out_a * lerp(0.66, 0.9, bright) * swept(w - 50.0, by);
            let label = format!("{}%", b.percent);
            let (_, tw) = self.text.get(fs, &label, F_BATT);
            let tx = w - 28.0 - tw;
            let low = b.percent <= 15 && !b.charging;
            let col = if low { RED } else { INK };
            self.text(fs, out, &label, F_BATT, tx, by - self.text.line_h(F_BATT) * 0.5, col.fade(a));
            let icon = self.icons.battery(b.percent, b.charging);
            let ic = if b.charging { acc_a } else { col };
            if a > 0.003 {
                out.icons.push(IconDraw { id: icon, x: (tx - 22.0) * s, y: (by - 8.0) * s, size: (16.0 * s).round(), color: ic.fade(a) });
            }
        }
        self.text.end_frame();
    }

    fn draw_clock(&mut self, fs: &mut FontSystem, out: &mut DrawList, cx: f32, cy: f32, a: f32) {
        if self.glyphs.is_empty() {
            return;
        }
        // Cell widths follow the glyphs (proportional figures), springing
        // when a digit of another width arrives.
        for i in 0..self.glyphs.len() {
            let ch = self.glyphs[i].cur;
            let (_, gw) = self.text.get(fs, ch.encode_utf8(&mut [0; 4]), F_CLOCK);
            let g = &mut self.glyphs[i];
            if g.w.target != gw {
                if g.w.target == 0.0 {
                    g.w.snap(gw);
                } else {
                    g.w.set(gw);
                }
            }
        }
        let total: f32 = self.glyphs.iter().map(|g| g.w.get()).sum();
        let lh = self.text.line_h(F_CLOCK);
        let y = cy - lh * 0.5;
        let mut x = cx - total * 0.5;
        let ink = INK.fade(0.97);
        let tint = self.accent.0.mix(self.accent.1, 0.4);
        for i in 0..self.glyphs.len() {
            let (cur, prev, age, cw) = {
                let g = &self.glyphs[i];
                (g.cur, g.prev, g.since.map(|t0| self.time - t0), g.w.get())
            };
            let mid = x + cw * 0.5;
            let mut buf = [0; 4];
            let (_, gw) = self.text.get(fs, cur.encode_utf8(&mut buf), F_CLOCK);
            match age {
                Some(age) if prev != cur && age < DIGIT_IN.1 => {
                    // Outgoing: gone fast, so the two never smear together.
                    let o = 1.0 - ease_out(age / DIGIT_OUT);
                    if o > 0.0 {
                        let mut pb = [0; 4];
                        let (_, pw) = self.text.get(fs, prev.encode_utf8(&mut pb), F_CLOCK);
                        self.text(fs, out, prev.encode_utf8(&mut pb), F_CLOCK, mid - pw * 0.5, y, ink.fade(a * o * o));
                    }
                    // Incoming: rises in tinted with the accent, settles to ink.
                    let p = span(age, DIGIT_IN);
                    let ia = ease_out(span(age, (DIGIT_IN.0, DIGIT_IN.0 + 0.24)));
                    let col = tint.mix(ink, smooth(p));
                    self.text(fs, out, cur.encode_utf8(&mut buf), F_CLOCK, mid - gw * 0.5, y, col.with_a(0.97).fade(a * ia));
                }
                _ => {
                    self.text(fs, out, cur.encode_utf8(&mut buf), F_CLOCK, mid - gw * 0.5, y, ink.fade(a));
                }
            }
            x += cw;
        }
    }

    fn draw_identity(&mut self, fs: &mut FontSystem, out: &mut DrawList, cx: f32, cy: f32, ui: f32) {
        let s = self.scale;
        let (acc_a, acc_b) = self.accent;
        let e = self.e_avatar.get();
        let a = clamp01(e) * ui;
        if a <= 0.003 {
            return;
        }
        let k = 0.72 + 0.28 * e;
        let r = AV_R * k;
        let v = clamp01(self.verify.get());
        let fail = clamp01(self.fail.get());
        // A thin iridescent hairline ring (the accent gradient under
        // another accent). Its gradient turns while verifying, and it
        // flushes red on a wrong password with the field. No glow.
        let ang = -0.52 + v * self.time * 4.2;
        let (dx, dy) = (ang.cos(), ang.sin());
        let red = fail * 0.85;
        let ring = paint::circle(s, cx, cy, r + RING_W).on_mix(RED, 1.0 - red, dx, dy, 0.08).shadow(Rgba::hex(0x000000, 0.45), 18.0, 8.0);
        out.shapes.push(ring.alpha(a).done());
        // Dark gap between the ring and what's inside.
        out.shapes.push(paint::circle(s, cx, cy, r).fill(Rgba::hex(0x09090b, a)).done());
        if self.has_avatar {
            let mut img = ImageDraw::new(self.key(4), self.px(Rect::new(cx - r + 0.5, cy - r + 0.5, 2.0 * r - 1.0, 2.0 * r - 1.0)));
            img.radius = r * s;
            img.alpha = a;
            out.images.push(img);
        } else {
            // Flat matte ink disk; the initial carries the accent.
            out.shapes.push(paint::circle(s, cx, cy, r - 0.5).fill(Rgba::hex(0x111116, a)).done());
            let initial = self.initial.clone();
            let f = Font { size: F_INITIAL.size * k, ..F_INITIAL };
            self.text_c(fs, out, &initial, f, cx, cy + 1.0, acc_a.mix(acc_b, 0.45).with_a(1.0).fade(0.95 * a));
        }
    }

    fn draw_field(&mut self, fs: &mut FontSystem, out: &mut DrawList, cx: f32, cy: f32, ui: f32, succ_t: Option<f32>) {
        let s = self.scale;
        let t = self.time;
        let (acc_a, acc_b) = self.accent;
        let e = self.e_field.get();
        let a = clamp01(e) * ui;
        if a <= 0.003 {
            return;
        }
        let morph_raw = self.morph.get();
        let morph = clamp01(morph_raw);
        let core = self.core.get().max(0.0);
        let pop = self.pop.get();
        let fail = clamp01(self.fail.get());
        let v = clamp01(self.verify.get());
        let typed = clamp01(self.typed.get());
        let breath = 0.5 + 0.5 * (t * 4.0).sin();

        // Body: the typed width, shrinking to a circle on success (the
        // unclamped spring, so it overshoots and settles).
        let bw = lerp(self.pill_w.get() * (0.82 + 0.18 * e), PILL_H, morph_raw).max(8.0) * (1.0 + 0.07 * pop);
        let bh = PILL_H * (1.0 + 0.07 * pop);
        let sx = self.shake.get();
        let body = Rect::new(cx - bw * 0.5 + sx, cy - bh * 0.5, bw, bh);
        let radius = lerp(PILL_R, bh * 0.5, morph).min(bw.min(bh) * 0.5);
        // Prism Ink: flat matte ink edged by the prism hairline, which
        // strengthens a little while there's input (and breathes gently
        // while verifying). A wrong password tints it red. No glow.
        let lit = (typed * (1.0 - fail)).max(v * (1.0 - fail)) * (1.0 - morph);
        let k = (1.0 + 0.7 * lit * (0.8 + 0.2 * breath * v)) * (1.0 - fail);
        let fill = paint::INK.mix(Rgba::hex(0x2a0d14, 0.9), fail);
        out.shapes.push(
            paint::rect(s, body, radius)
                .fill(fill)
                .prism(paint::HAIR, k)
                .shadow(Rgba::hex(0x000000, 0.5), 22.0, 9.0)
                .alpha(a)
                .done(),
        );
        if fail > 0.003 {
            let c = RED.with_a(0.42 * fail);
            out.shapes.push(paint::rect(s, body, radius).rim(paint::HAIR, c, c).alpha(a).done());
        }

        // Success: an accent core grows out of the gathered dots and
        // fills the shrinking body (no muddy colour cross-fade).
        if core > 0.005 {
            let rc = bh * 0.5 * core;
            let flash = succ_t.map_or(0.0, |st| (1.0 - ((st - 0.24) / 0.22).abs()).max(0.0));
            out.shapes.push(paint::circle(s, cx + sx, cy, rc).on_fill(paint::IRID.0, paint::IRID.1, 0.1 + 0.15 * flash).alpha(a).done());
        }

        // Placeholder: only once every dot has gone, fading out as the
        // first new dot pops in.
        let left = self.dots.iter().map(|d| clamp01(d.s.get())).fold(0.0, f32::max);
        let gone = clamp01(1.0 - left * 4.0);
        let place = clamp01(self.place.get());
        let place_a = place * (1.0 - morph) * gone * a;
        let fcx = cx + sx;
        let pw = self.text.get(fs, "Password", F_FIELD).1;
        if pw != self.place_w {
            self.place_w = pw;
            if self.alive() == 0 {
                self.caret_x.snap(-pw * 0.5 - 7.0);
            }
        }
        if place_a > 0.003 {
            self.text_c(fs, out, "Password", F_FIELD, fcx, cy, INK.fade(0.36 * place_a));
        }

        // Dots (they ride the shake; red on a wrong password).
        let n = self.dots.len().max(1) as f32;
        for (i, d) in self.dots.iter().enumerate() {
            let sc = d.s.get().max(0.0);
            if sc <= 0.01 {
                continue;
            }
            let fi = i as f32;
            let phase = t * 7.0 - fi * 0.62;
            let pulse = (0.5 + 0.5 * phase.sin()).powi(3);
            let r = DOT_R * sc * (1.0 + 0.5 * v * pulse);
            let bob = -2.5 * v * pulse;
            let grad = acc_a.mix(acc_b, if n > 1.0 { fi / (n - 1.0) } else { 0.5 });
            let col = INK.mix(grad, v * (0.55 + 0.45 * pulse)).mix(RED, fail * 0.9).fade(0.95 * a);
            out.shapes.push(paint::circle(s, fcx + d.x.get(), cy + bob, r).fill(col).done());
        }

        // Caret.
        let ca = clamp01(self.caret_a.get()) * (1.0 - morph) * a * (1.0 - v) * if self.count == 0 { gone * place } else { 1.0 };
        if ca > 0.003 {
            let x = fcx + self.caret_x.get();
            out.shapes.push(paint::rect(s, Rect::new(x - 1.1, cy - 11.0, 2.2, 22.0), 1.1).on_fill(0.0, 1.0, 0.0).alpha(ca).done());
        }

        // Caps Lock chip, right end of the field.
        let caps = self.caps.get();
        if caps > 0.01 && morph < 0.99 {
            let ka = clamp01(caps) * a * (1.0 - morph);
            let cw = 40.0 * (0.6 + 0.4 * caps);
            let ch = 22.0 * (0.6 + 0.4 * caps);
            let ccx = body.x + body.w - 12.0 - 20.0;
            out.shapes.push(
                paint::rect(s, Rect::new(ccx - cw * 0.5, cy - ch * 0.5, cw, ch), 7.0)
                    .fill(AMBER.fade(0.13 * ka))
                    .rim(1.0, AMBER.fade(0.22 * ka), AMBER.fade(0.22 * ka))
                    .done(),
            );
            self.text_c(fs, out, "CAPS", F_CAPS, ccx + 0.5, cy, AMBER.fade(0.95 * ka));
        }

        // Success: the check wipes in along its stroke (left to right),
        // once the core has filled the body.
        if let (Some(t0), Some(_)) = (self.check_t0, succ_t) {
            let p = ease_out((t - t0) / (CHECK_AT.1 - CHECK_AT.0));
            if p > 0.0 {
                let k = CHECK * (1.0 + 0.07 * pop);
                let r = Rect::new(cx + sx - k * 0.5, cy - k * 0.5 + 0.5, k, k);
                let mut img = ImageDraw::new(self.key(3), self.px(r));
                img.alpha = a;
                // A vertical wipe edge: a huge circle far to the left.
                let far = 4000.0;
                let (x0, x1) = (r.x + k * 3.6 / 24.0, r.x + k * 20.4 / 24.0);
                img.reveal = [(x0 - far) * s, (cy) * s, (far + (x1 - x0) * p) * s, 2.5 * s];
                out.images.push(img);
            }
        }

        // Status line under the field.
        let st_cy = cy + PILL_H * 0.5 + 24.0;
        let (cur, prev) = (std::mem::take(&mut self.status.cur), std::mem::take(&mut self.status.prev));
        let p = clamp01(self.status.t.get());
        if p < 1.0 && !prev.is_empty() {
            self.text_c(fs, out, &prev, F_STATUS, cx, st_cy, self.status.prev_c.fade(a * (1.0 - smooth(p * 2.0)) * 0.95));
        }
        if !cur.is_empty() {
            self.text_c(fs, out, &cur, F_STATUS, cx, st_cy, self.status.cur_c.fade(a * smooth(p) * 0.95));
        }
        self.status.cur = cur;
        self.status.prev = prev;
    }
}
