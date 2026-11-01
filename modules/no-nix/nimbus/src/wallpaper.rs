//! Wallpapers: nimbus draws the desktop background itself (one Background
//! layer surface per output, see `backdrop`) and offers a cover-flow
//! selector window (`selector`, command "wallpapers").
//!
//! The choice is remembered in the config ("wallpaper" key). Applying one
//! broadcasts a `WallpaperChange` (as `Fact::Custom`) carrying the card it
//! was picked from and that card's picture: every backdrop grows the new
//! wallpaper out of the card at once and sharpens it when its own
//! full-size decode lands.

mod backdrop;
pub(crate) mod loader;
mod selector;
mod shapes;

use std::{collections::HashMap, path::PathBuf};

pub use backdrop::Backdrop;
pub use loader::fit;
use glyphon::FontSystem;

use crate::{
    gfx::{DrawList, ImageData},
    services::Fact,
    window::{Chrome, Ctx, Placement, WinScene},
};

/// "Show this wallpaper": sent by the selector, applied by every backdrop.
pub struct WallpaperChange {
    pub path: PathBuf,
    /// The card it was picked from (x, y, w, h) in the sending output's
    /// logical coordinates, and its corner radius.
    pub card: [f32; 4],
    #[allow(dead_code)]
    pub radius: f32,
    /// That output's logical size (other outputs map the card by ratio).
    pub out: (f32, f32),
    /// The card's picture (same crop as the screen). With it the wallpaper
    /// grows out of the card right away; without, it reveals in a circle
    /// from the card's centre once decoded.
    pub preview: Option<ImageData>,
    /// Seconds to wait on the card before growing.
    pub delay: f32,
}

/// The wallpaper selector window.
pub fn open(ctx: &mut Ctx) -> Box<dyn WinScene> {
    Box::new(selector::Selector::new(ctx, true))
}

/// The wallpaper a fact asks to show, if any: creates the desktop
/// background surface where none exists yet (even for a preview that is
/// not in the config).
pub fn wants_backdrop(f: &Fact) -> Option<PathBuf> {
    match f {
        Fact::Custom(a) => a.downcast_ref::<WallpaperChange>().map(|c| c.path.clone()),
        _ => None,
    }
}

// ── demo ────────────────────────────────────────────────────────────────

/// Lets `Demo::pump` deliver worker results to a backdrop.
struct Probe<'a> {
    b: &'a mut Backdrop,
    chrome: Chrome,
    bufs: HashMap<u64, glyphon::Buffer>,
    redraw: bool,
    /// An order map arrived; its analysis time.
    ms: Option<f32>,
}

impl WinScene for Probe<'_> {
    fn size(&self) -> (f32, f32) {
        (1.0, 1.0)
    }
    fn placement(&self) -> Placement {
        Placement::Center
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }
    fn apply(&mut self, f: &Fact) {
        if let Fact::Custom(a) = f
            && let Some(s) = a.downcast_ref::<backdrop::Shaped>()
        {
            self.ms = Some(s.ms);
        }
        self.redraw |= self.b.apply(f);
    }
    fn set_scale(&mut self, _: f32) {}
    fn step(&mut self, _: f32) -> bool {
        false
    }
    fn draw(&mut self, _: &mut FontSystem, out: &mut DrawList) {
        out.clear();
    }
    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.bufs
    }
}

/// Headless demo for `nimbus --snapshot` (NIMBUS_SNAPSHOT_ONLY=wallpaper).
pub fn demo(d: &mut crate::snapshot::Demo) {
    let mut w = selector::Selector::new(&mut d.ctx(), false);
    if w.len() == 0 {
        d.tick(&mut w, 1.0);
        d.shoot(&mut w, "wallpaper_empty");
        return;
    }
    // Open: the window grows out of the centre card, the rest fan out.
    for i in 0..10 {
        d.pump(&mut w, 0.0);
        d.shoot(&mut w, &format!("wallpaper_open_{i:02}"));
        d.tick(&mut w, DT * 5.0);
    }
    let t0 = std::time::Instant::now();
    while w.loading() && t0.elapsed().as_secs_f32() < 20.0 {
        d.pump(&mut w, 0.05);
        d.tick(&mut w, DT * 2.0);
    }
    // Thumbnails iris in as they arrive.
    d.shoot(&mut w, "wallpaper_reveal");
    d.tick(&mut w, 1.2);
    d.pump(&mut w, 0.6);
    d.tick(&mut w, 0.6);
    d.shoot(&mut w, "wallpaper_rest");
    if w.step(DT) {
        eprintln!("nimbus: wallpaper selector still animating at rest");
    }

    // Scroll two notches: frames mid-motion.
    w.demo_wheel(1.0);
    w.demo_wheel(1.0);
    for i in 0..8 {
        d.shoot(&mut w, &format!("wallpaper_scroll_{i:02}"));
        d.tick(&mut w, DT * 4.0);
    }
    d.tick(&mut w, 1.0);
    d.pump(&mut w, 0.8);
    d.tick(&mut w, 0.5);
    d.shoot(&mut w, "wallpaper_scrolled");
    w.demo_hover_card(1);
    d.tick(&mut w, 0.5);
    d.shoot(&mut w, "wallpaper_hover");
    w.demo_hover_card(0);

    // Drag-fling to the left: momentum, then it snaps onto a card.
    let c = w.centre_card();
    let (cx, cy) = (c.x + c.w * 0.5, c.y + c.h * 0.5);
    w.pointer_motion(cx, cy);
    w.press(cx, cy);
    for k in 1..=8 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        w.pointer_motion(cx - 40.0 * k as f32, cy);
        d.tick(&mut w, DT);
    }
    w.release();
    for i in 0..6 {
        d.shoot(&mut w, &format!("wallpaper_drag_{i:02}"));
        d.tick(&mut w, DT * 6.0);
    }
    w.pointer_leave();
    d.tick(&mut w, 1.0);
    d.pump(&mut w, 0.5);
    d.tick(&mut w, 0.5);
    d.shoot(&mut w, "wallpaper_dragged");

    // Two-finger swipe: the carousel follows the fingers, then settles.
    for i in 0..6 {
        w.pan(-45.0, 3.0, false);
        std::thread::sleep(std::time::Duration::from_millis(8));
        d.tick(&mut w, DT);
        if i == 3 {
            d.shoot(&mut w, "wallpaper_pan_mid");
        }
    }
    w.pan(0.0, 0.0, true);
    d.tick(&mut w, 1.5);
    d.shoot(&mut w, "wallpaper_panned");
    if w.step(DT) {
        eprintln!("nimbus: wallpaper selector still animating after a pan");
    }

    // The desktop before: the old wallpaper, decoded for the output.
    let Some(old) = w.path_at(-2) else { return };
    let out = (1828.0, 1142.0);
    let mut b = Backdrop::with_path(&mut d.ctx(), old, false);
    let mut list = DrawList::default();
    settle(d, &mut b, &mut list);
    d.render(&list, &HashMap::new(), out, "wallpaper_backdrop_old");

    // Enter on the centred card: the window folds into it while the new
    // wallpaper floods out of the same card along its own shapes (read off
    // the thumbnail at once, sharpened when the full-size decode lands).
    if w.demo_apply().is_none() {
        return;
    }
    w.close();
    wait(d, &mut b, 20.0);
    let mut t = 0.0;
    for (k, at) in [0.0f32, 0.2, 0.35, 0.5, 0.7, 1.0].into_iter().enumerate() {
        while t < at - 1e-4 {
            b.step(DT);
            w.step(DT);
            t += DT;
        }
        pump_backdrop(d, &mut b, 0.0);
        b.draw(&mut list, out.0, out.1);
        d.render(&list, &HashMap::new(), out, &format!("wallpaper_backdrop_{k}"));
        d.shoot(&mut w, &format!("wallpaper_close_{k}"));
    }
    d.tick(&mut w, 1.0);
    if !w.chrome().done() {
        eprintln!("nimbus: wallpaper selector did not finish closing");
    }
    wait(d, &mut b, 20.0);
    settle(d, &mut b, &mut list);
    eprintln!("nimbus: backdrop at rest: {} image(s)", list.images.len());

    // Space previews, Escape puts the kept one back.
    let mut w2 = selector::Selector::new(&mut d.ctx(), false);
    d.tick(&mut w2, 1.0);
    w2.demo_wheel(1.0);
    d.tick(&mut w2, 1.0);
    w2.demo_preview();
    w2.close();
    wait(d, &mut b, 20.0);
    settle(d, &mut b, &mut list);
    d.render(&list, &HashMap::new(), out, "wallpaper_backdrop_reverted");
    let mut frees = vec![];
    b.release(&mut frees);
    eprintln!("nimbus: backdrop released {} texture(s)", frees.len());
    d.render(&DrawList { frees, ..Default::default() }, &HashMap::new(), (4.0, 4.0), "wallpaper_backdrop_freed");

    // Films of the shape wipe between real wallpapers: one picked from a
    // card (thumbnail first), one without a picture (full decode only).
    let dir = loader::wallpaper_dir().join("nimbus");
    let card = [(1828.0 - 520.0) * 0.5, (1142.0 - 330.0) * 0.5 + 40.0, 520.0, 330.0];
    film(d, &dir.join("iris-haze.jpg"), &dir.join("mint-waves.jpg"), card, true, "wallpaper_film_a");
    film(d, &dir.join("graphite-dunes.jpg"), &dir.join("amber-horizon.jpg"), [1180.0, 640.0, 300.0, 190.0], false, "wallpaper_film_b");
    film(d, &dir.join("mint-waves.jpg"), &dir.join("iris-ribbons.jpg"), [140.0, 160.0, 300.0, 190.0], true, "wallpaper_film_c");
}

const DT: f32 = 1.0 / 120.0;

fn probe(b: &mut Backdrop) -> Probe<'_> {
    let cfg = crate::anim::presets::SMOOTH;
    Probe { b, chrome: Chrome::new(Default::default(), Default::default(), cfg, cfg), bufs: HashMap::new(), redraw: false, ms: None }
}

/// Deliver worker facts to `b` for up to `secs`; true if one asked to redraw.
fn pump_backdrop(d: &mut crate::snapshot::Demo, b: &mut Backdrop, secs: f32) -> bool {
    let mut p = probe(b);
    d.pump(&mut p, secs);
    if let Some(ms) = p.ms {
        eprintln!("nimbus: wallpaper shapes analysed in {ms:.1} ms");
    }
    p.redraw
}

/// Until a worker fact asks for a redraw (or `secs` of wall time pass).
fn wait(d: &mut crate::snapshot::Demo, b: &mut Backdrop, secs: f32) {
    let t0 = std::time::Instant::now();
    while !pump_backdrop(d, b, 0.05) && t0.elapsed().as_secs_f32() < secs {}
}

/// Let every pending decode / analysis land and every animation finish.
fn settle(d: &mut crate::snapshot::Demo, b: &mut Backdrop, list: &mut DrawList) {
    let t0 = std::time::Instant::now();
    loop {
        pump_backdrop(d, b, 0.05);
        let mut moving = false;
        for _ in 0..60 {
            moving |= b.step(DT);
        }
        b.draw(list, 1828.0, 1142.0);
        // Uploads / frees must reach the GPU even for frames not kept.
        let keep = DrawList { uploads: std::mem::take(&mut list.uploads), frees: std::mem::take(&mut list.frees), ..Default::default() };
        d.render(&keep, &HashMap::new(), (4.0, 4.0), "wallpaper_scratch");
        if (!moving && !b.busy()) || t0.elapsed().as_secs_f32() > 20.0 {
            break;
        }
    }
    b.draw(list, 1828.0, 1142.0);
}

/// GPU time of one full-screen layer of `list` drawn masked and plain
/// (ms): the difference of 9 vs 1 stacked copies, so the readback cancels.
fn gpu_cost(d: &mut crate::snapshot::Demo, list: &DrawList, out: (f32, f32)) -> (f32, f32) {
    let Some(top) = list.images.iter().rev().find(|i| i.mask.is_some()).copied() else { return (0.0, 0.0) };
    let px = ((out.0 * d.scale).round() as u32, (out.1 * d.scale).round() as u32);
    let mut target = d.gpu.create_offscreen(px.0, px.1);
    let mut time = |img: crate::gfx::ImageDraw, n: usize| {
        let l = DrawList { images: vec![img; n], ..Default::default() };
        let _ = d.gpu.render_offscreen(&mut target, &l, &HashMap::new(), d.icons);
        let t0 = std::time::Instant::now();
        for _ in 0..15 {
            let _ = d.gpu.render_offscreen(&mut target, &l, &HashMap::new(), d.icons);
        }
        t0.elapsed().as_secs_f32() * 1000.0 / 15.0
    };
    let mut plain = top;
    plain.mask = None;
    let m = (time(top, 9) - time(top, 1)) / 8.0;
    let p = (time(plain, 9) - time(plain, 1)) / 8.0;
    (m, p)
}

/// Render the change `from` → `to` as 20 frames across the wipe, plus a
/// timing line. `card` is the rect the change comes from (logical px).
fn film(d: &mut crate::snapshot::Demo, from: &std::path::Path, to: &std::path::Path, card: [f32; 4], preview: bool, name: &str) {
    if !from.is_file() || !to.is_file() {
        return;
    }
    let out = (1828.0f32, 1142.0f32);
    let mut b = Backdrop::with_path(&mut d.ctx(), from.to_path_buf(), false);
    let mut list = DrawList::default();
    settle(d, &mut b, &mut list);
    let preview = preview.then(|| loader::decode_cover(to, 672, 420).map(|p| p.0)).flatten();
    let change = WallpaperChange { path: to.to_path_buf(), card, radius: 22.0, out, preview, delay: 0.0 };
    b.apply(&Fact::Custom(std::sync::Arc::new(change)));
    wait(d, &mut b, 20.0);
    let frames = 20;
    let span = 1.5f32;
    let mut cpu = 0f32;
    let mut bench = None;
    let mut t = 0.0;
    for k in 0..frames {
        let at = span * k as f32 / (frames - 1) as f32;
        let c0 = std::time::Instant::now();
        while t < at - 1e-4 {
            b.step(DT);
            t += DT;
        }
        // The full-size decode lands whenever it is ready, as live.
        pump_backdrop(d, &mut b, 0.0);
        b.draw(&mut list, out.0, out.1);
        cpu += c0.elapsed().as_secs_f32();
        d.render(&list, &HashMap::new(), out, &format!("{name}_{k:02}"));
        if k == frames / 2 {
            bench = Some(gpu_cost(d, &list, out));
        }
    }
    let (masked, plain) = bench.unwrap_or_default();
    eprintln!(
        "nimbus: {name}: step+draw {:.3} ms/frame; GPU per full-screen layer {masked:.2} ms masked vs {plain:.2} ms plain",
        cpu * 1000.0 / frames as f32
    );
    settle(d, &mut b, &mut list);
    if b.step(DT) {
        eprintln!("nimbus: {name}: backdrop still animating after the wipe");
    }
    eprintln!("nimbus: {name}: at rest with {} image(s), {} texture(s) owned", list.images.len(), b.textures());
    let mut frees = vec![];
    b.release(&mut frees);
    d.render(&DrawList { frees, ..Default::default() }, &HashMap::new(), (4.0, 4.0), "wallpaper_scratch");
}
