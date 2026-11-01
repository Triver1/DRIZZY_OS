//! Pixels the lock scene needs, prepared off the UI thread: the wallpaper
//! (a full-resolution sharp copy for the focus-pull and the unlock iris,
//! and a quarter-resolution, heavily blurred and graded copy that the GPU
//! stretches with B-spline sampling and dither), the avatar, and the
//! success checkmark.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{gfx::ImageData, theme::Rgba};

/// The two backgrounds of one output size.
#[derive(Clone)]
pub struct Backdrop {
    /// ~1/4 of the output's physical size, blurred, darkened, vignetted.
    pub blur: ImageData,
    /// The output's physical size, untouched (the desktop, for the iris).
    pub sharp: ImageData,
    /// Brightness multiplier that dims `sharp` about as much as `blur` was
    /// graded (so the intro never shows anything brighter than the end).
    pub dim: f32,
    /// Mean sRGB luminance of `blur` (0..1): how much the UI must
    /// protect its text.
    pub luma: f32,
}

fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
}

fn first_image(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file() && is_image(p)).collect();
    v.sort();
    v.into_iter().next()
}

/// The configured wallpaper (`wallpaper` in ~/.config/nimbus/config), else
/// the first image in ~/Pictures/Wallpapers/nimbus or ~/Pictures/Wallpapers.
pub fn wallpaper_path(home: &str) -> Option<PathBuf> {
    if let Some(p) = crate::config::get("wallpaper").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let pics = Path::new(home).join("Pictures/Wallpapers");
    first_image(&pics.join("nimbus")).or_else(|| first_image(&pics))
}

/// Decode and prepare `path` for an output of `pw`×`ph` physical px (cover
/// fit). Falls back to a generated dark gradient.
pub fn backdrop(path: Option<&Path>, pw: u32, ph: u32, accent: (Rgba, Rgba)) -> Backdrop {
    let pw = pw.max(16);
    let ph = ph.max(16);
    path.and_then(|p| decode(p, pw, ph)).unwrap_or_else(|| gradient(pw, ph, accent))
}

fn decode(path: &Path, pw: u32, ph: u32) -> Option<Backdrop> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.into_rgb8();
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return None;
    }
    // Cover: crop the source to the output's aspect, centred.
    let aspect = pw as f32 / ph as f32;
    let (cw, ch) = if iw as f32 / ih as f32 > aspect {
        (((ih as f32 * aspect).round() as u32).clamp(1, iw), ih)
    } else {
        (iw, ((iw as f32 / aspect).round() as u32).clamp(1, ih))
    };
    let view = image::imageops::crop_imm(&img, (iw - cw) / 2, (ih - ch) / 2, cw, ch).to_image();
    drop(img);
    let (sw, sh) = (pw, ph);
    let sharp = if (cw, ch) == (sw, sh) {
        view
    } else if cw >= sw {
        image::imageops::thumbnail(&view, sw, sh)
    } else {
        image::imageops::resize(&view, sw, sh, image::imageops::FilterType::CatmullRom)
    };
    let (bw, bh) = ((pw / 4).max(1), (ph / 4).max(1));
    let small = image::imageops::thumbnail(&sharp, bw, bh);
    let (blur, k, luma) = grade(blur_rgb(&small, bw, bh), bw, bh);
    Some(Backdrop { blur, sharp: rgba(sharp.as_raw(), sw, sh), dim: sharp_dim(k), luma })
}

fn rgba(rgb: &[u8], w: u32, h: u32) -> ImageData {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for p in rgb.chunks_exact(3) {
        out.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    ImageData { w, h, rgba: Arc::new(out) }
}

fn to_lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}

/// Three box passes each way ≈ a wide gaussian. Blurred in linear light so
/// bright highlights bloom instead of going muddy.
fn blur_rgb(img: &image::RgbImage, w: u32, h: u32) -> Vec<[f32; 3]> {
    let (w, h) = (w as usize, h as usize);
    let mut a: Vec<[f32; 3]> = img.pixels().map(|p| [to_lin(p[0]), to_lin(p[1]), to_lin(p[2])]).collect();
    let mut b = a.clone();
    let r = ((w.min(h) as f32) / 40.0).round().clamp(2.0, 24.0) as isize;
    let box_pass = |src: &[[f32; 3]], dst: &mut [[f32; 3]], n: usize, lines: usize, at: &dyn Fn(usize, usize) -> usize| {
        let k = 1.0 / (2 * r + 1) as f32;
        for l in 0..lines {
            let px = |i: isize| src[at(l, i.clamp(0, n as isize - 1) as usize)];
            let mut acc = [0.0f32; 3];
            for i in -r..=r {
                let p = px(i);
                acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
            }
            for i in 0..n as isize {
                dst[at(l, i as usize)] = [acc[0] * k, acc[1] * k, acc[2] * k];
                let (add, sub) = (px(i + r + 1), px(i - r));
                acc = [acc[0] + add[0] - sub[0], acc[1] + add[1] - sub[1], acc[2] + add[2] - sub[2]];
            }
        }
    };
    let row = |y: usize, x: usize| y * w + x;
    let col = |x: usize, y: usize| y * w + x;
    for _ in 0..3 {
        box_pass(&a, &mut b, w, h, &row);
        box_pass(&b, &mut a, h, w, &col);
    }
    a
}

/// The sRGB-space multiplier that brings the sharp copy close to the
/// graded blur (grade's `k` on linear light, plus the average vignette).
fn sharp_dim(k: f32) -> f32 {
    (k * 0.8).powf(1.0 / 2.2).clamp(0.0, 1.0)
}

/// Darken only as much as needed, add a touch of saturation (more for
/// near-grey images) and a vignette; quantise to sRGB with dither.
/// Returns the image, the linear gain applied and the mean sRGB luminance.
fn grade(lin: Vec<[f32; 3]>, w: u32, h: u32) -> (ImageData, f32, f32) {
    /// Mean linear luminance aimed for (≈ 16 % sRGB grey): matte black
    /// even for white wallpapers.
    const TARGET: f32 = 0.022;
    let lum = |p: &[f32; 3]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let n = lin.len().max(1) as f32;
    // How colourful the image is: grey ones get a little more saturation.
    let chroma = lin.iter().map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])).sum::<f32>() / n;
    let mean0 = lin.iter().map(lum).sum::<f32>() / n;
    let rel = if mean0 > 0.0 { chroma / mean0 } else { 0.0 };
    let sat = if rel < 0.25 { 1.45 } else { 1.22 };
    let shaped: Vec<[f32; 3]> = lin
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (x, y) = ((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5);
            let (nx, ny) = (x / w as f32 * 2.0 - 1.0, y / h as f32 * 2.0 - 1.0);
            let d = (nx * nx * 0.75 + ny * ny).sqrt();
            let t = ((d - 0.35) / 1.05).clamp(0.0, 1.0);
            let vig = 1.0 - 0.55 * t * t * (3.0 - 2.0 * t);
            // Slightly darker at the bottom, where the field sits.
            let floor = 1.0 - 0.10 * ((ny + 0.2) / 1.2).clamp(0.0, 1.0);
            let l = lum(p);
            p.map(|v| (l + (v - l) * sat).max(0.0) * vig * floor)
        })
        .collect();
    let mean = shaped.iter().map(lum).sum::<f32>() / n;
    // Dark wallpapers keep ~85 % of their light; bright ones go down to
    // the target. Never brighten.
    let k = if mean > 0.0 { (TARGET / mean).clamp(0.03, 0.85) } else { 0.85 };
    let mut out = Vec::with_capacity(shaped.len() * 4);
    // Triangular dither while quantising: the texture keeps sub-LSB
    // detail that filtering then averages, so stretched gradients don't step.
    let mut seed = 0x9e37_79b9u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut luma = 0.0;
    for p in &shaped {
        let px = p.map(|v| to_srgb(v * k) * 255.0);
        luma += (0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]) / 255.0;
        for c in px {
            let d = rnd() + rnd() - 1.0;
            out.push((c + d + 0.5).clamp(0.0, 255.0) as u8);
        }
        out.push(255);
    }
    (ImageData { w, h, rgba: Arc::new(out) }, k, luma / n)
}

/// No wallpaper: deep near-black with two faint accent glows.
fn gradient(pw: u32, ph: u32, (a, b): (Rgba, Rgba)) -> Backdrop {
    let (w, h) = ((pw / 4).max(1), (ph / 4).max(1));
    let lin = |c: Rgba| [to_lin((c.0 * 255.0) as u8), to_lin((c.1 * 255.0) as u8), to_lin((c.2 * 255.0) as u8)];
    let (la, lb, base) = (lin(a), lin(b), lin(Rgba::hex(0x0b0b0e, 1.0)));
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let blob = |cx: f32, cy: f32, r: f32| {
                let d = ((u - cx) * (u - cx) * (w as f32 / h as f32).powi(2) + (v - cy) * (v - cy)).sqrt() / r;
                (-d * d).exp()
            };
            let (ka, kb) = (blob(0.22, 0.18, 0.55) * 0.10, blob(0.82, 0.88, 0.6) * 0.08);
            px.push([0, 1, 2].map(|i| base[i] + la[i] * ka + lb[i] * kb));
        }
    }
    // Brighten before grading darkens it again.
    let px: Vec<[f32; 3]> = px.into_iter().map(|p| p.map(|c| c * 1.15)).collect();
    let (blur, _, luma) = grade(px, w, h);
    Backdrop { sharp: blur.clone(), blur, dim: 1.0, luma }
}

/// ~/.face, ~/.face.icon or AccountsService's icon, square, `px` wide.
pub fn avatar(home: &str, login: &str, px: u32) -> Option<ImageData> {
    let candidates = [
        Path::new(home).join(".face"),
        Path::new(home).join(".face.icon"),
        Path::new("/var/lib/AccountsService/icons").join(login),
    ];
    let img = candidates.iter().find_map(|p| image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.decode().ok())?.into_rgba8();
    let (w, h) = img.dimensions();
    let s = w.min(h);
    if s == 0 {
        return None;
    }
    let sq = image::imageops::crop_imm(&img, (w - s) / 2, (h - s) / 2, s, s).to_image();
    let out = image::imageops::resize(&sq, px, px, image::imageops::FilterType::Triangle);
    Some(ImageData { w: px, h: px, rgba: Arc::new(out.into_raw()) })
}

/// A checkmark stroke, `px` square, in `color` (straight alpha).
pub fn checkmark(px: u32, color: Rgba) -> Option<ImageData> {
    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'>\
        <path d='M5.2 12.6 L9.9 17.2 L18.8 7.4' fill='none' stroke='#fff' stroke-width='2.7' \
        stroke-linecap='round' stroke-linejoin='round'/></svg>";
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(SVG, &opt).ok()?;
    let mut pm = resvg::tiny_skia::Pixmap::new(px, px)?;
    let k = px as f32 / 24.0;
    resvg::render(&tree, resvg::usvg::Transform::from_scale(k, k), &mut pm.as_mut());
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let (r, g, b) = (q(color.0), q(color.1), q(color.2));
    let rgba = pm.data().chunks_exact(4).flat_map(|p| [r, g, b, p[3]]).collect();
    Some(ImageData { w: px, h: px, rgba: Arc::new(rgba) })
}
