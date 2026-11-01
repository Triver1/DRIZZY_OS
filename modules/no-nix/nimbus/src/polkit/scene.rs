//! The prompt: a Prism Ink card centred on a full-output Overlay surface,
//! which dims what's behind it and swallows clicks while it's up.
//!
//!   ┌────────────────────────────────────────┐
//!   │ (◍)  Authentication required           │
//!   │ Authentication is needed to run …      │
//!   │ (D) drizzy                             │   ← picker rows when several
//!   │ [  •  •  •  •  • |                  ]  │   ← the lock's field
//!   │          Wrong password                │
//!   │ Details ›          [Cancel] [Authenti…]│
//!   └────────────────────────────────────────┘
//!
//! The password is typed into `secret` and leaves by value on Enter, so
//! drawing code only ever sees a count. Closing, for any reason, wipes it
//! at that moment rather than when the fold animation ends.
//!
//! The card's own text is fixed; polkit's strings arrive sanitised and are
//! shaped as plain glyphs, on lines of their own, and cut with an ellipsis.

use std::{
    collections::HashMap,
    rc::Rc,
    sync::{Arc, mpsc},
};

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};
use smithay_client_toolkit::seat::keyboard::Keysym;

use super::{
    BROKER,
    broker::{Info, Msg, View},
    helper,
};
use crate::{
    anim::{Spring, SpringCfg},
    bar::{Font, Rect, TextCache},
    gfx::{DrawList, IconDraw, TextDraw},
    icons::IconSet,
    lock::Secret,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    window::{Chrome, Ctx, Cursor, KeyInput, Keyboard, Placement, WinScene, keys},
};

// ── type ────────────────────────────────────────────────────────────────

const F_TITLE: Font = Font::new(16.0, 600).family("Geist");
const F_MSG: Font = Font::new(13.5, 450).family("Geist");
const F_NAME: Font = Font::new(13.0, 550).family("Geist");
const F_FULL: Font = Font::new(12.5, 450).family("Geist");
const F_INITIAL: Font = Font::new(10.5, 650).family("Geist");
const F_FIELD: Font = Font::new(14.0, 450).family("Geist");
const F_STATUS: Font = Font::new(12.5, 550).family("Geist");
const F_BTN: Font = Font::new(13.0, 600).family("Geist");
const F_LINK: Font = Font::new(12.5, 500).family("Geist");
const F_CAPS: Font = Font::new(9.0, 700).family("Geist").track(0.1);
const F_LABEL: Font = Font::new(11.5, 500).family("Geist");
/// Details values. Not Geist Mono: that face loads lazily, and until it
/// has, fallback glyphs can come out blank. What a password prompt says it
/// is running can't have characters missing.
const F_VALUE: Font = Font::new(11.5, 450).family("Geist");

// ── geometry (logical px) ───────────────────────────────────────────────

const W: f32 = 420.0;
const PAD: f32 = 22.0;
const RADIUS: f32 = 20.0;
const HEAD_H: f32 = 32.0;
const LINE_MAX: usize = 4;
const ROW_H: f32 = 30.0;
const ROWS_MAX: usize = 4;
const FIELD_H: f32 = 46.0;
const FIELD_R: f32 = 14.0;
const STATUS_H: f32 = 28.0;
const BTN_H: f32 = 36.0;
const BTN_R: f32 = 11.0;
const CANCEL_W: f32 = 84.0;
const AUTH_W: f32 = 122.0;
const DETAILS_W: f32 = 76.0;
const DETAILS_H: f32 = 62.0;
/// Lines of the program's path in Details: the name at its end is the point.
const PROGRAM_LINES: usize = 3;
const VALUE_X: f32 = 62.0;
const DOT_R: f32 = 4.5;
const DOT_SP: f32 = 18.0;
const DOT_SP_MIN: f32 = 11.0;
const MAX_DOTS: usize = 28;

// ── motion ──────────────────────────────────────────────────────────────

const OPEN: SpringCfg = SpringCfg::new(0.28, 0.82);
const POP: SpringCfg = SpringCfg::new(0.34, 0.50);
const DIE: SpringCfg = SpringCfg::new(0.20, 1.0);
const SLIDE: SpringCfg = SpringCfg::new(0.30, 0.76);
const SHAKE: SpringCfg = SpringCfg::new(0.15, 0.17);
const SHAKE_KICK: f32 = -560.0;
const FAIL_FADE: SpringCfg = SpringCfg::new(1.1, 1.0);
const FADE: SpringCfg = SpringCfg::new(0.28, 1.0);
const XFADE: SpringCfg = SpringCfg::new(0.34, 1.0);
const CARET: SpringCfg = SpringCfg::new(0.12, 1.0);
const HOVER: SpringCfg = SpringCfg::new(0.22, 1.0);

const BLINK: f32 = 0.53;
/// Stop blinking (no more wake-ups) this long after the last keystroke.
const BLINK_FOR: f32 = 15.0;
/// How long the card stays after a success, and after an error it reports.
const HOLD_OK: f32 = 0.10;
const HOLD_ERROR: f32 = 1.6;

const TEXT: Rgba = Rgba::hex(0xf4f4f6, 1.0);
const DIM: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
const RED: Rgba = Rgba::hex(0xff8a9a, 1.0);
const AMBER: Rgba = Rgba::hex(0xffc069, 1.0);
const HOVER_FILL: Rgba = Rgba::hex(0xffffff, 0.08);
const FIELD_FILL: Rgba = Rgba::hex(0xffffff, 0.05);
/// The dim over the rest of the output.
const SCRIM: f32 = 0.42;

/// Small hints from the host, as `Fact::Custom`.
enum Hint {
    Focus(bool),
    Caps(bool),
}

pub fn focus_fact(on: bool) -> Fact {
    Fact::Custom(Arc::new(Hint::Focus(on)))
}

pub fn caps_fact(on: bool) -> Fact {
    Fact::Custom(Arc::new(Hint::Caps(on)))
}

/// Does this payload need a window that isn't open yet?
pub fn wants_window(f: &Fact) -> bool {
    matches!(f, Fact::Custom(a) if matches!(a.downcast_ref::<View>(), Some(View::Show(_))))
}

/// The host has nowhere to show this request (no output): cancel it.
pub fn no_output(f: &Fact) {
    if let Fact::Custom(a) = f
        && let Some(View::Show(i)) = a.downcast_ref::<View>()
        && let Some(tx) = BROKER.get()
    {
        let _ = tx.send(Msg::Gone { id: i.id });
    }
}

/// Overwrite key text the event carried, once used.
pub fn wipe_text(t: Option<String>) {
    if let Some(t) = t {
        let mut b = t.into_bytes();
        for x in b.iter_mut() {
            unsafe { std::ptr::write_volatile(x, 0) };
        }
    }
}

pub fn open(ctx: &mut Ctx) -> Box<dyn WinScene> {
    Box::new(Prompt::new(ctx, BROKER.get().cloned()))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    /// Nothing to show yet.
    Empty,
    /// The field takes input.
    Idle,
    Verifying,
    /// Over. The card folds at this scene time.
    Leaving(f32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ctl {
    None,
    Field,
    Cancel,
    Auth,
    Details,
    User(u8),
}

struct Dot {
    x: Spring,
    s: Spring,
    dying: bool,
    delay: f32,
}

/// A line that cross-fades to its next value.
struct Swap {
    cur: String,
    cur_c: Rgba,
    prev: String,
    prev_c: Rgba,
    t: Spring,
}

impl Swap {
    fn new() -> Self {
        Self { cur: String::new(), cur_c: DIM, prev: String::new(), prev_c: DIM, t: Spring::new(1.0, XFADE) }
    }
    fn set(&mut self, s: &str, c: Rgba) {
        if s == self.cur && c == self.cur_c {
            return;
        }
        self.prev = std::mem::replace(&mut self.cur, s.to_string());
        self.prev_c = std::mem::replace(&mut self.cur_c, c);
        self.t.snap(0.0);
        self.t.set(1.0);
    }
}

/// Where everything sits, top down, in card coordinates.
struct Layout {
    msg_y: f32,
    who_y: f32,
    field_y: f32,
    status_y: f32,
    foot_y: f32,
    details_y: f32,
    closed_h: f32,
}

pub struct Prompt {
    chrome: Chrome,
    text: TextCache,
    icons: Rc<IconSet>,
    scale: f32,
    size: (f32, f32),
    tx: Option<mpsc::Sender<Msg>>,

    /// The password being typed. Never read here except for its length.
    secret: Secret,
    info: Option<Arc<Info>>,
    user: usize,
    lines: Vec<String>,
    program: Vec<String>,
    /// Fonts differ per scale: the message is broken again after a change.
    wrapped_at: f32,
    phase: Phase,
    /// A "Wrong password" that the next keystroke clears.
    wrong: bool,
    focused: bool,

    // field
    placeholder: String,
    place_w: f32,
    count: usize,
    dots: Vec<Dot>,
    shake: Spring,
    fail: Spring,
    verify: Spring,
    typed: Spring,
    place: Spring,
    caret_a: Spring,
    caret_x: Spring,
    caret_on: bool,
    blink_next: f32,
    blink_until: f32,
    caps_on: bool,
    caps: Spring,
    status: Swap,

    // card
    content: Spring,
    details: Spring,
    details_open: bool,
    card_h: Spring,
    armed: Spring,
    top: f32,
    hovered: Ctl,
    pressed: Ctl,
    hover: [Spring; 3],
    row_hover: Spring,
    row_hover_y: Spring,
    pointer: (f32, f32),
    hits: Vec<(Rect, Ctl)>,
}

impl Prompt {
    fn new(ctx: &mut Ctx, tx: Option<mpsc::Sender<Msg>>) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        let size = (ctx.out_w, ctx.out_h);
        let top = (size.1 * 0.5 - 140.0).max(40.0);
        let home = Rect::new((size.0 - W) * 0.5, top, W, 280.0);
        let start = Rect::new(home.x + W * 0.04, top + 12.0, W * 0.92, 280.0 * 0.92);
        let mut caret_a = Spring::new(1.0, CARET);
        caret_a.snap(1.0);
        Self {
            chrome: Chrome::new(start, home, OPEN, OPEN),
            text,
            icons: ctx.icons.clone(),
            scale: ctx.scale,
            size,
            tx,
            secret: Secret::new(),
            info: None,
            user: 0,
            lines: vec![],
            program: vec![],
            wrapped_at: 0.0,
            phase: Phase::Empty,
            wrong: false,
            focused: true,
            placeholder: "Password".into(),
            place_w: 60.0,
            count: 0,
            dots: vec![],
            shake: Spring::new(0.0, SHAKE),
            fail: Spring::new(0.0, FAIL_FADE),
            verify: Spring::new(0.0, FADE),
            typed: Spring::new(0.0, FADE),
            place: Spring::new(1.0, FADE),
            caret_a,
            caret_x: Spring::new(0.0, SLIDE),
            caret_on: true,
            blink_next: BLINK,
            blink_until: BLINK_FOR,
            caps_on: false,
            caps: Spring::new(0.0, POP),
            status: Swap::new(),
            content: Spring::new(0.0, XFADE),
            details: Spring::new(0.0, SLIDE),
            details_open: false,
            card_h: Spring::new(280.0, SLIDE),
            armed: Spring::new(0.0, FADE),
            top,
            hovered: Ctl::None,
            pressed: Ctl::None,
            hover: [Spring::new(0.0, HOVER), Spring::new(0.0, HOVER), Spring::new(0.0, HOVER)],
            row_hover: Spring::new(0.0, HOVER),
            row_hover_y: Spring::new(0.0, SLIDE),
            pointer: (-1000.0, -1000.0),
            hits: vec![],
        }
    }

    fn send(&self, m: Msg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(m);
        }
    }

    fn id(&self) -> Option<u64> {
        self.info.as_ref().map(|i| i.id)
    }

    /// The field takes input. Everything else on the card is off once it's
    /// on its way out.
    fn live(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Verifying) && !self.chrome.closing
    }

    /// Up, as far as the rest of the desktop is concerned: from the moment
    /// the surface exists (before the first frame, a keystroke must not land
    /// in the app) until it starts to leave.
    fn holds_input(&self) -> bool {
        !self.chrome.closing && !matches!(self.phase, Phase::Leaving(_))
    }

    fn show(&mut self, info: Arc<Info>) {
        self.secret.clear();
        self.user = info.user.min(info.users.len().saturating_sub(1));
        self.info = Some(info);
        self.wrapped_at = 0.0;
        self.phase = Phase::Idle;
        self.wrong = false;
        self.placeholder = "Password".into();
        self.dots.clear();
        self.count = 0;
        self.typed.snap(0.0);
        self.place.snap(1.0);
        self.fail.snap(0.0);
        self.verify.snap(0.0);
        self.status.set("", DIM);
        self.details_open = false;
        self.details.snap(0.0);
        self.pressed = Ctl::None;
        self.content.snap(0.0);
        self.content.set(1.0);
        self.chrome.reopen();
        self.poke();
        self.layout_dots(true);
    }

    /// Restart the caret blink.
    fn poke(&mut self) {
        self.caret_on = true;
        self.caret_a.set(1.0);
        self.blink_next = self.chrome.time + BLINK;
        self.blink_until = self.chrome.time + BLINK_FOR;
    }

    fn alive(&self) -> usize {
        self.dots.iter().filter(|d| !d.dying).count()
    }

    fn field_w() -> f32 {
        W - PAD * 2.0
    }

    /// Dot spacing for `n` dots.
    fn spacing(&self, n: usize) -> f32 {
        let pad = 26.0 + if self.caps_on { 44.0 } else { 0.0 };
        if n == 0 { DOT_SP } else { ((Self::field_w() - pad * 2.0) / n as f32).clamp(DOT_SP_MIN, DOT_SP) }
    }

    fn caret_gap(sp: f32) -> f32 {
        sp * 0.5 + 6.0
    }

    /// Offset of dot `i` of `n`: dots and caret centred as one group.
    fn dot_x(i: usize, n: usize, sp: f32) -> f32 {
        (i as f32 - (n as f32 - 1.0) * 0.5) * sp - Self::caret_gap(sp) * 0.5
    }

    fn caret_rest(&self, n: usize) -> f32 {
        if n == 0 {
            -self.place_w * 0.5 - 7.0
        } else {
            let sp = self.spacing(n);
            (n as f32 - 1.0) * 0.5 * sp + Self::caret_gap(sp) * 0.5
        }
    }

    fn layout_dots(&mut self, snap: bool) {
        let n = self.alive();
        let sp = self.spacing(n);
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
        let c = self.caret_rest(n);
        if snap { self.caret_x.snap(c) } else { self.caret_x.set(c) }
    }

    /// Pop every dot out, right to left, `stagger` s apart.
    fn drop_dots(&mut self, stagger: f32) {
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

    fn nudge(&mut self) {
        self.shake.kick(SHAKE_KICK * 0.45);
        self.poke();
    }

    fn clear_status(&mut self) {
        if self.wrong {
            self.wrong = false;
            self.status.set("", RED);
        }
    }

    fn typed_one(&mut self) {
        self.clear_status();
        self.poke();
        let n = self.alive();
        let sp = self.spacing(n + 1);
        let x = Self::dot_x(n, n + 1, sp);
        let mut s = Spring::new(0.0, POP);
        s.set(1.0);
        self.dots.push(Dot { x: Spring::new(x, SLIDE), s, dying: false, delay: 0.0 });
        if self.alive() > MAX_DOTS
            && let Some(d) = self.dots.iter_mut().find(|d| !d.dying)
        {
            d.dying = true;
            d.s.cfg = DIE;
            d.s.set(0.0);
        }
        self.count = self.secret.chars();
        self.typed.set(1.0);
        self.layout_dots(false);
    }

    fn erased_one(&mut self) {
        self.clear_status();
        self.poke();
        self.count = self.secret.chars();
        if self.count < MAX_DOTS
            && let Some(d) = self.dots.iter_mut().rev().find(|d| !d.dying)
        {
            d.dying = true;
            d.s.cfg = DIE;
            d.s.set(0.0);
        }
        self.typed.set(if self.count > 0 { 1.0 } else { 0.0 });
        self.layout_dots(false);
    }

    fn clear_field(&mut self) {
        self.secret.clear();
        self.count = 0;
        self.poke();
        self.drop_dots(0.018);
        self.typed.set(0.0);
        self.layout_dots(false);
    }

    /// Add typed text, char by char, within what the helper can take.
    fn input(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.secret.bytes().len() + c.len_utf8() > helper::MAX_RESPONSE {
                self.nudge();
                return;
            }
            if self.secret.push(c) {
                self.typed_one();
            }
        }
    }

    fn submit(&mut self) {
        if self.phase != Phase::Idle || self.chrome.closing {
            return;
        }
        let Some(id) = self.id() else { return };
        if self.secret.is_empty() {
            self.nudge();
            return;
        }
        let secret = std::mem::take(&mut self.secret);
        self.send(Msg::Submit { id, secret });
        self.phase = Phase::Verifying;
        self.verify.set(1.0);
        self.caret_a.set(0.0);
        self.status.set("Verifying…", DIM);
        self.wrong = false;
    }

    fn cancel(&mut self) {
        if let Some(id) = self.id()
            && self.live()
        {
            self.send(Msg::Dismiss { id });
        }
        self.leave(0.0);
    }

    /// Off the screen: wiped now, gone after `hold` seconds.
    fn leave(&mut self, hold: f32) {
        self.secret.clear();
        self.count = 0;
        if hold <= 0.0 {
            self.fold();
        } else {
            self.phase = Phase::Leaving(self.chrome.time + hold);
            self.chrome.wake(self.chrome.time + hold);
        }
    }

    fn fold(&mut self) {
        self.phase = Phase::Empty;
        self.info = None;
        self.chrome.close();
    }

    fn pick(&mut self, i: usize) {
        let Some(info) = self.info.clone() else { return };
        if self.phase != Phase::Idle || i >= info.users.len() || i == self.user {
            return;
        }
        self.user = i;
        self.clear_field();
        self.clear_status();
        self.send(Msg::Pick { id: info.id, user: i });
    }

    fn cycle(&mut self, by: isize) {
        let n = self.info.as_ref().map_or(0, |i| i.users.len()) as isize;
        if n > 1 {
            self.pick((self.user as isize + by).rem_euclid(n) as usize);
        }
    }

    fn toggle_details(&mut self) {
        self.details_open = !self.details_open;
        self.details.set(if self.details_open { 1.0 } else { 0.0 });
    }

    fn apply_view(&mut self, v: &View) {
        if let View::Show(i) = v {
            return self.show(i.clone());
        }
        let id = match v {
            View::Prompt { id, .. } | View::Note { id, .. } | View::Wrong { id, .. } | View::Done { id, .. } => *id,
            View::Show(_) => return,
        };
        // A request we've already left (Escape beat the broker's answer).
        if self.id() != Some(id) {
            return;
        }
        match v {
            View::Prompt { text, again, .. } => {
                self.placeholder = text.clone();
                if *again {
                    self.phase = Phase::Idle;
                    self.verify.set(0.0);
                    self.status.set("", DIM);
                    self.drop_dots(0.012);
                    self.count = 0;
                    self.typed.set(0.0);
                    self.poke();
                    self.layout_dots(false);
                }
            }
            View::Note { text, error, .. } => {
                self.status.set(text, if *error { RED } else { DIM });
                self.wrong = false;
            }
            View::Wrong { note, fails, .. } => {
                self.phase = Phase::Idle;
                self.secret.clear();
                self.verify.set(0.0);
                self.shake.kick(SHAKE_KICK);
                self.fail.snap(1.0);
                self.fail.set(0.0);
                self.drop_dots(0.012);
                self.count = 0;
                self.typed.set(0.0);
                let msg = match note {
                    Some(n) => n.clone(),
                    None if *fails >= 3 => format!("Wrong password · {fails} attempts"),
                    None => "Wrong password".into(),
                };
                self.status.set(&msg, RED);
                // "Verifying…" is over: it doesn't get to cross-fade.
                self.status.prev.clear();
                self.wrong = true;
                self.poke();
                self.layout_dots(false);
                // The caret reappears at the placeholder once dots are gone.
                let rest = self.caret_rest(0);
                self.caret_x.snap(rest);
                self.caret_a.snap(0.0);
            }
            View::Done { ok, note, more, .. } => {
                self.verify.set(0.0);
                if *more {
                    // The next `Show` follows and takes over the card.
                    self.phase = Phase::Empty;
                    self.secret.clear();
                    return;
                }
                match note {
                    Some(n) => {
                        self.status.set(n, RED);
                        self.leave(HOLD_ERROR);
                    }
                    None if *ok => self.leave(HOLD_OK),
                    None => self.leave(0.0),
                }
            }
            View::Show(_) => {}
        }
    }

    /// Top-down positions for the current content (card coordinates).
    fn layout(&self) -> Layout {
        let lh = self.text.line_h(F_MSG);
        let msg_y = PAD + HEAD_H + 12.0;
        let who_y = msg_y + self.lines.len().max(1) as f32 * lh + 12.0;
        let n = self.info.as_ref().map_or(1, |i| i.users.len()).clamp(1, ROWS_MAX);
        let field_y = who_y + n as f32 * ROW_H + 10.0;
        let status_y = field_y + FIELD_H;
        let foot_y = status_y + STATUS_H + 4.0;
        let details_y = foot_y + BTN_H + 8.0;
        Layout { msg_y, who_y, field_y, status_y, foot_y, details_y, closed_h: foot_y + BTN_H + PAD }
    }

    /// Break the message into the card's lines (cut when drawn).
    fn wrap(&mut self, fs: &mut FontSystem) {
        if self.wrapped_at == self.scale {
            return;
        }
        self.wrapped_at = self.scale;
        let s = self.info.as_ref().map_or(String::new(), |i| i.message.clone());
        self.lines = wrap_lines(fs, &s, F_MSG, W - PAD * 2.0, self.scale, LINE_MAX);
        let p = self.info.as_ref().and_then(|i| i.program.clone()).unwrap_or_else(|| "Not given".to_string());
        self.program = wrap_lines(fs, &p, F_VALUE, W - PAD * 2.0 - VALUE_X, self.scale, PROGRAM_LINES);
        // The card keeps its top edge from now on (details open downwards).
        let h = self.layout().closed_h;
        self.top = ((self.size.1 - h) * 0.5).round().max(8.0);
        self.card_h.snap(h);
        let home = Rect::new(((self.size.0 - W) * 0.5).round(), self.top, W, h);
        self.chrome.home = home;
        self.chrome.start = Rect::new(home.x + W * 0.04, self.top + h * 0.04, W * 0.92, h * 0.92);
    }

    /// Text cut to `max` px, its line box centred on `cy`.
    fn line(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, max: f32, c: Rgba, clip: [i32; 4]) -> f32 {
        let (k, w) = self.text.get_fit(fs, s, f, max);
        if c.3 > 0.003 {
            let sc = self.scale;
            let y = cy - self.text.line_h(f) * 0.5;
            out.texts.push(TextDraw { key: k, x: (x * sc).round(), y: (y * sc).round(), color: c, clip });
        }
        w
    }

    fn centred(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, cx: f32, cy: f32, max: f32, c: Rgba, clip: [i32; 4]) {
        let w = self.text.get_fit(fs, s, f, max).1;
        self.line(fs, out, s, f, cx - w * 0.5, cy, max, c, clip);
    }

    fn draw_field(&mut self, fs: &mut FontSystem, out: &mut DrawList, r: Rect, a: f32, clip: [i32; 4]) {
        let s = self.scale;
        let t = self.chrome.time;
        let (acc_a, acc_b) = theme::accent();
        let fail = self.fail.get().clamp(0.0, 1.0);
        let v = self.verify.get().clamp(0.0, 1.0);
        let typed = self.typed.get().clamp(0.0, 1.0);
        let breath = 0.5 + 0.5 * (t * 4.0).sin();
        let sx = self.shake.get();
        let body = Rect::new(r.x + sx, r.y, r.w, r.h);
        let cy = r.y + r.h * 0.5;
        let fcx = body.x + body.w * 0.5;
        // Prism Ink: a quiet recess edged by the hairline, stronger with
        // input, breathing while verifying, red on a wrong password.
        let lit = (typed * (1.0 - fail)).max(v * (1.0 - fail));
        let k = (0.8 + 0.8 * lit * (0.8 + 0.2 * breath * v)) * (1.0 - fail);
        out.shapes.push(paint::rect(s, body, FIELD_R).fill(FIELD_FILL).prism(paint::HAIR, k).alpha(a).done());
        if fail > 0.003 {
            // A layer of its own: blending the fills would pass through grey.
            let c = RED.with_a(0.42 * fail);
            out.shapes.push(paint::rect(s, body, FIELD_R).fill(Rgba::hex(0x2a0d14, 0.9 * fail)).rim(paint::HAIR, c, c).alpha(a).done());
        }

        // Placeholder: PAM's prompt, once every dot has gone.
        let left = self.dots.iter().map(|d| d.s.get().clamp(0.0, 1.0)).fold(0.0, f32::max);
        let gone = (1.0 - left * 4.0).clamp(0.0, 1.0);
        let place = self.place.get().clamp(0.0, 1.0);
        let text = self.placeholder.clone();
        let pw = self.text.get_fit(fs, &text, F_FIELD, r.w - 60.0).1;
        if pw != self.place_w {
            self.place_w = pw;
            if self.alive() == 0 {
                self.caret_x.snap(-pw * 0.5 - 7.0);
            }
        }
        let pa = place * gone * a;
        if pa > 0.003 {
            self.centred(fs, out, &text, F_FIELD, fcx, cy, r.w - 60.0, TEXT.fade(0.36 * pa), clip);
        }

        // Dots ride the shake; a wave while verifying, red when wrong.
        let n = self.dots.len().max(1) as f32;
        for (i, d) in self.dots.iter().enumerate() {
            let sc = d.s.get().max(0.0);
            if sc <= 0.01 {
                continue;
            }
            let fi = i as f32;
            let pulse = (0.5 + 0.5 * (t * 7.0 - fi * 0.62).sin()).powi(3);
            let rd = DOT_R * sc * (1.0 + 0.5 * v * pulse);
            let grad = acc_a.mix(acc_b, if n > 1.0 { fi / (n - 1.0) } else { 0.5 });
            let col = TEXT.mix(grad, v * (0.55 + 0.45 * pulse)).mix(RED, fail * 0.9).fade(0.95 * a);
            out.shapes.push(paint::circle(s, fcx + d.x.get(), cy - 2.5 * v * pulse, rd).fill(col).done());
        }

        // Caret: only where the keyboard really is.
        let focus = if self.focused { 1.0 } else { 0.0 };
        let ca = self.caret_a.get().clamp(0.0, 1.0) * a * (1.0 - v) * focus * if self.count == 0 { gone * place } else { 1.0 };
        if ca > 0.003 {
            let x = fcx + self.caret_x.get();
            out.shapes.push(paint::rect(s, Rect::new(x - 1.1, cy - 10.0, 2.2, 20.0), 1.1).on_fill(0.0, 1.0, 0.0).alpha(ca).done());
        }

        // Caps Lock chip, right end of the field.
        let caps = self.caps.get();
        if caps > 0.01 {
            let ka = caps.clamp(0.0, 1.0) * a;
            let (cw, ch) = (40.0 * (0.6 + 0.4 * caps), 20.0 * (0.6 + 0.4 * caps));
            let ccx = body.x + body.w - 12.0 - 20.0;
            out.shapes.push(
                paint::rect(s, Rect::new(ccx - cw * 0.5, cy - ch * 0.5, cw, ch), 7.0)
                    .fill(AMBER.fade(0.13 * ka))
                    .rim(1.0, AMBER.fade(0.22 * ka), AMBER.fade(0.22 * ka))
                    .done(),
            );
            self.centred(fs, out, "CAPS", F_CAPS, ccx + 0.5, cy, 40.0, AMBER.fade(0.95 * ka), clip);
        }
    }

    fn draw_who(&mut self, fs: &mut FontSystem, out: &mut DrawList, card: Rect, y: f32, a: f32, clip: [i32; 4]) {
        let Some(info) = self.info.clone() else { return };
        let s = self.scale;
        let n = info.users.len();
        let many = n > 1;
        // The window of rows always holds the selected one.
        let first = (self.user + 1).saturating_sub(ROWS_MAX).min(n.saturating_sub(ROWS_MAX));
        let hy = self.row_hover_y.get();
        let ha = self.row_hover.get().clamp(0.0, 1.0);
        if many && ha > 0.003 {
            out.shapes.push(paint::rect(s, Rect::new(card.x + PAD - 6.0, hy, W - PAD * 2.0 + 12.0, ROW_H - 2.0), 9.0).fill(HOVER_FILL.fade(ha * a)).done());
        }
        for (row, (i, u)) in info.users.iter().enumerate().skip(first).take(ROWS_MAX).enumerate() {
            let ry = y + row as f32 * ROW_H;
            let r = Rect::new(card.x + PAD - 6.0, ry, W - PAD * 2.0 + 12.0, ROW_H - 2.0);
            let sel = i == self.user;
            if many {
                self.hits.push((r, Ctl::User(i as u8)));
                if sel {
                    out.shapes.push(paint::rect(s, r, 9.0).fill(Rgba::hex(0xffffff, 0.06 * a)).done());
                    out.shapes.push(paint::rect(s, Rect::new(r.x + 1.5, ry + 7.0, 3.0, ROW_H - 16.0), 1.5).on_fill(0.0, 1.0, 0.0).alpha(a).done());
                }
            }
            let cy = ry + (ROW_H - 2.0) * 0.5;
            let dx = card.x + PAD + 13.0;
            out.shapes.push(paint::circle(s, dx, cy, 10.0).fill(Rgba::hex(0xffffff, 0.09 * a)).done());
            let initial: String = u.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            self.centred(fs, out, &initial, F_INITIAL, dx, cy, 16.0, TEXT.fade(0.9 * a), clip);
            let tx = dx + 18.0;
            let on = !many || sel;
            let w = self.line(fs, out, &u.login, F_NAME, tx, cy, 160.0, (if on { TEXT } else { DIM }).fade(0.95 * a), clip);
            if u.name != u.login {
                self.line(fs, out, &u.name, F_FULL, tx + w + 8.0, cy, W - PAD * 2.0 - 40.0 - w, MUTED.fade(a), clip);
            }
        }
    }

    fn draw_buttons(&mut self, fs: &mut FontSystem, out: &mut DrawList, card: Rect, y: f32, a: f32, clip: [i32; 4]) {
        let s = self.scale;
        let cy = y + BTN_H * 0.5;
        let right = card.x + W - PAD;
        let (auth, cancel) = (Rect::new(right - AUTH_W, y, AUTH_W, BTN_H), Rect::new(right - AUTH_W - 8.0 - CANCEL_W, y, CANCEL_W, BTN_H));
        let k = self.armed.get().clamp(0.0, 1.0);
        let lift = 0.10 * self.hover[1].get().clamp(0.0, 1.0);
        out.shapes.push(paint::rect(s, auth, BTN_R).on_mix(paint::FAINT, k, paint::IRID.0, paint::IRID.1, lift).alpha(a).done());
        self.centred(fs, out, "Authenticate", F_BTN, auth.x + AUTH_W * 0.5, cy, AUTH_W - 12.0, MUTED.mix(theme::on_ink(), k).with_a(1.0).fade(a), clip);
        let h = self.hover[0].get().clamp(0.0, 1.0);
        out.shapes.push(paint::rect(s, cancel, BTN_R).fill(paint::FAINT.fade(0.5 + 0.5 * h)).alpha(a).done());
        self.centred(fs, out, "Cancel", F_BTN, cancel.x + CANCEL_W * 0.5, cy, CANCEL_W - 12.0, TEXT.fade(0.92 * a), clip);
        let dr = Rect::new(card.x + PAD - 6.0, y + 4.0, DETAILS_W + 12.0, BTN_H - 8.0);
        let dh = self.hover[2].get().clamp(0.0, 1.0);
        if dh > 0.003 {
            out.shapes.push(paint::rect(s, dr, 9.0).fill(HOVER_FILL.fade(dh * a)).done());
        }
        let w = self.line(fs, out, "Details", F_LINK, card.x + PAD, cy, DETAILS_W, MUTED.mix(TEXT, dh * 0.8).fade(a), clip);
        // A chevron that turns: two strokes, redrawn per the spring.
        let t = self.details.get().clamp(0.0, 1.0);
        let (cx, c) = (card.x + PAD + w + 10.0, MUTED.mix(TEXT, dh * 0.8).fade(a));
        let id = self.icons.id(&["go-next-symbolic"]);
        if t < 0.5 {
            out.icons.push(IconDraw { id, x: (cx - 5.0) * s, y: (cy - 5.0) * s, size: (10.0 * s).round(), color: c.fade(1.0 - t * 2.0) });
        }
        if t > 0.0 {
            let w = 4.0 + 3.0 * t;
            out.shapes.push(paint::rect(s, Rect::new(cx - w * 0.5, cy - 0.75, w, 1.5), 0.75).fill(c.fade(t)).done());
        }
        self.hits.push((auth, Ctl::Auth));
        self.hits.push((cancel, Ctl::Cancel));
        self.hits.push((dr, Ctl::Details));
    }

    fn draw_details(&mut self, fs: &mut FontSystem, out: &mut DrawList, card: Rect, y: f32, a: f32, clip: [i32; 4]) {
        let Some(info) = self.info.clone() else { return };
        let s = self.scale;
        let t = self.details.get().clamp(0.0, 1.0);
        let da = t * t * a;
        if da <= 0.003 {
            return;
        }
        paint::divider(&mut out.shapes, s, card.x + PAD, y, W - PAD * 2.0, da);
        let (lx, vx, vw) = (card.x + PAD, card.x + PAD + VALUE_X, W - PAD * 2.0 - VALUE_X);
        let cy = y + 20.0;
        self.line(fs, out, "Action", F_LABEL, lx, cy, 58.0, MUTED.fade(da), clip);
        self.line(fs, out, &info.action, F_VALUE, vx, cy, vw, DIM.fade(da), clip);
        let cy = cy + 22.0;
        self.line(fs, out, "Program", F_LABEL, lx, cy, 58.0, MUTED.fade(da), clip);
        let lh = self.text.line_h(F_VALUE);
        for (i, line) in self.program.clone().iter().enumerate() {
            self.line(fs, out, line, F_VALUE, vx, cy + i as f32 * lh, vw, DIM.fade(da), clip);
        }
    }

    fn set_hover(&mut self) {
        let (x, y) = self.pointer;
        self.hovered = self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Ctl::None, |(_, c)| *c);
    }
}

/// `s` broken into at most `max_lines` lines of `max` px; the last holds the
/// rest and is cut when drawn.
fn wrap_lines(fs: &mut FontSystem, s: &str, f: Font, max: f32, scale: f32, max_lines: usize) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    let mut b = Buffer::new(fs, Metrics::new(f.size * scale, (f.size * 1.3).round() * scale));
    b.set_wrap(Wrap::WordOrGlyph);
    b.set_size(Some(max * scale), None);
    let attrs = Attrs::new().family(Family::Name(f.family.unwrap_or("Geist"))).weight(Weight(f.weight));
    b.set_text(s, &attrs, Shaping::Advanced, None);
    b.shape_until_scroll(fs, false);
    let mut out: Vec<String> = vec![];
    let mut at = 0;
    for run in b.layout_runs() {
        let end = run.glyphs.iter().map(|g| g.end).max().unwrap_or(at).clamp(at, s.len());
        if !s.is_char_boundary(end) {
            continue;
        }
        if out.len() + 1 >= max_lines {
            break;
        }
        out.push(s[at..end].trim().to_string());
        at = end;
    }
    let rest = s[at..].trim();
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

impl Drop for Prompt {
    fn drop(&mut self) {
        // The surface went away under a live request (output unplugged,
        // layer closed): nobody can answer it, so it can't stay pending.
        if let Some(id) = self.id() {
            self.send(Msg::Gone { id });
        }
    }
}

impl WinScene for Prompt {
    fn size(&self) -> (f32, f32) {
        self.size
    }
    fn placement(&self) -> Placement {
        Placement::Center
    }
    /// The moment it's on its way out the keys go back to the apps.
    fn keyboard(&self) -> Keyboard {
        if self.holds_input() { Keyboard::Exclusive } else { Keyboard::None }
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }
    fn on_fact(&mut self, f: &Fact) -> bool {
        let Fact::Custom(a) = f else { return false };
        if let Some(v) = a.downcast_ref::<View>() {
            self.apply_view(v);
            return true;
        }
        match a.downcast_ref::<Hint>() {
            Some(Hint::Focus(on)) => {
                self.focused = *on;
                self.poke();
                true
            }
            Some(Hint::Caps(on)) => {
                if *on != self.caps_on {
                    self.caps_on = *on;
                    self.caps.set(if *on { 1.0 } else { 0.0 });
                    self.layout_dots(false);
                }
                true
            }
            None => false,
        }
    }
    fn set_scale(&mut self, s: f32) {
        if s != self.scale {
            self.scale = s;
            self.text.set_scale(s);
        }
    }
    fn step(&mut self, dt: f32) -> bool {
        let mut m = self.chrome.step(dt);
        let t = self.chrome.time;
        if let Phase::Leaving(at) = self.phase {
            if t >= at {
                self.fold();
                m = true;
            } else {
                self.chrome.wake(at);
            }
        }
        for d in &mut self.dots {
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
        self.dots.retain(|d| !(d.dying && d.delay <= 0.0 && d.s.at_rest()));
        m |= before != self.dots.len();
        self.place.set(if self.count == 0 && self.dots.is_empty() && self.phase == Phase::Idle { 1.0 } else { 0.0 });
        self.armed.set(if self.phase == Phase::Idle && self.count > 0 { 1.0 } else { 0.0 });
        let extra = self.program.len().saturating_sub(1) as f32 * self.text.line_h(F_VALUE);
        let h = self.layout().closed_h + if self.details_open { DETAILS_H + extra } else { 0.0 };
        if self.wrapped_at != 0.0 {
            self.card_h.set(h);
        }
        for s in [
            &mut self.shake,
            &mut self.fail,
            &mut self.verify,
            &mut self.typed,
            &mut self.place,
            &mut self.caret_x,
            &mut self.caps,
            &mut self.content,
            &mut self.details,
            &mut self.card_h,
            &mut self.armed,
            &mut self.row_hover,
            &mut self.row_hover_y,
        ] {
            m |= s.step(dt);
        }
        m |= self.status.t.step(dt);
        let hover = [self.hovered == Ctl::Cancel, self.hovered == Ctl::Auth, self.hovered == Ctl::Details];
        for (s, on) in self.hover.iter_mut().zip(hover) {
            s.set(if on { 1.0 } else { 0.0 });
            m |= s.step(dt);
        }
        // Blink on a timer, eased by a fast spring; no frames between flips
        // and none at all once the user has gone quiet.
        if self.phase == Phase::Idle && self.focused {
            if t < self.blink_until {
                while t >= self.blink_next {
                    self.caret_on = !self.caret_on;
                    self.blink_next += BLINK;
                }
                self.chrome.wake(self.blink_next);
            } else {
                self.caret_on = true;
            }
            self.caret_a.set(if self.caret_on { 1.0 } else { 0.0 });
        }
        m |= self.caret_a.step(dt);
        m |= self.phase == Phase::Verifying;
        m
    }
    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        self.wrap(fs);
        let s = self.scale;
        let (sw, sh) = self.size;
        let a = self.chrome.openness().clamp(0.0, 1.0);

        // Soft dim over the whole output.
        out.shapes.push(paint::rect(s, Rect::new(0.0, 0.0, sw, sh), 0.0).fill(Rgba::hex(0x000000, SCRIM * a)).done());

        let h = self.card_h.get();
        let mut home = self.chrome.home;
        home.h = h;
        self.chrome.home = home;
        let r = self.chrome.morph();
        self.chrome.body = r;
        let card = paint::ink(s, r, RADIUS).shadow(Rgba::hex(0x000000, 0.5), 26.0, 12.0).spot(self.pointer.0, self.pointer.1, 170.0, 0.05 * a).alpha(a);
        out.shapes.push(card.done());
        out.glass(r, RADIUS, a);

        let ca = a * a * self.content.get().clamp(0.0, 1.0);
        if ca > 0.003 && self.info.is_some() {
            let clip = [(r.x * s) as i32, (r.y * s) as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32];
            let l = self.layout();
            let (x, y) = (home.x, home.y);

            // Header.
            let cy = y + PAD + HEAD_H * 0.5;
            let cx = x + PAD + HEAD_H * 0.5;
            out.shapes.push(paint::circle(s, cx, cy, HEAD_H * 0.5).fill(Rgba::hex(0xffffff, 0.07 * ca)).done());
            let id = self.icons.id(&["system-lock-screen-symbolic"]);
            out.icons.push(IconDraw { id, x: (cx - 8.0) * s, y: (cy - 8.0) * s, size: (16.0 * s).round(), color: TEXT.fade(0.9 * ca) });
            self.line(fs, out, "Authentication required", F_TITLE, x + PAD + HEAD_H + 12.0, cy, W - PAD * 2.0 - HEAD_H - 12.0, TEXT.fade(ca), clip);

            // polkit's message, line by line; only the last can be cut.
            let lh = self.text.line_h(F_MSG);
            for (i, line) in self.lines.clone().iter().enumerate() {
                self.line(fs, out, line, F_MSG, x + PAD, y + l.msg_y + (i as f32 + 0.5) * lh, W - PAD * 2.0, DIM.fade(ca), clip);
            }

            self.draw_who(fs, out, home, y + l.who_y, ca, clip);

            let field = Rect::new(x + PAD, y + l.field_y, W - PAD * 2.0, FIELD_H);
            self.hits.push((field, Ctl::Field));
            self.draw_field(fs, out, field, ca, clip);

            // Status under the field, cross-faded in place.
            let st_cy = y + l.status_y + STATUS_H * 0.5 + 1.0;
            let (cur, prev) = (std::mem::take(&mut self.status.cur), std::mem::take(&mut self.status.prev));
            let p = self.status.t.get().clamp(0.0, 1.0);
            if p < 1.0 && !prev.is_empty() {
                self.centred(fs, out, &prev, F_STATUS, x + W * 0.5, st_cy, W - PAD * 2.0, self.status.prev_c.fade(ca * (1.0 - (p * 2.0).min(1.0))), clip);
            }
            if !cur.is_empty() {
                self.centred(fs, out, &cur, F_STATUS, x + W * 0.5, st_cy, W - PAD * 2.0, self.status.cur_c.fade(ca * p), clip);
            }
            self.status.cur = cur;
            self.status.prev = prev;

            self.draw_buttons(fs, out, home, y + l.foot_y, ca, clip);
            self.draw_details(fs, out, home, y + l.details_y, ca, clip);
        }
        self.text.end_frame();
    }
    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }
    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pointer = (x, y);
        self.set_hover();
        if let Ctl::User(_) = self.hovered {
            if let Some((r, _)) = self.hits.iter().find(|(_, c)| *c == self.hovered) {
                if self.row_hover.get() < 0.02 {
                    self.row_hover_y.snap(r.y);
                } else {
                    self.row_hover_y.set(r.y);
                }
            }
            self.row_hover.set(1.0);
        } else {
            self.row_hover.set(0.0);
        }
        None
    }
    fn pointer_leave(&mut self) {
        self.pointer = (-1000.0, -1000.0);
        self.hovered = Ctl::None;
        self.row_hover.set(0.0);
    }
    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pointer = (x, y);
        self.set_hover();
        self.pressed = self.hovered;
        None
    }
    fn release(&mut self) -> (Option<Action>, bool) {
        let c = std::mem::replace(&mut self.pressed, Ctl::None);
        if c == self.hovered && self.live() {
            match c {
                Ctl::Cancel => self.cancel(),
                Ctl::Auth => self.submit(),
                Ctl::Details => self.toggle_details(),
                Ctl::User(i) => self.pick(i as usize),
                Ctl::Field | Ctl::None => {}
            }
        }
        (None, false)
    }
    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if !self.live() {
            return (None, false);
        }
        let busy = self.phase == Phase::Verifying;
        match k.sym {
            keys::ESCAPE => self.cancel(),
            keys::RETURN | keys::KP_ENTER => self.submit(),
            _ if busy => {}
            keys::TAB if k.shift => self.cycle(-1),
            keys::TAB | keys::DOWN => self.cycle(1),
            keys::UP => self.cycle(-1),
            keys::BACKSPACE if k.ctrl => self.clear_field(),
            0x75 | 0x55 if k.ctrl => self.clear_field(),
            keys::BACKSPACE => {
                if self.secret.pop() {
                    self.erased_one();
                }
            }
            _ if k.ctrl || k.alt => {}
            _ => match k.text.as_deref() {
                Some(t) if !t.is_empty() => self.input(t),
                _ => {
                    if let Some(c) = Keysym::from(k.sym).key_char() {
                        self.input(c.encode_utf8(&mut [0; 4]));
                    }
                }
            },
        }
        (None, false)
    }
    /// A held key may repeat characters and Backspace. Enter and Escape
    /// must be deliberate.
    fn key_repeat(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if matches!(k.sym, keys::ESCAPE | keys::RETURN | keys::KP_ENTER | keys::TAB | keys::UP | keys::DOWN) {
            return (None, false);
        }
        self.key(k)
    }
    fn cursor(&self) -> Cursor {
        match self.hovered {
            Ctl::Field => Cursor::Text,
            Ctl::Cancel | Ctl::Details | Ctl::User(_) => Cursor::Pointer,
            Ctl::Auth if self.phase == Phase::Idle && self.count > 0 => Cursor::Pointer,
            _ => Cursor::Default,
        }
    }
    /// The whole output while up, so a stray click can't land on an app or
    /// press through; nothing once it's leaving.
    fn input_region(&self) -> Vec<Rect> {
        if self.holds_input() { vec![Rect::new(0.0, 0.0, self.size.0, self.size.1)] } else { vec![] }
    }
    /// The host wants the window gone: treat it as Cancel, never as a
    /// silent close that would leave the request pending.
    fn close(&mut self) {
        self.cancel();
    }
    fn reopen(&mut self) {
        self.chrome.reopen();
    }
}

/// Headless review of every state. No broker exists in this mode (`tx` is
/// None), so what's typed goes nowhere and is wiped on drop.
pub fn demo(d: &mut crate::snapshot::Demo) {
    use super::broker::Who;
    assert!(BROKER.get().is_none(), "the snapshot rig must not run with an agent");
    let me = Who { uid: 1000, login: "drizzy".into(), name: "Ada Lovelace".into() };
    let info = |id, users: Vec<Who>| {
        Arc::new(Info {
            id,
            message: "Authentication is needed to run `/run/current-system/sw/bin/true' as the super user".into(),
            action: "org.freedesktop.policykit.exec".into(),
            program: Some("/run/current-system/sw/bin/true".into()),
            users,
            user: 0,
        })
    };
    let key = |text: &str| KeyInput { sym: 0x61, text: Some(text.into()), ctrl: false, shift: false, alt: false };
    let sym = |sym: u32| KeyInput { sym, text: None, ctrl: false, shift: false, alt: false };
    let shoot = |d: &mut crate::snapshot::Demo, p: &mut Prompt, name: &str| {
        d.shoot(p, name);
        let (b, s) = (p.chrome.body, d.scale);
        let _ = std::fs::write(d.dir.join(name).with_extension("card"), format!("{} {} {} {} {}\n", b.x * s, b.y * s, b.w * s, b.h * s, RADIUS * s));
    };
    let new = |d: &mut crate::snapshot::Demo, users: Vec<Who>| {
        let mut ctx = d.ctx();
        let mut p = Prompt::new(&mut ctx, None);
        p.apply_view(&View::Show(info(1, users)));
        p.apply_view(&View::Prompt { id: 1, text: "Password".into(), again: false });
        p
    };

    let mut p = new(d, vec![me.clone()]);
    for i in 0..24 {
        if i % 4 == 0 {
            shoot(d, &mut p, &format!("polkit_open_{i:03}"));
        }
        d.tick(&mut p, 1.0 / 120.0);
    }
    d.tick(&mut p, 0.8);
    shoot(d, &mut p, "polkit_idle");

    for c in "correct horse".chars() {
        p.key(&key(c.encode_utf8(&mut [0; 4])));
        d.tick(&mut p, 0.05);
    }
    d.tick(&mut p, 0.5);
    shoot(d, &mut p, "polkit_typing");

    p.key(&sym(keys::RETURN));
    d.tick(&mut p, 0.45);
    shoot(d, &mut p, "polkit_verifying");

    p.apply_view(&View::Wrong { id: 1, note: None, fails: 1 });
    d.tick(&mut p, 0.05);
    shoot(d, &mut p, "polkit_wrong_shake");
    d.tick(&mut p, 0.6);
    shoot(d, &mut p, "polkit_wrong");

    p.toggle_details();
    d.tick(&mut p, 0.7);
    shoot(d, &mut p, "polkit_details");
    p.toggle_details();

    p.apply_view(&View::Done { id: 1, ok: false, note: None, more: false });
    for i in 0..24 {
        if i % 6 == 0 {
            shoot(d, &mut p, &format!("polkit_close_{i:03}"));
        }
        d.tick(&mut p, 1.0 / 120.0);
    }
    let fold = format!("folded after 0.2 s: {}\n", p.chrome.done());
    d.tick(&mut p, 1.0);
    let closed = format!("closed at rest: done={} animating={}\n", p.chrome.done(), p.step(1.0 / 120.0));

    // Three admins, a long command, and the second one chosen.
    let users = vec![
        Who { uid: 0, login: "root".into(), name: "root".into() },
        me.clone(),
        Who { uid: 1001, login: "alice".into(), name: "Alice Lidell".into() },
    ];
    let mut p = new(d, users);
    p.apply_view(&View::Show(Arc::new(Info {
        id: 2,
        message: "Authentication is needed to run `/nix/store/ab12cd34-some-long-tool/bin/do-something-rather ... --with-many-options=true' as the super user, because a package asked for it and it will need root for a while".into(),
        action: "org.freedesktop.policykit.exec".into(),
        program: Some("/nix/store/ab12cd34ef56gh78ij90kl12mn34op56-some-long-tool-1.2.3/bin/do-something-rather-long".into()),
        users: vec![
            Who { uid: 0, login: "root".into(), name: "root".into() },
            me.clone(),
            Who { uid: 1001, login: "alice".into(), name: "Alice Lidell".into() },
        ],
        user: 1,
    })));
    d.tick(&mut p, 0.8);
    if let Some((r, _)) = p.hits.iter().find(|(_, c)| *c == Ctl::User(2)) {
        let (x, y) = (r.x + 40.0, r.y + 10.0);
        p.pointer_motion(x, y);
    }
    d.tick(&mut p, 0.4);
    shoot(d, &mut p, "polkit_picker");
    p.toggle_details();
    d.tick(&mut p, 0.7);
    shoot(d, &mut p, "polkit_picker_details");

    // PAM speaking up, with Caps Lock on.
    let mut p = new(d, vec![me]);
    d.tick(&mut p, 0.6);
    p.on_fact(&caps_fact(true));
    p.apply_view(&View::Note { id: 1, text: "Your account expires in 3 days".into(), error: false });
    d.tick(&mut p, 0.6);
    shoot(d, &mut p, "polkit_note_caps");

    // At rest: the caret stops once the user has gone quiet.
    d.tick(&mut p, BLINK_FOR + 1.0);
    let mut l = DrawList::default();
    p.draw(&mut d.gpu.fonts, &mut l);
    p.chrome.wake_at = None;
    let moving = p.step(1.0 / 120.0);
    let rest = format!("open at rest: animating={moving} wake={:?}\n", p.chrome.wake_at);
    let _ = std::fs::write(d.dir.join("polkit_rest.txt"), format!("{fold}{closed}{rest}"));
    // A wrong password's message must not hold the window open either.
    p.apply_view(&View::Done { id: 1, ok: false, note: Some("Authentication is unavailable".into()), more: false });
    d.tick(&mut p, 0.5);
    shoot(d, &mut p, "polkit_unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bar::Rect,
        polkit::broker::{Info, Who},
        services::Fact,
    };

    fn prompt() -> (Prompt, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        let (ft, _fr) = calloop::channel::channel::<Fact>();
        let mut fs = FontSystem::new();
        let icons = Rc::new(IconSet::discover());
        let mut ctx = Ctx { fs: &mut fs, icons, scale: 1.0, facts: &[], out_w: 1600.0, out_h: 1000.0, tabs: [Rect::new(0.0, 0.0, 0.0, 0.0); 3], fact_tx: ft };
        (Prompt::new(&mut ctx, Some(tx)), rx)
    }

    fn who(uid: u32, login: &str) -> Who {
        Who { uid, login: login.into(), name: login.into() }
    }

    fn info(id: u64, users: Vec<Who>) -> Arc<Info> {
        Arc::new(Info { id, message: "Run a thing".into(), action: "org.test.run".into(), program: None, user: 0, users })
    }

    fn shown() -> (Prompt, mpsc::Receiver<Msg>) {
        let (mut p, rx) = prompt();
        p.apply_view(&View::Show(info(1, vec![who(1000, "alice")])));
        (p, rx)
    }

    fn typed(p: &mut Prompt, s: &str) {
        for c in s.chars() {
            p.key(&KeyInput { sym: c as u32, text: Some(c.to_string()), ctrl: false, shift: false, alt: false });
        }
    }

    fn sym(p: &mut Prompt, sym: u32) {
        p.key(&KeyInput { sym, text: None, ctrl: false, shift: false, alt: false });
    }

    #[test]
    fn enter_hands_over_the_password_and_keeps_no_copy() {
        let (mut p, rx) = shown();
        typed(&mut p, "pässword");
        assert_eq!(p.count, 8);
        sym(&mut p, keys::RETURN);
        let Ok(Msg::Submit { id, secret }) = rx.try_recv() else { panic!("no submit") };
        assert_eq!(id, 1);
        assert_eq!(secret.bytes(), "pässword".as_bytes());
        assert!(p.secret.is_empty(), "the field holds a fresh buffer");
        assert_eq!(p.phase, Phase::Verifying);
    }

    #[test]
    fn enter_on_empty_sends_nothing() {
        let (mut p, rx) = shown();
        sym(&mut p, keys::RETURN);
        assert!(rx.try_recv().is_err());
        assert_eq!(p.phase, Phase::Idle);
    }

    #[test]
    fn a_second_enter_while_verifying_sends_nothing() {
        let (mut p, rx) = shown();
        typed(&mut p, "x");
        sym(&mut p, keys::RETURN);
        rx.try_recv().unwrap();
        sym(&mut p, keys::RETURN);
        typed(&mut p, "y");
        assert!(rx.try_recv().is_err());
        assert!(p.secret.is_empty(), "typing while verifying goes nowhere");
    }

    #[test]
    fn escape_cancels_and_wipes_at_once() {
        let (mut p, rx) = shown();
        typed(&mut p, "hunter2");
        let raw = p.secret.raw();
        assert_eq!(unsafe { std::slice::from_raw_parts(raw, 7) }, b"hunter2");
        sym(&mut p, keys::ESCAPE);
        assert_eq!(unsafe { std::slice::from_raw_parts(raw, 7) }, &[0u8; 7], "wiped when it closed, not when it finished folding");
        assert!(matches!(rx.try_recv(), Ok(Msg::Dismiss { id: 1 })));
        assert!(p.chrome.closing);
        assert_eq!(p.keyboard(), Keyboard::None);
        assert!(p.input_region().is_empty(), "clicks go back to the apps");
    }

    #[test]
    fn it_holds_the_keyboard_before_the_first_request_is_applied() {
        let (p, _rx) = prompt();
        assert_eq!(p.keyboard(), Keyboard::Exclusive);
        assert_eq!(p.input_region().len(), 1);
    }

    #[test]
    fn an_error_it_reports_gives_the_desktop_back_while_it_shows() {
        let (mut p, _rx) = shown();
        typed(&mut p, "abc");
        p.apply_view(&View::Done { id: 1, ok: false, note: Some("Authentication is unavailable".into()), more: false });
        assert!(p.secret.is_empty());
        assert_eq!(p.keyboard(), Keyboard::None);
        assert!(p.input_region().is_empty());
        assert!(!p.chrome.closing, "still showing the message");
        sym(&mut p, keys::RETURN);
        typed(&mut p, "x");
        assert!(p.secret.is_empty());
        for _ in 0..240 {
            p.step(1.0 / 120.0);
        }
        assert!(p.chrome.closing, "it folds by itself");
    }

    #[test]
    fn escape_while_verifying_still_cancels() {
        let (mut p, rx) = shown();
        typed(&mut p, "x");
        sym(&mut p, keys::RETURN);
        rx.try_recv().unwrap();
        sym(&mut p, keys::ESCAPE);
        assert!(matches!(rx.try_recv(), Ok(Msg::Dismiss { .. })));
    }

    #[test]
    fn control_characters_never_reach_the_password() {
        let (mut p, _rx) = shown();
        typed(&mut p, "a\nb\rc\td\u{0}e\u{1b}f");
        assert_eq!(p.secret.bytes(), b"abcdef");
    }

    #[test]
    fn the_field_stops_at_what_the_helper_can_take() {
        let (mut p, _rx) = shown();
        typed(&mut p, &"x".repeat(helper::MAX_RESPONSE + 50));
        assert_eq!(p.secret.bytes().len(), helper::MAX_RESPONSE);
        // A multi-byte character doesn't straddle the limit.
        let (mut p, _rx) = shown();
        typed(&mut p, &"x".repeat(helper::MAX_RESPONSE - 1));
        typed(&mut p, "é");
        assert_eq!(p.secret.bytes().len(), helper::MAX_RESPONSE - 1);
    }

    #[test]
    fn backspace_and_kill_edit_in_place() {
        let (mut p, _rx) = shown();
        typed(&mut p, "abc");
        sym(&mut p, keys::BACKSPACE);
        assert_eq!(p.secret.bytes(), b"ab");
        assert_eq!(p.count, 2);
        p.key(&KeyInput { sym: keys::BACKSPACE, text: None, ctrl: true, shift: false, alt: false });
        assert!(p.secret.is_empty());
        typed(&mut p, "abc");
        p.key(&KeyInput { sym: 0x75, text: Some("\u{15}".into()), ctrl: true, shift: false, alt: false });
        assert!(p.secret.is_empty());
        assert_eq!(p.count, 0);
    }

    #[test]
    fn modified_keys_type_nothing() {
        let (mut p, _rx) = shown();
        p.key(&KeyInput { sym: 0x76, text: Some("v".into()), ctrl: true, shift: false, alt: false });
        p.key(&KeyInput { sym: 0x76, text: Some("v".into()), ctrl: false, shift: false, alt: true });
        assert!(p.secret.is_empty());
    }

    #[test]
    fn wrong_password_clears_and_lets_the_user_retry() {
        let (mut p, rx) = shown();
        typed(&mut p, "nope");
        sym(&mut p, keys::RETURN);
        rx.try_recv().unwrap();
        p.apply_view(&View::Wrong { id: 1, note: None, fails: 1 });
        assert_eq!(p.phase, Phase::Idle);
        assert_eq!(p.status.cur, "Wrong password");
        assert!(p.shake.vel != 0.0);
        assert!(p.dots.iter().all(|d| d.dying));
        // The next keystroke clears the message and types.
        typed(&mut p, "y");
        assert_eq!(p.status.cur, "");
        assert_eq!(p.count, 1);
        sym(&mut p, keys::RETURN);
        assert!(matches!(rx.try_recv(), Ok(Msg::Submit { .. })));
    }

    #[test]
    fn cancel_authentication_closes_without_a_trace() {
        let (mut p, rx) = shown();
        typed(&mut p, "half a pass");
        p.apply_view(&View::Done { id: 1, ok: false, note: None, more: false });
        assert!(p.secret.is_empty());
        assert!(p.chrome.closing);
        assert_eq!(p.info, None);
        drop(p);
        assert!(rx.try_recv().is_err(), "the broker already knows: no Gone");
    }

    #[test]
    fn views_for_another_request_change_nothing() {
        let (mut p, _rx) = shown();
        typed(&mut p, "abc");
        p.apply_view(&View::Done { id: 9, ok: true, note: None, more: false });
        p.apply_view(&View::Wrong { id: 9, note: None, fails: 1 });
        assert_eq!(p.count, 3);
        assert!(!p.chrome.closing);
    }

    #[test]
    fn the_next_request_takes_over_the_card() {
        let (mut p, _rx) = shown();
        typed(&mut p, "for the first");
        p.apply_view(&View::Done { id: 1, ok: true, note: None, more: true });
        assert!(p.secret.is_empty(), "a password can't carry over to the next request");
        p.apply_view(&View::Show(info(2, vec![who(1000, "alice")])));
        assert!(!p.chrome.closing);
        assert_eq!(p.id(), Some(2));
        assert_eq!(p.count, 0);
        assert_eq!(p.keyboard(), Keyboard::Exclusive);
    }

    #[test]
    fn a_request_arriving_during_the_fold_reopens_it() {
        let (mut p, _rx) = shown();
        sym(&mut p, keys::ESCAPE);
        assert!(p.chrome.closing);
        p.apply_view(&View::Show(info(2, vec![who(1000, "alice")])));
        assert!(!p.chrome.closing);
        assert_eq!(p.keyboard(), Keyboard::Exclusive);
    }

    #[test]
    fn losing_the_window_cancels_a_live_request() {
        let (p, rx) = shown();
        drop(p);
        assert!(matches!(rx.try_recv(), Ok(Msg::Gone { id: 1 })));
    }

    #[test]
    fn the_host_closing_it_is_a_cancel() {
        let (mut p, rx) = shown();
        WinScene::close(&mut p);
        assert!(matches!(rx.try_recv(), Ok(Msg::Dismiss { id: 1 })));
    }

    #[test]
    fn repeats_of_enter_and_escape_are_ignored() {
        let (mut p, rx) = shown();
        typed(&mut p, "x");
        for s in [keys::RETURN, keys::KP_ENTER, keys::ESCAPE] {
            p.key_repeat(&KeyInput { sym: s, text: None, ctrl: false, shift: false, alt: false });
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(p.phase, Phase::Idle);
        p.key_repeat(&KeyInput { sym: 0x78, text: Some("x".into()), ctrl: false, shift: false, alt: false });
        p.key_repeat(&KeyInput { sym: keys::BACKSPACE, text: None, ctrl: false, shift: false, alt: false });
        assert_eq!(p.count, 1);
    }

    #[test]
    fn picking_clears_the_field_and_tells_the_broker() {
        let (mut p, rx) = prompt();
        p.apply_view(&View::Show(info(1, vec![who(0, "root"), who(1000, "alice"), who(1001, "bob")])));
        typed(&mut p, "for root");
        sym(&mut p, keys::TAB);
        assert_eq!(p.user, 1);
        assert!(p.secret.is_empty(), "one user's password can't go to another");
        assert!(matches!(rx.try_recv(), Ok(Msg::Pick { id: 1, user: 1 })));
        p.key(&KeyInput { sym: keys::TAB, text: None, ctrl: false, shift: true, alt: false });
        assert!(matches!(rx.try_recv(), Ok(Msg::Pick { id: 1, user: 0 })));
        sym(&mut p, keys::UP);
        assert!(matches!(rx.try_recv(), Ok(Msg::Pick { id: 1, user: 2 })));
    }

    #[test]
    fn hints_and_foreign_facts() {
        let (mut p, _rx) = shown();
        assert!(!p.on_fact(&Fact::Custom(Arc::new(42u32))), "another module's payload: no redraw");
        assert!(p.on_fact(&caps_fact(true)));
        assert!(p.caps_on);
        assert!(p.on_fact(&focus_fact(false)));
        assert!(!p.focused);
        assert!(wants_window(&Fact::Custom(Arc::new(View::Show(info(1, vec![]))))));
        assert!(!wants_window(&Fact::Custom(Arc::new(View::Done { id: 1, ok: true, note: None, more: false }))));
    }

    #[test]
    fn wrapping_keeps_the_text_and_caps_the_lines() {
        let mut fs = FontSystem::new();
        let s = "word ".repeat(100);
        let lines = wrap_lines(&mut fs, s.trim(), F_MSG, 200.0, 1.0, LINE_MAX);
        assert!(lines.len() <= LINE_MAX);
        assert!(!lines.is_empty());
        assert!(wrap_lines(&mut fs, "", F_MSG, 200.0, 1.0, 4).is_empty());
        assert_eq!(wrap_lines(&mut fs, "short", F_MSG, 400.0, 1.0, 4), ["short"]);
    }

    #[test]
    fn everything_shown_uses_a_face_that_is_always_loaded() {
        // A lazily loaded family renders blanks until it arrives.
        for f in [F_TITLE, F_MSG, F_NAME, F_FULL, F_INITIAL, F_FIELD, F_STATUS, F_BTN, F_LINK, F_CAPS, F_LABEL, F_VALUE] {
            assert_eq!(f.family, Some("Geist"));
        }
    }

    #[test]
    fn a_long_program_keeps_its_name() {
        let mut fs = FontSystem::new();
        let p = "/nix/store/ab12cd34ef56gh78ij90kl12mn34op56-some-long-tool-1.2.3/bin/do-something-rather-long";
        let lines = wrap_lines(&mut fs, p, F_VALUE, W - PAD * 2.0 - VALUE_X, 1.75, PROGRAM_LINES);
        assert!(lines.len() <= PROGRAM_LINES);
        assert!(lines.last().unwrap().ends_with("rather-long"), "{lines:?}");
    }

    #[test]
    fn wipe_text_overwrites_in_place() {
        let s = String::from("k");
        let p = s.as_ptr();
        wipe_text(Some(s));
        let _ = p; // Freed: nothing to read safely. Exercised for the path.
    }
}
