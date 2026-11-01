//! The wallpaper selector: a cover-flow carousel in a Prism Ink window
//! (flat matte ink, the prism hairline; no floor reflection, no glow).
//!
//! - The window grows out of the centre card (it is the seed of the open
//!   morph) and the other cards fan out from behind it; closing folds
//!   everything back into that card, which then fades as the new wallpaper
//!   blooms out from the same spot.
//! - The carousel position is one spring: wheel, arrows and clicks move
//!   its target, drags move it directly and release with momentum, and it
//!   always settles on a card.
//! - Thumbnails are decoded on worker threads at the size they are drawn
//!   (cached on disk), uploaded once, and iris in when a slow decode lands
//!   (cache hits just appear, riding the open morph). Only a window of
//!   cards around the position is loaded; far ones are freed again. The
//!   settled centre card swaps in a sharper copy. Everything is freed when
//!   the window goes away.
//! - Space previews the centred card on the desktop; Enter (or a click on
//!   it) keeps it. Closing without keeping puts the old wallpaper back.
//!
//! Draw order trick: images always draw after shapes, so anything that has
//! to sit *over* a card (dimming, rings) is drawn as an image of a tiny
//! solid or gradient texture (the iridescent ring is a ramp texture), in
//! back-to-front order.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};

use glyphon::FontSystem;

use super::{
    WallpaperChange,
    loader::{self, Entry, Job, Pool, Thumb},
};
use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    gfx::{DrawList, IconDraw, ImageData, ImageDraw, TextDraw},
    icons::IconSet,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    window::{Chrome, Ctx, Cursor, KeyInput, Placement, WinScene, keys},
};

/// Shadow margin around the body on the surface.
const M: f32 = 40.0;
const BASE_W: f32 = 1240.0;
const BASE_H: f32 = 590.0;
const CARD_W: f32 = 460.0;
const HEADER: f32 = 64.0;
/// Logical px of drag per card.
const DRAG_PX: f32 = 340.0;
/// Thumbnails are loaded this many cards either side of the target…
const KEEP: usize = 6;
/// …and freed again beyond this.
const EVICT: usize = 10;
/// Most position dots shown at once (they scroll beyond that).
const DOTS: usize = 21;
/// Width of the label wipe's soft edge (logical px).
const WIPE_SOFT: f32 = 44.0;
/// Card corner radius at the centre.
const CARD_R: f32 = 15.0;

const F_TITLE: Font = Font::new(17.0, 650).family("Geist");
const F_CHIP: Font = Font::new(11.5, 650).family("Geist");
const F_NAME: Font = Font::new(19.0, 650).family("Geist");
const F_META: Font = Font::new(12.5, 500).family("Geist");
const F_HINT: Font = Font::new(12.0, 500).family("Geist");
const F_KEY: Font = Font::new(11.0, 650).family("Geist");
const F_EMPTY: Font = Font::new(15.0, 600).family("Geist");

const SHADOW: Rgba = Rgba::hex(0x000000, 0.6);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);

const OPEN_W: SpringCfg = SpringCfg::new(0.5, 0.84);
const OPEN_H: SpringCfg = SpringCfg::new(0.58, 0.8);
const FOLD: SpringCfg = SpringCfg::new(0.36, 1.0);
const POS: SpringCfg = SpringCfg::new(0.5, 0.82);
const FAN_OPEN: SpringCfg = SpringCfg::new(0.72, 0.74);
const FAN_CLOSE: SpringCfg = SpringCfg::new(0.3, 1.0);
const SHOW: SpringCfg = SpringCfg::new(0.5, 0.92);
const LABEL: SpringCfg = SpringCfg::new(0.34, 1.0);
const SHARPEN: SpringCfg = SpringCfg::new(0.28, 1.0);

// Solid/gradient helper textures (offsets from the key base).
const K_INK: u64 = 0xff_ff00;
const K_WHITE: u64 = 0xff_ff01;
const K_ACCENT: u64 = 0xff_ff02;
/// The body colour behind the cards (dimming, placeholders).
const INK: [u8; 3] = [0x09, 0x09, 0x0d];

/// Prism Ink's iridescent ramp (the shader's, on the CPU) as a texture
/// row, for rings drawn as images.
fn irid_row(n: usize) -> Vec<u8> {
    let stops = [(0.0, [0.624, 0.863, 1.0]), (0.4, [0.788, 0.698, 1.0]), (0.7, [1.0, 0.702, 0.894]), (1.0, [1.0, 0.824, 0.659])];
    let mut v = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f32 / (n - 1).max(1) as f32;
        let j = stops.iter().rposition(|s| s.0 <= t).unwrap_or(0).min(stops.len() - 2);
        let (a, b) = (stops[j], stops[j + 1]);
        let k = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
        let c: [f32; 3] = std::array::from_fn(|q| a.1[q] + (b.1[q] - a.1[q]) * k);
        let l = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
        for q in c {
            v.push(((q + (l - q) * 0.14) * 255.0).round() as u8);
        }
        v.push(255);
    }
    v
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    None,
    Header,
    Close,
    Card(usize),
    Stage,
    Dot(usize),
}

struct Item {
    e: Entry,
    /// Low tier: queued, then uploaded.
    queued: bool,
    ready: bool,
    pending: Option<ImageData>,
    /// The pending picture came fast (cache hit, or while opening): no iris.
    instant: bool,
    meta: String,
    avg: Rgba,
    lift: Spring,
    shown: Spring,
    current: Spring,
}

/// The sharp copy of the settled centre card.
struct Sharp {
    idx: usize,
    ready: bool,
    pending: Option<ImageData>,
    fade: Spring,
}

/// A touchpad scroll in progress (fingers on the pad).
struct Pan {
    /// Position without the rubber band.
    raw: f32,
    last: Instant,
    vel: f32,
}

struct Press {
    x: f32,
    pos: f32,
    hit: Hit,
    dragging: bool,
    last: (Instant, f32),
    vel: f32,
}

pub struct Selector {
    chrome: Chrome,
    text: TextCache,
    icons: std::rc::Rc<IconSet>,
    scale: f32,
    size: (f32, f32),
    fw: f32,
    fh: f32,
    /// Card size at the centre (logical) and thumbnail sizes (physical).
    card: (f32, f32),
    low_px: (u32, u32),
    hi_px: (u32, u32),
    owner: u64,
    keybase: u64,
    items: Vec<Item>,
    pool: Pool,
    tx: calloop::channel::Sender<Fact>,
    persist: bool,
    /// Static strings, made once.
    dir_label: String,
    count_label: String,
    /// Surface position on the output (home margins) and output size.
    base: (f32, f32),
    out: (f32, f32),

    pos: Spring,
    tail: Spring,
    fan: Spring,
    press_k: Spring,
    close_h: Spring,
    label: usize,
    prev_label: usize,
    /// +1: the new name wipes in from the right (cards moved left).
    label_dir: f32,
    label_t: Spring,
    sharp: Option<Sharp>,
    scroll_acc: f32,
    pan: Option<Pan>,
    /// Target the loaded thumbnail window is centred on.
    window_c: usize,
    opened: Instant,

    /// The wallpaper when the window opened, the one in the config, and
    /// the one on the desktop now (Space previews change only the last).
    orig: Option<usize>,
    saved: Option<usize>,
    applied: Option<usize>,
    /// Enter / click: fold into the card where it is (no flight home).
    fold_here: bool,

    hovered: Hit,
    /// Last pointer position (surface coords), to keep hover honest while
    /// cards slide under a still pointer.
    pointer: Option<(f32, f32)>,
    press: Option<Press>,
    hits: Vec<(Rect, Hit)>,
    order: Vec<(f32, usize)>,
    shift: (f32, f32),
    frees: Vec<u64>,
    solids: bool,
    freed: bool,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Horizontal distance of a card `a` slots from the centre.
fn spread_x(a: f32) -> f32 {
    720.0 * (1.0 - (-a / 1.95).exp())
}

fn rubber(x: f32) -> f32 {
    0.55 * x / (1.0 + x)
}

/// Crop an image draw to `clip` (physical), adjusting its texture window.
fn clipped(mut img: ImageDraw, clip: [f32; 4]) -> Option<ImageDraw> {
    let [x, y, w, h] = img.rect;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let (x0, y0, x1, y1) = (x.max(clip[0]), y.max(clip[1]), (x + w).min(clip[2]), (y + h).min(clip[3]));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let [u0, v0, u1, v1] = img.uv;
    let fu = |px: f32| u0 + (u1 - u0) * (px - x) / w;
    let fv = |py: f32| v0 + (v1 - v0) * (py - y) / h;
    img.uv = [fu(x0), fv(y0), fu(x1), fv(y1)];
    img.rect = [x0, y0, x1 - x0, y1 - y0];
    Some(img)
}

fn human_bytes(b: u64) -> String {
    match b {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b >> 10),
        b => format!("{b} B"),
    }
}

/// A punchier version of an image's mean colour, for the glow under the
/// centre card.
fn glow_of(avg: [f32; 3]) -> Rgba {
    let l = avg[0] * 0.3 + avg[1] * 0.59 + avg[2] * 0.11;
    let sat = |c: f32| (l + (c - l) * 1.7).clamp(0.0, 1.0);
    let c = [sat(avg[0]), sat(avg[1]), sat(avg[2])];
    let mx = c[0].max(c[1]).max(c[2]).max(0.05);
    let k = (0.8 / mx).min(2.2);
    Rgba((c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0), 1.0)
}

impl Selector {
    pub fn new(ctx: &mut Ctx, persist: bool) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        // Fit the window to small outputs; fonts keep their size.
        let k = ((ctx.out_w - 2.0 * M - 60.0) / BASE_W).min((ctx.out_h - 2.0 * M - 60.0) / BASE_H).clamp(0.55, 1.0);
        let (fw, fh) = ((BASE_W * k).round(), (BASE_H * k).round().max(420.0));
        let size = (fw + 2.0 * M, fh + 2.0 * M);
        let aspect = if ctx.out_h > 0.0 { (ctx.out_w / ctx.out_h).clamp(1.25, 2.4) } else { 1.6 };
        let card = ((CARD_W * k).round(), (CARD_W * k / aspect).round());
        let px = |f: f32| (((card.0 * f * ctx.scale).round() as u32).max(16), ((card.1 * f * ctx.scale).round() as u32).max(10));
        let saved = crate::config::get("wallpaper").map(PathBuf::from);
        let entries = loader::scan(saved.as_deref());
        let current = saved.as_ref().and_then(|s| entries.iter().position(|e| &e.path == s));
        let start_idx = current.unwrap_or(0);
        let owner = loader::next_id();
        let items = entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let meta = Self::meta_of(&e, None);
                let on = if Some(i) == current { 1.0 } else { 0.0 };
                Item {
                    e,
                    queued: false,
                    ready: false,
                    pending: None,
                    instant: false,
                    meta,
                    avg: Rgba::hex(0x30303a, 1.0),
                    lift: Spring::new(0.0, presets::SNAPPY),
                    shown: Spring::new(0.0, SHOW),
                    current: Spring::new(on, presets::SMOOTH),
                }
            })
            .collect();
        let home = Rect::new(M, M, fw, fh);
        let stage_cy = M + HEADER + 30.0 * k + card.1 * 0.5;
        let seed = Rect::new(M + fw * 0.5 - card.0 * 0.5, stage_cy - card.1 * 0.5, card.0, card.1);
        let base = (((ctx.out_w - size.0) * 0.5).round(), ((ctx.out_h - size.1) * 0.5).round());
        let mut fan = Spring::new(0.0, FAN_OPEN);
        fan.set(1.0);
        let mut s = Self {
            chrome: Chrome::new(seed, home, OPEN_W, OPEN_H),
            text,
            icons: ctx.icons.clone(),
            scale: ctx.scale,
            size,
            fw,
            fh,
            card,
            low_px: px(0.64),
            hi_px: px(1.04),
            owner,
            keybase: 0x5754_0000_0000_0000 | (owner << 24),
            items,
            pool: Pool::new(ctx.fact_tx.clone(), owner),
            tx: ctx.fact_tx.clone(),
            persist,
            dir_label: loader::pretty_dir(),
            count_label: String::new(),
            base,
            out: (ctx.out_w, ctx.out_h),
            pos: Spring::new(start_idx as f32, POS),
            tail: Spring::new(start_idx as f32, presets::TAIL),
            fan,
            press_k: Spring::new(0.0, presets::SNAPPY),
            close_h: Spring::new(0.0, presets::SNAPPY),
            label: start_idx,
            prev_label: start_idx,
            label_dir: 1.0,
            label_t: Spring::new(1.0, LABEL),
            sharp: None,
            scroll_acc: 0.0,
            pan: None,
            window_c: start_idx,
            opened: Instant::now(),
            orig: current,
            saved: current,
            applied: current,
            fold_here: false,
            hovered: Hit::None,
            pointer: None,
            press: None,
            hits: Vec::with_capacity(64),
            order: Vec::with_capacity(32),
            shift: (0.0, 0.0),
            frees: vec![],
            solids: false,
            freed: false,
        };
        s.count_label = s.items.len().to_string();
        loader::prune(s.items.iter().map(|it| it.e.path.clone()).collect());
        // The seed card shows its picture from the first frame when cached.
        if let Some(it) = s.items.get_mut(start_idx)
            && let Some((img, src)) = loader::cached_thumbnail(&it.e.path, s.low_px.0, s.low_px.1)
        {
            it.avg = glow_of(loader::average(&img));
            it.meta = Self::meta_of(&it.e, Some(src));
            it.pending = Some(img);
            it.instant = true;
        }
        s.sync_window();
        s
    }

    fn meta_of(e: &Entry, dims: Option<(u32, u32)>) -> String {
        let mut m = String::new();
        if let Some(f) = &e.folder {
            m.push_str(f);
            m.push_str("  ·  ");
        }
        if let Some((w, h)) = dims {
            m.push_str(&format!("{w} × {h}  ·  "));
        }
        m.push_str(e.kind);
        m.push_str("  ·  ");
        m.push_str(&human_bytes(e.bytes));
        m
    }

    fn tex_key(&self, i: usize, hi: bool) -> u64 {
        self.keybase | ((i as u64) << 1) | hi as u64
    }

    fn centre(&self) -> usize {
        (self.pos.get().round().max(0.0) as usize).min(self.items.len().saturating_sub(1))
    }

    fn target(&self) -> usize {
        (self.pos.target.round().max(0.0) as usize).min(self.items.len().saturating_sub(1))
    }

    /// Load the thumbnails within `KEEP` cards of the target (nearest
    /// first), drop queued ones that fell out, free those beyond `EVICT`.
    fn sync_window(&mut self) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let c = self.target();
        self.window_c = c;
        for i in self.pool.cancel_low() {
            if let Some(it) = self.items.get_mut(i) {
                it.queued = false;
            }
        }
        let base = self.keybase;
        for (i, it) in self.items.iter_mut().enumerate() {
            if i.abs_diff(c) > EVICT {
                if it.ready {
                    self.frees.push(base | ((i as u64) << 1));
                    it.ready = false;
                    it.shown.snap(0.0);
                }
                it.pending = None;
            }
        }
        let mut idx: Vec<usize> = (c.saturating_sub(KEEP)..=(c + KEEP).min(n - 1))
            .filter(|&i| {
                let it = &self.items[i];
                !it.ready && !it.queued && it.pending.is_none()
            })
            .collect();
        idx.sort_by_key(|&i| i.abs_diff(c));
        let (w, h) = self.low_px;
        for i in idx {
            self.items[i].queued = true;
            self.pool.push(Job { idx: i, hi: false, path: self.items[i].e.path.clone(), w, h }, false);
        }
    }

    fn go(&mut self, i: isize) {
        if self.items.is_empty() {
            return;
        }
        let t = i.clamp(0, self.items.len() as isize - 1) as f32;
        self.pos.set(t);
        self.tail.set(t);
    }

    fn stage_cy(&self) -> f32 {
        M + HEADER + 30.0 * (self.card.0 / CARD_W) + self.card.1 * 0.5
    }

    /// Card rect and depth `a` for slot offset `d` (fan applied).
    fn card_rect(&self, d: f32, lift: f32, centre_press: f32) -> (Rect, f32) {
        let k = self.card.0 / CARD_W;
        let a = d.abs() * self.fan.get().max(0.0);
        let s = 1.0 / (1.0 + 0.45 * a) * (1.0 + 0.025 * lift) * (1.0 - 0.035 * centre_press);
        let x = M + self.fw * 0.5 + d.signum() * spread_x(a) * k;
        let y = self.stage_cy() + 12.0 * k * a.powf(1.25) - 7.0 * lift;
        let (w, h) = (self.card.0 * s, self.card.1 * s);
        (Rect::new(x - w * 0.5, y - h * 0.5, w, h), a)
    }

    /// Show the centred card on the desktop; `keep` also saves it (Enter,
    /// click) and makes the window fold into the card where it is.
    fn apply_centre(&mut self, keep: bool) {
        if self.items.is_empty() {
            return;
        }
        let i = self.target();
        if keep {
            self.fold_here = true;
            self.orig = Some(i);
            if self.persist && self.saved != Some(i) {
                crate::config::set("wallpaper", &self.items[i].e.path.to_string_lossy());
            }
            self.saved = Some(i);
        }
        if self.applied == Some(i) {
            return;
        }
        // The card's picture goes along: the wallpaper grows out of it.
        let preview = {
            let p = &self.items[i].e.path;
            loader::cached_thumbnail(p, self.hi_px.0, self.hi_px.1).or_else(|| loader::cached_thumbnail(p, self.low_px.0, self.low_px.1)).map(|(img, _)| img)
        };
        // A kept pick waits for the fold to reach the card; a preview grows
        // from behind the open window right away.
        self.send_change(i, preview, if keep { 0.1 } else { 0.0 });
    }

    /// Tell the backdrops to show item `i`, starting from the centre card.
    fn send_change(&mut self, i: usize, preview: Option<ImageData>, delay: f32) {
        self.applied = Some(i);
        for (k, it) in self.items.iter_mut().enumerate() {
            it.current.set(if k == i { 1.0 } else { 0.0 });
        }
        let (r, _) = self.card_rect(0.0, 0.0, 0.0);
        let (ox, oy) = (self.base.0 + self.chrome.surf_origin.0 as f32 + self.shift.0, self.base.1 + self.chrome.surf_origin.1 as f32 + self.shift.1);
        let change = WallpaperChange {
            path: self.items[i].e.path.clone(),
            card: [ox + r.x, oy + r.y, r.w, r.h],
            radius: CARD_R,
            out: self.out,
            preview,
            delay,
        };
        let _ = self.tx.send(Fact::Custom(Arc::new(change)));
    }

    /// Position with the rubber band past either end.
    fn band(&self, raw: f32) -> f32 {
        let n = self.items.len().max(1) as f32;
        if raw < 0.0 {
            -rubber(-raw)
        } else if raw > n - 1.0 {
            n - 1.0 + rubber(raw - (n - 1.0))
        } else {
            raw
        }
    }

    /// Fingers lifted: glide on with their speed, settle on a card.
    fn end_pan(&mut self) {
        let Some(p) = self.pan.take() else { return };
        let vel = if p.last.elapsed().as_secs_f32() > 0.08 { 0.0 } else { p.vel.clamp(-40.0, 40.0) };
        let n = self.items.len().max(1) as f32;
        let t = (self.pos.get() + vel * 0.2).round().clamp(0.0, n - 1.0);
        self.pos.vel = vel;
        self.pos.set(t);
        self.tail.set(t);
    }

    fn hit_at(&self, x: f32, y: f32) -> Hit {
        let (x, y) = (x - self.shift.0, y - self.shift.1);
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Hit::None, |(_, h)| *h)
    }

    fn set_hover(&mut self, h: Hit) {
        if h == self.hovered {
            return;
        }
        if let Hit::Card(i) = self.hovered
            && let Some(it) = self.items.get_mut(i)
        {
            it.lift.set(0.0);
        }
        if let Hit::Card(i) = h
            && let Some(it) = self.items.get_mut(i)
        {
            it.lift.set(1.0);
        }
        self.close_h.set(if h == Hit::Close { 1.0 } else { 0.0 });
        self.hovered = h;
    }

    fn upload_solids(&mut self, out: &mut DrawList) {
        let (aa, ab) = theme::accent();
        let px = |c: Rgba| [(c.0 * 255.0) as u8, (c.1 * 255.0) as u8, (c.2 * 255.0) as u8, 255];
        let solid = |p: [u8; 4]| ImageData { w: 1, h: 1, rgba: Arc::new(p.to_vec()) };
        // The current wallpaper's ring: iridescent under Prism, else the
        // accent gradient.
        let acc = if theme::prism() {
            irid_row(64)
        } else {
            let mut acc = px(aa).to_vec();
            acc.extend_from_slice(&px(ab));
            acc
        };
        out.uploads.push((self.keybase | K_INK, solid([INK[0], INK[1], INK[2], 255])));
        out.uploads.push((self.keybase | K_WHITE, solid([255, 255, 255, 255])));
        out.uploads.push((self.keybase | K_ACCENT, ImageData { w: (acc.len() / 4) as u32, h: 1, rgba: Arc::new(acc) }));
        self.solids = true;
    }

    /// Every texture this window owns (for the final free).
    fn free_all(&mut self, frees: &mut Vec<u64>) {
        frees.append(&mut self.frees);
        for (i, it) in self.items.iter_mut().enumerate() {
            if it.ready {
                frees.push(self.keybase | ((i as u64) << 1));
                it.ready = false;
            }
            it.pending = None;
            it.queued = false;
        }
        if let Some(s) = self.sharp.take()
            && s.ready
        {
            frees.push(self.tex_key(s.idx, true));
        }
        if self.solids {
            frees.extend([K_INK, K_WHITE, K_ACCENT].map(|k| self.keybase | k));
            self.solids = false;
        }
        self.pool.cancel();
        self.freed = true;
    }

    /// Once the carousel has settled, fetch a sharp copy of the centre card.
    fn want_sharp(&mut self) {
        if self.chrome.closing || self.items.is_empty() || !self.pos.at_rest() || self.press.is_some() {
            return;
        }
        let c = self.centre();
        // After its iris has finished: the sharp copy just cross-fades in.
        if self.sharp.as_ref().is_some_and(|s| s.idx == c) || !self.items[c].ready || self.items[c].shown.get() < 0.98 {
            return;
        }
        if let Some(old) = self.sharp.take()
            && old.ready
        {
            self.frees.push(self.tex_key(old.idx, true));
        }
        self.sharp = Some(Sharp { idx: c, ready: false, pending: None, fade: Spring::new(0.0, SHARPEN) });
        let (w, h) = self.hi_px;
        self.pool.push(Job { idx: c, hi: true, path: self.items[c].e.path.clone(), w, h }, true);
    }

    /// The centred card changed: wipe its name in from the side it came
    /// from. Mid-wipe changes in the same direction keep the edge moving
    /// (only the incoming name is swapped), so a fling never strobes.
    fn update_label(&mut self) {
        let c = self.centre();
        if c == self.label {
            return;
        }
        let dir = if c > self.label { 1.0 } else { -1.0 };
        let t = self.label_t.get();
        if dir != self.label_dir || t >= 0.5 || self.prev_label == self.label {
            self.prev_label = self.label;
            self.label_t.snap(0.0);
        }
        self.label_dir = dir;
        self.label = c;
        self.label_t.set(1.0);
    }

    // ── drawing helpers ─────────────────────────────────────────────────

    fn px(&self, r: Rect) -> [i32; 4] {
        let s = self.scale;
        [(r.x * s).floor() as i32, (r.y * s).floor() as i32, ((r.x + r.w) * s).ceil() as i32, ((r.y + r.h) * s).ceil() as i32]
    }

    fn text(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect) {
        if c.3 <= 0.002 {
            return;
        }
        let s = self.scale;
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: self.px(clip) });
    }

    /// Text cut by a soft vertical wipe edge at `e` (logical x), visible on
    /// its left (`true`) or right side: solid away from the edge, then thin
    /// clip strips with a ramped alpha across `WIPE_SOFT`.
    #[allow(clippy::too_many_arguments)]
    fn wipe_text(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect, wipe: Option<(f32, bool)>) {
        let Some((e, left)) = wipe else {
            self.text(out, key, x, y, c, clip);
            return;
        };
        if c.3 <= 0.002 {
            return;
        }
        let s = self.scale;
        let (cy0, cy1) = ((clip.y * s).floor() as i32, ((clip.y + clip.h) * s).ceil() as i32);
        let (cx0, cx1) = (clip.x * s, (clip.x + clip.w) * s);
        let mut strip = |x0: f32, x1: f32, a: f32| {
            let (x0, x1) = ((x0 * s).max(cx0).round() as i32, (x1 * s).min(cx1).round() as i32);
            if x1 > x0 && a > 0.002 {
                out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c.fade(a), clip: [x0, cy0, x1, cy1] });
            }
        };
        let (a0, a1) = (e - WIPE_SOFT * 0.5, e + WIPE_SOFT * 0.5);
        if left {
            strip(f32::MIN / 4.0, a0, 1.0);
        } else {
            strip(a1, f32::MAX / 4.0, 1.0);
        }
        const N: usize = 7;
        for k in 0..N {
            let f = (k as f32 + 0.5) / N as f32;
            let sx = a0 + WIPE_SOFT * k as f32 / N as f32;
            strip(sx, sx + WIPE_SOFT / N as f32, if left { 1.0 - f } else { f });
        }
    }

    /// Text with its left edge at `x` and vertical centre at `cy`.
    #[allow(clippy::too_many_arguments)]
    fn label_at(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, x: f32, cy: f32, c: Rgba, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text(out, k, x, cy - lh * 0.5, c, clip);
        w
    }

    fn image(&self, out: &mut DrawList, key: u64, r: Rect, radius: f32, alpha: f32, clip: [f32; 4]) -> Option<usize> {
        if alpha <= 0.003 {
            return None;
        }
        let s = self.scale;
        let mut img = ImageDraw::new(key, [r.x * s, r.y * s, r.w * s, r.h * s]);
        img.radius = radius * s;
        img.alpha = alpha.min(1.0);
        out.images.push(clipped(img, clip)?);
        Some(out.images.len() - 1)
    }

    fn keycap(&mut self, fs: &mut FontSystem, out: &mut DrawList, cap: &str, x: f32, cy: f32, a: f32, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, cap, F_KEY);
        let bw = (w + 12.0).max(22.0);
        let r = Rect::new(x, cy - 11.0, bw, 22.0);
        out.shapes.push(
            paint::rect(self.scale, r, 7.0)
                .fill(Rgba::hex(0xffffff, 0.06))
                .alpha(a)
                .done(),
        );
        let lh = self.text.line_h(F_KEY);
        self.text(out, k, x + (bw - w) * 0.5, cy - lh * 0.5, SOFT.fade(a), clip);
        bw
    }

    /// Width of a keycap + hint pair, for right-aligned layout.
    fn hint_w(&mut self, fs: &mut FontSystem, cap: &str, hint: &str) -> f32 {
        let (_, cw) = self.text.get(fs, cap, F_KEY);
        let (_, hw) = self.text.get(fs, hint, F_HINT);
        (cw + 12.0).max(22.0) + 7.0 + hw
    }

    fn draw_cards(&mut self, out: &mut DrawList, r: Rect, fade: f32) {
        let sc = self.scale;
        let clip = {
            let c = r.inflate(-1.0);
            [c.x * sc, c.y * sc, (c.x + c.w) * sc, (c.y + c.h) * sc]
        };
        let pos = self.pos.get();
        let n = self.items.len();
        let centre = self.centre();
        self.order.clear();
        for i in 0..n {
            let d = i as f32 - pos;
            if d.abs() < 3.6 {
                self.order.push((d.abs(), i));
            }
        }
        self.order.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut order = std::mem::take(&mut self.order);
        for &(_, i) in &order {
            let it = &self.items[i];
            let d = i as f32 - pos;
            let lift = it.lift.get();
            let press = if i == centre { self.press_k.get() } else { 0.0 };
            let (rc, a) = self.card_rect(d, lift, press);
            let s = rc.w / self.card.0;
            let rad = CARD_R * s.max(0.5);
            // Two cards each side, the third fades out into the edge.
            let edge = ((2.65 - a) / 0.5).clamp(0.0, 1.0);
            // Folding: the other cards leave first, so only the picked one
            // is left to hand over to the desktop.
            let leave = if self.chrome.closing && i != centre { ((self.chrome.openness() - 0.45) / 0.4).clamp(0.0, 1.0) } else { 1.0 };
            let al = fade * edge * leave;
            // Share of the card inside the body (it grows open around them);
            // shapes can't be clipped, so a cut card casts no shadow.
            let inside = ((rc.x + rc.w).min(r.x + r.w) - rc.x.max(r.x)).max(0.0) / rc.w;
            if al <= 0.003 {
                continue;
            }
            let dim = (0.68 * (1.0 - (-0.95 * a).exp())) * (1.0 - 0.6 * lift);
            let shown = it.shown.get();
            let cur = it.current.get().clamp(0.0, 1.0);
            out.shapes.push(
                paint::rect(sc, rc.inflate(-2.0), rad)
                    .fill(Rgba::TRANSPARENT)
                    .shadow(Rgba::hex(0x000000, 0.62 * (1.0 - 0.5 * dim)), 18.0 * s, 12.0 * s)
                    .alpha(al * ((inside - 0.9) / 0.1).clamp(0.0, 1.0))
                    .done(),
            );
            // Rings: an iridescent hairline (current wallpaper) with a dark
            // gap, else a faint white hairline.
            if cur > 0.003 {
                // Full strength near the centre, a quiet marker far out.
                let ra = al * cur * (1.0 - 0.75 * dim);
                self.image(out, self.keybase | K_ACCENT, rc.inflate(3.5), rad + 3.5, ra, clip);
                self.image(out, self.keybase | K_INK, rc.inflate(2.0), rad + 2.0, ra, clip);
            }
            self.image(out, self.keybase | K_WHITE, rc.inflate(1.0), rad + 1.0, al * (0.10 + 0.08 * lift) * (1.0 - cur), clip);
            if !it.ready || shown < 0.995 {
                // Opaque placeholder (hides the cards behind it).
                self.image(out, self.keybase | K_INK, rc, rad, al, clip);
                self.image(out, self.keybase | K_WHITE, rc, rad, al * 0.045, clip);
            }
            if it.ready {
                // Iris in: a soft circle opens from the card's centre while
                // the picture settles from a slight zoom.
                let sh = shown.max(0.0);
                let z = 0.06 * (1.0 - sh).max(0.0);
                if let Some(k) = self.image(out, self.tex_key(i, false), rc, rad, al, clip) {
                    let img = &mut out.images[k];
                    img.uv = [img.uv[0] + z * (0.5 - img.uv[0]) * 2.0, img.uv[1] + z * (0.5 - img.uv[1]) * 2.0, img.uv[2] - z * (img.uv[2] - 0.5) * 2.0, img.uv[3] - z * (img.uv[3] - 0.5) * 2.0];
                    if sh < 0.995 {
                        let soft = 22.0 * s * sc;
                        let (cx, cy) = ((rc.x + rc.w * 0.5) * sc, (rc.y + rc.h * 0.5) * sc);
                        let far = (rc.w * 0.5 * sc).hypot(rc.h * 0.5 * sc);
                        img.reveal = [cx, cy, sh * (far + soft) * 1.02, soft];
                    }
                }
                if let Some(sp) = &self.sharp
                    && sp.idx == i
                    && sp.ready
                {
                    let k = ((0.5 - a) / 0.3).clamp(0.0, 1.0);
                    self.image(out, self.tex_key(i, true), rc, rad, al * sp.fade.get().clamp(0.0, 1.0) * k * sh.min(1.0), clip);
                }
            }
            self.image(out, self.keybase | K_INK, rc, rad, al * dim, clip);
            if fade > 0.5 && a < 3.2 && !self.chrome.closing {
                self.hits.push((rc, Hit::Card(i)));
            }
        }
        order.clear();
        self.order = order;
    }
}

impl WinScene for Selector {
    fn size(&self) -> (f32, f32) {
        self.size
    }
    fn placement(&self) -> Placement {
        Placement::Center
    }
    fn exclusive_keyboard(&self) -> bool {
        true
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }

    fn apply(&mut self, f: &Fact) {
        let Fact::Custom(any) = f else { return };
        let Some(t) = any.downcast_ref::<Thumb>() else { return };
        if t.owner != self.owner || self.freed {
            return;
        }
        let img = t.img.lock().unwrap().take();
        if t.hi {
            if let Some(sp) = &mut self.sharp
                && sp.idx == t.idx
                && !sp.ready
            {
                sp.pending = img;
            }
            return;
        }
        let far = t.idx.abs_diff(self.target()) > EVICT;
        let instant = t.cached || self.chrome.openness() < 1.0 || self.opened.elapsed().as_secs_f32() < 0.12;
        let Some(it) = self.items.get_mut(t.idx) else { return };
        it.queued = false;
        if t.src.0 > 0 {
            it.meta = Self::meta_of(&it.e, Some(t.src));
            it.avg = glow_of(t.avg);
        }
        if far || it.ready {
            return;
        }
        it.pending = img;
        it.instant = instant;
    }

    fn set_scale(&mut self, s: f32) {
        self.scale = s;
        self.text.set_scale(s);
    }

    fn step(&mut self, dt: f32) -> bool {
        // Keep hover honest while cards slide under a still pointer (hits
        // are last frame's; this runs before the springs it retargets).
        if let Some((x, y)) = self.pointer
            && self.press.is_none()
            && !self.chrome.grabbing
            && !self.chrome.closing
        {
            let h = self.hit_at(x, y);
            self.set_hover(h);
        }
        // Fingers that went quiet without a stop event: settle.
        let mut m = false;
        if let Some(p) = &self.pan {
            let idle = p.last.elapsed().as_secs_f32();
            if idle > 0.15 {
                self.end_pan();
                m = true;
            } else {
                self.chrome.wake(self.chrome.time + 0.16 - idle);
            }
        }
        m |= self.chrome.step(dt);
        for s in [&mut self.pos, &mut self.tail, &mut self.fan, &mut self.press_k, &mut self.close_h, &mut self.label_t] {
            m |= s.step(dt);
        }
        for it in &mut self.items {
            m |= it.lift.step(dt) | it.shown.step(dt) | it.current.step(dt);
            m |= it.pending.is_some();
        }
        if let Some(sp) = &mut self.sharp {
            m |= sp.fade.step(dt) | sp.pending.is_some();
        }
        m
    }

    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        let sc = self.scale;
        if self.freed {
            return;
        }
        if !self.solids {
            self.upload_solids(out);
        }
        if !self.chrome.closing && self.target() != self.window_c {
            self.sync_window();
        }
        out.frees.append(&mut self.frees);
        for i in 0..self.items.len() {
            if let Some(img) = self.items[i].pending.take() {
                out.uploads.push((self.tex_key(i, false), img));
                let it = &mut self.items[i];
                it.ready = true;
                // Slow decodes iris in; cache hits are simply there.
                it.shown.snap(if it.instant { 1.0 } else { 0.0 });
                it.shown.set(1.0);
            }
        }
        if let Some(sp) = &mut self.sharp
            && let Some(img) = sp.pending.take()
        {
            sp.ready = true;
            sp.fade.set(1.0);
            let k = self.keybase | ((sp.idx as u64) << 1) | 1;
            out.uploads.push((k, img));
        }
        self.update_label();
        self.want_sharp();

        let open = self.chrome.openness();
        let r = self.chrome.morph();
        self.chrome.body = r;
        // Closing ends on the centre card: fade it all out on the way in.
        let fade = if self.chrome.closing { (open / 0.35).clamp(0.0, 1.0) } else { 1.0 };
        out.shapes.push(
            paint::ink(sc, r, lerp(15.0, 26.0, open.clamp(0.0, 1.0)))
                .shadow(SHADOW, 18.0 + 14.0 * open.max(0.0), 6.0 + 10.0 * open.max(0.0))
                .alpha(fade)
                .done(),
        );
        out.glass(r, lerp(15.0, 26.0, open.clamp(0.0, 1.0)), fade);
        self.hits.push((r, Hit::Stage));
        self.draw_cards(out, r, fade);

        let a = fade * ((open - 0.6) / 0.35).clamp(0.0, 1.0);
        if a > 0.0 {
            self.draw_chrome(fs, out, r, a);
        }
        self.text.end_frame();
        self.shift = self.chrome.translate(out, sc);
        if self.chrome.done() {
            self.free_all(&mut out.frees);
        }
    }

    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.pointer = Some((x, y));
        if self.chrome.closing {
            return None;
        }
        if let Some(p) = &mut self.press {
            let dx = x - p.x;
            if !p.dragging && dx.abs() > 6.0 {
                p.dragging = true;
                p.x = x;
            }
            if p.dragging {
                let raw = p.pos - (x - p.x) / (DRAG_PX * self.card.0 / CARD_W);
                let v = self.band(raw);
                let Some(p) = &mut self.press else { return None };
                let now = Instant::now();
                let dt = now.duration_since(p.last.0).as_secs_f32().max(1.0 / 500.0);
                p.vel = p.vel * 0.5 + (v - p.last.1) / dt * 0.5;
                p.last = (now, v);
                self.pos.snap(v);
                self.tail.set(v);
                self.press_k.set(0.0);
                return None;
            }
        }
        let h = self.hit_at(x, y);
        self.set_hover(h);
        None
    }

    fn pointer_leave(&mut self) {
        self.pointer = None;
        if self.press.is_none() {
            self.set_hover(Hit::None);
        }
    }

    fn press(&mut self, x: f32, y: f32) -> Option<Action> {
        if self.chrome.closing {
            return None;
        }
        self.end_pan();
        let h = self.hit_at(x, y);
        match h {
            Hit::Header => self.chrome.start_grab(),
            Hit::Dot(i) => self.go(i as isize),
            Hit::Card(_) | Hit::Stage | Hit::Close => {
                if h == Hit::Card(self.centre()) {
                    self.press_k.set(1.0);
                }
                self.press = Some(Press { x, pos: self.pos.get(), hit: h, dragging: false, last: (Instant::now(), self.pos.get()), vel: 0.0 });
            }
            Hit::None => {}
        }
        None
    }

    fn release(&mut self) -> (Option<Action>, bool) {
        self.chrome.end_grab();
        self.press_k.set(0.0);
        let Some(p) = self.press.take() else { return (None, false) };
        if p.dragging {
            // Glide on with the throw and settle on the nearest card.
            let stale = p.last.0.elapsed().as_secs_f32() > 0.08;
            let vel = if stale { 0.0 } else { p.vel.clamp(-40.0, 40.0) };
            let n = self.items.len().max(1) as f32;
            let t = (self.pos.get() + vel * 0.2).round().clamp(0.0, n - 1.0);
            self.pos.vel = vel;
            self.pos.set(t);
            self.tail.set(t);
            return (None, false);
        }
        match p.hit {
            Hit::Close if self.hovered == Hit::Close => return (None, true),
            Hit::Card(i) if i == self.centre() && self.pos.target.round() as usize == i => {
                self.apply_centre(true);
                return (None, true);
            }
            Hit::Card(i) => self.go(i as isize),
            _ => {}
        }
        (None, false)
    }

    fn scroll(&mut self, dy: f32) -> Option<Action> {
        if self.chrome.closing || self.pan.is_some() {
            return None;
        }
        if self.scroll_acc.signum() != dy.signum() {
            self.scroll_acc = 0.0;
        }
        self.scroll_acc += dy;
        let mut t = self.target() as isize;
        while self.scroll_acc >= 1.0 {
            self.scroll_acc -= 1.0;
            t += 1;
        }
        while self.scroll_acc <= -1.0 {
            self.scroll_acc += 1.0;
            t -= 1;
        }
        self.go(t);
        None
    }

    fn pan(&mut self, dx: f32, dy: f32, end: bool) -> bool {
        if self.chrome.closing || self.press.is_some() || self.items.is_empty() {
            return true;
        }
        // Whichever axis the fingers mostly move on drives the carousel.
        let d = if dx.abs() >= dy.abs() { dx } else { dy };
        if d != 0.0 {
            let now = Instant::now();
            let step = d / (DRAG_PX * self.card.0 / CARD_W);
            let p = self.pan.get_or_insert(Pan { raw: self.pos.get(), last: now, vel: 0.0 });
            let before = p.raw;
            p.raw += step;
            let after = p.raw;
            let (before, raw) = (self.band(before), self.band(after));
            let Some(p) = &mut self.pan else { return true };
            let dt = now.duration_since(p.last).as_secs_f32().max(1.0 / 500.0);
            p.vel = p.vel * 0.5 + (raw - before) / dt * 0.5;
            p.last = now;
            self.pos.snap(raw);
            self.tail.set(raw);
        }
        if end {
            self.end_pan();
        }
        true
    }

    fn key(&mut self, k: &KeyInput) -> (Option<Action>, bool) {
        if self.chrome.closing {
            return (None, false);
        }
        let t = self.target() as isize;
        match k.sym {
            keys::ESCAPE => return (None, true),
            keys::LEFT | keys::UP => self.go(t - 1),
            keys::RIGHT | keys::DOWN | keys::TAB => self.go(t + 1),
            keys::PAGE_UP => self.go(t - 5),
            keys::PAGE_DOWN => self.go(t + 5),
            keys::HOME => self.go(0),
            keys::END => self.go(isize::MAX / 2),
            keys::RETURN | keys::KP_ENTER => {
                self.apply_centre(true);
                return (None, true);
            }
            // Space: preview on the desktop, keep browsing.
            0x20 => self.apply_centre(false),
            _ => {}
        }
        (None, false)
    }

    fn cursor(&self) -> Cursor {
        if self.chrome.grabbing || self.press.as_ref().is_some_and(|p| p.dragging) {
            return Cursor::Grabbing;
        }
        match self.hovered {
            Hit::Header => Cursor::Grab,
            Hit::Card(_) | Hit::Close | Hit::Dot(_) => Cursor::Pointer,
            _ => Cursor::Default,
        }
    }

    fn close(&mut self) {
        if self.chrome.closing {
            return;
        }
        // A preview that was not kept: put the old wallpaper back, revealed
        // from the card the window folds into.
        if self.applied != self.orig
            && let Some(o) = self.orig
        {
            self.send_change(o, None, 0.0);
        }
        self.chrome.open_w.cfg = FOLD;
        self.chrome.open_h.cfg = FOLD;
        if self.fold_here {
            self.chrome.close_in_place();
        } else {
            self.chrome.close();
        }
        self.fan.cfg = FAN_CLOSE;
        self.fan.set(0.0);
        self.press = None;
        self.pan = None;
        self.set_hover(Hit::None);
        // Stop decoding; whatever is queued is not needed any more.
        if self.sharp.as_ref().is_some_and(|sp| !sp.ready) {
            self.sharp = None;
        }
        for (i, hi) in self.pool.cancel() {
            if !hi && let Some(it) = self.items.get_mut(i) {
                it.queued = false;
            }
        }
    }

    fn reopen(&mut self) {
        self.chrome.open_w.cfg = OPEN_W;
        self.chrome.open_h.cfg = OPEN_H;
        self.chrome.reopen();
        self.fan.cfg = FAN_OPEN;
        self.fan.set(1.0);
        self.fold_here = false;
        self.sync_window();
    }

    fn release_textures(&mut self, frees: &mut Vec<u64>) {
        if !self.freed {
            self.free_all(frees);
        }
    }
}

impl Selector {
    /// Header and footer, laid out on the window's home rect and revealed
    /// by the growing body (`body` clips the text).
    fn draw_chrome(&mut self, fs: &mut FontSystem, out: &mut DrawList, body: Rect, a: f32) {
        let sc = self.scale;
        let clip = body.inflate(-1.0);
        let r = Rect::new(M, M, self.fw, self.fh);
        // Shapes can't be clipped: header and footer wait until the body
        // has grown past them.
        let a = a * ((r.x + 16.0 - body.x) / 10.0).clamp(0.0, 1.0);
        let af = a * ((body.y + body.h - (r.y + r.h - 24.0)) / 10.0).clamp(0.0, 1.0);
        let a = a * ((r.y + 16.0 - body.y) / 10.0).clamp(0.0, 1.0);
        let cx = r.x + r.w * 0.5;
        // Header: title, count, close. The header is the drag handle.
        let hy = r.y + 34.0;
        let tw = self.label_at(fs, out, "Wallpapers", F_TITLE, r.x + 28.0, hy, TEXT.fade(a), clip);
        let (ck, cw) = self.text.get(fs, &self.count_label, F_CHIP);
        let chip = Rect::new(r.x + 28.0 + tw + 10.0, hy - 10.0, cw + 14.0, 20.0);
        out.shapes.push(paint::rect(sc, chip, 10.0).fill(Rgba::hex(0xffffff, 0.07)).alpha(a).done());
        let lh = self.text.line_h(F_CHIP);
        self.text(out, ck, chip.x + 7.0, hy - lh * 0.5, SOFT.fade(a), clip);
        self.hits.push((Rect::new(r.x, r.y, r.w - 64.0, HEADER), Hit::Header));
        let close = Rect::new(r.x + r.w - 48.0, r.y + 19.0, 30.0, 30.0);
        let ch = self.close_h.get();
        // A bare icon; a faint circle only under the pointer.
        if ch > 0.003 {
            out.shapes.push(paint::rect(sc, close, 15.0).fill(Rgba::hex(0xffffff, 0.10)).alpha(a * ch).done());
        }
        let id = self.icons.id(&["window-close-symbolic"]);
        out.icons.push(IconDraw { id, x: (close.x + 7.0) * sc, y: (close.y + 7.0) * sc, size: (16.0 * sc).round(), color: SOFT.mix(TEXT, ch).fade(a) });
        self.hits.push((close, Hit::Close));

        let a = af;
        let n = self.items.len();
        if n == 0 {
            let msg = format!("No images in {}", self.dir_label);
            let (k, w) = self.text.get(fs, &msg, F_EMPTY);
            let lh = self.text.line_h(F_EMPTY);
            self.text(out, k, cx - w * 0.5, self.stage_cy() - lh * 0.5, MUTED.fade(a), clip);
            return;
        }

        // Name + details of the centred card: a directional wipe with a
        // soft edge. The old name is wiped away towards where the cards
        // went while the new one is uncovered from the side its card came
        // in from; each line sweeps across its own extent.
        let ny = r.y + r.h - 116.0;
        let my = ny + 27.0;
        let maxw = r.w - 360.0;
        let t = self.label_t.get().clamp(0.0, 1.0);
        let wiping = t < 1.0 && self.prev_label != self.label && self.prev_label < n;
        let (cw, dw) = (self.text.get(fs, "Current", F_META).1, self.text.get(fs, "  ·  ", F_META).1);
        let (mut nspan, mut mspan) = (0.0f32, 0.0f32);
        let ids = [self.prev_label, self.label];
        let parts: &[usize] = if wiping { &ids } else { &ids[1..] };
        for &idx in parts {
            nspan = nspan.max(self.text.get_fit(fs, &self.items[idx].e.name, F_NAME, maxw).1);
            let lead = (cw + dw) * self.items[idx].current.get().clamp(0.0, 1.0);
            mspan = mspan.max(self.text.get(fs, &self.items[idx].meta, F_META).1 + lead);
        }
        let dir = self.label_dir;
        let sweep = |span: f32| {
            let (l, rr) = (cx - span * 0.5 - WIPE_SOFT, cx + span * 0.5 + WIPE_SOFT);
            if dir > 0.0 { lerp(rr, l, t) } else { lerp(l, rr, t) }
        };
        let (ne, me) = (sweep(nspan), sweep(mspan));
        let (aa, _) = theme::accent();
        for (p, &idx) in parts.iter().enumerate() {
            let is_new = !wiping || p == 1;
            // Old: keeps the side the cards moved to; new: the other one.
            let keep_left = (dir > 0.0) != is_new;
            let wipe = |e: f32| wiping.then_some((e, keep_left));
            let dx = if !wiping {
                0.0
            } else if is_new {
                dir * 16.0 * (1.0 - t)
            } else {
                -dir * 16.0 * t
            };
            let x = cx + dx;
            // The outgoing name also dims as it leaves (regions never overlap
            // outside the soft edge, so nothing ever dips to blank).
            let a = if is_new { a } else { a * (1.0 - 0.45 * t) };
            let (k, w) = self.text.get_fit(fs, &self.items[idx].e.name, F_NAME, maxw);
            let lh = self.text.line_h(F_NAME);
            self.wipe_text(out, k, x - w * 0.5, ny - lh * 0.5, TEXT.fade(a), clip, wipe(ne + dx));
            let cur = self.items[idx].current.get().clamp(0.0, 1.0);
            let (mk, mw) = self.text.get(fs, &self.items[idx].meta, F_META);
            let ck = self.text.get(fs, "Current", F_META).0;
            let dk = self.text.get(fs, "  ·  ", F_META).0;
            let lead = (cw + dw) * cur;
            let x = x - (mw + lead) * 0.5;
            let y = my - self.text.line_h(F_META) * 0.5;
            if cur > 0.003 {
                self.wipe_text(out, ck, x, y, aa.fade(a * cur), clip, wipe(me + dx));
                self.wipe_text(out, dk, x + cw, y, MUTED.fade(a * cur), clip, wipe(me + dx));
            }
            self.wipe_text(out, mk, x + lead, y, MUTED.fade(a), clip, wipe(me + dx));
        }

        // Position: dots with a liquid accent pill (head = position, tail
        // lags behind). Past `DOTS` the row scrolls with the position and
        // the dots shrink away towards its ends.
        let dy = r.y + r.h - 40.0;
        let sp = 13.0;
        let half = (DOTS / 2) as f32;
        let (head, tail) = (self.pos.get(), self.tail.get());
        let wc = if n <= DOTS { (n - 1) as f32 * 0.5 } else { head.clamp(half, (n - 1) as f32 - half) };
        let xi = |f: f32| cx + (f - wc) * sp;
        let (lo_i, hi_i) = ((wc - half - 1.0).floor().max(0.0) as usize, ((wc + half + 1.0).ceil() as usize).min(n - 1));
        for i in lo_i..=hi_i {
            let off = (i as f32 - wc).abs();
            let k = if n <= DOTS { 1.0 } else { ((half + 0.5 - off) / 2.5).clamp(0.0, 1.0) };
            if k <= 0.02 {
                continue;
            }
            let dx = xi(i as f32);
            let near = (1.0 - (i as f32 - head).abs()).clamp(0.0, 1.0);
            out.shapes.push(paint::circle(sc, dx, dy, 2.5 * (0.45 + 0.55 * k)).fill(Rgba::hex(0xffffff, 0.16 + 0.1 * near)).alpha(a * k.sqrt()).done());
            if k > 0.5 {
                self.hits.push((Rect::new(dx - sp * 0.5, dy - 10.0, sp, 20.0), Hit::Dot(i)));
            }
        }
        let lim = (wc - half - 0.3).max(-0.3);
        let lim_hi = (wc + half + 0.3).min(n as f32 - 0.7);
        let (lo, hi) = (head.min(tail).max(lim), head.max(tail).min(lim_hi));
        let pill = Rect::new(xi(lo) - 7.0, dy - 3.0, (hi - lo).max(0.0) * sp + 14.0, 6.0);
        out.shapes.push(paint::rect(sc, pill, 3.0).on_fill(1.0, 0.0, 0.0).alpha(a).done());

        // Footer: folder on the left, keys on the right.
        let (dk, _) = self.text.get(fs, &self.dir_label, F_HINT);
        let lh = self.text.line_h(F_HINT);
        self.text(out, dk, r.x + 28.0, dy - lh * 0.5, MUTED.fade(a * 0.9), clip);
        let hints = [("← →", "Browse"), ("Space", "Preview"), ("↵", "Set")];
        let mut total = 0.0;
        for (c, h) in hints {
            total += self.hint_w(fs, c, h) + 18.0;
        }
        let mut x = r.x + r.w - 28.0 - (total - 18.0);
        for (c, h) in hints {
            let bw = self.keycap(fs, out, c, x, dy, a, clip);
            let hw = self.label_at(fs, out, h, F_HINT, x + bw + 7.0, dy, MUTED.fade(a), clip);
            x += bw + 7.0 + hw + 18.0;
        }
    }
}

/// Public for `demo()`: drive the carousel without a pointer.
impl Selector {
    pub fn demo_wheel(&mut self, dy: f32) {
        let _ = self.scroll(dy);
    }
    pub fn demo_hover_card(&mut self, d: isize) {
        let i = (self.target() as isize + d).clamp(0, self.items.len() as isize - 1) as usize;
        self.set_hover(Hit::Card(i));
    }
    pub fn demo_apply(&mut self) -> Option<PathBuf> {
        let i = self.target();
        self.apply_centre(true);
        self.items.get(i).map(|it| it.e.path.clone())
    }
    pub fn demo_preview(&mut self) {
        self.apply_centre(false);
    }
    pub fn path_at(&self, d: isize) -> Option<PathBuf> {
        let i = self.target() as isize + d;
        (i >= 0).then(|| self.items.get(i as usize).map(|it| it.e.path.clone())).flatten()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// Thumbnails still on their way.
    pub fn loading(&self) -> bool {
        self.items.iter().any(|i| i.queued || i.pending.is_some())
    }
    pub fn centre_card(&self) -> Rect {
        self.card_rect(0.0, 0.0, 0.0).0
    }
}
