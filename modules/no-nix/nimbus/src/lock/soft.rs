//! The no-GPU lock screen: what each lock surface shows when Vulkan is
//! missing, the device is lost, or the locker restarted itself after a
//! fault. Plain CPU pixels into a wl_shm buffer (XRGB8888, little endian),
//! no fonts, no textures, nothing that can fail: matte black, and on the
//! primary output a Prism Ink field (matte ink, the prism hairline) with
//! one dot per typed character. Typing,
//! Enter and PAM work exactly as in the full scene.

use crate::theme::Rgba;

#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    Idle,
    Busy,
    Wrong,
    Done,
}

pub struct Field {
    pub dots: usize,
    pub mood: Mood,
    pub caps: bool,
}

const MAX_DOTS: usize = 16;

fn px(c: Rgba) -> [f32; 3] {
    [c.0 * 255.0, c.1 * 255.0, c.2 * 255.0]
}

fn put(canvas: &mut [u8], i: usize, c: [f32; 3]) {
    let q = |v: f32| v.clamp(0.0, 255.0) as u8;
    canvas[i..i + 4].copy_from_slice(&[q(c[2]), q(c[1]), q(c[0]), 255]);
}

fn get(canvas: &[u8], i: usize) -> [f32; 3] {
    [canvas[i + 2] as f32, canvas[i + 1] as f32, canvas[i] as f32]
}

fn blend(canvas: &mut [u8], i: usize, c: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let b = get(canvas, i);
    put(canvas, i, [b[0] + (c[0] - b[0]) * a, b[1] + (c[1] - b[1]) * a, b[2] + (c[2] - b[2]) * a]);
}

/// Signed distance to a rounded rect centred at the origin.
fn sd_rrect(x: f32, y: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let (qx, qy) = (x.abs() - hw + r, y.abs() - hh + r);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

/// The prism hairline's colour and strength at an edge point (x, y)
/// relative to a rounded rect's centre: cyan left, magenta right, cool
/// top, amber bottom, blended through the corners by the edge normal.
fn prism_rim(x: f32, y: f32, hw: f32, hh: f32, r: f32) -> ([f32; 3], f32) {
    let (qx, qy) = (x.abs() - hw + r, y.abs() - hh + r);
    let (nx, ny) = if qx > 0.0 && qy > 0.0 {
        let l = (qx * qx + qy * qy).sqrt().max(1e-4);
        (qx / l * x.signum(), qy / l * y.signum())
    } else if qx > qy {
        (x.signum(), 0.0)
    } else {
        (0.0, y.signum())
    };
    let w = [(-nx).max(0.0).powi(2), nx.max(0.0).powi(2), (-ny).max(0.0).powi(2), ny.max(0.0).powi(2)];
    let al = [0.18 * w[0], 0.17 * w[1], 0.10 * w[2], 0.10 * w[3]];
    let cols = [[120.0, 220.0, 255.0], [255.0, 120.0, 210.0], [170.0, 200.0, 255.0], [255.0, 190.0, 150.0]];
    let a: f32 = al.iter().sum();
    let c = [0, 1, 2].map(|k| (0..4).map(|e| cols[e][k] * al[e]).sum::<f32>() / a.max(1e-4));
    (c, a)
}

/// Paint a `w`×`h` buffer (`scale` physical px per logical px).
pub fn paint(canvas: &mut [u8], w: u32, h: u32, scale: f32, field: Option<&Field>, accent: (Rgba, Rgba)) {
    let (w, h) = (w as usize, h as usize);
    // Flat matte black.
    let c = px(Rgba::hex(0x0b0b0e, 1.0));
    let q = |v: f32| v.clamp(0.0, 255.0) as u8;
    let p = [q(c[2]), q(c[1]), q(c[0]), 255];
    for px in canvas.chunks_exact_mut(4).take(w * h) {
        px.copy_from_slice(&p);
    }
    let Some(f) = field else { return };
    let s = scale;
    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.58);
    let (pw, ph, pr) = (300.0 * s, 52.0 * s, 17.0 * s);
    // Matte ink; a wrong password tints it (and its edge) red, success
    // fills it with the accent. Otherwise the edge is the prism hairline,
    // a little stronger while verifying.
    let (fill, red) = match f.mood {
        Mood::Wrong => (px(Rgba::hex(0x2a0d14, 1.0)), Some(px(Rgba::hex(0xff8a9a, 1.0)))),
        Mood::Done => (px(accent.0.mix(accent.1, 0.5)), None),
        Mood::Busy | Mood::Idle => (px(Rgba::hex(0x0e0e12, 1.0)), None),
    };
    let rim_k = match f.mood {
        Mood::Busy => 1.6,
        Mood::Done => 0.0,
        _ => 1.0,
    };
    let (x0, x1) = ((cx - pw * 0.5 - 2.0).max(0.0) as usize, ((cx + pw * 0.5 + 2.0) as usize).min(w));
    let (y0, y1) = ((cy - ph * 0.5 - 2.0).max(0.0) as usize, ((cy + ph * 0.5 + 2.0) as usize).min(h));
    for y in y0..y1 {
        for x in x0..x1 {
            let d = sd_rrect(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, pw * 0.5, ph * 0.5, pr);
            let i = (y * w + x) * 4;
            blend(canvas, i, fill, (0.5 - d).clamp(0.0, 1.0));
            // The hairline just inside the edge.
            let band = (1.0 - (d + 0.62 * s).abs() / (0.62 * s).max(0.5)).clamp(0.0, 1.0);
            if band > 0.0 {
                let (rc, ra) = match red {
                    Some(c) => (c, 0.42),
                    None => {
                        let (c, a) = prism_rim(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, pw * 0.5, ph * 0.5, pr);
                        (c, a * rim_k)
                    }
                };
                blend(canvas, i, rc, band * ra);
            }
        }
    }
    let ink = match f.mood {
        Mood::Done => px(Rgba::hex(0x1a1426, 1.0)),
        Mood::Busy => px(accent.1),
        _ => px(Rgba::hex(0xf4f4f6, 1.0)),
    };
    let n = f.dots.min(MAX_DOTS);
    let (sp, r) = (16.0 * s, 4.5 * s);
    for k in 0..n {
        let dx = cx + (k as f32 - (n as f32 - 1.0) * 0.5) * sp;
        let (bx0, bx1) = ((dx - r - 1.0).max(0.0) as usize, ((dx + r + 2.0) as usize).min(w));
        let (by0, by1) = ((cy - r - 1.0).max(0.0) as usize, ((cy + r + 2.0) as usize).min(h));
        for y in by0..by1 {
            for x in bx0..bx1 {
                let d = ((x as f32 + 0.5 - dx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() - r;
                blend(canvas, (y * w + x) * 4, ink, (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }
    // Caps Lock: an amber bar under the field.
    if f.caps {
        let amber = px(Rgba::hex(0xffc069, 1.0));
        let (bw, bh) = (40.0 * s, 3.0 * s);
        let by = cy + ph * 0.5 + 14.0 * s;
        for y in (by as usize).min(h)..((by + bh) as usize).min(h) {
            for x in ((cx - bw * 0.5) as usize).min(w)..((cx + bw * 0.5) as usize).min(w) {
                blend(canvas, (y * w + x) * 4, amber, 0.9);
            }
        }
    }
}
