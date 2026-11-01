//! Content-aware reveal order for wallpaper changes.
//!
//! The new picture is read at 256 px wide in OKLab and its contours found
//! (Sobel on lightness and chroma, scaled by the picture's own contrast).
//! Its shapes are the stretches between those contours: connected cells
//! off any contour, specks dropped, contour cells handed to the shape they
//! lean into (seeded watershed). From the point the change came from, a
//! geodesic distance floods the grid (Dijkstra): moving inside a shape is
//! cheap, crossing a contour costs in proportion to its strength, stepping
//! into another shape costs extra. Each shape then fills faster than the
//! flood reaches it and a beat apart from its neighbours, so the picture
//! arrives shape by shape, along its own lines.
//!
//! The result is a small RGBA8 texture laid over the picture's UVs:
//! R + G / 256 = reveal order (0 first, 1 last; 16 bits over two
//! channels), B = when the shape under the pixel starts to fill, A =
//! contour strength. Runs on a short-lived thread, once per change, in
//! ~15-40 ms.

use std::{cmp::Reverse, collections::BinaryHeap, sync::Arc};

use crate::gfx::ImageData;

/// Grid width of the analysis (height follows the picture's aspect).
const GRID_W: usize = 256;

/// A finished order map.
pub struct Order {
    pub img: ImageData,
    /// Analysis wall time, for the demo's report.
    pub ms: f32,
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// OKLab from linear sRGB.
fn oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// Area-average `src` onto a `gw`×`gh` grid of OKLab values (sparse taps:
/// a cell of a 4K picture reads ~16 pixels, enough for a low-pass).
fn downscale(src: &ImageData, gw: usize, gh: usize) -> Vec<[f32; 3]> {
    let lut: Vec<f32> = (0..=255u8).map(srgb_to_linear).collect();
    let (sw, sh) = (src.w as usize, src.h as usize);
    let mut out = Vec::with_capacity(gw * gh);
    for gy in 0..gh {
        let (y0, y1) = (gy * sh / gh, ((gy + 1) * sh / gh).max(gy * sh / gh + 1).min(sh));
        let sy = ((y1 - y0) / 4).max(1);
        for gx in 0..gw {
            let (x0, x1) = (gx * sw / gw, ((gx + 1) * sw / gw).max(gx * sw / gw + 1).min(sw));
            let sx = ((x1 - x0) / 4).max(1);
            let mut acc = [0f32; 3];
            let mut n = 0f32;
            for y in (y0..y1).step_by(sy) {
                let row = &src.rgba[(y * sw) * 4..(y * sw + sw) * 4];
                for x in (x0..x1).step_by(sx) {
                    let p = &row[x * 4..x * 4 + 3];
                    acc[0] += lut[p[0] as usize];
                    acc[1] += lut[p[1] as usize];
                    acc[2] += lut[p[2] as usize];
                    n += 1.0;
                }
            }
            let n = n.max(1.0);
            out.push(oklab(acc[0] / n, acc[1] / n, acc[2] / n));
        }
    }
    out
}

/// Separable [1 2 1]/4 blur, clamped at the borders.
fn blur(v: &[[f32; 3]], w: usize, h: usize) -> Vec<[f32; 3]> {
    let mut tmp = vec![[0f32; 3]; v.len()];
    for y in 0..h {
        for x in 0..w {
            let (a, b, c) = (v[y * w + x.saturating_sub(1)], v[y * w + x], v[y * w + (x + 1).min(w - 1)]);
            tmp[y * w + x] = [0, 1, 2].map(|k| (a[k] + 2.0 * b[k] + c[k]) * 0.25);
        }
    }
    let mut out = vec![[0f32; 3]; v.len()];
    for y in 0..h {
        for x in 0..w {
            let (a, b, c) = (tmp[y.saturating_sub(1) * w + x], tmp[y * w + x], tmp[(y + 1).min(h - 1) * w + x]);
            out[y * w + x] = [0, 1, 2].map(|k| (a[k] + 2.0 * b[k] + c[k]) * 0.25);
        }
    }
    out
}

/// [`blur`] for one channel.
fn blur1(v: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; v.len()];
    for y in 0..h {
        for x in 0..w {
            tmp[y * w + x] = (v[y * w + x.saturating_sub(1)] + 2.0 * v[y * w + x] + v[y * w + (x + 1).min(w - 1)]) * 0.25;
        }
    }
    let mut out = vec![0f32; v.len()];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = (tmp[y.saturating_sub(1) * w + x] + 2.0 * tmp[y * w + x] + tmp[(y + 1).min(h - 1) * w + x]) * 0.25;
        }
    }
    out
}

/// `q`-quantile of `v` (sorts a copy).
fn quantile(v: &[f32], q: f32) -> f32 {
    let mut c = v.to_vec();
    let k = ((c.len() as f32 - 1.0) * q) as usize;
    let (_, m, _) = c.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
    *m
}

/// Smooth value noise in 0..1 (a few octaves, hashed lattice).
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    fn hash(x: i32, y: i32, s: u32) -> f32 {
        let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ s.wrapping_mul(0xcb1a_b31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 65535.0
    }
    let mut v = 0.0;
    let mut amp = 0.5;
    let (mut x, mut y) = (x, y);
    for o in 0..2 {
        let (xi, yi) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - xi as f32, y - yi as f32);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let s = seed.wrapping_add(o * 977);
        let a = hash(xi, yi, s) + (hash(xi + 1, yi, s) - hash(xi, yi, s)) * sx;
        let b = hash(xi, yi + 1, s) + (hash(xi + 1, yi + 1, s) - hash(xi, yi + 1, s)) * sx;
        v += (a + (b - a) * sy) * amp;
        amp *= 0.5;
        x *= 2.03;
        y *= 2.03;
    }
    v / 0.75
}

/// The cell the eye goes to first: bright, saturated, near the middle.
fn salient(lab: &[[f32; 3]], w: usize, h: usize) -> (f32, f32) {
    let mut best = (f32::MIN, 0.5, 0.5);
    let r = (w / 24).max(1) as isize;
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut s = 0.0;
            let mut n = 0.0;
            for dy in (-r..=r).step_by(2) {
                for dx in (-r..=r).step_by(2) {
                    let (xx, yy) = ((x as isize + dx).clamp(0, w as isize - 1) as usize, (y as isize + dy).clamp(0, h as isize - 1) as usize);
                    let c = lab[yy * w + xx];
                    s += c[0] + 1.5 * c[1].hypot(c[2]);
                    n += 1.0;
                }
            }
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let centre = 1.0 - 0.6 * ((u - 0.5).powi(2) + (v - 0.5).powi(2)) * 4.0;
            let score = s / n * centre;
            if score > best.0 {
                best = (score, u, v);
            }
        }
    }
    (best.1, best.2)
}

/// The picture's shapes: the stretches between its contours (4-connected
/// cells under `THR` contour strength), specks dropped; contour cells and
/// specks then join the shape they lean into (seeded watershed on contour
/// strength). Dense labels and their count.
fn shapes(edge: &[f32], w: usize, h: usize) -> (Vec<u32>, usize) {
    const THR: f32 = 0.3;
    const FREE: u32 = u32::MAX;
    let n = w * h;
    let min = (n / 400).max(12);
    let nbrs = |i: usize| {
        let (x, y) = (i % w, i / w);
        [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)].into_iter().flatten()
    };
    let mut lab = vec![FREE; n];
    let mut count = 0u32;
    let mut stack = vec![];
    let mut members = vec![];
    for s in 0..n {
        if lab[s] != FREE || edge[s] >= THR {
            continue;
        }
        members.clear();
        lab[s] = count;
        stack.push(s);
        while let Some(i) = stack.pop() {
            members.push(i);
            for j in nbrs(i) {
                if lab[j] == FREE && edge[j] < THR {
                    lab[j] = count;
                    stack.push(j);
                }
            }
        }
        if members.len() < min {
            // A speck: up for grabs, but never a seed again.
            for &i in &members {
                lab[i] = FREE - 1;
            }
        } else {
            count += 1;
        }
    }
    if count == 0 {
        return (vec![0; n], 1);
    }
    let mut heap = BinaryHeap::new();
    for i in 0..n {
        if lab[i] < count {
            for j in nbrs(i).filter(|&j| lab[j] >= count) {
                heap.push(Reverse((edge[j].to_bits(), j as u32, lab[i])));
            }
        }
    }
    while let Some(Reverse((_, j, l))) = heap.pop() {
        let j = j as usize;
        if lab[j] < count {
            continue;
        }
        lab[j] = l;
        for k in nbrs(j).filter(|&k| lab[k] >= count) {
            heap.push(Reverse((edge[k].to_bits(), k as u32, l)));
        }
    }
    (lab, count as usize)
}

/// `passes` of a 3×3-ish average that never mixes two shapes.
fn smooth_within(v: &[f32], labels: &[u32], w: usize, h: usize, passes: usize) -> Vec<f32> {
    let mut a = v.to_vec();
    let mut b = vec![0f32; a.len()];
    for _ in 0..passes {
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let (mut sum, mut wt) = (a[i] * 2.0, 2.0);
                for j in [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)].into_iter().flatten() {
                    if labels[j] == labels[i] {
                        sum += a[j];
                        wt += 1.0;
                    }
                }
                b[i] = sum / wt;
            }
        }
        std::mem::swap(&mut a, &mut b);
    }
    a
}

/// 0..1 from a small integer.
fn hash01(v: u32) -> f32 {
    let mut h = v.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.0
}

/// Reveal order for `img` flooding out of `origin` (fractions of the
/// picture; None: its most salient spot).
pub fn analyse(img: &ImageData, origin: Option<(f32, f32)>) -> Order {
    let t0 = std::time::Instant::now();
    let w = GRID_W.min(img.w as usize).max(2);
    let h = ((w as f32 * img.h as f32 / img.w.max(1) as f32).round() as usize).max(2);
    let n = w * h;
    let lab = blur(&downscale(img, w, h), w, h);

    // Contour strength: Sobel on OKLab, scaled by the picture's own
    // contrast so a dim wallpaper finds its shapes as well as a bright one.
    let mut grad = vec![0f32; n];
    for y in 0..h {
        for x in 0..w {
            let at = |dx: isize, dy: isize| lab[(y as isize + dy).clamp(0, h as isize - 1) as usize * w + (x as isize + dx).clamp(0, w as isize - 1) as usize];
            let mut g2 = 0.0;
            for k in 0..3 {
                let wk = if k == 0 { 1.0 } else { 1.4 };
                let gx = at(1, -1)[k] + 2.0 * at(1, 0)[k] + at(1, 1)[k] - at(-1, -1)[k] - 2.0 * at(-1, 0)[k] - at(-1, 1)[k];
                let gy = at(-1, 1)[k] + 2.0 * at(0, 1)[k] + at(1, 1)[k] - at(-1, -1)[k] - 2.0 * at(0, -1)[k] - at(1, -1)[k];
                g2 += (gx * gx + gy * gy) * wk * wk;
            }
            grad[y * w + x] = g2.sqrt() * 0.125;
        }
    }
    let scale = quantile(&grad, 0.9).max(0.002);
    let edge: Vec<f32> = grad.iter().map(|g| 1.0 - (-(g / scale).powi(2) * 0.7).exp()).collect();
    let (labels, count) = shapes(&edge, w, h);

    // Geodesic flood from the origin.
    let (ou, ov) = origin.unwrap_or_else(|| salient(&lab, w, h));
    let (ox, oy) = ((ou.clamp(0.0, 1.0) * w as f32).min(w as f32 - 1.0), (ov.clamp(0.0, 1.0) * h as f32).min(h as f32 - 1.0));
    let mut d = vec![f32::INFINITY; n];
    let mut heap = BinaryHeap::new();
    let seed_r = 2.5f32;
    for y in 0..h {
        for x in 0..w {
            let r = (x as f32 + 0.5 - ox).hypot(y as f32 + 0.5 - oy);
            if r <= seed_r {
                d[y * w + x] = r;
                heap.push(Reverse((r.to_bits(), (y * w + x) as u32)));
            }
        }
    }
    const STEPS: [(isize, isize, f32); 8] =
        [(1, 0, 1.0), (-1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0), (1, 1, 1.414), (-1, 1, 1.414), (1, -1, 1.414), (-1, -1, 1.414)];
    while let Some(Reverse((db, i))) = heap.pop() {
        let di = f32::from_bits(db);
        let i = i as usize;
        if di > d[i] {
            continue;
        }
        let (x, y) = ((i % w) as isize, (i / w) as isize);
        for &(dx, dy, len) in &STEPS {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                continue;
            }
            let j = ny as usize * w + nx as usize;
            let e = edge[i].max(edge[j]);
            // Cheap inside a shape, dear across a contour, dearer still
            // into the next shape.
            let mut c = len * (1.0 + 9.0 * e * e);
            if labels[i] != labels[j] {
                c += 4.0 + 16.0 * e;
            }
            let nd = di + c;
            if nd < d[j] {
                d[j] = nd;
                heap.push(Reverse((nd.to_bits(), j as u32)));
            }
        }
    }
    // Round off the octile flood's corners inside each shape.
    let d = smooth_within(&d, &labels, w, h, 6);

    // Each shape fills faster than the flood reaches it, a beat apart:
    // the picture arrives piece by piece rather than as one blob.
    let mut start = vec![f32::INFINITY; count];
    for i in 0..n {
        start[labels[i] as usize] = start[labels[i] as usize].min(d[i]);
    }
    let dmax = d.iter().copied().fold(1.0f32, f32::max);
    let mut o: Vec<f32> = (0..n)
        .map(|i| {
            let l = labels[i] as usize;
            let stagger = hash01(l as u32) * dmax * 0.03;
            let local = start[l] + stagger + (d[i] - start[l]) * 0.45;
            // Low, slow noise: organic fronts, not torn ones.
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            local + (noise(x / 26.0, y / 26.0, 7) - 0.5) * dmax * 0.04
        })
        .collect();
    // Even pacing: half rank (area grows steadily), half distance (keeps
    // the pauses at strong contours).
    let mut idx: Vec<u32> = (0..n as u32).collect();
    idx.sort_unstable_by(|a, b| o[*a as usize].total_cmp(&o[*b as usize]));
    let (lo, hi) = (o[idx[0] as usize], o[idx[n - 1] as usize]);
    let span = (hi - lo).max(1e-3);
    let mut rank = vec![0f32; n];
    for (r, &i) in idx.iter().enumerate() {
        rank[i as usize] = r as f32 / (n - 1) as f32;
    }
    for i in 0..n {
        o[i] = 0.5 * rank[i] + 0.5 * (o[i] - lo) / span;
    }
    // Round off the grid: jumps between shapes become short ramps, so
    // fronts trace smooth curves instead of cell staircases.
    for _ in 0..3 {
        o = blur1(&o, w, h);
    }
    // Start of each shape in the same units.
    let mut sstart = vec![1f32; count];
    for i in 0..n {
        sstart[labels[i] as usize] = sstart[labels[i] as usize].min(o[i]);
    }

    let mut rgba = Vec::with_capacity(n * 4);
    for i in 0..n {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        // Order in 16 bits over two channels (filtering is linear, so the
        // shader's R + G / 256 interpolates exactly).
        let v = o[i].clamp(0.0, 1.0) * 255.0;
        let (hi, lo) = (v.floor().min(255.0), ((v - v.floor()) * 256.0).floor().min(255.0));
        rgba.extend_from_slice(&[hi as u8, lo as u8, q(sstart[labels[i] as usize]), q(edge[i])]);
    }
    Order { img: ImageData { w: w as u32, h: h as u32, rgba: Arc::new(rgba) }, ms: t0.elapsed().as_secs_f32() * 1000.0 }
}

