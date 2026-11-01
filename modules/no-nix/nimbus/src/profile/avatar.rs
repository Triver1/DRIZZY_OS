//! The picture: finding candidates, reading them without trusting them,
//! cutting the crop and putting it where the lock looks.
//!
//! ~/.face is the lock's first choice, and what the login screen's published
//! copy is made from. It is replaced atomically, and only ever if it is a
//! plain file: a link there is someone's deliberate setup (a home-manager
//! file, a link into a dotfiles repo), so nimbus says so and leaves it alone.

use std::{
    cmp::Reverse,
    fmt,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::SystemTime,
};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage, imageops, metadata::Orientation};

use super::{Preview, crop::Crop};
use crate::{gfx::ImageData, wallpaper::loader};

/// The saved picture's width and height.
pub const FACE_PX: u32 = 512;
/// Largest file looked at, longest side and pixel count decoded.
const MAX_FILE: u64 = 64 << 20;
const MAX_DIM: u32 = 16_384;
const MAX_PIXELS: u64 = 120_000_000;
const MAX_ALLOC: u64 = 512 << 20;
/// Longest side of the copy shown while cropping.
const PREVIEW_MAX: u32 = 1400;
/// How deep the picker looks under ~/Pictures, how many directory entries
/// it reads at most, and how many pictures it offers.
const DEPTH: usize = 3;
const MAX_SEEN: usize = 20_000;
pub const MAX_PICS: usize = 60;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Missing,
    NotFile,
    TooBig,
    Unreadable,
    /// No usable home directory.
    NoHome,
    /// This file is something other than a plain file.
    NotPlain(&'static str),
    /// Writing failed (the reason stays out of the UI).
    Io,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Missing => f.write_str("There's no file there."),
            Error::NotFile => f.write_str("That isn't a file."),
            Error::TooBig => f.write_str("That picture is too big."),
            Error::Unreadable => f.write_str("That doesn't open as a picture."),
            Error::NoHome => f.write_str("Can't find your home folder."),
            Error::NotPlain(n) => write!(f, "Left ~/{n} alone: it isn't a plain file."),
            Error::Io => f.write_str("Couldn't save the picture."),
        }
    }
}

fn is_picture(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
}

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MAX_DIM);
    l.max_image_height = Some(MAX_DIM);
    l.max_alloc = Some(MAX_ALLOC);
    l
}

fn sane_size(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_PIXELS
}

fn reader(path: &Path) -> Result<ImageReader<std::io::BufReader<std::fs::File>>, Error> {
    let mut r = ImageReader::open(path).map_err(|_| Error::Unreadable)?.with_guessed_format().map_err(|_| Error::Unreadable)?;
    if !matches!(r.format(), Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)) {
        return Err(Error::Unreadable);
    }
    r.limits(limits());
    Ok(r)
}

/// Is it a file of a size worth opening (never blocks on a FIFO)?
fn check_file(path: &Path) -> Result<(), Error> {
    let m = std::fs::metadata(path).map_err(|_| Error::Missing)?;
    if !m.is_file() {
        return Err(Error::NotFile);
    }
    if m.len() > MAX_FILE {
        return Err(Error::TooBig);
    }
    Ok(())
}

/// Decode an untrusted file, upright, within the limits.
pub fn decode(path: &Path) -> Result<RgbaImage, Error> {
    check_file(path)?;
    let mut dec = reader(path)?.into_decoder().map_err(|_| Error::TooBig)?;
    let (w, h) = dec.dimensions();
    if !sane_size(w, h) {
        return Err(Error::TooBig);
    }
    let orient = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(|_| Error::Unreadable)?;
    img.apply_orientation(orient);
    Ok(img.into_rgba8())
}

/// `path`, downscaled for the crop view.
pub fn preview(path: &Path) -> Result<Preview, Error> {
    let img = decode(path)?;
    let (w, h) = img.dimensions();
    let k = (PREVIEW_MAX as f32 / w.max(h) as f32).min(1.0);
    let (pw, ph) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
    let raw = if (pw, ph) == (w, h) { img.into_raw() } else { loader::fit(img.into_raw(), w, h, 4, pw, ph) };
    Ok(Preview { img: ImageData { w: pw, h: ph, rgba: Arc::new(raw) }, size: (w, h) })
}

/// The crop, resampled to `px` square.
pub fn render(img: &RgbaImage, crop: &Crop, px: u32) -> RgbaImage {
    let (x, y, s) = crop.square_in(img.width(), img.height());
    let sq = imageops::crop_imm(img, x, y, s, s).to_image();
    let raw = loader::fit(sq.into_raw(), s, s, 4, px, px);
    RgbaImage::from_raw(px, px, raw).unwrap_or_else(|| RgbaImage::new(px, px))
}

fn encode(img: &RgbaImage) -> Result<Vec<u8>, Error> {
    let mut out = vec![];
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).map_err(|_| Error::Io)?;
    Ok(out)
}

/// `~/name` may be replaced or removed: absent, or a plain file.
fn replaceable(home: &Path, name: &'static str) -> Result<(), Error> {
    match std::fs::symlink_metadata(home.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::Io),
        Ok(m) if m.is_file() => Ok(()),
        Ok(_) => Err(Error::NotPlain(name)),
    }
}

fn usable_home(home: &Path) -> Result<(), Error> {
    if home.is_absolute() && home.is_dir() { Ok(()) } else { Err(Error::NoHome) }
}

/// Make `png` the picture: a temp file beside ~/.face (O_EXCL, so it can't
/// be a link), synced, then renamed over it. On any failure ~/.face stays
/// as it was and the temp file goes.
pub fn write_face(home: &Path, png: &[u8]) -> Result<(), Error> {
    static N: AtomicU32 = AtomicU32::new(0);
    usable_home(home)?;
    replaceable(home, ".face")?;
    let tmp = home.join(format!(".face.nimbus.{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let res = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).custom_flags(libc::O_NOFOLLOW).open(&tmp).map_err(|_| Error::Io)?;
        f.write_all(png).map_err(|_| Error::Io)?;
        // Whatever the umask says: other tools read this file too.
        f.set_permissions(std::fs::Permissions::from_mode(0o644)).map_err(|_| Error::Io)?;
        f.sync_all().map_err(|_| Error::Io)?;
        drop(f);
        // Once more, right before it counts.
        replaceable(home, ".face")?;
        std::fs::rename(&tmp, home.join(".face")).map_err(|_| Error::Io)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return res;
    }
    if let Ok(d) = std::fs::File::open(home) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Decode, crop, resample, encode, write.
pub fn save(home: &Path, src: &Path, crop: &Crop) -> Result<(), Error> {
    // Fail before the slow part if it can't work anyway.
    usable_home(home)?;
    replaceable(home, ".face")?;
    let img = decode(src)?;
    write_face(home, &encode(&render(&img, crop, FACE_PX))?)
}

/// Is there a picture of nimbus's kind to remove?
pub fn has_face(home: &Path) -> bool {
    [".face", ".face.icon"].iter().any(|n| std::fs::symlink_metadata(home.join(n)).is_ok())
}

/// Remove the picture the lock would read from home. A link is left alone,
/// and reported if it still leads to something (a link to ~/.face stops
/// mattering once ~/.face is gone).
pub fn remove_face(home: &Path) -> Result<(), Error> {
    usable_home(home)?;
    for n in [".face", ".face.icon"] {
        if replaceable(home, n).is_ok() {
            match std::fs::remove_file(home.join(n)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(Error::Io),
            }
        }
    }
    for n in [".face", ".face.icon"] {
        if std::fs::metadata(home.join(n)).is_ok() {
            return Err(Error::NotPlain(n));
        }
    }
    Ok(())
}

struct Found {
    /// Lives under a Screenshots folder: offered after everything else.
    shot: bool,
    time: SystemTime,
    path: PathBuf,
}

fn walk(dir: &Path, depth: usize, shot: bool, budget: &mut usize, out: &mut Vec<Found>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        // `file_type` doesn't follow links: a linked folder is not walked,
        // so no loops and no wandering out of ~/Pictures.
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            if depth > 0 {
                walk(&e.path(), depth - 1, shot || name.eq_ignore_ascii_case("screenshots"), budget, out);
            }
            continue;
        }
        let path = e.path();
        if !is_picture(&path) {
            continue;
        }
        let Ok(m) = std::fs::metadata(&path) else { continue };
        if m.is_file() && m.len() <= MAX_FILE {
            out.push(Found { shot, time: m.modified().unwrap_or(SystemTime::UNIX_EPOCH), path });
        }
    }
}

/// Its header says it can be decoded within the limits.
fn plausible(path: &Path) -> bool {
    reader(path).ok().and_then(|r| r.into_dimensions().ok()).is_some_and(|(w, h)| sane_size(w, h))
}

/// Pictures under `root`, newest first, depth-limited. Run off the UI thread.
pub fn scan(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut budget = MAX_SEEN;
    walk(root, DEPTH, false, &mut budget, &mut found);
    found.sort_by_key(|f| (f.shot, Reverse(f.time)));
    found.into_iter().map(|f| f.path).filter(|p| plausible(p)).take(MAX_PICS).collect()
}

fn from_hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (from_hex(b[i + 1]), from_hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What someone typed or pasted as a path: quotes, `file://`, `~`.
pub fn parse_path(s: &str, home: &Path) -> Option<PathBuf> {
    let s = s.lines().map(str::trim).find(|l| !l.is_empty())?;
    let s = s.trim_matches(|c| c == '"' || c == '\'').trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return rest.starts_with('/').then(|| PathBuf::from(percent_decode(rest)));
    }
    if s == "~" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return Some(home.join(rest));
    }
    let p = Path::new(s);
    Some(if p.is_absolute() { p.to_path_buf() } else { home.join(p) })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{MetadataExt, symlink};

    use super::*;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("nimbus-avatar-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// A made-up picture: red on the left half, blue on the right.
    fn split(path: &Path, w: u32, h: u32) {
        let img = image::RgbImage::from_fn(w, h, |x, _| if x < w / 2 { image::Rgb([230, 30, 30]) } else { image::Rgb([30, 30, 230]) });
        img.save_with_format(path, ImageFormat::Png).unwrap();
    }

    /// By its contents: ~/.face has no extension.
    fn load(p: &Path) -> DynamicImage {
        image::load_from_memory(&std::fs::read(p).unwrap()).unwrap()
    }

    fn entries(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn saves_a_square_png_atomically_and_readably() {
        let d = dir("save");
        let src = d.0.join("src.png");
        split(&src, 900, 400);
        let home = d.0.join("home");
        std::fs::create_dir(&home).unwrap();
        save(&home, &src, &Crop::new(900, 400)).unwrap();
        let bytes = std::fs::read(home.join(".face")).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!((out.width(), out.height()), (FACE_PX, FACE_PX));
        assert_eq!(std::fs::metadata(home.join(".face")).unwrap().mode() & 0o777, 0o644);
        assert_eq!(entries(&home), [".face"], "no temp file left");
        // The lock's own reader takes it.
        assert!(crate::lock::avatar(&home.to_string_lossy(), "nobody-here", 64).is_some());
    }

    #[test]
    fn the_crop_decides_what_is_in_the_picture() {
        let d = dir("crop");
        let src = d.0.join("src.png");
        split(&src, 800, 200);
        let mut c = Crop::new(800, 200);
        // All the way left: only the red half is inside.
        c.pan(10_000.0, 0.0, 300.0);
        save(&d.0, &src, &c).unwrap();
        let out = load(&d.0.join(".face")).into_rgb8();
        assert!(out.pixels().all(|p| p[0] > 200 && p[2] < 60));
        c.pan(-10_000.0, 0.0, 300.0);
        save(&d.0, &src, &c).unwrap();
        let out = load(&d.0.join(".face")).into_rgb8();
        assert!(out.pixels().all(|p| p[2] > 200 && p[0] < 60));
    }

    #[test]
    fn transparency_survives() {
        let d = dir("alpha");
        let src = d.0.join("src.png");
        let img = RgbaImage::from_fn(64, 64, |x, _| image::Rgba([200, 200, 200, if x < 32 { 0 } else { 255 }]));
        img.save_with_format(&src, ImageFormat::Png).unwrap();
        save(&d.0, &src, &Crop::new(64, 64)).unwrap();
        let out = load(&d.0.join(".face")).into_rgba8();
        assert_eq!(out.get_pixel(0, 256)[3], 0);
        assert_eq!(out.get_pixel(511, 256)[3], 255);
    }

    #[test]
    fn replaces_an_existing_plain_file() {
        let d = dir("plain");
        std::fs::write(d.0.join(".face"), b"old").unwrap();
        write_face(&d.0, b"new").unwrap();
        assert_eq!(std::fs::read(d.0.join(".face")).unwrap(), b"new");
    }

    #[test]
    fn refuses_a_symlink_and_leaves_both_ends_alone() {
        let d = dir("link");
        let victim = d.0.join("victim");
        std::fs::write(&victim, b"untouched").unwrap();
        symlink(&victim, d.0.join(".face")).unwrap();
        assert_eq!(write_face(&d.0, b"new"), Err(Error::NotPlain(".face")));
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        assert!(std::fs::symlink_metadata(d.0.join(".face")).unwrap().is_symlink());
        assert_eq!(entries(&d.0), [".face", "victim"], "no temp file left");
        // The whole save refuses before doing any work, too.
        let src = d.0.join("src.png");
        split(&src, 32, 32);
        assert_eq!(save(&d.0, &src, &Crop::new(32, 32)), Err(Error::NotPlain(".face")));
        // So does a link that leads nowhere.
        std::fs::remove_file(d.0.join(".face")).unwrap();
        symlink(d.0.join("gone"), d.0.join(".face")).unwrap();
        assert_eq!(write_face(&d.0, b"new"), Err(Error::NotPlain(".face")));
        assert!(!d.0.join("gone").exists());
    }

    #[test]
    fn refuses_anything_else_that_is_not_a_plain_file() {
        let d = dir("notfile");
        std::fs::create_dir(d.0.join(".face")).unwrap();
        assert_eq!(write_face(&d.0, b"x"), Err(Error::NotPlain(".face")));
        assert!(d.0.join(".face").is_dir());
    }

    #[test]
    fn a_home_that_is_not_there_is_an_error_not_a_stray_file() {
        let d = dir("nohome");
        assert_eq!(write_face(Path::new(""), b"x"), Err(Error::NoHome));
        assert_eq!(write_face(Path::new("relative"), b"x"), Err(Error::NoHome));
        assert_eq!(write_face(&d.0.join("missing"), b"x"), Err(Error::NoHome));
    }

    #[test]
    fn corrupt_and_hostile_files_are_errors() {
        let d = dir("corrupt");
        let check = |name: &str, bytes: &[u8]| {
            let p = d.0.join(name);
            std::fs::write(&p, bytes).unwrap();
            assert!(decode(&p).is_err(), "{name}");
            assert!(preview(&p).is_err(), "{name}");
        };
        check("text.png", b"this is not a picture");
        check("empty.jpg", b"");
        check("head.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
        check("head.jpg", &[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F']);
        check("head.webp", b"RIFF\x24\0\0\0WEBPVP8 ");
        // A real PNG cut in half.
        let good = d.0.join("good.png");
        split(&good, 300, 300);
        let bytes = std::fs::read(&good).unwrap();
        check("cut.png", &bytes[..bytes.len() / 2]);
        // An SVG, and other things the shell doesn't decode.
        check("x.svg.png", b"<svg xmlns='http://www.w3.org/2000/svg'/>");
        // Random noise behind each signature.
        let mut seed = 0x2f6e_2b1au32;
        let mut noise: Vec<u8> = (0..4096)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        noise[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        check("noise.png", &noise);
        noise[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
        check("noise.jpg", &noise);
    }

    /// Decoders get the same bytes a hostile or damaged file could hold:
    /// valid pictures with bytes flipped and tails cut, thousands of times.
    /// They may fail, they must not panic (the release build aborts).
    #[test]
    fn damaged_pictures_never_panic() {
        let d = dir("fuzz");
        let img = RgbaImage::from_fn(48, 48, |x, y| image::Rgba([(x * 5) as u8, (y * 5) as u8, ((x ^ y) * 9) as u8, 255]));
        let rgb = DynamicImage::ImageRgba8(img.clone()).into_rgb8();
        let mut seeds: Vec<(&str, Vec<u8>)> = vec![];
        for (name, fmt) in [("f.png", ImageFormat::Png), ("f.webp", ImageFormat::WebP)] {
            let mut b = vec![];
            img.write_to(&mut std::io::Cursor::new(&mut b), fmt).unwrap();
            seeds.push((name, b));
        }
        let mut jpeg = vec![];
        rgb.write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg).unwrap();
        seeds.push(("f.jpg", jpeg));
        let mut seed = 0x9e37_79b9u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as usize
        };
        for (name, good) in seeds {
            let p = d.0.join(name);
            for _ in 0..1500 {
                let mut b = good.clone();
                for _ in 0..1 + rnd() % 8 {
                    let i = rnd() % b.len();
                    b[i] = rnd() as u8;
                }
                if rnd() % 5 == 0 {
                    b.truncate(rnd() % b.len());
                }
                std::fs::write(&p, &b).unwrap();
                let _ = decode(&p);
            }
        }
    }

    #[test]
    fn a_picture_claiming_to_be_enormous_is_refused_without_decoding() {
        let d = dir("bomb");
        // A valid PNG header claiming 60000×60000 (the CRC is real).
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&60_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut crc = !0u32;
        for b in &ihdr {
            crc ^= *b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
            }
        }
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\r".to_vec();
        png.extend_from_slice(&ihdr);
        png.extend_from_slice(&(!crc).to_be_bytes());
        let p = d.0.join("bomb.png");
        std::fs::write(&p, &png).unwrap();
        assert!(decode(&p).is_err());
        assert!(!plausible(&p));
    }

    #[test]
    fn size_and_kind_are_checked_before_opening() {
        let d = dir("size");
        let big = d.0.join("big.png");
        std::fs::File::create(&big).unwrap().set_len(MAX_FILE + 1).unwrap();
        assert_eq!(decode(&big).err(), Some(Error::TooBig));
        assert_eq!(decode(&d.0.join("nothing.png")).err(), Some(Error::Missing));
        assert_eq!(decode(&d.0).err(), Some(Error::NotFile));
        // A FIFO would hang an open(): it must not get that far.
        let fifo = d.0.join("fifo.png");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(decode(&fifo).err(), Some(Error::NotFile));
    }

    #[test]
    fn exif_rotation_is_applied() {
        let d = dir("exif");
        // A 4×2 JPEG with EXIF orientation 6 (rotate 90°): upright it is 2×4.
        let mut jpeg = vec![];
        image::RgbImage::from_pixel(4, 2, image::Rgb([120, 90, 60])).write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg).unwrap();
        let exif: &[u8] = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xff, 0xe1]);
        out.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(exif);
        out.extend_from_slice(&jpeg[2..]);
        let p = d.0.join("rot.jpg");
        std::fs::write(&p, &out).unwrap();
        let img = decode(&p).unwrap();
        assert_eq!(img.dimensions(), (2, 4));
    }

    #[test]
    fn removing_goes_back_to_nothing() {
        let d = dir("remove");
        assert!(!has_face(&d.0));
        remove_face(&d.0).unwrap();
        std::fs::write(d.0.join(".face"), b"a").unwrap();
        std::fs::write(d.0.join(".face.icon"), b"b").unwrap();
        assert!(has_face(&d.0));
        remove_face(&d.0).unwrap();
        assert!(!has_face(&d.0));
    }

    #[test]
    fn removing_leaves_links_alone_and_says_so() {
        let d = dir("remove-link");
        let target = d.0.join("me.png");
        std::fs::write(&target, b"mine").unwrap();
        symlink(&target, d.0.join(".face")).unwrap();
        assert_eq!(remove_face(&d.0), Err(Error::NotPlain(".face")));
        assert_eq!(std::fs::read(&target).unwrap(), b"mine");
        assert!(std::fs::symlink_metadata(d.0.join(".face")).is_ok());
    }

    #[test]
    fn a_link_to_the_removed_file_is_no_obstacle() {
        let d = dir("remove-chain");
        std::fs::write(d.0.join(".face"), b"a").unwrap();
        symlink(d.0.join(".face"), d.0.join(".face.icon")).unwrap();
        remove_face(&d.0).unwrap();
        assert!(!d.0.join(".face").exists());
    }

    #[test]
    fn the_scan_is_recent_first_bounded_and_stays_inside() {
        let d = dir("scan");
        let root = d.0.join("Pictures");
        std::fs::create_dir_all(root.join("a/b/c/d/e")).unwrap();
        std::fs::create_dir_all(root.join("Screenshots")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        let at = |p: PathBuf, secs: u64| {
            split(&p, 8, 8);
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)).unwrap();
        };
        at(root.join("old.png"), 1_000);
        at(root.join("new.PNG"), 3_000);
        at(root.join("a/mid.png"), 2_000);
        at(root.join("a/b/c/deep.png"), 500);
        at(root.join("a/b/c/d/too-deep.png"), 9_000);
        at(root.join(".hidden/secret.png"), 9_000);
        at(root.join(".dot.png"), 9_000);
        at(root.join("Screenshots/shot.png"), 9_000);
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        std::fs::write(root.join("broken.png"), b"not a picture").unwrap();
        // A link back up must not loop, and a link out must not be walked.
        symlink(&root, root.join("a/loop")).unwrap();
        symlink(d.0.join("outside"), root.join("away")).unwrap();
        // A FIFO with a picture's name must not hang the scan.
        let c = std::ffi::CString::new(root.join("fifo.png").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let names: Vec<String> = scan(&root).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["new.PNG", "mid.png", "old.png", "deep.png", "shot.png"]);
    }

    #[test]
    fn the_scan_caps_what_it_offers() {
        let d = dir("scan-cap");
        for i in 0..MAX_PICS + 5 {
            split(&d.0.join(format!("{i}.png")), 4, 4);
        }
        assert_eq!(scan(&d.0).len(), MAX_PICS);
        assert!(scan(&d.0.join("no-such-folder")).is_empty());
    }

    #[test]
    fn paths_come_in_many_shapes() {
        let home = Path::new("/home/ada");
        let p = |s: &str| parse_path(s, home).map(|p| p.display().to_string());
        assert_eq!(p("/tmp/me.png").as_deref(), Some("/tmp/me.png"));
        assert_eq!(p("  '/tmp/me.png'\n").as_deref(), Some("/tmp/me.png"));
        assert_eq!(p("\"/tmp/my pic.png\"").as_deref(), Some("/tmp/my pic.png"));
        assert_eq!(p("~/Pictures/me.png").as_deref(), Some("/home/ada/Pictures/me.png"));
        assert_eq!(p("Pictures/me.png").as_deref(), Some("/home/ada/Pictures/me.png"));
        assert_eq!(p("file:///tmp/my%20pic.png").as_deref(), Some("/tmp/my pic.png"));
        assert_eq!(p("file://localhost/tmp/a.png").as_deref(), Some("/tmp/a.png"));
        assert_eq!(p("file:///a.png\r\nfile:///b.png").as_deref(), Some("/a.png"));
        assert_eq!(p("file://remote.host/a.png"), None);
        assert_eq!(p("file:///bad%zz%2").as_deref(), Some("/bad%zz%2"));
        assert_eq!(p("   "), None);
        assert_eq!(p(""), None);
    }
}
