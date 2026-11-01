//! The wallpaper itself: one Background layer surface per output, drawn as
//! a single full-screen texture decoded at the output's physical size.
//!
//! A change floods the new picture in along its own shapes: a short-lived
//! thread reads the picture (the selector card's thumbnail at once, or the
//! full-size decode) and builds an order map from where the change came
//! from (`shapes`); the layer is then wiped in through that map with a
//! glassy front (`wipe.wgsl`) while it settles from a slight zoom. The
//! full-size decode sharpens a thumbnail layer in place. Covered layers and
//! finished order maps are freed; at rest exactly one texture exists and
//! nothing animates.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use super::{WallpaperChange, loader, shapes};
use crate::{
    anim::{Spring, SpringCfg},
    gfx::{DrawList, ImageData, ImageDraw},
    services::Fact,
    window::Ctx,
};

const ZOOM: SpringCfg = SpringCfg::new(1.5, 1.0);
/// Thumbnail → full-size picture, in place.
const SHARPEN: SpringCfg = SpringCfg::new(0.32, 1.0);
const ZOOM_FROM: f32 = 1.05;
/// Length of the shape wipe.
const WIPE_SECS: f32 = 1.3;
/// Width of the wipe front, logical px.
const FRONT: f32 = 22.0;
/// Glass refraction at the front, logical px.
const REFRACT: f32 = 6.0;
/// Rim highlight strength at the front (premultiplied, ≤ ~0.15).
const RIM: f32 = 0.1;
/// Most textures alive at once (during rapid changes).
const CAP: usize = 3;
/// A frosted backdrop decodes at this fraction of the output: the blur
/// leaves no detail worth more, and it stays cheap to hold.
const FROST_DIV: u32 = 6;

/// An order map for one layer, from the analysis thread.
pub struct Shaped {
    owner: u64,
    layer: u64,
    img: Mutex<Option<ImageData>>,
    pub ms: f32,
}

struct Layer {
    key: u64,
    path: PathBuf,
    /// Shows the full-size picture (false: still the selector's thumbnail).
    full: bool,
    /// The full-size texture fading in over the thumbnail.
    sharp: Option<(u64, Spring)>,
    zoom: Spring,
    /// Wipe / zoom centre as a fraction of the output.
    origin: (f32, f32),
    /// Its order map is being computed: not shown yet.
    waiting: bool,
    /// The order map texture and seconds into the wipe (None: shown whole).
    wipe: Option<(u64, f32)>,
    /// Seconds to wait before the wipe starts (the selector's fold
    /// reaching the card).
    hold: f32,
}

impl Layer {
    fn covered(&self) -> bool {
        !self.waiting && self.wipe.is_none()
    }
    /// How much of the screen it shows, roughly.
    fn progress(&self) -> f32 {
        if self.waiting {
            0.0
        } else {
            self.wipe.map_or(1.0, |(_, c)| ease(c / WIPE_SECS))
        }
    }
    fn release(&self, frees: &mut Vec<u64>) {
        frees.push(self.key);
        frees.extend(self.sharp.as_ref().map(|s| s.0));
        frees.extend(self.wipe.map(|w| w.0));
    }
}

/// Wipe progress over time: quick off the mark, then settling in.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let out = 1.0 - (1.0 - x).powi(2);
    out * 0.6 + x * x * (3.0 - 2.0 * x) * 0.4
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub struct Backdrop {
    id: u64,
    keybase: u64,
    next_key: u64,
    tx: calloop::channel::Sender<Fact>,
    scale: f32,
    /// Surface size (logical), as last drawn.
    size: (f32, f32),
    /// The wallpaper we want on screen.
    path: PathBuf,
    /// Last decode asked for (path, physical size).
    requested: Option<(PathBuf, u32, u32)>,
    generation: u64,
    latest: Arc<AtomicU64>,
    /// Where the next wipe starts (fraction of the output); None: the
    /// picture's most salient spot.
    origin: Option<(f32, f32)>,
    layers: Vec<Layer>,
    uploads: Vec<(u64, ImageData)>,
    frees: Vec<u64>,
    time: f32,
    /// The overview's backdrop: the wallpaper blurred, changing without
    /// a wipe (it is out of sight whenever the wallpaper is picked).
    frost: bool,
}

impl Backdrop {
    /// None when no wallpaper is configured.
    pub fn new(ctx: &mut Ctx, frost: bool) -> Option<Self> {
        let path = PathBuf::from(crate::config::get("wallpaper")?);
        path.is_file().then(|| Self::with_path(ctx, path, frost))
    }

    pub fn with_path(ctx: &mut Ctx, path: PathBuf, frost: bool) -> Self {
        let id = loader::next_id();
        let mut b = Self {
            id,
            keybase: 0x5742_0000_0000_0000 | (id << 24),
            next_key: 0,
            tx: ctx.fact_tx.clone(),
            scale: ctx.scale,
            size: (ctx.out_w, ctx.out_h),
            path: path.clone(),
            requested: None,
            generation: 0,
            latest: Arc::new(AtomicU64::new(0)),
            origin: None,
            layers: Vec::with_capacity(CAP),
            uploads: vec![],
            frees: vec![],
            time: 0.0,
            frost,
        };
        b.request(path);
        b
    }

    fn physical(&self) -> (u32, u32) {
        (((self.size.0 * self.scale).round() as u32).max(1), ((self.size.1 * self.scale).round() as u32).max(1))
    }

    /// The size pictures are decoded at.
    fn decode_size(&self) -> (u32, u32) {
        let (w, h) = self.physical();
        if self.frost { ((w / FROST_DIV).max(1), (h / FROST_DIV).max(1)) } else { (w, h) }
    }

    fn shown(&self) -> Option<&PathBuf> {
        self.layers.last().map(|l| &l.path)
    }

    fn new_key(&mut self) -> u64 {
        self.next_key += 1;
        self.keybase | self.next_key
    }

    fn request(&mut self, path: PathBuf) {
        let (w, h) = self.decode_size();
        if self.requested.as_ref().is_some_and(|(p, rw, rh)| *p == path && *rw == w && *rh == h) {
            return;
        }
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        self.requested = Some((path.clone(), w, h));
        loader::request_decode(self.tx.clone(), self.id, self.generation, self.latest.clone(), path, w, h, self.frost);
    }

    /// Drop the pending decode: the picture on screen is wanted after all.
    fn cancel(&mut self) {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        let (w, h) = self.decode_size();
        self.requested = self.shown().map(|p| (p.clone(), w, h));
    }

    /// Read `img`'s shapes on a short-lived thread; the order map comes
    /// back as a `Shaped` fact for layer `layer`. False if no thread could
    /// be started (the layer is then shown whole).
    fn analyse(&self, layer: u64, img: ImageData, origin: Option<(f32, f32)>) -> bool {
        let (tx, owner) = (self.tx.clone(), self.id);
        let job = move || {
            let o = shapes::analyse(&img, origin);
            drop(img);
            let msg = Shaped { owner, layer, img: Mutex::new(Some(o.img)), ms: o.ms };
            let _ = tx.send(Fact::Custom(Arc::new(msg)));
        };
        let spawned = std::thread::Builder::new().name("nimbus-shapes".into()).spawn(job);
        if let Err(e) = &spawned {
            eprintln!("nimbus: wallpaper analysis: {e}");
        }
        spawned.is_ok()
    }

    /// A layer that appears once its order map is ready.
    fn layer(&self, key: u64, path: PathBuf, full: bool, hold: f32) -> Layer {
        Layer {
            key,
            path,
            full,
            sharp: None,
            zoom: Spring::new(ZOOM_FROM, ZOOM),
            origin: self.origin.unwrap_or((0.5, 0.5)),
            waiting: true,
            wipe: None,
            hold,
        }
    }

    /// Returns true when a redraw is needed.
    pub fn apply(&mut self, f: &Fact) -> bool {
        let Fact::Custom(any) = f else { return false };
        if let Some(c) = any.downcast_ref::<WallpaperChange>() {
            return self.change(c);
        }
        if let Some(s) = any.downcast_ref::<Shaped>() {
            return self.shaped(s);
        }
        let Some(d) = any.downcast_ref::<loader::Decoded>() else { return false };
        if d.owner != self.id || d.generation != self.generation {
            return false;
        }
        let Some(img) = d.img.lock().unwrap().take() else { return false };
        let key = self.new_key();
        if self.frost {
            self.uploads.push((key, img));
            let mut l = self.layer(key, d.path.clone(), true, 0.0);
            l.waiting = false;
            l.zoom.snap(1.0);
            self.push_layer(l);
            self.drop_hidden();
            return true;
        }
        if let Some(top) = self.layers.last_mut()
            && top.path == d.path
            && !top.full
        {
            // The thumbnail's full-size picture: sharpen in place.
            self.uploads.push((key, img));
            top.full = true;
            let mut s = Spring::new(0.0, SHARPEN);
            s.set(1.0);
            top.sharp = Some((key, s));
            return true;
        }
        if self.shown() == Some(&d.path) {
            // Same picture at a new size (scale change): swap without a show.
            self.uploads.push((key, img));
            let mut l = self.layer(key, d.path.clone(), true, 0.0);
            l.waiting = false;
            l.zoom.snap(1.0);
            self.push_layer(l);
            self.drop_hidden();
            return true;
        }
        let origin = self.origin.take();
        let shaping = self.analyse(key, img.clone(), origin);
        self.uploads.push((key, img));
        let mut l = self.layer(key, d.path.clone(), true, 0.0);
        l.origin = origin.unwrap_or((0.5, 0.5));
        l.waiting = shaping;
        self.push_layer(l);
        // Nothing new to show until the order map is in.
        !shaping
    }

    /// The order map for a waiting layer arrived: start its wipe.
    fn shaped(&mut self, s: &Shaped) -> bool {
        if s.owner != self.id {
            return false;
        }
        let Some(i) = self.layers.iter().position(|l| l.key == s.layer && l.waiting) else { return false };
        let Some(img) = s.img.lock().unwrap().take() else { return false };
        let key = self.new_key();
        self.uploads.push((key, img));
        let l = &mut self.layers[i];
        l.waiting = false;
        l.wipe = Some((key, 0.0));
        true
    }

    fn change(&mut self, c: &WallpaperChange) -> bool {
        self.path = c.path.clone();
        if self.frost {
            self.request(c.path.clone());
            return false;
        }
        if self.shown() == Some(&c.path) {
            if self.layers.last().is_some_and(|l| l.full) {
                if self.requested.as_ref().is_some_and(|r| r.0 != c.path) {
                    self.cancel();
                }
            } else {
                // Its thumbnail is up but the decode may have been superseded.
                self.request(c.path.clone());
            }
            return false;
        }
        // The card's centre, mapped by ratio onto this output (and so
        // never outside it).
        let fx = |v: f32, of: f32| if of > 0.0 { v / of } else { 0.5 };
        let (cx, cy) = (fx(c.card[0] + c.card[2] * 0.5, c.out.0), fx(c.card[1] + c.card[3] * 0.5, c.out.1));
        self.origin = Some((cx.clamp(0.0, 1.0), cy.clamp(0.0, 1.0)));
        if let Some(img) = &c.preview {
            // Read the shapes off the card's picture right away; the
            // full-size decode sharpens the layer later.
            let key = self.new_key();
            let origin = self.origin.take();
            let shaping = self.analyse(key, img.clone(), origin);
            self.uploads.push((key, img.clone()));
            let mut l = self.layer(key, c.path.clone(), false, c.delay);
            l.origin = origin.unwrap_or((0.5, 0.5));
            l.waiting = shaping;
            self.push_layer(l);
            if !shaping {
                self.request(c.path.clone());
                return true;
            }
        }
        self.request(c.path.clone());
        false
    }

    /// Add a layer on top, keeping at most `CAP` and the bottom one always
    /// covering the screen (no bare surface ever shows).
    fn push_layer(&mut self, l: Layer) {
        if self.layers.len() >= CAP {
            if self.layers[1].progress() > 0.6 {
                // Nearly there: finish it and let it be the new base.
                let b = &mut self.layers[1];
                b.zoom.snap(1.0);
                b.waiting = false;
                b.hold = 0.0;
                if let Some((k, _)) = b.wipe.take() {
                    self.frees.push(k);
                }
                self.layers.remove(0).release(&mut self.frees);
            } else {
                self.layers.remove(1).release(&mut self.frees);
            }
        }
        self.layers.push(l);
    }

    /// Free every layer under the topmost fully revealed one.
    fn drop_hidden(&mut self) {
        if let Some(top) = self.layers.iter().rposition(Layer::covered)
            && top > 0
        {
            for l in self.layers.drain(..top) {
                l.release(&mut self.frees);
            }
        }
    }

    pub fn set_scale(&mut self, s: f32) {
        self.scale = s;
    }

    pub fn step(&mut self, dt: f32) -> bool {
        let mut moving = false;
        for l in &mut self.layers {
            if l.hold > 0.0 {
                l.hold -= dt;
                moving = true;
            } else if !l.waiting {
                // The zoom settles with the wipe, from when it starts.
                l.zoom.set(1.0);
                moving |= l.zoom.step(dt);
                if let Some((k, c)) = &mut l.wipe {
                    *c += dt;
                    moving = true;
                    if *c >= WIPE_SECS {
                        self.frees.push(*k);
                        l.wipe = None;
                    }
                }
            }
            if let Some((k, s)) = &mut l.sharp {
                moving |= s.step(dt);
                if s.at_rest() {
                    self.frees.push(l.key);
                    l.key = *k;
                    l.sharp = None;
                }
            }
        }
        // Frees go out with the draw that follows this step.
        self.drop_hidden();
        moving
    }

    /// Fill `out` for a surface of `w`×`h` logical px.
    pub fn draw(&mut self, out: &mut DrawList, w: f32, h: f32) {
        out.clear();
        if w > 0.0 && h > 0.0 {
            self.size = (w, h);
        }
        // The surface size or scale changed: re-decode at the new size.
        let want = self.decode_size();
        if self.requested.as_ref().is_some_and(|(_, rw, rh)| (*rw, *rh) != want) {
            self.requested = None;
            self.request(self.path.clone());
        }
        out.uploads.append(&mut self.uploads);
        out.frees.append(&mut self.frees);
        let (pw, ph) = self.physical();
        let (pw, ph) = (pw as f32, ph as f32);
        for l in &self.layers {
            if l.waiting || l.hold > 0.0 {
                continue;
            }
            // The zoom about the wipe's origin.
            let (ox, oy) = (l.origin.0 * pw, l.origin.1 * ph);
            let z = l.zoom.get();
            let rect = [ox - ox * z, oy - oy * z, pw * z, ph * z];
            let mut img = ImageDraw::new(l.key, rect);
            img.smooth = self.frost;
            if let Some((k, c)) = l.wipe {
                let x = c / WIPE_SECS;
                // Run a hair past 1 so the last shape is fully in.
                let t = ease(x) * 1.02;
                let fade = 1.0 - smoothstep(0.82, 1.0, x);
                img.mask = Some((k, [t, FRONT * self.scale, REFRACT * self.scale * fade, RIM * fade]));
            }
            out.images.push(img);
            if let Some((k, s)) = &l.sharp {
                let mut sharp = img;
                sharp.key = *k;
                sharp.alpha = s.get().clamp(0.0, 1.0);
                out.images.push(sharp);
            }
        }
    }

    /// Every texture this backdrop owns, for when its surface goes away
    /// without a final frame (output unplugged, layer closed).
    pub fn release(&mut self, frees: &mut Vec<u64>) {
        for l in self.layers.drain(..) {
            l.release(frees);
        }
        frees.append(&mut self.frees);
        self.uploads.clear();
        self.cancel();
    }

    /// Something is on its way or moving (demo: wait for rest).
    pub fn busy(&self) -> bool {
        self.layers.last().is_none_or(|l| !l.full || l.path != self.path)
            || self.layers.iter().any(|l| l.waiting || l.hold > 0.0 || l.wipe.is_some() || l.sharp.is_some())
    }

    /// Textures this backdrop owns on the GPU (demo: leak check).
    pub fn textures(&self) -> usize {
        self.layers.iter().map(|l| 1 + l.sharp.is_some() as usize + l.wipe.is_some() as usize).sum()
    }

    pub fn wake_at(&self) -> Option<f32> {
        None
    }

    pub fn sync_time(&mut self, t: f32) {
        self.time = self.time.max(t);
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        // Let the decode worker skip whatever it had for us.
        self.latest.store(self.generation + 1, Ordering::Relaxed);
    }
}
