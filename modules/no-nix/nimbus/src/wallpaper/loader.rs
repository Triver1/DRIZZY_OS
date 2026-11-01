//! Wallpaper files: discovery, cover-fit decoding at exactly the size that
//! is drawn, the on-disk thumbnail cache and a small on-demand worker pool.
//!
//! Everything here runs on worker threads except `scan` (a couple of
//! `read_dir`s). Full-resolution pixels only ever exist inside a worker,
//! for the moment it takes to resample them.

use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{gfx::ImageData, services::Fact};

/// One wallpaper candidate.
pub struct Entry {
    pub path: PathBuf,
    /// File stem, for the label.
    pub name: String,
    /// Sub-folder of the wallpaper directory, if any.
    pub folder: Option<String>,
    pub kind: &'static str,
    pub bytes: u64,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn wallpaper_dir() -> PathBuf {
    home().join("Pictures").join("Wallpapers")
}

fn kind_of(p: &Path) -> Option<&'static str> {
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => "JPEG",
        "png" => "PNG",
        "webp" => "WebP",
        _ => return None,
    })
}

fn entry(path: PathBuf, folder: Option<String>) -> Option<Entry> {
    let kind = kind_of(&path)?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().into_owned();
    Some(Entry { path, name, folder, kind, bytes: meta.len() })
}

/// `~/Pictures/Wallpapers` and its direct sub-folders, plus `saved` if it
/// lives elsewhere. Top-level images first, then each folder; by name.
pub fn scan(saved: Option<&Path>) -> Vec<Entry> {
    let dir = wallpaper_dir();
    let mut top = vec![];
    let mut nested = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for f in rd.flatten() {
            let p = f.path();
            if p.is_dir() {
                let folder = f.file_name().to_string_lossy().into_owned();
                if folder.starts_with('.') {
                    continue;
                }
                if let Ok(sub) = std::fs::read_dir(&p) {
                    nested.extend(sub.flatten().filter_map(|g| entry(g.path(), Some(folder.clone()))));
                }
            } else if let Some(e) = entry(p, None) {
                top.push(e);
            }
        }
    }
    let key = |e: &Entry| (e.folder.clone().unwrap_or_default().to_lowercase(), e.name.to_lowercase());
    top.sort_by_key(key);
    nested.sort_by_key(key);
    top.append(&mut nested);
    if let Some(s) = saved
        && !top.iter().any(|e| e.path == s)
        && let Some(e) = entry(s.to_path_buf(), s.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()))
    {
        top.insert(0, e);
    }
    top
}

/// "~/Pictures/Wallpapers" style display path.
pub fn pretty_dir() -> String {
    let d = wallpaper_dir();
    match d.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => d.display().to_string(),
    }
}

// ── resampling ──────────────────────────────────────────────────────────

/// Filter taps for one axis: output pixel `i` reads `idx[off[i]..off[i+1]]`.
struct Taps {
    off: Vec<u32>,
    idx: Vec<u32>,
    w: Vec<f32>,
}

/// Map `n` output pixels onto the source span `[start, start + n * step)`.
/// Downscaling integrates exact box coverage (area averaging); upscaling
/// is bilinear.
fn taps(n: u32, src_len: u32, start: f32, step: f32) -> Taps {
    let mut t = Taps { off: Vec::with_capacity(n as usize + 1), idx: vec![], w: vec![] };
    let last = src_len.saturating_sub(1) as i64;
    for o in 0..n {
        t.off.push(t.idx.len() as u32);
        let a = start + o as f32 * step;
        if step >= 1.0 {
            let b = a + step;
            let mut i = a.floor() as i64;
            while (i as f32) < b {
                let w = (b.min(i as f32 + 1.0) - a.max(i as f32)) / step;
                if w > 1e-4 {
                    t.idx.push(i.clamp(0, last) as u32);
                    t.w.push(w);
                }
                i += 1;
            }
        } else {
            let c = a + step * 0.5 - 0.5;
            let i0 = c.floor();
            let f = c - i0;
            t.idx.push((i0 as i64).clamp(0, last) as u32);
            t.w.push(1.0 - f);
            t.idx.push((i0 as i64 + 1).clamp(0, last) as u32);
            t.w.push(f);
        }
    }
    t.off.push(t.idx.len() as u32);
    t
}

/// Cover-fit `src` (`ch` = 3 or 4 bytes per pixel) into `w`×`h` RGBA,
/// centred. One row of f32 scratch; no full-size intermediates.
fn cover(src: &[u8], sw: u32, sh: u32, ch: usize, w: u32, h: u32) -> Vec<u8> {
    resample(src, sw, sh, ch, w, h, false)
}

/// Fit a decoded picture into exactly `w`×`h` (`w`/`h` keep its aspect),
/// keeping transparency as straight alpha (file previews).
pub fn fit(mut raw: Vec<u8>, sw: u32, sh: u32, ch: usize, w: u32, h: u32) -> Vec<u8> {
    if ch != 4 {
        return resample(&raw, sw, sh, ch, w, h, false);
    }
    // Filter premultiplied, or clear pixels bleed their colour in.
    for p in raw.chunks_exact_mut(4) {
        let a = p[3] as u32;
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let mut o = resample(&raw, sw, sh, ch, w, h, true);
    for p in o.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    o
}

fn resample(src: &[u8], sw: u32, sh: u32, ch: usize, w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let k = (w as f32 / sw as f32).max(h as f32 / sh as f32);
    let step = 1.0 / k;
    let (x0, y0) = ((sw as f32 - w as f32 * step) * 0.5, (sh as f32 - h as f32 * step) * 0.5);
    let tx = taps(w, sw, x0, step);
    let ty = taps(h, sh, y0, step);
    let (lo, hi) = (*tx.idx.iter().min().unwrap_or(&0) as usize, *tx.idx.iter().max().unwrap_or(&0) as usize + 1);
    let mut row = vec![0f32; (hi - lo) * ch];
    let mut out = vec![0u8; (w * h * 4) as usize];
    let stride = sw as usize * ch;
    for oy in 0..h as usize {
        row.iter_mut().for_each(|v| *v = 0.0);
        for t in ty.off[oy] as usize..ty.off[oy + 1] as usize {
            let (sy, wy) = (ty.idx[t] as usize, ty.w[t]);
            let s = &src[sy * stride + lo * ch..sy * stride + hi * ch];
            for (a, &b) in row.iter_mut().zip(s) {
                *a += b as f32 * wy;
            }
        }
        let o = &mut out[oy * w as usize * 4..(oy + 1) * w as usize * 4];
        for ox in 0..w as usize {
            let mut acc = [0f32, 0.0, 0.0, 255.0];
            if ch == 4 {
                acc[3] = 0.0;
            }
            for t in tx.off[ox] as usize..tx.off[ox + 1] as usize {
                let (sx, wx) = (tx.idx[t] as usize - lo, tx.w[t]);
                let p = &row[sx * ch..sx * ch + ch];
                for c in 0..ch {
                    acc[c] += p[c] * wx;
                }
            }
            if alpha {
                for c in 0..4 {
                    o[ox * 4 + c] = (acc[c] + 0.5).clamp(0.0, 255.0) as u8;
                }
                continue;
            }
            // Wallpapers are opaque: transparent images sit on black.
            let k = acc[3].clamp(0.0, 255.0) / 255.0;
            for c in 0..3 {
                o[ox * 4 + c] = (acc[c] * k + 0.5).clamp(0.0, 255.0) as u8;
            }
            o[ox * 4 + 3] = 255;
        }
    }
    out
}

/// A decoded source picture (full resolution, 3 or 4 bytes per pixel).
struct Src {
    raw: Vec<u8>,
    w: u32,
    h: u32,
    ch: usize,
}

impl Src {
    fn open(path: &Path) -> Option<Self> {
        // Phones store portraits on their side and say so in EXIF.
        let mut dec = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
        let orient = image::ImageDecoder::orientation(&mut dec).unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img = image::DynamicImage::from_decoder(dec).ok()?;
        img.apply_orientation(orient);
        let (w, h) = (img.width(), img.height());
        if w == 0 || h == 0 {
            return None;
        }
        let (raw, ch) = match img {
            image::DynamicImage::ImageRgb8(b) => (b.into_raw(), 3),
            image::DynamicImage::ImageRgba8(b) => (b.into_raw(), 4),
            // 16-bit / grey / … : 8-bit RGB is all a wallpaper needs.
            other if !other.color().has_alpha() => (other.into_rgb8().into_raw(), 3),
            other => (other.into_rgba8().into_raw(), 4),
        };
        Some(Self { raw, w, h, ch })
    }

    fn cover(&self, w: u32, h: u32) -> Option<ImageData> {
        (w > 0 && h > 0).then(|| ImageData { w, h, rgba: Arc::new(cover(&self.raw, self.w, self.h, self.ch, w, h)) })
    }
}

/// Decode `path` and cover-fit it to `w`×`h`. Returns the pixels and the
/// source dimensions.
pub fn decode_cover(path: &Path, w: u32, h: u32) -> Option<(ImageData, (u32, u32))> {
    let src = Src::open(path)?;
    Some((src.cover(w, h)?, (src.w, src.h)))
}

/// Mean colour (0..1), sampled sparsely.
pub fn average(img: &ImageData) -> [f32; 3] {
    let mut acc = [0f64; 3];
    let mut n = 0f64;
    for p in img.rgba.chunks_exact(4).step_by(23) {
        for c in 0..3 {
            acc[c] += p[c] as f64;
        }
        n += 1.0;
    }
    let n = n.max(1.0) * 255.0;
    [(acc[0] / n) as f32, (acc[1] / n) as f32, (acc[2] / n) as f32]
}

// ── thumbnail cache ─────────────────────────────────────────────────────

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".cache"))
        .join("nimbus")
        .join("thumbs")
}

/// FNV-1a: stable across builds (unlike `DefaultHasher`).
fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Identity of a source picture: its path, size and mtime (an edit makes
/// a new key, so stale thumbnails are never read back).
fn source_key(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    let mut k = fnv(path.as_os_str().as_encoded_bytes(), 0xcbf2_9ce4_8422_2325);
    for v in [meta.len(), mtime.as_secs(), mtime.subsec_nanos() as u64, 3] {
        k = fnv(&v.to_le_bytes(), k);
    }
    Some(k)
}

/// `<source key>-<w>x<h>.rgba`: every size of one picture shares the
/// prefix, so pruning never needs to know which sizes other outputs use.
fn cache_file(path: &Path, w: u32, h: u32) -> Option<PathBuf> {
    Some(cache_dir().join(format!("{:016x}-{w}x{h}.rgba", source_key(path)?)))
}

const MAGIC: &[u8; 4] = b"NWT1";
const DAY: std::time::Duration = std::time::Duration::from_secs(86_400);

fn cache_read(f: &Path, w: u32, h: u32) -> Option<(ImageData, (u32, u32))> {
    let mut file = std::fs::File::open(f).ok()?;
    let mut head = [0u8; 20];
    file.read_exact(&mut head).ok()?;
    let u = |i: usize| u32::from_le_bytes(head[i..i + 4].try_into().unwrap());
    if &head[..4] != MAGIC || u(4) != w || u(8) != h {
        return None;
    }
    let mut px = vec![0u8; (w * h * 4) as usize];
    file.read_exact(&mut px).ok()?;
    // Mark it as used (pruning drops sizes nobody asked for in a while).
    let now = std::time::SystemTime::now();
    if file.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).unwrap_or_default() > DAY) {
        let _ = file.set_modified(now);
    }
    Some((ImageData { w, h, rgba: Arc::new(px) }, (u(12), u(16))))
}

fn cache_write(f: &Path, img: &ImageData, src: (u32, u32)) {
    let Some(dir) = f.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = f.with_extension(format!("tmp{}", std::process::id()));
    let ok = (|| {
        let mut o = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        o.write_all(MAGIC)?;
        for v in [img.w, img.h, src.0, src.1] {
            o.write_all(&v.to_le_bytes())?;
        }
        o.write_all(&img.rgba)?;
        o.flush()
    })();
    if ok.is_ok() {
        let _ = std::fs::rename(&tmp, f);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// A thumbnail at exactly `w`×`h`: from the disk cache, or decoded (and
/// then cached). The flag says it came from the cache.
pub fn thumbnail(path: &Path, w: u32, h: u32) -> Option<(ImageData, (u32, u32), bool)> {
    let f = cache_file(path, w, h);
    if let Some(f) = &f
        && let Some((img, src)) = cache_read(f, w, h)
    {
        return Some((img, src, true));
    }
    let (img, src) = decode_cover(path, w, h)?;
    if let Some(f) = &f {
        cache_write(f, &img, src);
    }
    Some((img, src, false))
}

/// The cached thumbnail only (no decode): a ~1 MB read, fine to do inline.
pub fn cached_thumbnail(path: &Path, w: u32, h: u32) -> Option<(ImageData, (u32, u32))> {
    cache_read(&cache_file(path, w, h)?, w, h)
}

/// Delete cached thumbnails of pictures that are gone or changed, sizes
/// unused for a month (an output that is no longer around) and temp files
/// left by a crash. Runs on its own short-lived thread.
pub fn prune(paths: Vec<PathBuf>) {
    let _ = std::thread::Builder::new().name("nimbus-thumbs-gc".into()).spawn(move || {
        let keep: std::collections::HashSet<String> = paths.iter().filter_map(|p| source_key(p)).map(|k| format!("{k:016x}")).collect();
        let Ok(rd) = std::fs::read_dir(cache_dir()) else { return };
        let now = std::time::SystemTime::now();
        let age = |f: &std::fs::DirEntry| f.metadata().and_then(|m| m.modified()).map_or(std::time::Duration::MAX, |t| now.duration_since(t).unwrap_or_default());
        for f in rd.flatten() {
            let p = f.path();
            let (Some(stem), Some(ext)) = (p.file_stem().and_then(|s| s.to_str()), p.extension().and_then(|e| e.to_str())) else { continue };
            let stale = if ext == "rgba" {
                stem.split('-').next().is_none_or(|k| !keep.contains(k)) || age(&f) > DAY * 30
            } else {
                ext.starts_with("tmp") && age(&f) > std::time::Duration::from_secs(600)
            };
            if stale {
                let _ = std::fs::remove_file(p);
            }
        }
    });
}

// ── worker pool ─────────────────────────────────────────────────────────

pub struct Job {
    pub idx: usize,
    pub hi: bool,
    pub path: PathBuf,
    pub w: u32,
    pub h: u32,
}

/// A decoded thumbnail on its way to the window that asked for it.
pub struct Thumb {
    pub owner: u64,
    pub idx: usize,
    pub hi: bool,
    /// Came from the disk cache (fast: no need to dress up its arrival).
    pub cached: bool,
    /// Taken by the window.
    pub img: Mutex<Option<ImageData>>,
    pub src: (u32, u32),
    pub avg: [f32; 3],
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    alive: usize,
}

/// Threads exist only while there is work: each one drains the queue and
/// exits. Dropping (or `cancel`) discards whatever has not started.
pub struct Pool {
    q: Arc<Mutex<Queue>>,
    tx: calloop::channel::Sender<Fact>,
    owner: u64,
    max: usize,
}

impl Pool {
    pub fn new(tx: calloop::channel::Sender<Fact>, owner: u64) -> Self {
        let max = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1)).clamp(1, 3);
        Self { q: Arc::default(), tx, owner, max }
    }

    pub fn push(&self, job: Job, urgent: bool) {
        let mut q = self.q.lock().unwrap();
        if urgent {
            q.jobs.push_front(job);
        } else {
            q.jobs.push_back(job);
        }
        if q.alive < self.max {
            q.alive += 1;
            let (q2, tx, owner) = (self.q.clone(), self.tx.clone(), self.owner);
            let spawned = std::thread::Builder::new().name("nimbus-thumbs".into()).spawn(move || worker(q2, tx, owner));
            if spawned.is_err() {
                q.alive -= 1;
            }
        }
    }

    /// Forget queued jobs (the window closed). Returns their indices.
    pub fn cancel(&self) -> Vec<(usize, bool)> {
        self.q.lock().unwrap().jobs.drain(..).map(|j| (j.idx, j.hi)).collect()
    }

    /// Forget the queued low-tier jobs (the carousel moved on). Returns
    /// their indices; the sharp copy stays queued.
    pub fn cancel_low(&self) -> Vec<usize> {
        let mut q = self.q.lock().unwrap();
        let mut out = vec![];
        q.jobs.retain(|j| {
            if !j.hi {
                out.push(j.idx);
            }
            j.hi
        });
        out
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn worker(q: Arc<Mutex<Queue>>, tx: calloop::channel::Sender<Fact>, owner: u64) {
    loop {
        let job = {
            let mut g = q.lock().unwrap();
            match g.jobs.pop_front() {
                Some(j) => j,
                None => {
                    g.alive -= 1;
                    return;
                }
            }
        };
        let (img, src, avg, cached) = match thumbnail(&job.path, job.w, job.h) {
            Some((img, src, cached)) => {
                let avg = average(&img);
                (Some(img), src, avg, cached)
            }
            None => (None, (0, 0), [0.0; 3], false),
        };
        let msg = Thumb { owner, idx: job.idx, hi: job.hi, cached, img: Mutex::new(img), src, avg };
        if tx.send(Fact::Custom(Arc::new(msg))).is_err() {
            let mut g = q.lock().unwrap();
            g.jobs.clear();
            g.alive -= 1;
            return;
        }
    }
}

// ── full-size wallpaper decode (backdrops) ──────────────────────────────

/// The wallpaper decoded for one output.
pub struct Decoded {
    pub owner: u64,
    pub generation: u64,
    pub path: PathBuf,
    pub img: Mutex<Option<ImageData>>,
}

/// One backdrop's newest wish. Superseded as soon as its owner asks again
/// (`latest` moves on).
struct Req {
    tx: calloop::channel::Sender<Fact>,
    owner: u64,
    generation: u64,
    latest: Arc<AtomicU64>,
    path: PathBuf,
    w: u32,
    h: u32,
    frost: bool,
}

impl Req {
    fn live(&self) -> bool {
        self.latest.load(Ordering::Relaxed) == self.generation
    }
}

/// Single-slot-per-owner mailbox served by at most one decode thread.
#[derive(Default)]
struct Mailbox {
    reqs: Vec<Req>,
    running: bool,
}

fn mailbox() -> &'static Mutex<Mailbox> {
    static M: std::sync::OnceLock<Mutex<Mailbox>> = std::sync::OnceLock::new();
    M.get_or_init(Mutex::default)
}

/// Ask for `path` cover-fitted to `w`×`h` for backdrop `owner`; replaces
/// that owner's previous request. One worker thread serves every output
/// (one full-size picture in memory at a time) and exits when idle.
/// `frost`: blur and dim the result.
#[allow(clippy::too_many_arguments)]
pub fn request_decode(tx: calloop::channel::Sender<Fact>, owner: u64, generation: u64, latest: Arc<AtomicU64>, path: PathBuf, w: u32, h: u32, frost: bool) {
    let mut m = mailbox().lock().unwrap();
    m.reqs.retain(|r| r.owner != owner && r.live());
    m.reqs.push(Req { tx, owner, generation, latest, path, w, h, frost });
    if !m.running {
        m.running = std::thread::Builder::new().name("nimbus-wallpaper".into()).spawn(decode_worker).is_ok();
    }
}

fn decode_worker() {
    // The last decoded source, kept only while another output still wants it.
    let mut src: Option<(PathBuf, Option<Src>)> = None;
    loop {
        let group: Vec<Req> = {
            let mut m = mailbox().lock().unwrap();
            m.reqs.retain(Req::live);
            let held = src.as_ref().and_then(|(p, _)| m.reqs.iter().position(|r| &r.path == p));
            let Some(k) = held.or((!m.reqs.is_empty()).then_some(0)) else {
                m.running = false;
                return;
            };
            let (path, w, h, frost) = (m.reqs[k].path.clone(), m.reqs[k].w, m.reqs[k].h, m.reqs[k].frost);
            let (g, rest): (Vec<Req>, Vec<Req>) = std::mem::take(&mut m.reqs).into_iter().partition(|r| r.path == path && r.w == w && r.h == h && r.frost == frost);
            m.reqs = rest;
            g
        };
        let (path, w, h) = (group[0].path.clone(), group[0].w, group[0].h);
        if src.as_ref().is_none_or(|(p, _)| *p != path) {
            drop(src.take());
            src = Some((path.clone(), Src::open(&path)));
        }
        // Superseded while decoding: skip the resample.
        if !group.iter().any(Req::live) {
            continue;
        }
        let img = src.as_ref().and_then(|(_, s)| s.as_ref()?.cover(w, h));
        let img = if group[0].frost { img.map(frosted) } else { img };
        for r in group.iter().filter(|r| r.live()) {
            let msg = Decoded { owner: r.owner, generation: r.generation, path: path.clone(), img: Mutex::new(img.clone()) };
            let _ = r.tx.send(Fact::Custom(Arc::new(msg)));
        }
        drop(img);
        if !mailbox().lock().unwrap().reqs.iter().any(|r| r.path == path) {
            src = None;
        }
    }
}

/// Frosted glass: a wide blur, dimmed so workspaces lift off it.
fn frosted(img: ImageData) -> ImageData {
    const SIGMA: f32 = 22.0;
    const DIM: f32 = 0.74;
    let Some(buf) = image::RgbaImage::from_raw(img.w, img.h, img.rgba.to_vec()) else { return img };
    let mut px = image::imageops::fast_blur(&buf, SIGMA).into_raw();
    for p in px.chunks_exact_mut(4) {
        for c in &mut p[..3] {
            *c = (*c as f32 * DIM) as u8;
        }
    }
    ImageData { w: img.w, h: img.h, rgba: Arc::new(px) }
}

/// Process-unique ids (window instances, backdrops, texture key ranges).
pub fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frosted_blurs_and_dims() {
        // A hard black/white edge.
        let (w, h) = (120u32, 40u32);
        let mut px = vec![255u8; (w * h * 4) as usize];
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            if (i as u32 % w) < w / 2 {
                p[..3].fill(0);
            }
        }
        let out = frosted(ImageData { w, h, rgba: Arc::new(px) });
        assert_eq!((out.w, out.h), (w, h));
        let at = |x: u32| out.rgba[((h / 2 * w + x) * 4) as usize];
        // The edge is gone: neighbours across it are close.
        assert!(at(w / 2).abs_diff(at(w / 2 - 1)) < 8);
        // Dimmed: even the white side stays well below full.
        assert!(at(w - 1) < 200);
        assert!(out.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }
}
