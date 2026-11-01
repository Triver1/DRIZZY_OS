//! "led": matte black hardware modules with dot-matrix LED panels.
//!
//!   ( ▮▮ ▮▮ ▮▮ )        ( NIMBUS — ~/NEWFLAKE )        ( ◢ ıl [■■□] 43% ) ( 10:59 )
//!
//! Every module is a floating matte bezel holding a recessed LED window.
//! Inside, every LED of the grid exists (unlit ones are faintly visible) and
//! the dots never move: only the light does. Text is rasterised from the
//! "Silkscreen" pixel font into bitmaps (cached per string) and each pixel
//! becomes one LED. Motion is light travelling across a fixed grid: wipes
//! with a scan head, a VU sweep across the workspace meters, digits that
//! scroll through the rows, a scan on power-on.

use std::{collections::HashMap, rc::Rc};

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent};

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Bar, Hit, Rect},
    gfx::DrawList,
    paint,
    services::NetInfo,
    theme::Rgba,
};

pub const FONTS: &[&str] = &["Silkscreen"];

// ── palette ─────────────────────────────────────────────────────────────

const BEZEL_TOP: Rgba = Rgba::hex(0x131315, 1.0);
const BEZEL_BOT: Rgba = Rgba::hex(0x0a0a0b, 1.0);
const BEZEL_HOV_TOP: Rgba = Rgba::hex(0x19191c, 1.0);
const WIN_TOP: Rgba = Rgba::hex(0x030303, 1.0);
const WIN_BOT: Rgba = Rgba::hex(0x060606, 1.0);

const UNLIT: Rgba = Rgba::hex(0x1f1a14, 1.0);
const UNLIT_HI: Rgba = Rgba::hex(0x4a3b26, 1.0);
const AMBER: Rgba = Rgba::hex(0xffb21e, 1.0);
const AMBER_HOT: Rgba = Rgba::hex(0xffe2a0, 1.0);
const AMBER_EDGE: Rgba = Rgba::hex(0xe07800, 1.0);
const AMBER_GLOW: Rgba = Rgba::hex(0xff9000, 1.0);
const RED: Rgba = Rgba::hex(0xff3d2a, 1.0);
const RED_HOT: Rgba = Rgba::hex(0xffb09a, 1.0);
const RED_EDGE: Rgba = Rgba::hex(0xc01808, 1.0);
const RED_GLOW: Rgba = Rgba::hex(0xff2a10, 1.0);

// ── geometry (logical px) ───────────────────────────────────────────────

const MARGIN: f32 = 9.0;
const GAP: f32 = 8.0;
const CY: f32 = 21.0;
/// Fine pitch in *physical* px at scale 1.75; snapped per scale.
const PITCH: f32 = 4.0 / 1.75;
/// Rows in a fine panel; the coarse (clock) panel has (ROWS + 1) / 2.
const ROWS: i32 = 9;
/// Bitmap row of the font's cap top (Silkscreen caps are 5 px tall).
const CAP0: i32 = 2;
/// Window padding beyond the outer dot centres.
const WPAD: f32 = 3.0;
/// Bezel thickness around the window.
const BI: f32 = 4.0;
const BEZEL_R: f32 = 10.5;
const WIN_R: f32 = 6.5;
const DOT_K: f32 = 0.36;

const TITLE_MAX: i32 = 190;
const VOL_N: i32 = 11;
const WS_LEVELS: i32 = 4;
const WS_TRACK: f32 = 0.05;

const POWER: SpringCfg = SpringCfg::new(0.62, 0.66);
const WIPE: SpringCfg = SpringCfg::new(0.55, 1.0);
const MORPH: SpringCfg = SpringCfg::new(0.32, 1.0);
const WIDTH: SpringCfg = SpringCfg::new(0.50, 0.68);
const DRAWER: SpringCfg = SpringCfg::new(0.48, 0.62);
const LEVEL: SpringCfg = SpringCfg::new(0.42, 0.62);

// ── dot-matrix bitmaps ──────────────────────────────────────────────────

/// One bit per pixel, column-major: bit `r` of `cols[c]` is row `r`, where
/// row 7 is the first row below the baseline.
struct Bitmap {
    cols: Vec<u16>,
    /// (byte end, right ink column) per glyph, for truncation.
    ends: Vec<(usize, i32)>,
}

impl Bitmap {
    fn w(&self) -> i32 {
        self.cols.len() as i32
    }
    #[inline]
    fn at(&self, c: i32, r: i32) -> bool {
        c >= 0 && (c as usize) < self.cols.len() && (0..16).contains(&r) && self.cols[c as usize] >> r & 1 == 1
    }
}

fn raster(fs: &mut FontSystem, sc: &mut SwashCache, s: &str) -> Bitmap {
    let mut b = Buffer::new(fs, Metrics::new(8.0, 8.0));
    b.set_size(None, None);
    b.set_text(s, &Attrs::new().family(Family::Name("Silkscreen")), Shaping::Advanced, None);
    b.shape_until_scroll(fs, false);
    let mut px: Vec<(i32, i32)> = Vec::new();
    let mut ends = Vec::new();
    for run in b.layout_runs() {
        let base = run.line_y.round() as i32;
        for g in run.glyphs {
            let p = g.physical((0.0, 0.0), 1.0);
            let mut right = i32::MIN;
            if let Some(img) = sc.get_image_uncached(fs, p.cache_key) {
                let (w, h) = (img.placement.width as i32, img.placement.height as i32);
                let stride = match img.content {
                    SwashContent::Mask => 1,
                    SwashContent::SubpixelMask | SwashContent::Color => 4,
                };
                for j in 0..h {
                    for i in 0..w {
                        let k = ((j * w + i) as usize) * stride + stride - 1;
                        if img.data.get(k).copied().unwrap_or(0) >= 110 {
                            let x = p.x + img.placement.left + i;
                            let y = p.y + base - img.placement.top + j;
                            px.push((x, y - base + 7));
                            right = right.max(x);
                        }
                    }
                }
            }
            ends.push((g.end, right));
        }
    }
    let (x0, x1) = px.iter().fold((i32::MAX, i32::MIN), |(a, b), (x, _)| (a.min(*x), b.max(*x)));
    if px.is_empty() {
        return Bitmap { cols: vec![], ends: vec![] };
    }
    let mut cols = vec![0u16; (x1 - x0 + 1) as usize];
    for (x, y) in px {
        if (0..16).contains(&y) {
            cols[(x - x0) as usize] |= 1 << y;
        }
    }
    let ends = ends.into_iter().map(|(e, r)| (e, if r == i32::MIN { -1 } else { r - x0 })).collect();
    Bitmap { cols, ends }
}

// ── state ───────────────────────────────────────────────────────────────

struct WsCol {
    id: u64,
    level: Spring,
    hov: Spring,
}

const M_WS: usize = 0;
const M_TITLE: usize = 1;
const M_METERS: usize = 2;
const M_CLOCK: usize = 3;

pub struct State {
    swash: SwashCache,
    cache: HashMap<String, Rc<Bitmap>>,
    fit: HashMap<String, Rc<Bitmap>>,
    digits: Vec<Rc<Bitmap>>,
    power: [Spring; 4],
    delay: [f32; 4],
    hover: [Spring; 4],
    rects: [Rect; 4],
    seen_title: f32,
    seen_focus: f32,
    quiet: bool,
    wipe: Spring,
    title_half: Spring,
    title_show: Spring,
    drawer: Spring,
    vol: Spring,
    bat: Spring,
    net: Spring,
    cols: Vec<WsCol>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            swash: SwashCache::new(),
            cache: HashMap::new(),
            fit: HashMap::new(),
            digits: vec![],
            power: [Spring::new(0.0, POWER); 4],
            delay: [0.0, 0.07, 0.14, 0.2],
            hover: [Spring::new(0.0, presets::SNAPPY); 4],
            rects: [Rect::default(); 4],
            seen_title: -100.0,
            seen_focus: -100.0,
            quiet: false,
            wipe: Spring::new(1.0, WIPE),
            title_half: Spring::new(0.0, WIDTH),
            title_show: Spring::new(0.0, presets::SMOOTH),
            drawer: Spring::new(0.0, DRAWER),
            vol: Spring::new(0.0, LEVEL),
            bat: Spring::new(0.0, LEVEL),
            net: Spring::new(0.0, LEVEL),
            cols: vec![],
        }
    }
}

impl State {
    fn text(&mut self, fs: &mut FontSystem, s: &str) -> Rc<Bitmap> {
        if let Some(b) = self.cache.get(s) {
            return b.clone();
        }
        if self.cache.len() > 96 {
            self.cache.clear();
        }
        let b = Rc::new(raster(fs, &mut self.swash, &s.to_uppercase()));
        self.cache.insert(s.to_string(), b.clone());
        b
    }

    /// Uppercased, truncated with "..." to at most `TITLE_MAX` columns.
    fn title(&mut self, fs: &mut FontSystem, s: &str) -> Rc<Bitmap> {
        if let Some(b) = self.fit.get(s) {
            return b.clone();
        }
        if self.fit.len() > 32 {
            self.fit.clear();
        }
        let up = s.to_uppercase();
        let full = raster(fs, &mut self.swash, &up);
        let b = if full.w() <= TITLE_MAX {
            full
        } else {
            let ell = raster(fs, &mut self.swash, "...").w() + 2;
            let mut budget = TITLE_MAX - ell;
            loop {
                let cut = full.ends.iter().filter(|(_, r)| *r >= 0 && *r <= budget).map(|(e, _)| *e).last().unwrap_or(0);
                let t = raster(fs, &mut self.swash, &format!("{}...", up[..cut].trim_end()));
                if t.w() <= TITLE_MAX || cut == 0 {
                    break t;
                }
                budget -= 4;
            }
        };
        let b = Rc::new(b);
        self.fit.insert(s.to_string(), b.clone());
        b
    }
}

// ── motion ──────────────────────────────────────────────────────────────

/// Arm the wipe when the title changes. Called from `draw` too, which runs
/// first after a fact lands, so the old text never pops for a frame.
fn sync_title(bar: &Bar, st: &mut State) {
    if bar.title_changed != st.seen_title {
        st.seen_title = bar.title_changed;
        // A different window: the full scan-head wipe. The same window
        // retitling itself (terminals do it constantly): only the dots
        // that differ cross-fade in place, with no travelling light.
        st.quiet = bar.focus_changed == st.seen_focus && st.seen_focus > -100.0;
        st.seen_focus = bar.focus_changed;
        st.wipe.cfg = if st.quiet { MORPH } else { WIPE };
        st.wipe.snap(0.0);
        st.wipe.set(1.0);
    }
}

pub fn step(bar: &Bar, st: &mut State, dt: f32) -> bool {
    let mut m = false;
    for i in 0..4 {
        if st.power[i].target == 0.0 {
            st.delay[i] -= dt;
            if st.delay[i] <= 0.0 {
                st.power[i].set(1.0);
            }
            m = true;
        }
        m |= st.power[i].step(dt);
        let over = bar.pointer.is_some_and(|(x, y)| st.rects[i].contains(x, y));
        st.hover[i].set(if over { 1.0 } else { 0.0 });
        m |= st.hover[i].step(dt);
    }

    sync_title(bar, st);
    st.title_show.set(if bar.title.cur.is_some() { 1.0 } else { 0.0 });
    st.drawer.set(if bar.hovered == Hit::Clock { 1.0 } else { 0.0 });
    st.vol.set(bar.volume.map_or(0.0, |v| if v.muted { 0.0 } else { v.percent.min(100) as f32 / 100.0 }));
    st.bat.set(bar.battery.map_or(0.0, |b| b.percent.min(100) as f32 / 100.0));
    st.net.set(match bar.net {
        NetInfo::Offline => 0.0,
        NetInfo::Ethernet => 4.0,
        NetInfo::Wifi(q) => match q {
            0..=20 => 1.0,
            21..=45 => 2.0,
            46..=70 => 3.0,
            _ => 4.0,
        },
    });
    m |= st.wipe.step(dt) | st.title_half.step(dt) | st.title_show.step(dt) | st.drawer.step(dt);
    m |= st.vol.step(dt) | st.bat.step(dt) | st.net.step(dt);

    // Workspace meters follow the live list by id.
    st.cols.retain(|c| bar.workspaces.iter().any(|w| w.id == c.id));
    if st.cols.len() != bar.workspaces.len() || st.cols.iter().zip(&bar.workspaces).any(|(c, w)| c.id != w.id) {
        let mut old = std::mem::take(&mut st.cols);
        for w in &bar.workspaces {
            let c = match old.iter().position(|c| c.id == w.id) {
                Some(p) => old.remove(p),
                None => WsCol { id: w.id, level: Spring::new(0.0, LEVEL), hov: Spring::new(0.0, presets::SNAPPY) },
            };
            st.cols.push(c);
        }
    }
    for (c, w) in st.cols.iter_mut().zip(&bar.workspaces) {
        c.level.set((w.windows as i32).min(WS_LEVELS) as f32);
        c.hov.set(if bar.hovered == Hit::Workspace(w.id) { 1.0 } else { 0.0 });
        m |= c.level.step(dt) | c.hov.step(dt);
    }
    m
}

// ── drawing helpers ─────────────────────────────────────────────────────

#[inline]
fn snap(v: f32, s: f32) -> f32 {
    (v * s).round() / s
}

#[inline]
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// 1 at 0, falling to 0 at |t| = 1.
#[inline]
fn bell(t: f32) -> f32 {
    let k = (1.0 - t * t).max(0.0);
    k * k
}

/// One LED. `lit` 0 = off, 1 = on, >1 = white-hot flash. `glow` lifts the
/// unlit dome (hover), `a` is the module's power alpha.
#[inline]
fn led(out: &mut DrawList, s: f32, x: f32, y: f32, r: f32, lit: f32, glow: f32, red: bool, a: f32) {
    let base = UNLIT.mix(UNLIT_HI, clamp01(glow));
    let mut sh = paint::circle(s, x, y, r);
    if lit < 0.01 {
        sh = sh.fill(base.fade(a)).rim(r * 0.45, Rgba::hex(0xffffff, 0.035 * a), Rgba::TRANSPARENT);
    } else {
        let (core, hot, edge, bloom) =
            if red { (RED, RED_HOT, RED_EDGE, RED_GLOW) } else { (AMBER, AMBER_HOT, AMBER_EDGE, AMBER_GLOW) };
        let l = lit.min(1.0);
        let c = base.mix(core, l).mix(hot, clamp01((lit - 1.0) * 1.6) * 0.85 + 0.12 * l);
        let e = base.mix(edge, l);
        let b = lit.min(1.4);
        sh = sh
            .fill(c.fade(a))
            .rim(r * 0.42, e.fade(a * 0.9), e.mix(c, 0.3).fade(a * 0.9))
            .bloom(bloom.fade(0.30 * b * b * a), (r * 1.35).min(2.1));
    }
    out.shapes.push(sh.done());
}

/// The matte bezel: a contact shadow, a long soft shadow, faint rim light.
fn bezel(out: &mut DrawList, s: f32, r: Rect, a: f32, hov: f32, spot: (f32, f32)) {
    out.shapes.push(paint::rect(s, r, BEZEL_R).shadow(Rgba::hex(0x000000, 0.55 * a), 1.4, 1.0).done());
    out.shapes.push(
        paint::rect(s, r, BEZEL_R)
            .gradient(BEZEL_TOP.mix(BEZEL_HOV_TOP, hov), BEZEL_BOT, 0.0, 1.0)
            .rim(1.0, Rgba::hex(0xffffff, 0.075 + 0.07 * hov), Rgba::hex(0xffffff, 0.02 + 0.015 * hov))
            .shadow(Rgba::hex(0x000000, 0.42 + 0.1 * hov), 7.0 + 2.0 * hov, 3.0 + 1.0 * hov)
            .spot(spot.0, spot.1, 60.0, 0.35 * hov)
            .alpha(a)
            .done(),
    );
}

/// The recessed LED window: dark lip at the top, a catch-light at the bottom.
fn window(out: &mut DrawList, s: f32, r: Rect, a: f32) {
    out.shapes.push(
        paint::rect(s, r, WIN_R)
            .gradient(WIN_TOP, WIN_BOT, 0.0, 1.0)
            .rim(1.0, Rgba::hex(0x000000, 0.9), Rgba::hex(0xffffff, 0.075))
            .alpha(a)
            .done(),
    );
}

/// Acrylic cover over the LEDs: a faint sheen on the upper half.
fn cover_glass(out: &mut DrawList, s: f32, r: Rect, a: f32) {
    let g = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h * 0.46);
    out.shapes.push(
        paint::rect(s, g, WIN_R - 1.0)
            .gradient(Rgba::hex(0xffffff, 0.04 * a), Rgba::hex(0xffffff, 0.0), 0.0, 1.0)
            .done(),
    );
}

/// Power-on scan: (lit gate, scan-head flash) for a dot at fraction `xn`.
#[inline]
fn scan(p: f32, xn: f32) -> (f32, f32) {
    let sx = p * 1.3 - 0.1;
    let t = (sx - xn) / 0.15;
    (clamp01(t), if p < 1.0 || t < 1.0 { bell(t - 0.5) } else { 0.0 })
}

struct Grid {
    pitch: f32,
    r: f32,
    y0: f32,
}

impl Grid {
    #[inline]
    fn y(&self, row: i32) -> f32 {
        self.y0 + row as f32 * self.pitch
    }
}

// ── draw ────────────────────────────────────────────────────────────────

pub fn draw(bar: &mut Bar, st: &mut State, fs: &mut FontSystem, out: &mut DrawList) {
    if bar.width <= 0.0 {
        return;
    }
    let s = bar.scale;
    let p1 = (PITCH * s).round().max(2.0) / s;
    let p2 = p1 * 2.0;
    let span = p1 * (ROWS - 1) as f32;
    let win_h = span + WPAD * 2.0;
    let bez_h = win_h + BI * 2.0;
    let spot = (bar.spot_x.get(), bar.spot_y.get());
    let ptr = bar.pointer;

    if st.digits.is_empty() {
        for c in "0123456789:".chars() {
            let b = Rc::new(raster(fs, &mut st.swash, &c.to_string()));
            st.digits.push(b);
        }
    }

    let enter = |st: &State, i: usize| {
        let p = st.power[i].get();
        (p, clamp01(p * 2.4), -(1.0 - p) * 8.0)
    };
    // Pointer flashlight: unlit LEDs near the cursor catch a little light.
    let torch = |x: f32, y: f32, h: f32| -> f32 {
        if h <= 0.001 {
            return 0.0;
        }
        match ptr {
            Some(_) => {
                let (dx, dy) = (x - spot.0, y - spot.1);
                h * (-(dx * dx + dy * dy) / (2.0 * 14.0 * 14.0)).exp()
            }
            None => 0.0,
        }
    };

    // ── workspaces: VU columns ──────────────────────────────────────────
    {
        let (p, a, dy) = enter(st, M_WS);
        let hov = st.hover[M_WS].get().clamp(0.0, 1.2);
        let n = bar.workspaces.len().max(1) as i32;
        // Coarse grid: one channel per workspace with an unlit column
        // between, a pad column each end. Rows 0..4 are the meter, the
        // bottom row is a light pipe carrying the active marker.
        let cols = 2 * n + 1;
        let win_w = (cols - 1) as f32 * p2 + WPAD * 2.0;
        let body = Rect::new(MARGIN, CY - bez_h * 0.5 + dy, win_w + BI * 2.0, bez_h);
        st.rects[M_WS] = body;
        bar.hits.push((body, Hit::Panel));
        if a > 0.05 {
            bar.glass.push(body);
        }
        if a > 0.002 {
            bezel(out, s, body, a, hov, spot);
            let win = Rect::new(body.x + BI, body.y + BI, win_w, win_h);
            window(out, s, win, a);
            let x0 = snap(win.x + WPAD, s);
            let g = Grid { pitch: p2, r: p2 * DOT_K * 0.94, y0: snap(win.y + WPAD, s) };
            let (h, t) = (bar.ws_head.get(), bar.ws_tail.get());
            let (lo, hi) = (h.min(t), h.max(t));
            let moving = clamp01((hi - lo) * 1.6);
            for c in 0..cols {
                let x = x0 + c as f32 * p2;
                let (gate, flash) = scan(p, c as f32 / (cols - 1) as f32);
                let i = (c - 1) / 2;
                let inside = c % 2 == 1 && i < bar.workspaces.len() as i32;
                let (act, level, chov, red) = if inside {
                    let w = &bar.workspaces[i as usize];
                    let col = st.cols.get(i as usize);
                    let dist = (lo - i as f32).max(i as f32 - hi).max(0.0);
                    let act = clamp01(1.0 - dist);
                    let lv = col.map_or(0.0, |c| c.level.get());
                    // The travelling light kicks every channel it crosses
                    // up to full scale, like a VU needle sweeping past.
                    let lv = lv + (WS_LEVELS as f32 - lv).max(0.0) * act * moving;
                    (act, lv, col.map_or(0.0, |c| c.hov.get()), w.urgent)
                } else {
                    (0.0, 0.0, 0.0, false)
                };
                for r in 0..WS_LEVELS {
                    let y = g.y(r);
                    let mut lit = 0.0;
                    if inside {
                        let k = (WS_LEVELS - 1 - r) as f32;
                        lit = WS_TRACK + (1.0 - WS_TRACK) * clamp01(level - k) * (0.34 + 0.66 * act);
                    }
                    let lit = lit * gate * (0.92 + 0.08 * hov) + flash * 0.5 * (lit + 0.25);
                    let glow = hov * 0.1 + torch(x, y, hov) * 0.7 + chov * 0.65;
                    led(out, s, x, y, g.r, lit, glow, red, a);
                }
            }
            // Light pipe: a fixed diffuser; the light inside it slides.
            let py = g.y(WS_LEVELS);
            let ph = g.r * 1.15;
            let pipe = Rect::new(x0 - g.r, py - ph * 0.5, (cols - 1) as f32 * p2 + g.r * 2.0, ph);
            out.shapes.push(
                paint::rect(s, pipe, ph * 0.5)
                    .fill(UNLIT.mix(UNLIT_HI, hov * 0.12).fade(a))
                    .rim(0.6, Rgba::hex(0xffffff, 0.04 * a), Rgba::TRANSPARENT)
                    .done(),
            );
            let (gate, _) = scan(p, 0.5);
            let (l, r) = (x0 + (1.0 + 2.0 * lo) * p2 - g.r * 1.3, x0 + (1.0 + 2.0 * hi) * p2 + g.r * 1.3);
            let energy = ((g.r * 2.6) / (r - l)).clamp(0.55, 1.0) * gate;
            if energy > 0.01 {
                let lit = Rect::new(l, pipe.y, r - l, ph);
                out.shapes.push(
                    paint::rect(s, lit, ph * 0.5)
                        .gradient(AMBER.mix(AMBER_HOT, 0.2), AMBER, 0.0, 1.0)
                        .rim(0.7, AMBER_HOT.fade(0.7), AMBER_EDGE)
                        .bloom(AMBER_GLOW.fade(0.5 * energy), 2.6)
                        .alpha(a * energy)
                        .done(),
                );
            }
            cover_glass(out, s, win, a);
            if a > 0.05 {
                for (i, w) in bar.workspaces.iter().enumerate() {
                    let cx = x0 + (1 + 2 * i as i32) as f32 * p2;
                    bar.hits.push((Rect::new(cx - p2, body.y, p2 * 2.0, body.h), Hit::Workspace(w.id)));
                }
            }
        }
    }

    // ── title: dot-matrix with a column wipe ────────────────────────────
    {
        let cur = bar.title.cur.as_deref().map(|t| st.title(fs, t));
        let prev = bar.title.prev.as_deref().map(|t| st.title(fs, t));
        let pad_c = 2.0;
        sync_title(bar, st);
        if let Some(c) = &cur {
            // Hold the wider of the two texts while the wipe runs, then
            // let the bezel spring in around the new one.
            let wp = prev.as_ref().filter(|_| !st.quiet && st.wipe.get() < 0.8).map_or(0, |b| b.w());
            let target = (c.w().max(wp) as f32 * 0.5).ceil() + pad_c;
            if st.title_half.target != target {
                if st.title_half.get() < 1.0 {
                    st.title_half.snap(target * 0.6);
                }
                st.title_half.set(target);
            }
        }
        let (p, a, dy) = enter(st, M_TITLE);
        let show = st.title_show.get();
        let a = a * clamp01(show * 1.5);
        let hov = st.hover[M_TITLE].get().clamp(0.0, 1.2);
        let half = st.title_half.get().max(pad_c);
        let cx = snap(bar.width * 0.5, s);
        let win_w = half * 2.0 * p1 - p1 + WPAD * 2.0;
        let body = Rect::new(cx - win_w * 0.5 - BI, CY - bez_h * 0.5 + dy, win_w + BI * 2.0, bez_h);
        st.rects[M_TITLE] = body;
        if a > 0.002 {
            bar.hits.push((body, Hit::Panel));
            if a > 0.05 {
                bar.glass.push(body);
            }
            bezel(out, s, body, a, hov, spot);
            let win = Rect::new(body.x + BI, body.y + BI, win_w, win_h);
            window(out, s, win, a);
            let g = Grid { pitch: p1, r: p1 * DOT_K, y0: snap(win.y + WPAD, s) };
            // Columns k sit at cx + (k + 0.5) p1; text starts at -w/2.
            let k0 = |b: &Option<Rc<Bitmap>>| b.as_ref().map_or(0, |b| -(b.w() / 2));
            let (kc, kp) = (k0(&cur), k0(&prev));
            let wc = cur.as_ref().map_or(0, |b| b.w());
            let wp = prev.as_ref().map_or(0, |b| b.w());
            let (klo, khi) = ((kc.min(kp)) as f32, ((kc + wc).max(kp + wp)) as f32);
            const SOFT: f32 = 4.0;
            let wv = st.wipe.get();
            let front = klo - SOFT + wv * (khi - klo + 2.0 * SOFT);
            let wiping = !st.wipe.at_rest() && !st.quiet;
            let nk = half.ceil() as i32;
            for k in -nk..nk {
                let kf = k as f32 + 0.5;
                // Columns slide under the bezel edge as the window resizes.
                let edge = clamp01(half - kf.abs() + 0.5);
                if edge <= 0.01 {
                    continue;
                }
                let x = cx + kf * p1;
                let (gate, flash) = scan(p, (k + nk) as f32 / (2 * nk).max(1) as f32);
                let t = if st.quiet { clamp01(wv) } else { clamp01((front - k as f32) / SOFT) };
                let head = if wiping { bell((front - SOFT * 0.5 - k as f32) / (SOFT * 0.6)) } else { 0.0 };
                for r in 0..ROWS {
                    let y = g.y(r);
                    // Bitmap rows map 1:1 onto panel rows: caps land on 2..=6.
                    let on_c = cur.as_ref().is_some_and(|b| b.at(k - kc, r));
                    let on_p = prev.as_ref().is_some_and(|b| b.at(k - kp, r));
                    let mut lit = on_c as i32 as f32 * t + on_p as i32 as f32 * (1.0 - t);
                    lit = lit * gate * (0.9 + 0.1 * hov) * clamp01(show);
                    lit += head * (0.22 + 0.5 * lit) + flash * 0.5 * (lit + 0.25);
                    let glow = hov * 0.12 + torch(x, y, hov) * 0.8;
                    led(out, s, x, y, g.r * (0.6 + 0.4 * edge), lit * edge, glow, false, a * edge);
                }
            }
            cover_glass(out, s, win, a);
        }
    }

    // ── clock (+ date drawer) and meters, laid out right to left ────────
    let clock_x1 = bar.width - MARGIN;
    let dw = st.digits[..10].iter().map(|b| b.w()).max().unwrap_or(4);
    let colon_w = st.digits[10].w().max(1);
    let clock_cols = 1 + dw + 1 + dw + 1 + colon_w + 1 + dw + 1 + dw + 1;
    let clock_win_w = (clock_cols - 1) as f32 * p2 + WPAD * 2.0;
    let date = bar.clock.map(|c| {
        const WD: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
        const MO: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
        format!("{} {} {}", WD[c.weekday as usize % 7], c.day, MO[c.month as usize % 12])
    });
    let date_bm = date.as_deref().map(|d| st.text(fs, d));
    let dcols = date_bm.as_ref().map_or(0, |b| b.w()) + 2;
    let drawer_full = (dcols - 1) as f32 * p1 + WPAD * 2.0 + 4.0;
    let dr = st.drawer.get().max(0.0);
    let drawer_w = drawer_full * dr;
    {
        let (p, a, dy) = enter(st, M_CLOCK);
        let hov = st.hover[M_CLOCK].get().clamp(0.0, 1.2);
        let w = clock_win_w + BI * 2.0 + drawer_w;
        let body = Rect::new(clock_x1 - w, CY - bez_h * 0.5 + dy, w, bez_h);
        st.rects[M_CLOCK] = body;
        bar.hits.push((body, Hit::Clock));
        if a > 0.05 {
            bar.glass.push(body);
        }
        if a > 0.002 {
            bezel(out, s, body, a, hov, spot);
            let win = Rect::new(body.x + body.w - BI - clock_win_w, body.y + BI, clock_win_w, win_h);
            window(out, s, win, a);
            let g = Grid { pitch: p2, r: p2 * DOT_K * 0.94, y0: snap(win.y + WPAD, s) };
            let x0 = snap(win.x + WPAD, s);
            let rows2 = (ROWS + 1) / 2;
            // Column → (digit slot or colon, column within).
            let mut slots: [(i32, i32); 5] = [(0, 0); 5];
            {
                let mut c = 1;
                for (i, sl) in slots.iter_mut().enumerate() {
                    let w = if i == 2 { colon_w } else { dw };
                    *sl = (c, w);
                    c += w + 1;
                }
            }
            let digit = |ch: char| -> Option<usize> { ch.to_digit(10).map(|d| d as usize) };
            for c in 0..clock_cols {
                let x = x0 + c as f32 * p2;
                let (gate, flash) = scan(p, c as f32 / (clock_cols - 1) as f32);
                let slot = slots.iter().position(|(s0, w)| c >= *s0 && c < s0 + w);
                for r in 0..rows2 {
                    let y = g.y(r);
                    let mut lit = 0.0;
                    match slot {
                        Some(2) => {
                            let b = &st.digits[10];
                            let off = (colon_w - b.w()) / 2;
                            lit = b.at(c - slots[2].0 - off, r + CAP0) as i32 as f32;
                        }
                        Some(si) => {
                            let di = if si < 2 { si } else { si - 1 };
                            let d = &bar.digits[di];
                            let col = c - slots[si].0;
                            let sample = |ch: char, row: i32| -> f32 {
                                match digit(ch) {
                                    Some(n) => {
                                        let b = &st.digits[n];
                                        let off = (dw - b.w()) / 2;
                                        b.at(col - off, row + CAP0) as i32 as f32
                                    }
                                    None => 0.0,
                                }
                            };
                            // In place: the new digit wipes over the old one
                            // column by column, a hot scan head on the seam.
                            let pp = d.p.get();
                            if d.p.at_rest() && pp >= 1.0 {
                                lit = sample(d.cur, r);
                            } else {
                                const DS: f32 = 1.6;
                                let front = -DS + pp * (dw as f32 + 2.0 * DS);
                                let tt = clamp01((front - col as f32) / DS);
                                lit = sample(d.cur, r) * tt + sample(d.prev, r) * (1.0 - tt);
                                lit += bell((front - DS * 0.5 - col as f32) / (DS * 0.7)) * (0.25 + 0.5 * lit);
                            }
                        }
                        None => {}
                    }
                    let lit = lit * gate * (0.92 + 0.08 * hov) + flash * 0.5 * (lit + 0.25);
                    let glow = hov * 0.12 + torch(x, y, hov) * 0.8;
                    led(out, s, x, y, g.r, lit, glow, false, a);
                }
            }
            cover_glass(out, s, win, a);

            // Date drawer: its own fine-pitch window sliding out to the left.
            if let Some(b) = &date_bm
                && drawer_w > 2.0
            {
                let x1 = win.x - 4.0;
                let dwin = Rect::new(x1 - (drawer_w - 4.0), win.y, drawer_w - 4.0, win_h);
                window(out, s, dwin, a * clamp01(drawer_w / 8.0));
                let g1 = Grid { pitch: p1, r: p1 * DOT_K, y0: snap(win.y + WPAD, s) };
                let xr = snap(x1 - WPAD, s);
                let open = clamp01(dr * 1.4 - 0.25);
                for j in 0..dcols {
                    let x = xr - j as f32 * p1;
                    let edge = clamp01((x - dwin.x - WPAD * 0.5) / p1);
                    if edge <= 0.01 {
                        break;
                    }
                    let col = dcols - 2 - j; // one pad column each side
                    for r in 0..ROWS {
                        let y = g1.y(r);
                        let lit = b.at(col, r - 2 + CAP0) as i32 as f32 * open;
                        let glow = hov * 0.12 + torch(x, y, hov) * 0.8;
                        led(out, s, x, y, g1.r * (0.6 + 0.4 * edge), lit * edge, glow, false, a * edge);
                    }
                }
                cover_glass(out, s, dwin, a);
            }
        }
    }

    // ── meters: volume wedge, signal bars, battery segments ─────────────
    {
        let (p, a, dy) = enter(st, M_METERS);
        let hov = st.hover[M_METERS].get().clamp(0.0, 1.2);
        let bat_txt = bar.battery.map(|b| format!("{}%", b.percent));
        let tb = bat_txt.as_deref().map(|t| st.text(fs, t));
        let tw = tb.as_ref().map_or(0, |b| b.w());
        let (vol0, net0) = (1, 1 + VOL_N + 3);
        let bat0 = net0 + 7 + 3;
        let txt0 = bat0 + 16 + 2;
        let cols = txt0 + tw + 1;
        let win_w = (cols - 1) as f32 * p1 + WPAD * 2.0;
        let x1 = st.rects[M_CLOCK].x - GAP;
        let body = Rect::new(x1 - win_w - BI * 2.0, CY - bez_h * 0.5 + dy, win_w + BI * 2.0, bez_h);
        st.rects[M_METERS] = body;
        bar.hits.push((body, Hit::Status));
        if a > 0.05 {
            bar.glass.push(body);
        }
        if a > 0.002 {
            bezel(out, s, body, a, hov, spot);
            let win = Rect::new(body.x + BI, body.y + BI, win_w, win_h);
            window(out, s, win, a);
            let x0 = snap(win.x + WPAD, s);
            let g = Grid { pitch: p1, r: p1 * DOT_K, y0: snap(win.y + WPAD, s) };
            let muted = bar.volume.is_some_and(|v| v.muted);
            let vl = st.vol.get() * VOL_N as f32;
            let nl = st.net.get();
            let offline = matches!(bar.net, NetInfo::Offline);
            let bl = st.bat.get() * 4.0;
            let (low, charging) = bar.battery.map_or((false, false), |b| (b.percent <= 15 && !b.charging, b.charging));
            const TRACK: f32 = 0.13;
            for c in 0..cols {
                let x = x0 + c as f32 * p1;
                let (gate, flash) = scan(p, c as f32 / (cols - 1) as f32);
                for r in 0..ROWS {
                    let y = g.y(r);
                    let (mut lit, mut red) = (0.0f32, false);
                    let k = c - vol0;
                    if (0..VOL_N).contains(&k) {
                        let h = 1 + (k * 6 + 5) / 10;
                        if r <= 7 && r > 7 - h {
                            lit = if muted { TRACK } else { TRACK + (1.0 - TRACK) * clamp01(vl - k as f32) };
                            red = muted;
                        }
                    }
                    let k = c - net0;
                    if (0..7).contains(&k) && k % 2 == 0 {
                        let j = k / 2;
                        if r <= 7 && r > 7 - (1 + 2 * j) {
                            lit = TRACK + (1.0 - TRACK) * clamp01(nl - j as f32);
                            if offline && j == 0 {
                                (lit, red) = (1.0, true);
                            }
                        }
                    }
                    let k = c - bat0;
                    if (0..16).contains(&k) {
                        let border = ((r == 1 || r == 7) && (1..=13).contains(&k)) || ((k == 0 || k == 14) && (2..=6).contains(&r));
                        if border {
                            lit = 0.42;
                        } else if k == 15 && (3..=5).contains(&r) {
                            lit = if charging { 1.0 } else { 0.42 };
                        } else if (3..=5).contains(&r) && (2..=12).contains(&k) && (k - 2) % 3 < 2 {
                            let seg = ((k - 2) / 3) as f32;
                            lit = TRACK + (1.0 - TRACK) * clamp01(bl - seg);
                        }
                        red = low;
                    }
                    let k = c - txt0;
                    if let Some(b) = &tb
                        && (0..tw).contains(&k)
                    {
                        lit = b.at(k, r - 2 + CAP0) as i32 as f32;
                        red = low;
                    }
                    let lit = lit * gate * (0.92 + 0.08 * hov) + flash * 0.5 * (lit + 0.25);
                    let glow = hov * 0.12 + torch(x, y, hov) * 0.8;
                    led(out, s, x, y, g.r, lit, glow, red, a);
                }
            }
            cover_glass(out, s, win, a);
            let vx = x0 + (vol0 as f32 - 1.0) * p1;
            bar.hits.push((Rect::new(vx - WPAD, body.y, p1 * 11.0 + WPAD, body.h), Hit::Volume));
        }
    }
}
