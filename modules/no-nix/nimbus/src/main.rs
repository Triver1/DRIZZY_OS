//! nimbus: a glassy, spring-animated top bar for niri.
//!
//! Threads:
//! - services (niri, clock, power, audio): blocking I/O → `Fact`s
//! - this one: Wayland + springs + rendering. It never waits on a service;
//!   facts arrive over a channel and only move spring targets.
//!
//! Frames are produced only while something moves (or after an event), paced
//! by `wl_surface.frame` and timed with `wp_presentation`, so animation time
//! advances in exact refresh-period steps and idle costs zero frames.

mod anim;
mod appicons;
mod bar;
mod config;
mod designs;
mod files;
mod fonts;
mod gfx;
mod icons;
mod ipc;
mod launcher;
mod lock;
mod media;
mod monitor;
mod notify;
mod paint;
mod polkit;
mod prefs;
mod profile;
mod services;
mod settings;
mod snapshot;
mod theme;
mod toggles;
mod tray;
mod traymenu;
mod wallpaper;
mod weather;
mod window;

use std::{rc::Rc, time::Instant};

use calloop::{EventLoop, channel, timer::{TimeoutAction, Timer}};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    activation::{ActivationHandler, ActivationState, RequestData},
    background_effect::{BackgroundEffectHandler, BackgroundEffectState},
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_dispatch2, delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    presentation_time::{PresentTime, PresentationTimeHandler, PresentationTimeState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager},
        relative_pointer::{RelativeMotionEvent, RelativePointerHandler, RelativePointerState},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
};
use wayland_client::{
    Connection, Proxy, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};
use wayland_protocols::{
    ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
    wp::{
        cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape as CursorShape, WpCursorShapeDeviceV1},
        fractional_scale::v1::client::{wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, wp_fractional_scale_v1},
        relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1,
        presentation_time::client::wp_presentation_feedback,
        viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
    },
};

use crate::{
    bar::{Bar, Cmd, Rect},
    gfx::{DrawList, Gpu, Icons, SurfaceGfx},
    icons::IconSet,
    services::{Applier, Fact, NiriActions},
    prefs::Prefs,
    settings::{Action, Settings},
    window::{Ctx, Cursor, KeyInput, Placement, WinScene},
};

fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Maps wall time onto the display's vblank grid. Each frame is stamped with
/// the vblank it will be shown on, so springs advance by whole refresh
/// periods and motion stays perfectly even even if our wakeups jitter.
#[derive(Default)]
struct FrameClock {
    last_target: Option<u64>,
    last_present: Option<u64>,
    refresh: u64,
}

impl FrameClock {
    fn advance(&mut self, now: u64) -> f32 {
        let refresh = if self.refresh > 0 { self.refresh } else { 8_333_333 };
        let target = match self.last_present {
            Some(lp) if now >= lp && now - lp < 250_000_000 => lp + ((now - lp) / refresh + 1) * refresh,
            _ => now + refresh,
        };
        let dt = match self.last_target {
            Some(lt) if target <= lt => 0,
            Some(lt) if target - lt < 100_000_000 => target - lt,
            // Waking from idle: the first frame moves by exactly one period.
            _ => refresh,
        };
        self.last_target = Some(self.last_target.map_or(target, |lt| lt.max(target)));
        dt as f32 / 1e9
    }
}

/// Which of a panel's floating windows: the quick panel or the full window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Win {
    Quick,
    Full,
    Launcher,
    Monitor,
    Wallpapers,
    Toasts,
    /// A tray item's menu (com.canonical.dbusmenu).
    TrayMenu,
    Files,
    /// The polkit password prompt. Only the agent opens it.
    Polkit,
}

impl Win {
    const ALL: [Win; 9] = [Win::Quick, Win::Full, Win::Launcher, Win::Monitor, Win::Wallpapers, Win::Toasts, Win::TrayMenu, Win::Files, Win::Polkit];

    fn from_command(c: &str) -> Option<Win> {
        Some(match c {
            "settings" => Win::Quick,
            "prefs" => Win::Full,
            "launcher" => Win::Launcher,
            "monitor" => Win::Monitor,
            "wallpapers" => Win::Wallpapers,
            "notifications" => Win::Toasts,
            "files" => Win::Files,
            _ => return None,
        })
    }

    fn namespace(self) -> &'static str {
        match self {
            Win::Quick => "nimbus-settings",
            Win::Full => "nimbus-prefs",
            Win::Launcher => "nimbus-launcher",
            Win::Monitor => "nimbus-monitor",
            Win::Wallpapers => "nimbus-wallpapers",
            Win::Toasts => "nimbus-notifications",
            Win::TrayMenu => "nimbus-traymenu",
            Win::Files => "nimbus-files",
            Win::Polkit => "nimbus-polkit",
        }
    }
}

enum PopScene {
    Quick(Settings),
    Full(Prefs),
    Dyn(Box<dyn WinScene>),
}

/// Same call on the two concrete scenes; `$d` handles generic windows.
macro_rules! each {
    ($s:expr, $x:ident => $e:expr, $dn:ident => $d:expr) => {
        match $s {
            PopScene::Quick($x) => $e,
            PopScene::Full($x) => $e,
            PopScene::Dyn($dn) => $d,
        }
    };
}

impl PopScene {
    fn size(&self) -> (f32, f32) {
        match self {
            PopScene::Quick(_) => (settings::SW, settings::SH),
            PopScene::Full(_) => (prefs::SW, prefs::SH),
            PopScene::Dyn(d) => d.size(),
        }
    }
    fn apply(&mut self, f: &Fact) {
        each!(self, s => s.apply(f), d => d.apply(f))
    }
    /// Hand a module-private payload to the window; true when it was used
    /// (only then does the window redraw).
    fn apply_custom(&mut self, f: &Fact) -> bool {
        match self {
            PopScene::Dyn(d) => d.on_fact(f),
            PopScene::Full(s) => s.on_custom(f),
            _ => false,
        }
    }
    /// Texture keys the window still holds, before its surface is dropped.
    fn release_textures(&mut self, frees: &mut Vec<u64>) {
        match self {
            PopScene::Dyn(d) => d.release_textures(frees),
            PopScene::Full(s) => s.release_textures(frees),
            _ => {}
        }
    }
    fn set_scale(&mut self, v: f32) {
        each!(self, s => s.set_scale(v), d => d.set_scale(v))
    }
    fn step(&mut self, dt: f32) -> bool {
        each!(self, s => s.step(dt), d => {
            d.chrome_mut().wake_at = None;
            d.step(dt)
        })
    }
    fn draw(&mut self, fs: &mut glyphon::FontSystem, out: &mut DrawList) {
        each!(self, s => s.draw(fs, out), d => d.draw(fs, out))
    }
    fn text_buffers(&self) -> &std::collections::HashMap<u64, glyphon::Buffer> {
        each!(self, s => s.text_buffers(), d => d.text_buffers())
    }
    fn sync_time(&mut self, t: f32) {
        each!(self, s => s.time = s.time.max(t), d => {
            let c = d.chrome_mut();
            c.time = c.time.max(t)
        })
    }
    fn time(&self) -> f32 {
        each!(self, s => s.time, d => d.chrome().time)
    }
    fn wake_at(&self) -> Option<f32> {
        each!(self, s => s.wake_at, d => d.chrome().wake_at)
    }
    fn body(&self) -> Rect {
        each!(self, s => s.body, d => d.chrome().body)
    }
    fn input_region(&self) -> Vec<Rect> {
        each!(self, s => vec![s.body.inflate(2.0)], d => d.input_region())
    }
    fn margin(&self) -> (i32, i32) {
        each!(self, s => s.margin(), d => d.chrome().margin())
    }
    fn set_surf_origin(&mut self, o: (i32, i32)) {
        each!(self, s => s.surf_origin = o, d => d.chrome_mut().surf_origin = o)
    }
    fn done(&self) -> bool {
        each!(self, s => s.done(), d => d.chrome().done())
    }
    fn closing(&self) -> bool {
        each!(self, s => s.closing(), d => d.chrome().closing)
    }
    fn close(&mut self) {
        each!(self, s => s.close(), d => d.close())
    }
    fn reopen(&mut self) {
        each!(self, s => s.reopen(), d => d.reopen())
    }
    fn escape(&mut self) {
        each!(self, s => s.escape(), d => d.close())
    }
    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        each!(self, s => s.pointer_motion(x, y), d => d.pointer_motion(x, y))
    }
    fn pointer_leave(&mut self) {
        each!(self, s => s.pointer_leave(), d => d.pointer_leave())
    }
    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        each!(self, s => s.press(x, y), d => d.press(x, y))
    }
    fn release(&mut self) -> (Option<Action>, bool) {
        each!(self, s => s.release(), d => d.release())
    }
    fn scroll(&mut self, dy: f32) -> Option<Action> {
        each!(self, s => s.scroll(dy), d => d.scroll(dy))
    }
    fn pan(&mut self, dx: f32, dy: f32, end: bool) -> bool {
        match self {
            PopScene::Dyn(d) => d.pan(dx, dy, end),
            _ => false,
        }
    }
    fn key(&mut self, k: &KeyInput, repeat: bool) -> (Option<Action>, bool) {
        match self {
            PopScene::Dyn(d) if repeat => d.key_repeat(k),
            PopScene::Dyn(d) => d.key(k),
            PopScene::Full(s) => s.key(k),
            _ => {
                if k.sym == window::keys::ESCAPE {
                    self.escape();
                }
                (None, false)
            }
        }
    }
    fn grabbing(&self) -> bool {
        each!(self, s => s.grabbing(), d => d.chrome().grabbing)
    }
    fn cursor(&self) -> CursorShape {
        let c = each!(self, s => {
            if s.grabbing() {
                Cursor::Grabbing
            } else if s.over_grip() {
                Cursor::Grab
            } else if s.clickable() {
                Cursor::Pointer
            } else {
                Cursor::Default
            }
        }, d => d.cursor());
        let c = if let PopScene::Full(s) = self { s.cursor_hint().unwrap_or(c) } else { c };
        match c {
            Cursor::Default => CursorShape::Default,
            Cursor::Pointer => CursorShape::Pointer,
            Cursor::Grab => CursorShape::Grab,
            Cursor::Grabbing => CursorShape::Grabbing,
            Cursor::Text => CursorShape::Text,
        }
    }
    fn drag_by(&mut self, dx: f32, dy: f32) {
        each!(self, s => s.drag_by(dx, dy), d => d.chrome_mut().drag_by(dx, dy))
    }
}

/// A floating window's own layer surface. It exists only while open (or
/// animating closed), so a closed window costs nothing.
struct Popup {
    // Field order matters: the swapchain must go before the wl_surface.
    gfx: Option<SurfaceGfx>,
    effect: Option<Effect>,
    last_blur: Vec<[i32; 4]>,
    viewport: Option<WpViewport>,
    layer: LayerSurface,
    scene: PopScene,
    configured: bool,
    /// Has presented a frame (the bar may hide its tab now).
    shown: bool,
    frame_pending: bool,
    dirty: bool,
    animating: bool,
    clock: FrameClock,
    last_input: Option<[i32; 4]>,
    /// Home margins of the surface; drag offsets add to them.
    base_left: i32,
    base_top: i32,
    margin: (i32, i32),
    /// Keyboard interactivity last applied.
    kb: window::Keyboard,
}

/// A window's background-effect object, released before its surface.
struct Effect(ExtBackgroundEffectSurfaceV1);

impl Drop for Effect {
    fn drop(&mut self) {
        self.0.destroy();
    }
}

fn kb_mode(k: window::Keyboard) -> KeyboardInteractivity {
    match k {
        window::Keyboard::None => KeyboardInteractivity::None,
        window::Keyboard::OnDemand => KeyboardInteractivity::OnDemand,
        window::Keyboard::Exclusive => KeyboardInteractivity::Exclusive,
    }
}

/// The wallpaper on a Background layer surface (wallpaper.rs draws it).
struct Bg {
    gfx: Option<SurfaceGfx>,
    viewport: Option<WpViewport>,
    layer: LayerSurface,
    scene: wallpaper::Backdrop,
    size: (u32, u32),
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    animating: bool,
    clock: FrameClock,
}

struct Panel {
    popup: Option<Popup>,
    prefs: Option<Popup>,
    /// Launcher, monitor, wallpapers, toasts.
    dynw: [Option<Popup>; 7],
    backdrop: Option<Bg>,
    /// The frosted wallpaper behind the workspaces (overview, and the gap
    /// while switching).
    frost: Option<Bg>,
    // Field order matters: the swapchain must go before the wl_surface.
    gfx: Option<SurfaceGfx>,
    effect: Option<ExtBackgroundEffectSurfaceV1>,
    viewport: Option<WpViewport>,
    fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    layer: LayerSurface,
    output: wl_output::WlOutput,
    /// Night light: this output's gamma control and its table size.
    gamma: Option<(wayland_protocols_wlr::gamma_control::v1::client::zwlr_gamma_control_v1::ZwlrGammaControlV1, u32)>,
    bar: Bar,
    size: (u32, u32),
    scale120: u32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    animating: bool,
    clock: FrameClock,
    last_blur: Vec<[i32; 4]>,
    last_input: Vec<[i32; 4]>,
}

impl Panel {
    fn bg(&mut self, frost: bool) -> &mut Option<Bg> {
        if frost { &mut self.frost } else { &mut self.backdrop }
    }

    fn win(&mut self, w: Win) -> &mut Option<Popup> {
        match w {
            Win::Quick => &mut self.popup,
            Win::Full => &mut self.prefs,
            Win::Launcher => &mut self.dynw[0],
            Win::Monitor => &mut self.dynw[1],
            Win::Wallpapers => &mut self.dynw[2],
            Win::Toasts => &mut self.dynw[3],
            Win::TrayMenu => &mut self.dynw[4],
            Win::Files => &mut self.dynw[5],
            Win::Polkit => &mut self.dynw[6],
        }
    }

    fn win_ref(&self, w: Win) -> &Option<Popup> {
        match w {
            Win::Quick => &self.popup,
            Win::Full => &self.prefs,
            Win::Launcher => &self.dynw[0],
            Win::Monitor => &self.dynw[1],
            Win::Wallpapers => &self.dynw[2],
            Win::Toasts => &self.dynw[3],
            Win::TrayMenu => &self.dynw[4],
            Win::Files => &self.dynw[5],
            Win::Polkit => &self.dynw[6],
        }
    }

    fn scale(&self) -> f32 {
        self.scale120 as f32 / 120.0
    }
    fn physical(&self) -> (u32, u32) {
        let s = self.scale();
        ((self.size.0 as f32 * s).round() as u32, (self.size.1 as f32 * s).round() as u32)
    }
}

#[derive(Default)]
struct Stats {
    enabled: bool,
    frames: u64,
    cpu_ns: Vec<u64>,
    since: Option<Instant>,
    /// Frames per surface kind in the current window.
    by: Vec<(&'static str, u32)>,
}

impl Stats {
    fn record(&mut self, ns: u64, what: &'static str) {
        if !self.enabled {
            return;
        }
        match self.by.iter_mut().find(|(w, _)| *w == what) {
            Some((_, n)) => *n += 1,
            None => self.by.push((what, 1)),
        }
        self.frames += 1;
        self.cpu_ns.push(ns);
        let since = *self.since.get_or_insert_with(Instant::now);
        if since.elapsed().as_secs_f32() >= 5.0 {
            let mut v = std::mem::take(&mut self.cpu_ns);
            v.sort_unstable();
            let pct = |p: f32| v[((v.len() - 1) as f32 * p) as usize] as f64 / 1e6;
            eprintln!(
                "nimbus stats: {} frames ({}) in {:.1}s, cpu/frame p50 {:.3}ms p99 {:.3}ms max {:.3}ms",
                v.len(),
                self.by.drain(..).map(|(w, n)| format!("{w} {n}")).collect::<Vec<_>>().join(", "),
                since.elapsed().as_secs_f32(),
                pct(0.5),
                pct(0.99),
                pct(1.0)
            );
            self.since = Some(Instant::now());
        }
    }
}

struct App {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    fractional_mgr: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    effects: BackgroundEffectState,
    presentation: PresentationTimeState,
    cursor_shapes: Option<CursorShapeManager>,

    pointer: Option<wl_pointer::WlPointer>,
    shape_device: Option<WpCursorShapeDeviceV1>,
    enter_serial: u32,
    /// Serial of the last button press on a popup (activation tokens).
    press_serial: u32,
    seat: Option<wl_seat::WlSeat>,
    activation: Option<ActivationState>,
    cursor: Option<CursorShape>,
    pointer_panel: Option<usize>,

    gpu: Gpu,
    icons: Icons,
    iconset: Rc<IconSet>,
    panels: Vec<Panel>,
    latest: Vec<Fact>,
    actions: NiriActions,
    list: DrawList,
    stats: Stats,
    handle: calloop::LoopHandle<'static, App>,
    start_ns: u64,
    /// Absolute (bar clock) time of the pending wake-up timer, if any.
    wake_pending: Option<f32>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    kb_focus: Option<wl_surface::WlSurface>,
    mods: Modifiers,
    qh: Option<QueueHandle<App>>,
    relative: RelativePointerState,
    /// Exists only while a window drag is in progress.
    rel_pointer: Option<ZwpRelativePointerV1>,
    fact_tx: channel::Sender<Fact>,
    volume_applier: Applier,
    brightness_applier: Applier,
    mic_applier: Applier,
    started: Instant,
    toggles: toggles::Toggles,
}

impl App {
    fn panel_of(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.panels.iter().position(|p| p.layer.wl_surface() == surface)
    }

    fn popup_of(&self, surface: &wl_surface::WlSurface) -> Option<(usize, Win)> {
        self.panels.iter().enumerate().find_map(|(i, p)| {
            Win::ALL.into_iter().find(|w| p.win_ref(*w).as_ref().is_some_and(|pp| pp.layer.wl_surface() == surface)).map(|w| (i, w))
        })
    }

    /// Free the GPU textures of a window or backdrop that is dropped
    /// without a final frame (textures are shared by all surfaces).
    fn release_popup(&mut self, pp: Option<Popup>) {
        if let Some(mut pp) = pp {
            let mut frees = vec![];
            pp.scene.release_textures(&mut frees);
            self.gpu.free_textures(&frees);
        }
    }

    fn release_backdrop(&mut self, b: Option<Bg>) {
        if let Some(mut b) = b {
            let mut frees = vec![];
            b.scene.release(&mut frees);
            self.gpu.free_textures(&frees);
        }
    }

    /// Drop the panels `gone` says so, with everything they host.
    fn remove_panels(&mut self, gone: impl Fn(&Panel) -> bool) {
        let mut i = 0;
        while i < self.panels.len() {
            if gone(&self.panels[i]) {
                let mut p = self.panels.remove(i);
                for b in [p.backdrop.take(), p.frost.take()] {
                    self.release_backdrop(b);
                }
                for w in Win::ALL {
                    let pp = p.win(w).take();
                    self.release_popup(pp);
                }
            } else {
                i += 1;
            }
        }
        self.pointer_panel = None;
        if let Some(qh) = self.qh.clone() {
            self.refresh_caffeine(&qh);
            self.redraw_windows(&qh);
        }
    }

    /// The panel and which of its two backgrounds (true: the frosted one).
    fn backdrop_of(&self, surface: &wl_surface::WlSurface) -> Option<(usize, bool)> {
        let is = |b: &Option<Bg>| b.as_ref().is_some_and(|b| b.layer.wl_surface() == surface);
        self.panels.iter().enumerate().find_map(|(i, p)| if is(&p.backdrop) { Some((i, false)) } else if is(&p.frost) { Some((i, true)) } else { None })
    }

    /// The panel on the focused output (commands open windows there).
    fn focused_panel(&self) -> usize {
        let out = self.latest.iter().find_map(|f| match f {
            Fact::Niri(n) => n.focused_output.clone(),
            _ => None,
        });
        out.and_then(|o| self.panels.iter().position(|p| p.bar.output == o)).unwrap_or(0)
    }

    fn ctx(&mut self, i: usize) -> Ctx<'_> {
        let out_h = self.output_h(i);
        let p = &self.panels[i];
        Ctx {
            fs: &mut self.gpu.fonts,
            icons: self.iconset.clone(),
            scale: p.scale(),
            facts: &self.latest,
            out_w: p.bar.width,
            out_h,
            tabs: p.bar.tabs,
            fact_tx: self.fact_tx.clone(),
        }
    }

    fn popup_of_layer(&self, layer: &LayerSurface) -> Option<(usize, Win)> {
        self.popup_of(layer.wl_surface())
    }

    fn add_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let Some(info) = self.outputs.info(&output) else { return };
        let name = info.name.clone().unwrap_or_default();
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(qh, surface.clone(), Layer::Top, Some("nimbus"), Some(&output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, bar::SURFACE_H);
        layer.set_exclusive_zone(bar::EXCLUSIVE);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        let fractional = self.fractional_mgr.as_ref().map(|m| m.get_fractional_scale(&surface, qh, FracScale));
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, Viewport));
        let effect = self.effects.get_background_effect(&surface, qh).ok();
        layer.commit();

        let mut b = Bar::new(name, &mut self.gpu.fonts, self.iconset.clone());
        for f in &self.latest {
            b.apply(f);
        }
        let int_scale = info.scale_factor.max(1) as u32;
        self.panels.push(Panel {
            popup: None,
            prefs: None,
            dynw: [None, None, None, None, None, None, None],
            backdrop: None,
            frost: None,
            gfx: None,
            effect,
            viewport,
            fractional,
            layer,
            output,
            gamma: None,
            bar: b,
            size: (0, 0),
            scale120: int_scale * 120,
            configured: false,
            frame_pending: false,
            dirty: true,
            animating: false,
            clock: FrameClock::default(),
            last_blur: vec![],
            last_input: vec![],
        });
        let i = self.panels.len() - 1;
        self.gamma_for_panel(qh, i);
    }

    fn now(&self) -> f32 {
        (monotonic_ns() - self.start_ns) as f32 / 1e9
    }

    fn on_fact(&mut self, qh: &QueueHandle<Self>, fact: Fact) {
        if let Fact::Command(c) = &fact {
            // Nothing opens over a password prompt.
            if self.polkit_open() {
                return;
            }
            let i = self.focused_panel();
            if let Some(Some(req)) = files::parse_command(c) {
                self.show_in_files(qh, i, req);
                return;
            }
            match Win::from_command(c.as_str()) {
                Some(Win::Quick) => self.toggle_settings(qh, i),
                Some(Win::Full) => self.open_prefs(qh, i),
                Some(w) => self.toggle_window(qh, i, w),
                None => eprintln!("nimbus: unknown command {c:?} (try: {})", ipc::COMMANDS.join(", ")),
            }
            return;
        }
        if let Fact::Custom(_) = &fact {
            if polkit::wants_window(&fact) && !self.polkit_open() {
                if self.panels.is_empty() {
                    polkit::no_output(&fact);
                    return;
                }
                let i = self.focused_panel();
                self.toggle_window(qh, i, Win::Polkit);
                self.redraw_windows(qh);
            }
            // One toasts window for all outputs: it opens on the focused
            // one and stays there; facts below reach only that window.
            if notify::wants_window(&fact) && self.toasts_panel().is_none() {
                let i = self.focused_panel();
                self.toggle_window(qh, i, Win::Toasts);
            }
            // A tray menu arrived: show it on the output that was clicked
            // (replacing one that is already open).
            if traymenu::wants_window(&fact) {
                let i = self.pointer_panel.unwrap_or_else(|| self.focused_panel());
                if i < self.panels.len() {
                    self.panels[i].dynw[4] = None;
                    self.toggle_window(qh, i, Win::TrayMenu);
                }
                return;
            }
            if let Some(pid) = toggles::record_ended(&fact) {
                self.recording_ended(qh, pid);
                return;
            }
            if let Some(path) = wallpaper::wants_backdrop(&fact) {
                for i in 0..self.panels.len() {
                    for frost in [false, true] {
                        if self.panels[i].bg(frost).is_none() {
                            self.create_backdrop(qh, i, Some(path.clone()), frost);
                        }
                    }
                }
            }
            // Module-private payloads go to windows and backdrops only (the
            // bars never use them), and only one that used it redraws. They
            // are not kept in `latest` either: a window's last payload must
            // not outlive the window.
            for i in 0..self.panels.len() {
                for w in Win::ALL {
                    if let Some(pp) = self.panels[i].win(w)
                        && pp.scene.apply_custom(&fact)
                    {
                        self.request_popup_redraw(qh, i, w);
                    }
                }
                for frost in [false, true] {
                    if let Some(b) = self.panels[i].bg(frost)
                        && b.scene.apply(&fact)
                    {
                        self.request_bg_redraw(qh, i, frost);
                    }
                }
            }
            return;
        }
        let now = self.now();
        for i in 0..self.panels.len() {
            self.panels[i].bar.sync_time(now);
            self.panels[i].bar.apply(&fact);
            self.request_redraw(qh, i);
            for w in Win::ALL {
                if let Some(pp) = self.panels[i].win(w) {
                    pp.scene.apply(&fact);
                    self.request_popup_redraw(qh, i, w);
                }
            }
            for frost in [false, true] {
                if let Some(b) = self.panels[i].bg(frost)
                    && b.scene.apply(&fact)
                {
                    self.request_bg_redraw(qh, i, frost);
                }
            }
        }
        // Module-private payloads are transient (and may hold pixels):
        // never keep them as "latest".
        if matches!(fact, Fact::Custom(_)) {
            return;
        }
        let d = std::mem::discriminant(&fact);
        self.latest.retain(|f| std::mem::discriminant(f) != d);
        self.latest.push(fact);
    }

    fn request_redraw(&mut self, qh: &QueueHandle<Self>, i: usize) {
        let p = &mut self.panels[i];
        if p.frame_pending {
            p.dirty = true;
        } else {
            self.draw(qh, i);
        }
    }

    fn reconfigure(&mut self, i: usize) {
        let p = &mut self.panels[i];
        if !p.configured {
            return;
        }
        let (pw, ph) = p.physical();
        if let Some(vp) = &p.viewport {
            vp.set_destination(p.size.0 as i32, p.size.1 as i32);
        } else {
            p.layer.wl_surface().set_buffer_scale((p.scale120 / 120).max(1) as i32);
        }
        p.bar.resize(p.size.0 as f32, p.scale());
        let scale = p.scale();
        for b in [p.backdrop.as_mut(), p.frost.as_mut()].into_iter().flatten() {
            b.scene.set_scale(scale);
            if b.configured
                && let Some(g) = &mut b.gfx
            {
                self.gpu.resize(g, (b.size.0 as f32 * scale).round() as u32, (b.size.1 as f32 * scale).round() as u32);
            }
            b.dirty = true;
        }
        let [d0, d1, d2, d3, d4, d5, d6] = &mut p.dynw;
        for pp in [p.popup.as_mut(), p.prefs.as_mut(), d0.as_mut(), d1.as_mut(), d2.as_mut(), d3.as_mut(), d4.as_mut(), d5.as_mut(), d6.as_mut()].into_iter().flatten() {
            pp.scene.set_scale(scale);
            if let Some(g) = &mut pp.gfx {
                let (sw, sh) = pp.scene.size();
                self.gpu.resize(g, (sw * scale).round() as u32, (sh * scale).round() as u32);
            }
            pp.dirty = true;
        }
        match &mut p.gfx {
            Some(g) => self.gpu.resize(g, pw, ph),
            None => {
                let ptr = p.layer.wl_surface().id().as_ptr() as *mut std::ffi::c_void;
                p.gfx = Some(self.gpu.create_surface(ptr, pw, ph));
            }
        }
        p.dirty = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>, i: usize) {
        let t0 = Instant::now();
        let p = &mut self.panels[i];
        if !p.configured || p.gfx.is_none() {
            return;
        }
        let now_ns = monotonic_ns();
        let dt = p.clock.advance(now_ns);
        p.bar.sync_time((now_ns - self.start_ns) as f32 / 1e9 - dt);
        p.animating = p.bar.step(dt);
        let wake = if p.animating { None } else { p.bar.wake_at.map(|t| (t, p.bar.time)) };
        p.bar.draw(&mut self.gpu.fonts, &mut self.list);

        let surface = p.layer.wl_surface().clone();
        // Blur exactly the glass, rounded corners and all (ext-background-effect).
        if let Some(effect) = &p.effect {
            let mut rows = Vec::new();
            if p.bar.look.blur {
                for r in &p.bar.glass {
                    capsule_rows(*r, &mut rows);
                }
            }
            // Designs that mark their own glass (Deck's Prism Ink tabs).
            for (r, rad) in &self.list.blur {
                rounded_rows(*r, *rad, &mut rows);
            }
            if rows != p.last_blur
                && let Ok(region) = Region::new(&self.compositor)
            {
                for [x, y, w, h] in &rows {
                    region.add(*x, *y, *w, *h);
                }
                effect.set_blur_region(Some(region.wl_region()));
                p.last_blur = rows;
            }
        }
        // Clicks outside the islands fall through to whatever is below.
        let input: Vec<[i32; 4]> = p
            .bar
            .glass
            .iter()
            .map(|r| {
                let (x0, y0) = ((r.x - 3.0).floor() as i32, (r.y - 3.0).floor() as i32);
                let (x1, y1) = ((r.x + r.w + 3.0).ceil() as i32, (r.y + r.h + 3.0).ceil() as i32);
                [x0, y0, x1 - x0, y1 - y0]
            })
            .collect();
        if input != p.last_input
            && let Ok(region) = Region::new(&self.compositor)
        {
            for [x, y, w, h] in &input {
                region.add(*x, *y, *w, *h);
            }
            surface.set_input_region(Some(region.wl_region()));
            p.last_input = input;
        }

        surface.frame(qh, FrameCallbackData(surface.clone()));
        if p.animating {
            let _ = self.presentation.feedback(&surface, qh);
        }
        self.gpu.render(p.gfx.as_mut().unwrap(), &self.list, &p.bar.text.bufs, &self.icons);
        p.frame_pending = true;
        p.dirty = false;
        if let Some((at, now)) = wake {
            self.schedule_wake(qh, at, now);
        }
        self.stats.record(t0.elapsed().as_nanos() as u64, "bar");
    }

    /// One-shot timer that redraws every panel at bar-clock time `at`.
    fn schedule_wake(&mut self, qh: &QueueHandle<Self>, at: f32, now: f32) {
        if self.wake_pending.is_some_and(|p| p <= at) {
            return;
        }
        self.wake_pending = Some(at);
        let delay = std::time::Duration::from_secs_f32((at - now).max(0.0) + 0.001);
        let qh = qh.clone();
        let _ = self.handle.insert_source(Timer::from_duration(delay), move |_, _, app: &mut App| {
            app.wake_pending = None;
            for i in 0..app.panels.len() {
                app.request_redraw(&qh, i);
                for w in Win::ALL {
                    app.request_popup_redraw(&qh, i, w);
                }
                app.request_backdrop_redraw(&qh, i);
            }
            TimeoutAction::Drop
        });
    }

    fn run(&mut self, qh: &QueueHandle<Self>, i: usize, cmd: Cmd) {
        match cmd {
            Cmd::FocusWorkspace(id) => self.actions.focus_workspace(id),
            Cmd::WorkspaceUp => self.actions.workspace_up(),
            Cmd::WorkspaceDown => self.actions.workspace_down(),
            Cmd::Volume(d) => services::change_volume(d),
            Cmd::Mute => services::toggle_mute(),
            Cmd::ToggleSettings => self.toggle_settings(qh, i),
            Cmd::StopRecording => self.stop_recording(),
            Cmd::Tray(id, title, c, x) => {
                // A second click on the icon whose menu is open closes it.
                if let Some(pp) = self.panels[i].win(Win::TrayMenu) {
                    pp.scene.close();
                    self.request_popup_redraw(qh, i, Win::TrayMenu);
                    if c != tray::Click::Secondary {
                        return;
                    }
                }
                tray::click(&id, &title, c, x, self.fact_tx.clone());
            }
            Cmd::Media(c) => self.run_action(
                qh,
                i,
                match c {
                    media::Ctl::PlayPause => Action::MediaPlayPause,
                    media::Ctl::Next => Action::MediaNext,
                    media::Ctl::Prev => Action::MediaPrev,
                    media::Ctl::Seek(s) => Action::MediaSeek(s),
                },
            ),
        }
    }

    fn run_action(&mut self, qh: &QueueHandle<Self>, i: usize, a: Action) {
        match a {
            Action::OpenPrefs => self.open_prefs(qh, i),
            Action::NightLight(on) => self.set_night(qh, on),
            Action::Caffeine(on) => self.set_caffeine(qh, on),
            Action::Airplane(on) => {
                toggles::set_airplane(on);
                services::read_system(self.fact_tx.clone(), std::time::Duration::from_millis(1500));
            }
            Action::Screenshot => toggles::screenshot(),
            Action::Record(on) => {
                if on {
                    self.start_recording(qh);
                } else {
                    self.stop_recording();
                }
            }
            Action::MediaPlayPause => media::control(media::Ctl::PlayPause),
            Action::MediaNext => media::control(media::Ctl::Next),
            Action::MediaPrev => media::control(media::Ctl::Prev),
            Action::MediaSeek(s) => media::control(media::Ctl::Seek(s)),
            Action::Open(name) => {
                if let Some(w) = Win::from_command(name) {
                    self.toggle_window(qh, i, w);
                }
            }
            Action::SetDesign(d) => {
                config::set("design", d.name());
                let mut families = vec![];
                families.extend_from_slice(d.fonts());
                fonts::load(self.gpu.fonts.db_mut(), &families);
                for k in 0..self.panels.len() {
                    self.panels[k].bar.set_design(d);
                    self.request_redraw(qh, k);
                }
            }
            Action::SetAccent(k) => {
                theme::set_accent(k);
                for k in 0..self.panels.len() {
                    self.panels[k].bar.dirty_all();
                    self.request_redraw(qh, k);
                    self.request_popup_redraw(qh, k, Win::Quick);
                    self.request_popup_redraw(qh, k, Win::Full);
                }
            }
            Action::SetSink(id) => {
                std::thread::spawn(move || services::set_default_sink(id));
                services::read_audio(self.fact_tx.clone(), std::time::Duration::from_millis(400));
            }
            Action::MicVolume(v) => self.mic_applier.set(v),
            Action::MicMute => {
                services::toggle_mic_mute();
                services::read_audio(self.fact_tx.clone(), std::time::Duration::from_millis(300));
            }
            Action::SetScale(output, scale) => {
                services::set_output_scale(output, scale);
                services::read_outputs(self.fact_tx.clone());
            }
            Action::ReadAudio => services::read_audio(self.fact_tx.clone(), std::time::Duration::ZERO),
            Action::ReadOutputs => services::read_outputs(self.fact_tx.clone()),
            Action::ReadPower => services::read_power(self.fact_tx.clone()),
            Action::ReadAbout => services::read_about(self.fact_tx.clone(), self.started),
            Action::Volume(v) => self.volume_applier.set(v),
            Action::Brightness(v) => self.brightness_applier.set(v),
            Action::Mute => services::toggle_mute(),
            Action::Wifi(on) => {
                services::set_wifi(on);
                services::read_system(self.fact_tx.clone(), std::time::Duration::from_millis(1500));
                if on {
                    services::scan_wifi(self.fact_tx.clone(), std::time::Duration::from_secs(3));
                }
            }
            Action::Bluetooth(on) => {
                services::set_bluetooth(on);
                services::read_system(self.fact_tx.clone(), std::time::Duration::from_millis(1500));
                services::list_bt(self.fact_tx.clone(), std::time::Duration::from_secs(2));
            }
            Action::ScanWifi => services::scan_wifi(self.fact_tx.clone(), std::time::Duration::ZERO),
            Action::ScanBluetooth => services::list_bt(self.fact_tx.clone(), std::time::Duration::ZERO),
            Action::WifiConnect(ssid) => {
                services::wifi_connect(&ssid);
                services::scan_wifi(self.fact_tx.clone(), std::time::Duration::from_secs(4));
            }
            Action::WifiDisconnect(ssid) => {
                services::wifi_disconnect(&ssid);
                services::scan_wifi(self.fact_tx.clone(), std::time::Duration::from_secs(2));
            }
            Action::BtConnect(mac) => {
                services::bt_connect(&mac);
                services::list_bt(self.fact_tx.clone(), std::time::Duration::from_secs(4));
            }
            Action::BtDisconnect(mac) => {
                services::bt_disconnect(&mac);
                services::list_bt(self.fact_tx.clone(), std::time::Duration::from_secs(2));
            }
            Action::Lock => services::lock(),
            Action::Suspend => services::suspend(),
            Action::Reboot => services::reboot(),
            Action::PowerOff => services::power_off(),
        }
    }

    // ── floating windows (quick panel, full settings) ───────────────────

    #[allow(clippy::too_many_arguments)]
    fn make_popup(&mut self, qh: &QueueHandle<Self>, i: usize, ns: &str, scene: PopScene, base_left: i32, base_top: i32) -> Popup {
        let kb = match &scene {
            PopScene::Dyn(d) => d.keyboard(),
            _ => window::Keyboard::OnDemand,
        };
        let p = &self.panels[i];
        let (sw, sh) = scene.size();
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(qh, surface.clone(), Layer::Overlay, Some(ns), Some(&p.output));
        // Anchored top-left so dragging is just a margin change.
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(base_top, 0, 0, base_left);
        layer.set_size(sw as u32, sh as u32);
        // Sit over the bar itself rather than below its exclusive zone.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(kb_mode(kb));
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, Viewport));
        let effect = self.effects.get_background_effect(&surface, qh).ok().map(Effect);
        layer.commit();
        Popup {
            gfx: None,
            effect,
            last_blur: vec![],
            viewport,
            layer,
            scene,
            configured: false,
            shown: false,
            frame_pending: false,
            dirty: true,
            animating: false,
            clock: FrameClock::default(),
            last_input: None,
            base_left,
            base_top,
            margin: (0, 0),
            kb,
        }
    }

    fn polkit_open(&self) -> bool {
        self.panels.iter().any(|p| p.win_ref(Win::Polkit).is_some())
    }

    /// Redraw every window: their keyboard claim follows `polkit_open`.
    fn redraw_windows(&mut self, qh: &QueueHandle<Self>) {
        for i in 0..self.panels.len() {
            for w in Win::ALL {
                self.request_popup_redraw(qh, i, w);
            }
        }
    }

    fn toasts_panel(&self) -> Option<usize> {
        self.panels.iter().position(|p| p.win_ref(Win::Toasts).is_some())
    }

    /// The click just handled invoked a notification action: ask for an
    /// xdg-activation token (press serial, this surface) so the app may
    /// raise its window, then send the action with it.
    fn invoke_with_token(&mut self, qh: &QueueHandle<Self>, i: usize, w: Win) {
        let Some(inv) = notify::take_invoke() else { return };
        let surface = self.panels[i].win_ref(w).as_ref().map(|p| p.layer.wl_surface().clone());
        match (&self.activation, &self.seat) {
            (Some(a), Some(seat)) => {
                a.request_token(qh, RequestData { app_id: None, seat_and_serial: Some((seat.clone(), self.press_serial)), surface, udata: inv });
            }
            _ => notify::send_invoke(inv, None),
        }
    }

    fn output_h(&self, i: usize) -> f32 {
        self.outputs.info(&self.panels[i].output).and_then(|o| o.logical_size).map_or(1080.0, |s| s.1 as f32)
    }

    /// `files <path>`: open it, in the window that is up if there is one.
    /// Unlike the bare command, a path never closes the window.
    fn show_in_files(&mut self, qh: &QueueHandle<Self>, i: usize, req: files::Request) {
        let open = (0..self.panels.len()).find(|&k| self.panels[k].win_ref(Win::Files).is_some());
        let Some(k) = open else {
            files::set_pending(req);
            self.toggle_window(qh, i, Win::Files);
            return;
        };
        if let Some(pp) = self.panels[k].win(Win::Files) {
            if pp.scene.closing() {
                pp.scene.reopen();
            }
            pp.scene.apply(&files::deliver(req));
        }
        self.request_popup_redraw(qh, k, Win::Files);
    }

    /// Open a generic window (launcher, monitor, wallpapers, toasts), or
    /// close it if it is already open.
    fn toggle_window(&mut self, qh: &QueueHandle<Self>, i: usize, w: Win) {
        // Notifications live in a single window, wherever it is.
        let i = if w == Win::Toasts { self.toasts_panel().unwrap_or(i) } else { i };
        if i >= self.panels.len() || !matches!(w, Win::Polkit | Win::Toasts) && self.polkit_open() {
            return;
        }
        if let Some(pp) = self.panels[i].win(w) {
            if pp.scene.closing() {
                pp.scene.reopen();
            } else if w == Win::Toasts {
                // The toasts window toggles its own history mode.
                pp.scene.apply(&Fact::Command("notifications".into()));
            } else {
                pp.scene.close();
            }
            self.request_popup_redraw(qh, i, w);
            return;
        }
        let (out_w, out_h) = (self.panels[i].bar.width, self.output_h(i));
        let mut ctx = self.ctx(i);
        let mut scene: Box<dyn WinScene> = match w {
            Win::Launcher => launcher::open(&mut ctx),
            Win::Monitor => monitor::open(&mut ctx),
            Win::Wallpapers => wallpaper::open(&mut ctx),
            Win::Files => files::open(&mut ctx),
            Win::Toasts => notify::open(&mut ctx),
            Win::TrayMenu => match traymenu::open(&mut ctx) {
                Some(s) => s,
                None => return,
            },
            Win::Polkit => polkit::open(&mut ctx),
            Win::Quick | Win::Full => return,
        };
        let (sw, sh) = scene.size();
        let (bl, bt) = match scene.placement() {
            Placement::Center => ((out_w - sw) * 0.5, (out_h - sh) * 0.5),
            Placement::Top(y) => ((out_w - sw) * 0.5, y),
            Placement::TopRight(m) => (out_w - sw - m, 0.0),
        };
        let (bl, bt) = (bl.round() as i32, bt.round() as i32);
        // Let the window be dragged anywhere on the output.
        scene.chrome_mut().bounds = (-bl as f32, out_w - sw - bl as f32, -bt as f32, out_h - sh - bt as f32);
        let popup = self.make_popup(qh, i, w.namespace(), PopScene::Dyn(scene), bl, bt);
        *self.panels[i].win(w) = Some(popup);
    }

    /// `path`: show this one (else the configured wallpaper, if any).
    fn create_backdrop(&mut self, qh: &QueueHandle<Self>, i: usize, path: Option<std::path::PathBuf>, frost: bool) {
        let mut ctx = self.ctx(i);
        let scene = match path {
            Some(p) => Some(wallpaper::Backdrop::with_path(&mut ctx, p, frost)),
            None => wallpaper::Backdrop::new(&mut ctx, frost),
        };
        let Some(scene) = scene else { return };
        let p = &self.panels[i];
        let surface = self.compositor.create_surface(qh);
        let ns = if frost { "nimbus-backdrop" } else { "nimbus-wallpaper" };
        let layer = self.layer_shell.create_layer_surface(qh, surface.clone(), Layer::Background, Some(ns), Some(&p.output));
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&self.compositor) {
            surface.set_input_region(Some(region.wl_region()));
        }
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, Viewport));
        layer.commit();
        *self.panels[i].bg(frost) = Some(Bg {
            gfx: None,
            viewport,
            layer,
            scene,
            size: (0, 0),
            configured: false,
            frame_pending: false,
            dirty: true,
            animating: false,
            clock: FrameClock::default(),
        });
    }

    fn request_backdrop_redraw(&mut self, qh: &QueueHandle<Self>, i: usize) {
        self.request_bg_redraw(qh, i, false);
        self.request_bg_redraw(qh, i, true);
    }

    fn request_bg_redraw(&mut self, qh: &QueueHandle<Self>, i: usize, frost: bool) {
        let Some(b) = self.panels[i].bg(frost).as_mut() else { return };
        if b.frame_pending {
            b.dirty = true;
        } else {
            self.draw_backdrop(qh, i, frost);
        }
    }

    fn draw_backdrop(&mut self, qh: &QueueHandle<Self>, i: usize, frost: bool) {
        let now_ns = monotonic_ns();
        let wall = (now_ns - self.start_ns) as f32 / 1e9;
        let Some(b) = self.panels[i].bg(frost).as_mut() else { return };
        if !b.configured || b.gfx.is_none() {
            return;
        }
        let dt = b.clock.advance(now_ns);
        b.scene.sync_time(wall - dt);
        b.animating = b.scene.step(dt);
        let wake = if b.animating { None } else { b.scene.wake_at().map(|t| (t, wall)) };
        b.scene.draw(&mut self.list, b.size.0 as f32, b.size.1 as f32);
        let surface = b.layer.wl_surface().clone();
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if b.animating {
            let _ = self.presentation.feedback(&surface, qh);
        }
        let empty = std::collections::HashMap::new();
        self.gpu.render(b.gfx.as_mut().unwrap(), &self.list, &empty, &self.icons);
        b.frame_pending = true;
        b.dirty = false;
        if let Some((at, now)) = wake {
            self.schedule_wake(qh, at, now);
        }
    }

    fn toggle_settings(&mut self, qh: &QueueHandle<Self>, i: usize) {
        if let Some(pp) = &mut self.panels[i].popup {
            if pp.scene.closing() {
                pp.scene.reopen();
            } else {
                pp.scene.close();
            }
            self.request_popup_redraw(qh, i, Win::Quick);
            return;
        }
        let p = &self.panels[i];
        let base_left = ((p.bar.width - settings::SW) * 0.5).round() as i32;
        let tab = p.bar.center_tab;
        let title = p.bar.title.cur.clone().or_else(|| Some("Desktop".into()));
        let scale = p.scale();
        let start = Rect::new(tab.x - base_left as f32, tab.y, tab.w, tab.h);
        let (out_w, out_h, base) = (p.bar.width, self.output_h(i), base_left as f32);
        let mut scene = Settings::new(&mut self.gpu.fonts, self.iconset.clone(), scale, start, title, &self.latest);
        scene.set_bounds(-base, out_w - settings::SW - base, out_h - settings::SH);
        let popup = self.make_popup(qh, i, "nimbus-settings", PopScene::Quick(scene), base_left, 0);
        self.panels[i].popup = Some(popup);
        services::read_system(self.fact_tx.clone(), std::time::Duration::ZERO);
    }

    /// The gear: the quick panel grows into the full settings window.
    fn open_prefs(&mut self, qh: &QueueHandle<Self>, i: usize) {
        if let Some(pp) = &mut self.panels[i].prefs {
            pp.scene.reopen();
            self.request_popup_redraw(qh, i, Win::Full);
            return;
        }
        let p = &self.panels[i];
        let (out_w, out_h) = (p.bar.width, self.output_h(i));
        let base_left = ((out_w - prefs::SW) * 0.5).round() as i32;
        let base_top = 8;
        // Start from the quick panel's on-screen body, in this surface's space.
        let start = match &p.popup {
            Some(q) => {
                let b = q.scene.body();
                Rect::new(b.x + (q.base_left + q.margin.0 - base_left) as f32, b.y + (q.base_top + q.margin.1 - base_top) as f32, b.w, b.h)
            }
            None => Rect::new(prefs::SW * 0.5 - 220.0, prefs::M, 440.0, 300.0),
        };
        let gpu_name = self.gpu.adapter.get_info().name;
        let mut scene = Prefs::new(&mut self.gpu.fonts, self.iconset.clone(), p.scale(), start, gpu_name, &self.latest);
        scene.attach(self.fact_tx.clone());
        let (bl, bt) = (base_left as f32, base_top as f32);
        scene.set_bounds(-bl - prefs::M, out_w - prefs::SW - bl + prefs::M, -bt - prefs::M + 8.0, out_h - prefs::SH - bt + prefs::M);
        let popup = self.make_popup(qh, i, "nimbus-prefs", PopScene::Full(scene), base_left, base_top);
        self.panels[i].prefs = Some(popup);
        for a in Prefs::initial_reads() {
            self.run_action(qh, i, a);
        }
        services::read_system(self.fact_tx.clone(), std::time::Duration::ZERO);
    }

    fn popup_pointer(&mut self, qh: &QueueHandle<Self>, i: usize, w: Win, ev: &PointerEvent) {
        let (x, y) = (ev.position.0 as f32, ev.position.1 as f32);
        let Some(pp) = self.panels[i].win(w).as_mut() else { return };
        let scene = &mut pp.scene;
        let mut action = None;
        let mut close = false;
        match ev.kind {
            PointerEventKind::Enter { serial } => {
                self.enter_serial = serial;
                self.cursor = None;
                scene.pointer_motion(x, y);
            }
            PointerEventKind::Leave { .. } => scene.pointer_leave(),
            PointerEventKind::Motion { .. } => action = scene.pointer_motion(x, y),
            PointerEventKind::Press { button: 0x110, serial, .. } => {
                self.press_serial = serial;
                action = scene.press(x, y);
            }
            PointerEventKind::Release { button: 0x110, .. } => (action, close) = scene.release(),
            PointerEventKind::Axis { horizontal, vertical, source, .. } => {
                // Touchpads: the scene may follow the fingers (both axes).
                let fingers = matches!(source, Some(wl_pointer::AxisSource::Finger | wl_pointer::AxisSource::Continuous))
                    || (vertical.value120 == 0 && vertical.discrete == 0 && horizontal.value120 == 0 && horizontal.discrete == 0);
                let end = vertical.stop || horizontal.stop;
                if !(fingers && scene.pan(horizontal.absolute as f32, vertical.absolute as f32, end)) {
                    let dy = if vertical.value120 != 0 {
                        vertical.value120 as f32 / 120.0
                    } else if vertical.discrete != 0 {
                        vertical.discrete as f32
                    } else {
                        vertical.absolute as f32 / 24.0
                    };
                    if dy != 0.0 {
                        action = scene.scroll(dy);
                    }
                }
            }
            _ => {}
        }
        let grabbing = scene.grabbing();
        let want = scene.cursor();
        if !matches!(ev.kind, PointerEventKind::Leave { .. }) {
            self.set_cursor(want);
        }
        match (grabbing, self.rel_pointer.is_some()) {
            (true, false) => {
                if let Some(p) = &self.pointer {
                    self.rel_pointer = self.relative.get_relative_pointer(p, qh).ok();
                }
            }
            (false, true) => {
                if let Some(r) = self.rel_pointer.take() {
                    r.destroy();
                }
            }
            _ => {}
        }
        if let Some(a) = action {
            self.run_action(qh, i, a);
        }
        if close {
            self.close_settings(qh, i, w);
        }
        if w == Win::Toasts {
            self.invoke_with_token(qh, i, w);
        }
        self.request_popup_redraw(qh, i, w);
    }

    fn close_settings(&mut self, qh: &QueueHandle<Self>, i: usize, w: Win) {
        if let Some(pp) = self.panels[i].win(w) {
            pp.scene.close();
            self.request_popup_redraw(qh, i, w);
        }
    }

    fn request_popup_redraw(&mut self, qh: &QueueHandle<Self>, i: usize, w: Win) {
        let Some(pp) = self.panels[i].win(w).as_mut() else { return };
        if pp.frame_pending {
            pp.dirty = true;
        } else {
            self.draw_popup(qh, i, w);
        }
    }

    fn draw_popup(&mut self, qh: &QueueHandle<Self>, i: usize, w: Win) {
        let t0 = Instant::now();
        let now_ns = monotonic_ns();
        // While a password prompt is up no other window may hold the keyboard.
        let yield_keys = w != Win::Polkit && self.polkit_open();
        let wall = (now_ns - self.start_ns) as f32 / 1e9;
        let p = &mut self.panels[i];
        let Some(pp) = p.win(w).as_mut() else { return };
        if !pp.configured || pp.gfx.is_none() {
            return;
        }
        let dt = pp.clock.advance(now_ns);
        pp.scene.sync_time(wall - dt);
        pp.animating = pp.scene.step(dt);
        let wake = if pp.animating { None } else { pp.scene.wake_at().map(|t| (t, pp.scene.time())) };
        // Move the surface by whole pixels; the scene draws the remainder.
        // Layer-shell state is applied on commit, together with this frame.
        let margin = pp.scene.margin();
        if margin != pp.margin {
            pp.layer.set_margin(pp.base_top + margin.1, 0, 0, pp.base_left + margin.0);
            pp.margin = margin;
        }
        pp.scene.set_surf_origin(margin);
        // Keyboard interactivity follows the scene (committed with this frame).
        if let PopScene::Dyn(d) = &pp.scene {
            let want = if yield_keys { window::Keyboard::None } else { d.keyboard() };
            if want != pp.kb {
                pp.kb = want;
                pp.layer.set_keyboard_interactivity(kb_mode(pp.kb));
            }
        }
        pp.scene.draw(&mut self.gpu.fonts, &mut self.list);
        let surface = pp.layer.wl_surface().clone();
        // Blur behind the panels the scene drew, rounded corners and all.
        if let Some(effect) = &pp.effect {
            let mut rows = Vec::new();
            for (r, rad) in &self.list.blur {
                rounded_rows(*r, *rad, &mut rows);
            }
            if rows != pp.last_blur
                && let Ok(region) = Region::new(&self.compositor)
            {
                for [x, y, w, h] in &rows {
                    region.add(*x, *y, *w, *h);
                }
                effect.0.set_blur_region(Some(region.wl_region()));
                pp.last_blur = rows;
            }
        }
        // Input region: the scene's rects (Quick extends to the top edge).
        let mut rects = pp.scene.input_region();
        if w == Win::Quick
            && let Some(r) = rects.first_mut()
        {
            r.h += r.y;
            r.y = 0.0;
        }
        let key = rects.iter().fold([0i32; 4], |acc, r| [acc[0] ^ r.x as i32, acc[1] ^ r.y as i32, acc[2] ^ r.w as i32, acc[3] ^ (r.h as i32).rotate_left(rects.len() as u32)]);
        if pp.last_input != Some(key)
            && let Ok(region) = Region::new(&self.compositor)
        {
            for r in &rects {
                region.add(r.x.floor() as i32, r.y.floor() as i32, r.w.ceil() as i32 + 1, r.h.ceil() as i32 + 1);
            }
            surface.set_input_region(Some(region.wl_region()));
            pp.last_input = Some(key);
        }
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if pp.animating {
            let _ = self.presentation.feedback(&surface, qh);
        }
        self.gpu.render(pp.gfx.as_mut().unwrap(), &self.list, pp.scene.text_buffers(), &self.icons);
        pp.frame_pending = true;
        pp.dirty = false;
        let first = !pp.shown;
        pp.shown = true;
        let done = pp.scene.done();
        match (w, done, first) {
            (Win::Quick, true, _) => {
                // Folded back into the tab: show the tab, drop the surface.
                p.popup = None;
                p.bar.popup_cover = 0.0;
                self.request_redraw(qh, i);
            }
            (Win::Quick, false, _) => {
                // While the panel is the tab, hide the bar's copy; it comes
                // back once the panel has floated away below the bar.
                let cover = match &pp.scene {
                    PopScene::Quick(q) => q.tab_cover(),
                    _ => 1.0,
                };
                if (cover - p.bar.popup_cover).abs() > 0.002 {
                    p.bar.popup_cover = cover;
                    self.request_redraw(qh, i);
                }
            }
            (_, true, _) => {
                if let Some(mut pp) = p.win(w).take() {
                    let mut frees = Vec::new();
                    pp.scene.release_textures(&mut frees);
                    self.gpu.free_textures(&frees);
                }
            }
            (Win::Full, false, true) => {
                // The full window took over from the quick panel.
                if p.popup.take().is_some() {
                    p.bar.popup_cover = 0.0;
                    self.request_redraw(qh, i);
                }
            }
            _ => {}
        }
        if let Some((at, now)) = wake {
            self.schedule_wake(qh, at, now);
        }
        if w == Win::Polkit && done {
            self.redraw_windows(qh);
        }
        self.stats.record(t0.elapsed().as_nanos() as u64, w.namespace());
    }

    fn set_cursor(&mut self, want: CursorShape) {
        if self.cursor != Some(want)
            && let Some(dev) = &self.shape_device
        {
            dev.set_shape(self.enter_serial, want);
            self.cursor = Some(want);
        }
    }
}

/// Approximate a capsule with per-row rectangles (wl_region is rect-only).
/// Rows are inset half a pixel so blur never peeks outside the glass edge.
fn capsule_rows(r: Rect, out: &mut Vec<[i32; 4]>) {
    rounded_rows(r, r.h * 0.5, out);
}

/// The same for a rounded rect of corner radius `rad`.
fn rounded_rows(r: Rect, rad: f32, out: &mut Vec<[i32; 4]>) {
    let rad = rad.min(r.h * 0.5).min(r.w * 0.5).max(0.0);
    let (y0, y1) = (r.y.ceil() as i32, (r.y + r.h).floor() as i32);
    let mut run: Option<[i32; 4]> = None;
    for y in y0..y1 {
        let yc = y as f32 + 0.5;
        let dy = (r.y + rad - yc).max(yc - (r.y + r.h - rad)).max(0.0);
        let inset = if dy < rad { rad - (rad * rad - dy * dy).sqrt() } else { rad };
        let x0 = (r.x + inset + 0.5).ceil() as i32;
        let x1 = (r.x + r.w - inset - 0.5).floor() as i32;
        if x1 <= x0 {
            continue;
        }
        match &mut run {
            Some(cur) if cur[0] == x0 && cur[2] == x1 - x0 && cur[1] + cur[3] == y => cur[3] += 1,
            _ => {
                if let Some(c) = run.take() {
                    out.push(c);
                }
                run = Some([x0, y, x1 - x0, 1]);
            }
        }
    }
    out.extend(run);
}

// ── protocol glue ───────────────────────────────────────────────────────

struct FracMgr;
struct FracScale;
struct Viewporter;
struct Viewport;

impl Dispatch2<WpFractionalScaleManagerV1, App> for FracMgr {
    fn event(&self, _: &mut App, _: &WpFractionalScaleManagerV1, _: <WpFractionalScaleManagerV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<wp_fractional_scale_v1::WpFractionalScaleV1, App> for FracScale {
    fn event(
        &self,
        app: &mut App,
        proxy: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &Connection,
        qh: &QueueHandle<App>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && let Some(i) = app.panels.iter().position(|p| p.fractional.as_ref() == Some(proxy))
            && app.panels[i].scale120 != scale
        {
            app.panels[i].scale120 = scale;
            app.reconfigure(i);
            app.request_redraw(qh, i);
            app.request_backdrop_redraw(qh, i);
        }
    }
}

impl Dispatch2<WpViewporter, App> for Viewporter {
    fn event(&self, _: &mut App, _: &WpViewporter, _: <WpViewporter as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<WpViewport, App> for Viewport {
    fn event(&self, _: &mut App, _: &WpViewport, _: <WpViewport as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, qh: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        if self.fractional_mgr.is_none()
            && let Some(i) = self.panel_of(surface)
        {
            self.panels[i].scale120 = factor.max(1) as u32 * 120;
            self.reconfigure(i);
            self.request_redraw(qh, i);
            self.request_backdrop_redraw(qh, i);
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some((i, frost)) = self.backdrop_of(surface) {
            let b = self.panels[i].bg(frost).as_mut().unwrap();
            b.frame_pending = false;
            if b.animating || b.dirty {
                self.draw_backdrop(qh, i, frost);
            }
            return;
        }
        if let Some((i, w)) = self.popup_of(surface) {
            let pp = self.panels[i].win(w).as_mut().unwrap();
            pp.frame_pending = false;
            if pp.animating || pp.dirty {
                self.draw_popup(qh, i, w);
            }
            return;
        }
        if let Some(i) = self.panel_of(surface) {
            let p = &mut self.panels[i];
            p.frame_pending = false;
            if p.animating || p.dirty {
                self.draw(qh, i);
            }
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl PresentationTimeHandler for App {
    fn presentation_time_state(&mut self) -> &mut PresentationTimeState {
        &mut self.presentation
    }
    fn presented(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        feedback: &wp_presentation_feedback::WpPresentationFeedback,
        surface: &wl_surface::WlSurface,
        _: Vec<wl_output::WlOutput>,
        time: PresentTime,
        refresh: u32,
        _: u64,
        _: WEnum<wp_presentation_feedback::Kind>,
    ) {
        let _ = feedback;
        if time.clk_id as i32 == libc::CLOCK_MONOTONIC
            && let Some(i) = self.panel_of(surface)
        {
            let c = &mut self.panels[i].clock;
            c.last_present = Some(time.tv_sec * 1_000_000_000 + time.tv_nsec as u64);
            if refresh > 0 {
                c.refresh = refresh as u64;
            }
        }
    }
    fn discarded(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wp_presentation_feedback::WpPresentationFeedback, _: &wl_surface::WlSurface) {}
}

impl BackgroundEffectHandler for App {
    fn background_effect_state(&mut self) -> &mut BackgroundEffectState {
        &mut self.effects
    }
    fn update_capabilities(&mut self) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_output(qh, output);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.remove_panels(|p| p.output == output);
        self.pointer_panel = None;
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some((i, frost)) = self.backdrop_of(layer.wl_surface()) {
            let b = self.panels[i].bg(frost).take();
            self.release_backdrop(b);
            return;
        }
        if let Some((i, w)) = self.popup_of_layer(layer) {
            let pp = self.panels[i].win(w).take();
            self.release_popup(pp);
            if w == Win::Quick {
                self.panels[i].bar.popup_cover = 0.0;
            }
            if w == Win::Polkit {
                self.redraw_windows(qh);
            }
            return;
        }
        self.remove_panels(|p| &p.layer == layer);
        self.pointer_panel = None;
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface, cfg: LayerSurfaceConfigure, _: u32) {
        if let Some((i, frost)) = self.backdrop_of(layer.wl_surface()) {
            let scale = self.panels[i].scale();
            let fallback = (self.panels[i].size.0.max(1), 1080);
            let b = self.panels[i].bg(frost).as_mut().unwrap();
            b.size = (if cfg.new_size.0 > 0 { cfg.new_size.0 } else { fallback.0 }, if cfg.new_size.1 > 0 { cfg.new_size.1 } else { fallback.1 });
            b.configured = true;
            if let Some(vp) = &b.viewport {
                vp.set_destination(b.size.0 as i32, b.size.1 as i32);
            }
            let (pw, ph) = ((b.size.0 as f32 * scale).round() as u32, (b.size.1 as f32 * scale).round() as u32);
            match &mut b.gfx {
                Some(g) => self.gpu.resize(g, pw, ph),
                None => {
                    let ptr = b.layer.wl_surface().id().as_ptr() as *mut std::ffi::c_void;
                    b.gfx = Some(self.gpu.create_surface(ptr, pw, ph));
                }
            }
            b.dirty = true;
            self.request_bg_redraw(qh, i, frost);
            return;
        }
        if let Some((i, w)) = self.popup_of_layer(layer) {
            let scale = self.panels[i].scale();
            let pp = self.panels[i].win(w).as_mut().unwrap();
            let _ = cfg;
            pp.configured = true;
            let (sw, sh) = pp.scene.size();
            if let Some(vp) = &pp.viewport {
                vp.set_destination(sw as i32, sh as i32);
            }
            if pp.gfx.is_none() {
                let ptr = pp.layer.wl_surface().id().as_ptr() as *mut std::ffi::c_void;
                pp.gfx = Some(self.gpu.create_surface(ptr, (sw * scale).round() as u32, (sh * scale).round() as u32));
            }
            self.request_popup_redraw(qh, i, w);
            return;
        }
        let Some(i) = self.panels.iter().position(|p| &p.layer == layer) else { return };
        let p = &mut self.panels[i];
        let w = if cfg.new_size.0 > 0 {
            cfg.new_size.0
        } else {
            self.outputs.info(&p.output).and_then(|o| o.logical_size).map_or(1920, |s| s.0 as u32)
        };
        p.size = (w, cfg.new_size.1.max(bar::SURFACE_H));
        let first = !p.configured;
        p.configured = true;
        self.reconfigure(i);
        self.request_redraw(qh, i);
        // A saved wallpaper: show it on this output from the start.
        if first {
            for frost in [false, true] {
                if self.panels[i].bg(frost).is_none() {
                    self.create_backdrop(qh, i, None, frost);
                }
            }
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let cb: smithay_client_toolkit::seat::keyboard::repeat::RepeatCallback<App> =
                Box::new(|app: &mut App, _kb, ev| app.on_key(ev, true));
            self.keyboard = self.seats.get_keyboard_with_repeat(qh, &seat, None, self.handle.clone(), cb).ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            if let Ok(p) = self.seats.get_pointer(qh, &seat) {
                self.seat = Some(seat.clone());
                self.shape_device = self.cursor_shapes.as_ref().map(|m| m.get_shape_device(&p, qh));
                self.pointer = Some(p);
            }
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if cap == Capability::Pointer {
            if let Some(d) = self.shape_device.take() {
                d.destroy();
            }
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for ev in events {
            if let Some((i, w)) = self.popup_of(&ev.surface) {
                self.popup_pointer(qh, i, w, ev);
                continue;
            }
            let Some(i) = self.panel_of(&ev.surface) else { continue };
            let (x, y) = (ev.position.0 as f32, ev.position.1 as f32);
            let bar = &mut self.panels[i].bar;
            let mut cmd = None;
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.enter_serial = serial;
                    self.cursor = None;
                    self.pointer_panel = Some(i);
                    bar.pointer_motion(x, y);
                }
                PointerEventKind::Leave { .. } => {
                    bar.pointer_leave();
                    self.pointer_panel = None;
                }
                PointerEventKind::Motion { .. } => {
                    bar.pointer_motion(x, y);
                }
                PointerEventKind::Press { button: 0x110, .. } => bar.press(),
                PointerEventKind::Release { button: 0x110, .. } => cmd = bar.release(),
                PointerEventKind::Release { button: b @ (0x111 | 0x112), .. } => cmd = bar.release_button(b),
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.value120 != 0 {
                        vertical.value120 as f32 / 120.0
                    } else if vertical.discrete != 0 {
                        vertical.discrete as f32
                    } else {
                        vertical.absolute as f32 / 24.0
                    };
                    if dy != 0.0 {
                        cmd = bar.scroll(dy);
                    }
                }
                _ => {}
            }
            let want = if bar.hovered.clickable() { CursorShape::Pointer } else { CursorShape::Default };
            if !matches!(ev.kind, PointerEventKind::Leave { .. }) {
                self.set_cursor(want);
            }
            if let Some(c) = cmd {
                self.run(qh, i, c);
            }
            self.request_redraw(qh, i);
        }
    }
}

impl ActivationHandler for App {
    type RequestUdata = notify::Invoke;
    fn new_token(&mut self, token: String, data: &RequestData<notify::Invoke>) {
        notify::send_invoke(data.udata.clone(), Some(token));
    }
}

impl App {
    /// Tell an open password prompt about the keyboard's state.
    fn polkit_hint(&mut self, qh: &QueueHandle<Self>, f: Fact) {
        for i in 0..self.panels.len() {
            if let Some(pp) = self.panels[i].win(Win::Polkit)
                && pp.scene.apply_custom(&f)
            {
                self.request_popup_redraw(qh, i, Win::Polkit);
            }
        }
    }

    /// Free the textures of a panel's windows before the panel goes.
    fn release_windows(&mut self, i: usize) {
        let mut frees = Vec::new();
        for w in Win::ALL {
            if let Some(pp) = self.panels[i].win(w) {
                pp.scene.release_textures(&mut frees);
            }
        }
        self.gpu.free_textures(&frees);
    }

    fn on_key(&mut self, ev: KeyEvent, repeat: bool) {
        let Some(qh) = self.qh.clone() else { return };
        let Some((i, w)) = self.kb_focus.as_ref().and_then(|s| self.popup_of(s)) else { return };
        let mut k = KeyInput { sym: ev.keysym.raw(), text: ev.utf8, ctrl: self.mods.ctrl, shift: self.mods.shift, alt: self.mods.alt };
        let Some(pp) = self.panels[i].win(w).as_mut() else { return };
        let (action, close) = pp.scene.key(&k, repeat);
        // A character of a password: don't leave it on the heap.
        if w == Win::Polkit {
            polkit::wipe_text(k.text.take());
        }
        if let Some(a) = action {
            self.run_action(&qh, i, a);
        }
        if close {
            self.close_settings(&qh, i, w);
        }
        self.request_popup_redraw(&qh, i, w);
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        self.kb_focus = Some(surface.clone());
        for i in 0..self.panels.len() {
            if self.panels[i].bar.hold_focus(true) {
                self.request_redraw(qh, i);
            }
        }
        if self.popup_of(surface).is_some_and(|(_, w)| w == Win::Polkit) {
            self.polkit_hint(qh, polkit::focus_fact(true));
        }
    }
    fn leave(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        self.kb_focus = None;
        for p in &mut self.panels {
            p.bar.hold_focus(false);
        }
        // Focus went elsewhere (a click outside): fold the window away,
        // unless focus moved quick panel → full window, or it's the toasts,
        // or a password prompt took it (the prompt itself never closes on
        // focus loss, and windows get their focus back after it).
        if self.popup_of(surface).is_some_and(|(_, w)| w == Win::Polkit) {
            self.polkit_hint(qh, polkit::focus_fact(false));
        }
        if self.polkit_open() {
            return;
        }
        if let Some((i, w)) = self.popup_of(surface)
            && !(w == Win::Quick && self.panels[i].prefs.is_some())
            && w != Win::Toasts
        {
            self.close_settings(qh, i, w);
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        self.on_key(ev, false);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        self.on_key(ev, true);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        let caps = m.caps_lock != self.mods.caps_lock;
        self.mods = m;
        if caps {
            self.polkit_hint(qh, polkit::caps_fact(m.caps_lock));
        }
    }
}

impl RelativePointerHandler for App {
    fn relative_pointer_motion(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &ZwpRelativePointerV1,
        _: &wl_pointer::WlPointer,
        ev: RelativeMotionEvent,
    ) {
        for i in 0..self.panels.len() {
            for w in Win::ALL {
                if let Some(pp) = self.panels[i].win(w)
                    && pp.scene.grabbing()
                {
                    pp.scene.drag_by(ev.delta.0 as f32, ev.delta.1 as f32);
                    self.request_popup_redraw(qh, i, w);
                }
            }
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
delegate_dispatch2!(App);

// ── main ────────────────────────────────────────────────────────────────

fn main() {
    // Before `args()`: a path need not be UTF-8.
    if let Some(code) = files::decode_child_main().or_else(files::msg_main) {
        std::process::exit(code);
    }
    let args: Vec<String> = std::env::args().collect();
    // The session locker: its own process mode, before any bar setup.
    if args.get(1).map(String::as_str) == Some("lock") {
        std::process::exit(lock::run());
    }
    if args.get(1).map(String::as_str) == Some("msg") {
        let cmd = args.get(2).map_or("", |s| s.as_str());
        if let Err(e) = ipc::send(cmd) {
            eprintln!("nimbus msg: {e} (is nimbus running?)");
            std::process::exit(1);
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("weather-dump") {
        weather::dump();
        return;
    }
    if args.get(1).map(String::as_str) == Some("tray-dump") {
        tray::dump(args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4.0));
        return;
    }
    if args.get(1).map(String::as_str) == Some("media-dump") {
        match args.iter().position(|a| a == "--watch") {
            Some(i) => media::watch(args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(10.0)),
            None => media::dump(),
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        let dir = args.get(i + 1).map_or("nimbus-snapshots", |s| s.as_str());
        snapshot::run(std::path::Path::new(dir));
        return;
    }
    let conn = Connection::connect_to_env().expect("nimbus: no Wayland display");
    let (globals, queue) = registry_queue_init::<App>(&conn).expect("registry");
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell");
    let fractional_mgr = globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, FracMgr).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, Viewporter).ok();
    let effects = BackgroundEffectState::new(&globals, &qh);
    let presentation = PresentationTimeState::bind(&globals, &qh);
    let cursor_shapes = CursorShapeManager::bind(&globals, &qh).ok();
    let relative = RelativePointerState::bind(&globals, &qh);
    let activation = ActivationState::bind(&globals, &qh).ok();
    let mut toggles = toggles::Toggles::new();
    toggles.gamma_mgr = globals.bind(&qh, 1..=1, toggles::GammaMgr).ok();
    toggles.idle_mgr = globals.bind(&qh, 1..=1, toggles::IdleMgr).ok();

    let gpu = Gpu::new(Some(conn.backend().display_ptr() as *mut _));
    let iconset = Rc::new(IconSet::discover());
    let icons = Icons::load(&iconset.paths);

    let mut ev: EventLoop<'static, App> = EventLoop::try_new().expect("event loop");
    let handle = ev.handle();
    let (tx, rx) = channel::channel::<Fact>();
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        compositor,
        layer_shell,
        fractional_mgr,
        viewporter,
        effects,
        presentation,
        cursor_shapes,
        pointer: None,
        shape_device: None,
        enter_serial: 0,
        press_serial: 0,
        seat: None,
        activation,
        cursor: None,
        pointer_panel: None,
        gpu,
        icons,
        iconset,
        panels: vec![],
        latest: vec![],
        actions: NiriActions::spawn(),
        list: DrawList::default(),
        stats: Stats { enabled: std::env::var_os("NIMBUS_STATS").is_some(), ..Default::default() },
        handle: handle.clone(),
        start_ns: monotonic_ns(),
        wake_pending: None,
        keyboard: None,
        kb_focus: None,
        mods: Modifiers::default(),
        qh: Some(qh.clone()),
        relative,
        rel_pointer: None,
        fact_tx: tx.clone(),
        volume_applier: Applier::new("volume", services::set_volume),
        brightness_applier: Applier::new("brightness", services::set_brightness),
        mic_applier: Applier::new("mic", services::set_mic_volume),
        started: Instant::now(),
        toggles,
    };
    WaylandSource::new(conn, queue).insert(handle.clone()).expect("wayland source");

    let qh2 = qh.clone();
    handle
        .insert_source(rx, move |e, _, app: &mut App| {
            if let channel::Event::Msg(f) = e {
                app.on_fact(&qh2, f);
            }
        })
        .expect("fact channel");

    let bench = std::env::var("NIMBUS_BENCH").ok();
    if let Some(mode) = bench {
        // Synthetic, deterministic workload shared with the QML twin: every
        // 350 ms the active workspace moves and the title changes. In "idle"
        // mode the state is set once and never changes.
        let qh3 = qh.clone();
        let mut n = 0u64;
        let anim = mode != "idle";
        handle
            .insert_source(Timer::from_duration(std::time::Duration::from_millis(300)), move |_, _, app: &mut App| {
                let outputs: Vec<String> = app.panels.iter().map(|p| p.bar.output.clone()).collect();
                for o in outputs {
                    app.on_fact(&qh3, services::bench_snapshot(0, &o));
                }
                TimeoutAction::Drop
            })
            .expect("bench init");
        let qh3 = qh.clone();
        if anim { handle
            .insert_source(Timer::from_duration(std::time::Duration::from_millis(350)), move |_, _, app: &mut App| {
                n += 1;
                let outputs: Vec<String> = app.panels.iter().map(|p| p.bar.output.clone()).collect();
                for o in outputs {
                    app.on_fact(&qh3, services::bench_snapshot(n, &o));
                }
                TimeoutAction::ToDuration(std::time::Duration::from_millis(350))
            })
            .expect("bench timer"); }
        services::spawn_bench(tx);
    } else {
        ipc::serve(tx.clone());
        notify::start_daemon(tx.clone());
        files::start_dbus(tx.clone());
        polkit::start(tx.clone());
        weather::spawn(tx.clone());
        services::spawn_all(tx);
    }
    app.push_toggles(&qh);

    loop {
        ev.dispatch(None, &mut app).expect("dispatch");
    }
}
