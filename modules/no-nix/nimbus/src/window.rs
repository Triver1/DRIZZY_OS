//! Generic floating windows (launcher, process monitor, wallpapers,
//! notifications). Each lives on its own short-lived layer surface hosted
//! by main.rs; a module provides a `WinScene` and composes `Chrome` for
//! the shared behaviour:
//!
//! - open/close: the body morphs from a start rect (a tab, the screen
//!   centre, …) to its home rect, and back on close;
//! - dragging: relative pointer motion moves the surface by whole pixels
//!   (layer margins) and the scene draws the fractional rest; release
//!   flings with momentum and settles on a spring;
//! - time, wake-ups and the input region.
//!
//! Coordinates are logical px in the surface's own space. Scenes push
//! shapes/images/text into a `DrawList` and hit rects into their own list.

use std::{collections::HashMap, rc::Rc};

use glyphon::FontSystem;

use crate::{
    anim::{Spring, SpringCfg},
    bar::Rect,
    gfx::DrawList,
    icons::IconSet,
    services::Fact,
    settings::Action,
};

/// One key press (or repeat) for the focused window.
#[derive(Clone, Debug)]
pub struct KeyInput {
    /// Raw xkb keysym (see `xkeysym`/`Keysym` constants).
    pub sym: u32,
    /// Text this key produces with the current layout/modifiers, if any.
    pub text: Option<String>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

pub mod keys {
    pub const ESCAPE: u32 = 0xff1b;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const BACKSPACE: u32 = 0xff08;
    pub const DELETE: u32 = 0xffff;
    pub const TAB: u32 = 0xff09;
    pub const LEFT: u32 = 0xff51;
    pub const UP: u32 = 0xff52;
    pub const RIGHT: u32 = 0xff53;
    pub const DOWN: u32 = 0xff54;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cursor {
    Default,
    Pointer,
    Grab,
    Grabbing,
    Text,
}

/// Where a window's surface sits on the output (anchored top-left; these
/// give its home margins).
#[derive(Clone, Copy, Debug)]
pub enum Placement {
    /// Centred on the output.
    Center,
    /// Horizontally centred, `y` px from the top.
    Top(f32),
    /// Flush right with `margin` px, touching the top edge.
    TopRight(f32),
}

/// Everything a window needs to construct itself.
pub struct Ctx<'a> {
    pub fs: &'a mut FontSystem,
    pub icons: Rc<IconSet>,
    pub scale: f32,
    /// Latest fact of each kind (clock, battery, volume, …).
    pub facts: &'a [Fact],
    pub out_w: f32,
    pub out_h: f32,
    /// The bar's tab rects in output coordinates (left, centre, right),
    /// for windows that grow out of a tab. Zero-sized when not tabs.
    pub tabs: [Rect; 3],
    /// For modules that run their own worker threads (results come back as
    /// `Fact::Custom`).
    pub fact_tx: calloop::channel::Sender<Fact>,
}

/// Keyboard focus a window asks for (layer-shell keyboard interactivity).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keyboard {
    /// Never takes focus (keys stay with the apps).
    None,
    /// Focus on click.
    OnDemand,
    /// Takes focus while open.
    Exclusive,
}

pub trait WinScene {
    /// Surface size (logical px), fixed for the window's lifetime.
    fn size(&self) -> (f32, f32);
    fn placement(&self) -> Placement;
    /// Take keyboard focus exclusively while open (launcher). Otherwise the
    /// window gets focus on click and closes when focus leaves.
    fn exclusive_keyboard(&self) -> bool {
        false
    }
    /// Keyboard interactivity right now; re-applied whenever it changes.
    fn keyboard(&self) -> Keyboard {
        if self.exclusive_keyboard() { Keyboard::Exclusive } else { Keyboard::OnDemand }
    }
    fn chrome(&self) -> &Chrome;
    fn chrome_mut(&mut self) -> &mut Chrome;

    fn apply(&mut self, _f: &Fact) {}
    /// What the host calls for each fact. Return false when the fact did
    /// not change anything, and the host skips the redraw (a window should
    /// say false for other modules' `Fact::Custom` payloads). Default:
    /// `apply` and redraw.
    fn on_fact(&mut self, f: &Fact) -> bool {
        self.apply(f);
        true
    }
    fn set_scale(&mut self, s: f32);
    /// Advance animations; true while anything moves.
    fn step(&mut self, dt: f32) -> bool;
    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList);
    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer>;

    fn pointer_motion(&mut self, _x: f32, _y: f32) -> Option<Action> {
        None
    }
    fn pointer_leave(&mut self) {}
    fn press(&mut self, _x: f32, _y: f32) -> Option<Action> {
        None
    }
    /// Returns (action, close-this-window).
    fn release(&mut self) -> (Option<Action>, bool) {
        (None, false)
    }
    fn scroll(&mut self, _dy: f32) -> Option<Action> {
        None
    }
    /// Continuous scrolling (touchpad fingers), logical px on both axes;
    /// `end` when the fingers lift. Return false to get it as `scroll`
    /// steps instead.
    fn pan(&mut self, _dx: f32, _dy: f32, _end: bool) -> bool {
        false
    }
    /// Returns (action, close-this-window). Escape closes by default.
    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        (None, k.sym == keys::ESCAPE)
    }
    /// A held key's auto-repeat. Defaults to `key`; override to ignore
    /// repeats of keys that must be pressed deliberately.
    fn key_repeat(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        self.key(k)
    }
    fn cursor(&self) -> Cursor {
        if self.chrome().grabbing {
            Cursor::Grabbing
        } else {
            Cursor::Default
        }
    }
    /// Input region (surface coords). Defaults to the body.
    fn input_region(&self) -> Vec<Rect> {
        vec![self.chrome().body.inflate(2.0)]
    }
    /// Called when the window should go away (Escape, focus loss, toggle).
    fn close(&mut self) {
        self.chrome_mut().close();
    }
    fn reopen(&mut self) {
        self.chrome_mut().reopen();
    }
    /// The surface is being dropped (possibly mid-animation: output gone,
    /// layer closed by the compositor). Push every texture key this window
    /// still holds and stop any workers.
    fn release_textures(&mut self, _frees: &mut Vec<u64>) {}
}

const GLIDE: SpringCfg = SpringCfg::new(0.45, 0.9);

/// Shared window behaviour: open/close morph, drag + fling, timing.
pub struct Chrome {
    pub time: f32,
    pub wake_at: Option<f32>,
    /// Current body rect (surface coords, after the drag remainder).
    pub body: Rect,
    /// Whole-pixel surface offset committed this frame (set by main).
    pub surf_origin: (i32, i32),
    /// Where the body morphs from / to.
    pub start: Rect,
    pub home: Rect,
    /// 0 = at `start`, 1 = at `home`. Width and height use their own
    /// springs so the morph can lead with one axis.
    pub open_w: Spring,
    pub open_h: Spring,
    pub closing: bool,
    off_x: Spring,
    off_y: Spring,
    /// Allowed drag offset: (min_x, max_x, min_y, max_y).
    pub bounds: (f32, f32, f32, f32),
    pub grabbing: bool,
    fling: (f32, f32),
    last_move: Option<(f32, f32, f32)>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Chrome {
    /// `open_w`/`open_h` are the springs used for the morph's two axes.
    pub fn new(start: Rect, home: Rect, open_w: SpringCfg, open_h: SpringCfg) -> Self {
        let mut w = Spring::new(0.0, open_w);
        let mut h = Spring::new(0.0, open_h);
        w.set(1.0);
        h.set(1.0);
        Self {
            time: 0.0,
            wake_at: None,
            body: start,
            surf_origin: (0, 0),
            start,
            home,
            open_w: w,
            open_h: h,
            closing: false,
            off_x: Spring::new(0.0, GLIDE),
            off_y: Spring::new(0.0, GLIDE),
            bounds: (0.0, 0.0, 0.0, 0.0),
            grabbing: false,
            fling: (0.0, 0.0),
            last_move: None,
        }
    }

    /// The morphed body before the drag remainder (surface coords).
    pub fn morph(&self) -> Rect {
        let (w, h) = (self.open_w.get(), self.open_h.get());
        Rect::new(
            lerp(self.start.x, self.home.x, w),
            lerp(self.start.y, self.home.y, h),
            lerp(self.start.w, self.home.w, w),
            lerp(self.start.h, self.home.h, h),
        )
    }

    /// Overall openness 0..1 (min of both axes), for fading content in.
    pub fn openness(&self) -> f32 {
        self.open_w.get().min(self.open_h.get())
    }

    pub fn close(&mut self) {
        if !self.closing {
            self.closing = true;
            self.grabbing = false;
            self.open_w.set(0.0);
            self.open_h.set(0.0);
            // Fly back home while folding.
            self.off_x.set(0.0);
            self.off_y.set(0.0);
        }
    }

    /// Fold where the window is now (no flight home), e.g. into the
    /// element something was just picked from.
    pub fn close_in_place(&mut self) {
        if !self.closing {
            self.closing = true;
            self.grabbing = false;
            self.open_w.set(0.0);
            self.open_h.set(0.0);
            let (x, y) = (self.off_x.get(), self.off_y.get());
            self.off_x.snap(x);
            self.off_y.snap(y);
        }
    }

    pub fn reopen(&mut self) {
        self.closing = false;
        self.open_w.set(1.0);
        self.open_h.set(1.0);
    }

    pub fn done(&self) -> bool {
        self.closing && self.open_w.at_rest() && self.open_h.at_rest() && self.off_x.at_rest() && self.off_y.at_rest()
    }

    /// Advance chrome springs (call from the scene's `step`).
    pub fn step(&mut self, dt: f32) -> bool {
        self.time += dt;
        self.open_w.step(dt) | self.open_h.step(dt) | self.off_x.step(dt) | self.off_y.step(dt)
    }

    /// Ask for a frame at bar-clock time `t` without animating until then.
    pub fn wake(&mut self, t: f32) {
        self.wake_at = Some(self.wake_at.map_or(t, |w| w.min(t)));
    }

    pub fn margin(&self) -> (i32, i32) {
        (self.off_x.get().floor() as i32, self.off_y.get().floor() as i32)
    }

    pub fn start_grab(&mut self) {
        if !self.closing {
            self.grabbing = true;
            self.fling = (0.0, 0.0);
            self.last_move = None;
        }
    }

    pub fn drag_by(&mut self, dx: f32, dy: f32) {
        if !self.grabbing {
            return;
        }
        let (lx, hx, ly, hy) = self.bounds;
        let nx = (self.off_x.get() + dx).clamp(lx, hx.max(lx));
        let ny = (self.off_y.get() + dy).clamp(ly, hy.max(ly));
        if let Some((t, px, py)) = self.last_move {
            let dt = (self.time - t).max(1.0 / 240.0);
            self.fling.0 = self.fling.0 * 0.6 + (nx - px) / dt * 0.4;
            self.fling.1 = self.fling.1 * 0.6 + (ny - py) / dt * 0.4;
        }
        self.last_move = Some((self.time, nx, ny));
        self.off_x.snap(nx);
        self.off_y.snap(ny);
    }

    /// Let go: glide on with the throw's momentum and settle.
    pub fn end_grab(&mut self) {
        if !self.grabbing {
            return;
        }
        self.grabbing = false;
        let (lx, hx, ly, hy) = self.bounds;
        let tx = (self.off_x.get() + self.fling.0 * 0.12).clamp(lx, hx.max(lx));
        let ty = (self.off_y.get() + self.fling.1 * 0.12).clamp(ly, hy.max(ly));
        self.off_x.vel = self.fling.0;
        self.off_y.vel = self.fling.1;
        self.off_x.set(tx);
        self.off_y.set(ty);
    }

    /// Sub-pixel remainder of the drag offset (logical px).
    pub fn remainder(&self) -> (f32, f32) {
        (self.off_x.get() - self.surf_origin.0 as f32, self.off_y.get() - self.surf_origin.1 as f32)
    }

    /// Shift a finished frame by the drag remainder. Returns the shift so
    /// the scene can move its own hit rects by the same amount.
    pub fn translate(&mut self, out: &mut DrawList, scale: f32) -> (f32, f32) {
        let (dx, dy) = self.remainder();
        if dx == 0.0 && dy == 0.0 {
            return (0.0, 0.0);
        }
        let (px, py) = (dx * scale, dy * scale);
        for s in &mut out.shapes {
            s.rect[0] += px;
            s.rect[1] += py;
            s.spot[0] += px;
            s.spot[1] += py;
        }
        for i in &mut out.images {
            i.rect[0] += px;
            i.rect[1] += py;
            if i.reveal[2] >= 0.0 {
                i.reveal[0] += px;
                i.reveal[1] += py;
            }
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
        self.body.x += dx;
        self.body.y += dy;
        (dx, dy)
    }
    /// The surface is going away without a final frame (output removed,
    /// layer closed): hand over every texture key still uploaded.
    fn release_textures(&mut self, _frees: &mut Vec<u64>) {}
}

/// A minimal window (body + title) used by module stubs and as a reference
/// implementation of `WinScene` + `Chrome`.
pub struct Placeholder {
    chrome: Chrome,
    text: crate::bar::TextCache,
    scale: f32,
    title: &'static str,
    size: (f32, f32),
    placement: Placement,
}

impl Placeholder {
    pub fn new(ctx: &mut Ctx, title: &'static str, size: (f32, f32), placement: Placement) -> Self {
        let mut text = crate::bar::TextCache::new(ctx.fs, crate::theme::current());
        text.set_scale(ctx.scale);
        let home = Rect::new(24.0, 24.0, size.0 - 48.0, size.1 - 48.0);
        let start = Rect::new(size.0 * 0.5 - 40.0, size.1 * 0.5 - 20.0, 80.0, 40.0);
        let cfg = SpringCfg::new(0.5, 0.82);
        Self { chrome: Chrome::new(start, home, cfg, cfg), text, scale: ctx.scale, title, size, placement }
    }
}

impl WinScene for Placeholder {
    fn size(&self) -> (f32, f32) {
        self.size
    }
    fn placement(&self) -> Placement {
        self.placement
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }
    fn set_scale(&mut self, s: f32) {
        self.scale = s;
        self.text.set_scale(s);
    }
    fn step(&mut self, dt: f32) -> bool {
        self.chrome.wake_at = None;
        self.chrome.step(dt)
    }
    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        use crate::{paint, theme::Rgba};
        out.clear();
        let r = self.chrome.morph();
        self.chrome.body = r;
        out.shapes.push(
            paint::ink(self.scale, r, 24.0)
                .shadow(Rgba::hex(0x000000, 0.55), 20.0, 8.0)
                .done(),
        );
        out.glass(r, 24.0, 1.0);
        let a = ((self.chrome.openness() - 0.6) / 0.4).clamp(0.0, 1.0);
        let f = crate::bar::Font::new(15.0, 600).family("Geist");
        let (k, w) = self.text.get(fs, self.title, f);
        let s = self.scale;
        if a > 0.0 {
            out.texts.push(crate::gfx::TextDraw {
                key: k,
                x: ((r.x + (r.w - w) * 0.5) * s).round(),
                y: ((r.y + r.h * 0.5 - 10.0) * s).round(),
                color: Rgba::hex(0xf2f2f5, a),
                clip: [(r.x * s) as i32, (r.y * s) as i32, ((r.x + r.w) * s) as i32, ((r.y + r.h) * s) as i32],
            });
        }
        self.text.end_frame();
        self.chrome.translate(out, self.scale);
    }
    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }
    fn press(&mut self, _x: f32, _y: f32) -> Option<Action> {
        self.chrome.start_grab();
        None
    }
    fn release(&mut self) -> (Option<Action>, bool) {
        self.chrome.end_grab();
        (None, false)
    }
}
