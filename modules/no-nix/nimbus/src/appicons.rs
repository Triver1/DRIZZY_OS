//! Full-colour application icons (launcher, notifications): resolve an icon
//! name (or path) through the usual icon-theme directories and rasterise it
//! to straight-alpha RGBA at a given pixel size. Call from worker threads:
//! decoding is not free.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use crate::gfx::ImageData;

fn data_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut v = vec![];
        if let Some(h) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
        }) {
            v.push(h);
        }
        let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/run/current-system/sw/share:/usr/share".into());
        v.extend(sys.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
        v
    })
}

const THEMES: [&str; 3] = ["hicolor", "Adwaita", "breeze"];
const SIZES: [&str; 9] = ["scalable", "512x512", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "symbolic"];

/// Find the best file for an icon name (or accept an absolute path).
pub fn find(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let name = name.trim_end_matches(".png").trim_end_matches(".svg");
    for base in data_dirs() {
        for theme in THEMES {
            for size in SIZES {
                for cat in ["apps", "devices", "status", "places", "categories", "mimetypes"] {
                    for ext in ["svg", "png"] {
                        let f = base.join("icons").join(theme).join(size).join(cat).join(format!("{name}.{ext}"));
                        if f.exists() {
                            return Some(f);
                        }
                    }
                }
            }
        }
        for ext in ["svg", "png"] {
            let f = base.join("pixmaps").join(format!("{name}.{ext}"));
            if f.exists() {
                return Some(f);
            }
        }
    }
    None
}

/// Rasterise `path` to `px`×`px` RGBA (aspect kept, centred).
pub fn load(path: &Path, px: u32) -> Option<ImageData> {
    let px = px.clamp(8, 512);
    let mut out = vec![0u8; (px * px * 4) as usize];
    if path.extension().is_some_and(|e| e == "svg") {
        let data = std::fs::read(path).ok()?;
        let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
        let size = tree.size();
        let k = px as f32 / size.width().max(size.height());
        let (tx, ty) = ((px as f32 - size.width() * k) * 0.5, (px as f32 - size.height() * k) * 0.5);
        let mut pm = resvg::tiny_skia::Pixmap::new(px, px)?;
        resvg::render(&tree, resvg::usvg::Transform::from_scale(k, k).post_translate(tx, ty), &mut pm.as_mut());
        // tiny-skia is premultiplied; images are straight alpha.
        for (o, p) in out.chunks_exact_mut(4).zip(pm.pixels()) {
            let c = p.demultiply();
            o.copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
    } else {
        let img = image::open(path).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        let k = px as f32 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
        let small = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
        let (ox, oy) = ((px - nw) / 2, (px - nh) / 2);
        for y in 0..nh {
            for x in 0..nw {
                let s = small.get_pixel(x, y).0;
                let i = (((oy + y) * px + ox + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&s);
            }
        }
    }
    Some(ImageData { w: px, h: px, rgba: Arc::new(out) })
}

/// Resolve and rasterise in one go.
pub fn icon(name: &str, px: u32) -> Option<ImageData> {
    load(&find(name)?, px)
}

/// The icon name a window's app id stands for: the id itself when an icon
/// theme has it, else the `Icon=` of its desktop entry.
fn resolve_app(app_id: &str) -> Option<PathBuf> {
    let lower = app_id.to_lowercase();
    let last = lower.rsplit('.').next().unwrap_or(&lower).to_string();
    for n in [app_id, lower.as_str()] {
        if let Some(p) = find(n) {
            return Some(p);
        }
    }
    for id in [app_id, lower.as_str()] {
        for base in data_dirs() {
            let f = base.join("applications").join(format!("{id}.desktop"));
            let Ok(s) = std::fs::read_to_string(&f) else { continue };
            let icon = s
                .lines()
                .skip_while(|l| l.trim() != "[Desktop Entry]")
                .take_while(|l| !l.starts_with('[') || l.trim() == "[Desktop Entry]")
                .find_map(|l| l.strip_prefix("Icon="));
            if let Some(p) = icon.and_then(|i| find(i.trim())) {
                return Some(p);
            }
        }
    }
    find(&last)
}

/// Resolve `app_id`'s colour icon off-thread and send it as
/// `Fact::AppIcon` (cached: repeat requests answer at once).
pub fn request_app(app_id: &str, tx: &calloop::channel::Sender<crate::services::Fact>) {
    use std::{collections::HashMap, sync::Mutex};
    static CACHE: Mutex<Option<HashMap<String, Option<ImageData>>>> = Mutex::new(None);
    const PX: u32 = 64;
    if let Ok(mut c) = CACHE.lock()
        && let Some(hit) = c.get_or_insert_with(HashMap::new).get(app_id)
    {
        let _ = tx.send(crate::services::Fact::AppIcon(app_id.to_string(), hit.clone()));
        return;
    }
    let (id, tx) = (app_id.to_string(), tx.clone());
    let _ = std::thread::Builder::new().name("nimbus-appicon".into()).stack_size(512 * 1024).spawn(move || {
        let img = resolve_app(&id).and_then(|p| load(&p, PX));
        if let Ok(mut c) = CACHE.lock() {
            let m = c.get_or_insert_with(HashMap::new);
            if m.len() > 64 {
                m.clear();
            }
            m.insert(id.clone(), img.clone());
        }
        let _ = tx.send(crate::services::Fact::AppIcon(id, img));
    });
}

/// Synchronous variant for the snapshot rig.
pub fn app_icon_now(app_id: &str, px: u32) -> Option<ImageData> {
    resolve_app(app_id).and_then(|p| load(&p, px))
}
