//! Previews, on worker threads. File contents are untrusted bytes:
//!
//! - only regular files are ever opened (checked before and, with
//!   `O_NONBLOCK` so a FIFO swapped in cannot hang, after the open);
//! - reads are capped, text is sanitised for display, nothing is executed;
//! - images are decoded by a throw-away child process (this binary with
//!   `--preview-decode`, the open file as stdin, so the path is not even in
//!   its argv). nimbus builds with `panic = "abort"`: a decoder bug on a
//!   corrupt file must not take the bar down with it. The child also runs
//!   under memory and CPU limits, gets killed when the selection moves on
//!   or it takes too long, and full-size pixels never touch the bar's heap.

use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::{FileExt, MetadataExt, OpenOptionsExt}},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use super::kind::{self, Kind};
use crate::{gfx::ImageData, services::Fact, wallpaper};

/// Bytes read for sniffing and for a text preview.
pub const TEXT_CAP: usize = 64 * 1024;
/// Longest line kept (shaping happens on the main thread, so a preview
/// never holds more than fits).
pub const MAX_COLS: usize = 110;
/// Image file size the decoder is given at all.
const IMAGE_CAP: u64 = 256 * 1024 * 1024;
/// Largest source side, and what decoding may allocate.
const IMAGE_MAX_SIDE: u32 = 24_000;
const IMAGE_MAX_ALLOC: u64 = 900 * 1024 * 1024;
/// The child's address space, CPU seconds and wall-clock budget.
const CHILD_AS: u64 = 1800 * 1024 * 1024;
const CHILD_CPU: u64 = 12;
const CHILD_WALL: Duration = Duration::from_secs(10);
/// Entries counted, and sampled, for a folder preview.
const FOLDER_COUNT: usize = 20_000;
const FOLDER_SAMPLE: usize = 2_000;
pub const FOLDER_SHOW: usize = 9;

pub struct Req {
    pub owner: u64,
    pub generation: u64,
    /// The newest generation: anything older stops as soon as it can.
    pub latest: Arc<AtomicU64>,
    pub path: PathBuf,
    /// The preview box, physical px.
    pub max: (u32, u32),
    pub tx: calloop::channel::Sender<Fact>,
}

/// Coloured text, one string per class. Spaces keep the classes aligned
/// (Geist Mono is monospaced), so each draws as its own text run and all
/// of them fade like any other text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayers {
    pub plain: String,
    pub keyword: String,
    pub string: String,
    pub comment: String,
    /// The whole text in one run, for when the mono face is missing.
    pub all: String,
}

pub enum Content {
    Image { img: ImageData, src: (u32, u32) },
    Text { layers: TextLayers, code: bool },
    Folder { items: usize, more: bool, folders: usize, sample: Vec<(String, Kind)> },
    /// Binary or unreadable: metadata only.
    Nothing { note: Option<&'static str> },
}

/// A finished preview, as `Fact::Custom`.
pub struct Shown {
    pub owner: u64,
    pub generation: u64,
    pub content: Mutex<Option<Content>>,
    /// Where a symlink points, display-safe.
    pub link: Option<String>,
}

/// Open `path` for reading if, and only if, it is a regular file.
pub fn open_regular(path: &Path) -> std::io::Result<(File, std::fs::Metadata)> {
    let not_file = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file");
    if !std::fs::metadata(path)?.is_file() {
        return Err(not_file());
    }
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY).open(path)?;
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(not_file());
    }
    Ok((f, m))
}

/// `(format name)` for the images the decoder takes, by their magic bytes.
pub fn image_magic(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("JPEG")
    } else if h.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("PNG")
    } else if h.len() >= 12 && &h[..4] == b"RIFF" && &h[8..12] == b"WEBP" {
        Some("WebP")
    } else {
        None
    }
}

/// Is this text? Decides on the bytes, not the name.
pub fn looks_text(h: &[u8]) -> bool {
    if h.contains(&0) {
        return false;
    }
    let s = String::from_utf8_lossy(h);
    let bad = s.chars().filter(|&c| c == '\u{fffd}' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\x0c'))).count();
    // A multi-byte character cut by the read cap may stay invalid.
    bad * 50 <= s.chars().count().max(50)
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "crate", "def", "default", "defer", "del", "do", "elif", "else", "enum", "export", "extends",
    "extern", "false", "final", "finally", "fn", "for", "from", "func", "function", "go", "if", "impl", "import", "in", "inherit", "interface", "is", "lambda", "let", "loop",
    "match", "mod", "module", "move", "mut", "new", "nil", "None", "not", "null", "or", "and", "package", "pass", "private", "pub", "public", "raise", "ref", "return", "rec", "self",
    "Self", "static", "struct", "super", "switch", "then", "this", "throw", "trait", "true", "True", "False", "try", "type", "typeof", "undefined", "unsafe", "use", "using", "var",
    "void", "where", "while", "with", "yield",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Plain,
    Keyword,
    Str,
    Comment,
}

/// Line comment tokens and block comment pair by extension.
fn comments(ext: &str) -> (&'static [&'static str], Option<(&'static str, &'static str)>) {
    match ext {
        "py" | "sh" | "bash" | "zsh" | "fish" | "toml" | "yaml" | "yml" | "rb" | "pl" | "r" | "jl" | "ini" | "conf" | "cfg" | "cmake" | "mk" | "ex" | "exs" | "nix" => {
            (&["#"], if ext == "nix" { Some(("/*", "*/")) } else { None })
        }
        "lua" | "sql" | "hs" => (&["--"], None),
        "html" | "htm" | "xml" | "vue" | "svelte" => (&[], Some(("<!--", "-->"))),
        "css" | "scss" | "sass" | "less" => (&[], Some(("/*", "*/"))),
        "tex" | "erl" => (&["%"], None),
        "json" => (&[], None),
        _ => (&["//"], Some(("/*", "*/"))),
    }
}

/// Sanitise one line for display: tabs to spaces, invisible and control
/// characters to dots, capped to `MAX_COLS`.
fn clean_line(l: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(l.len().min(MAX_COLS));
    for c in l.chars() {
        if out.len() >= MAX_COLS {
            break;
        }
        match c {
            '\t' => out.extend(std::iter::repeat_n(' ', 4 - out.len() % 4)),
            '\r' => {}
            c if kind::unsafe_char(c) => out.push('\u{b7}'),
            c => out.push(c),
        }
    }
    out
}

fn classify(line: &[char], ext: &str, in_block: &mut bool) -> Vec<Class> {
    let (lines, block) = comments(ext);
    let n = line.len();
    let mut cls = vec![Class::Plain; n];
    let at = |i: usize, t: &str| t.chars().enumerate().all(|(k, c)| line.get(i + k) == Some(&c));
    let mut i = 0;
    while i < n {
        if *in_block {
            cls[i] = Class::Comment;
            if let Some((_, end)) = block
                && at(i, end)
            {
                let e = (i + end.chars().count()).min(n);
                cls[i..e].fill(Class::Comment);
                i = e;
                *in_block = false;
                continue;
            }
            i += 1;
            continue;
        }
        if lines.iter().any(|t| at(i, t)) {
            cls[i..].fill(Class::Comment);
            break;
        }
        if let Some((start, _)) = block
            && at(i, start)
        {
            *in_block = true;
            let e = (i + start.chars().count()).min(n);
            cls[i..e].fill(Class::Comment);
            i = e;
            continue;
        }
        let c = line[i];
        if c == '"' || c == '\'' || c == '`' {
            // `'` also starts lifetimes and apostrophes: only a string if
            // it closes on this line.
            if let Some(end) = (i + 1..n).scan(false, |esc, j| {
                let hit = !*esc && line[j] == c;
                *esc = !*esc && line[j] == '\\';
                Some((j, hit))
            }).find_map(|(j, hit)| hit.then_some(j)) {
                cls[i..=end].fill(Class::Str);
                i = end + 1;
                continue;
            }
            i += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let s = i;
            while i < n && (line[i].is_alphanumeric() || line[i] == '_') {
                i += 1;
            }
            let word: String = line[s..i].iter().collect();
            if KEYWORDS.binary_search(&word.as_str()).is_ok() || KEYWORDS.contains(&word.as_str()) {
                cls[s..i].fill(Class::Keyword);
            }
            continue;
        }
        i += 1;
    }
    cls
}

/// Lay `text` out for the preview, at most `rows` lines.
pub fn text_layers(text: &str, ext: Option<&str>, rows: usize) -> TextLayers {
    let mut t = TextLayers::default();
    let mut in_block = false;
    for raw in text.split('\n').take(rows) {
        let line = clean_line(raw);
        let cls = ext.map(|e| classify(&line, e, &mut in_block));
        for (i, &c) in line.iter().enumerate() {
            t.all.push(c);
            let k = cls.as_ref().map_or(Class::Plain, |v| v[i]);
            for (layer, class) in [(&mut t.plain, Class::Plain), (&mut t.keyword, Class::Keyword), (&mut t.string, Class::Str), (&mut t.comment, Class::Comment)] {
                layer.push(if k == class { c } else { ' ' });
            }
        }
        for s in [&mut t.plain, &mut t.keyword, &mut t.string, &mut t.comment, &mut t.all] {
            s.push('\n');
        }
    }
    t
}

fn ext_of(p: &Path) -> Option<String> {
    let k = kind::of_path(p);
    (k == Kind::Code).then(|| p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default())
}

fn folder(p: &Path) -> Content {
    let Ok(rd) = std::fs::read_dir(p) else { return Content::Nothing { note: Some("Can't read this folder") } };
    let (mut items, mut folders, mut more) = (0, 0, false);
    let mut names: Vec<(bool, String, Kind)> = vec![];
    for e in rd.flatten() {
        if items >= FOLDER_COUNT {
            more = true;
            break;
        }
        items += 1;
        let name = e.file_name();
        let dir = e.file_type().is_ok_and(|t| t.is_dir());
        folders += dir as usize;
        if names.len() < FOLDER_SAMPLE && name.as_bytes().first() != Some(&b'.') {
            names.push((dir, kind::shown(&name), if dir { Kind::Folder } else { kind::of_name(&name) }));
        }
    }
    names.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| kind::natural(&a.1, &b.1)));
    Content::Folder { items, more, folders, sample: names.into_iter().take(FOLDER_SHOW).map(|(_, n, k)| (n, k)).collect() }
}

/// Build the preview for one request (blocking: runs on a worker).
pub fn load(r: &Req, rows: usize) -> Content {
    let live = || r.latest.load(Ordering::Relaxed) == r.generation;
    let Ok(m) = std::fs::metadata(&r.path) else { return Content::Nothing { note: None } };
    if m.is_dir() {
        return folder(&r.path);
    }
    let Ok((f, m)) = open_regular(&r.path) else { return Content::Nothing { note: None } };
    if m.len() == 0 {
        return Content::Nothing { note: Some("Empty file") };
    }
    let mut head = vec![0u8; TEXT_CAP];
    let n = match f.read_at(&mut head, 0) {
        Ok(n) => n,
        Err(_) => return Content::Nothing { note: Some("Can't read this file") },
    };
    head.truncate(n);
    if image_magic(&head).is_some() {
        if m.len() > IMAGE_CAP {
            return Content::Nothing { note: Some("Too large to preview") };
        }
        return match decode_child(f, r.max, m.len(), &live) {
            Some((img, src)) => Content::Image { img, src },
            None => Content::Nothing { note: Some("Can't preview this image") },
        };
    }
    if !looks_text(&head) {
        return Content::Nothing { note: None };
    }
    let text = String::from_utf8_lossy(&head);
    let ext = ext_of(&r.path);
    Content::Text { layers: text_layers(&text, ext.as_deref(), rows), code: ext.is_some() }
}

pub fn run_preview(r: Req, rows: usize) {
    if r.latest.load(Ordering::Relaxed) != r.generation {
        return;
    }
    let link = std::fs::read_link(&r.path).ok().map(|t| kind::shown(t.as_os_str()));
    let content = load(&r, rows);
    if r.latest.load(Ordering::Relaxed) != r.generation {
        return;
    }
    let msg = Shown { owner: r.owner, generation: r.generation, content: Mutex::new(Some(content)), link };
    let _ = r.tx.send(Fact::Custom(Arc::new(msg)));
}

// ── the decoder child ───────────────────────────────────────────────────

const DECODE_ARG: &str = "--preview-decode";
const MAGIC: &[u8; 4] = b"NPV1";

/// Largest `w`×`h` with the source's aspect inside `max`, never upscaled.
pub fn contain(src: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    let k = (max.0 as f64 / src.0 as f64).min(max.1 as f64 / src.1 as f64).min(1.0);
    (((src.0 as f64 * k).round() as u32).max(1), ((src.1 as f64 * k).round() as u32).max(1))
}

/// Parent side: run the child on `file`, stopping when `live` turns false.
fn decode_child(file: File, max: (u32, u32), size: u64, live: &dyn Fn() -> bool) -> Option<(ImageData, (u32, u32))> {
    let mut child = Command::new("/proc/self/exe")
        .arg(DECODE_ARG)
        .arg(max.0.to_string())
        .arg(max.1.to_string())
        .arg(size.to_string())
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let fd = out.as_raw_fd();
    let t0 = Instant::now();
    let budget = 20 + max.0 as usize * max.1 as usize * 4;
    let mut data: Vec<u8> = Vec::with_capacity(budget.min(16 << 20));
    let mut buf = vec![0u8; 256 * 1024];
    let done = loop {
        if !live() || t0.elapsed() > CHILD_WALL || data.len() > budget {
            break false;
        }
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd for the duration of the call.
        let r = unsafe { libc::poll(&mut p, 1, 25) };
        if r < 0 {
            break false;
        }
        if r == 0 {
            continue;
        }
        match out.read(&mut buf) {
            Ok(0) => break true,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => break false,
        }
    };
    if !done {
        let _ = child.kill();
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    if !done || !ok {
        return None;
    }
    parse_output(&data, max)
}

/// Validate what the child wrote: it handled untrusted bytes, so nothing
/// it says is taken on faith.
pub fn parse_output(d: &[u8], max: (u32, u32)) -> Option<(ImageData, (u32, u32))> {
    let u = |i: usize| -> Option<u32> { Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?)) };
    if d.get(..4)? != MAGIC {
        return None;
    }
    let (w, h, sw, sh) = (u(4)?, u(8)?, u(12)?, u(16)?);
    if w == 0 || h == 0 || w > max.0 || h > max.1 || d.len() != 20 + w as usize * h as usize * 4 {
        return None;
    }
    Some((ImageData { w, h, rgba: Arc::new(d[20..].to_vec()) }, (sw, sh)))
}

/// Decode from any seekable reader into `max`, within the limits.
pub fn decode_stream<R: std::io::BufRead + std::io::Seek>(r: R, max: (u32, u32)) -> Option<(ImageData, (u32, u32))> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::new(r).with_guessed_format().ok()?;
    if !matches!(reader.format()?, image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP) {
        return None;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(IMAGE_MAX_SIDE);
    limits.max_image_height = Some(IMAGE_MAX_SIDE);
    limits.max_alloc = Some(IMAGE_MAX_ALLOC);
    reader.limits(limits);
    let mut dec = reader.into_decoder().ok()?;
    // The decoders honour `max_alloc` unevenly, and a tiny compressed file
    // can unpack to gigabytes: refuse by the size it says it will be.
    if dec.total_bytes() > IMAGE_MAX_ALLOC {
        return None;
    }
    let orient = dec.orientation().ok();
    let mut img = image::DynamicImage::from_decoder(dec).ok()?;
    if let Some(o) = orient {
        img.apply_orientation(o);
    }
    let src = (img.width(), img.height());
    if src.0 == 0 || src.1 == 0 {
        return None;
    }
    let (raw, ch) = match img {
        image::DynamicImage::ImageRgb8(b) => (b.into_raw(), 3),
        image::DynamicImage::ImageRgba8(b) => (b.into_raw(), 4),
        other if !other.color().has_alpha() => (other.into_rgb8().into_raw(), 3),
        other => (other.into_rgba8().into_raw(), 4),
    };
    let (w, h) = contain(src, max);
    Some((ImageData { w, h, rgba: Arc::new(wallpaper::fit(raw, src.0, src.1, ch, w, h)) }, src))
}

/// `nimbus --preview-decode <w> <h> <size>`: the child. Returns its exit
/// code, or None when these are not its arguments.
pub fn child_main() -> Option<i32> {
    let mut a = std::env::args_os().skip(1);
    if a.next()?.as_os_str() != OsStr::new(DECODE_ARG) {
        return None;
    }
    let mut num = || a.next()?.to_str()?.parse::<u64>().ok();
    let (Some(w), Some(h), Some(size)) = (num(), num(), num()) else { return Some(2) };
    let rlim = |res, v: u64| {
        let l = libc::rlimit { rlim_cur: v, rlim_max: v };
        // SAFETY: a plain struct passed by reference.
        unsafe { libc::setrlimit(res, &l) };
    };
    rlim(libc::RLIMIT_AS, CHILD_AS);
    rlim(libc::RLIMIT_CPU, CHILD_CPU);
    rlim(libc::RLIMIT_CORE, 0);
    rlim(libc::RLIMIT_FSIZE, 0);
    // SAFETY: fd 0 is ours; the File takes it over for the process's life.
    let file = unsafe { File::from_raw_fd(0) };
    if size > IMAGE_CAP || file.metadata().map_or(true, |m| !m.is_file() || m.len() > IMAGE_CAP || m.mode() == 0) {
        return Some(3);
    }
    let reader = std::io::BufReader::with_capacity(1 << 20, file);
    let Some((img, src)) = decode_stream(reader, (w as u32, h as u32)) else { return Some(4) };
    let mut o = std::io::stdout().lock();
    let head = [img.w, img.h, src.0, src.1];
    let ok = o.write_all(MAGIC).is_ok() && head.iter().all(|v| o.write_all(&v.to_le_bytes()).is_ok()) && o.write_all(&img.rgba).is_ok() && o.flush().is_ok();
    Some(if ok { 0 } else { 5 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testdir::TestDir;
    use std::os::unix::fs::symlink;

    fn req(path: &Path) -> Req {
        let (tx, _rx) = calloop::channel::channel();
        Req { owner: 1, generation: 1, latest: Arc::new(AtomicU64::new(1)), path: path.to_path_buf(), max: (64, 64), tx }
    }

    #[test]
    fn only_regular_files_open() {
        let t = TestDir::new("regular");
        t.write("plain.txt", b"hi");
        let cpath = std::ffi::CString::new(t.path().join("pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        symlink(t.path().join("pipe"), t.path().join("to-pipe")).unwrap();
        symlink(t.path().join("plain.txt"), t.path().join("to-plain")).unwrap();
        assert!(open_regular(&t.path().join("plain.txt")).is_ok());
        assert!(open_regular(&t.path().join("to-plain")).is_ok());
        // A FIFO with no writer would block a plain open forever.
        assert!(open_regular(&t.path().join("pipe")).is_err());
        assert!(open_regular(&t.path().join("to-pipe")).is_err());
        assert!(open_regular(Path::new("/dev/zero")).is_err());
        assert!(open_regular(Path::new("/dev/null")).is_err());
        assert!(open_regular(t.path()).is_err());
        // Previews of them come back empty, and quickly.
        let t0 = Instant::now();
        for p in [t.path().join("pipe"), t.path().join("to-pipe"), PathBuf::from("/dev/zero"), t.path().join("missing")] {
            assert!(matches!(load(&req(&p), 10), Content::Nothing { .. }));
        }
        assert!(t0.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn text_is_capped_and_sanitised() {
        let t = TestDir::new("text");
        // Far beyond the read cap.
        let big = "line of text\n".repeat(100_000);
        t.write("big.txt", big.as_bytes());
        let Content::Text { layers, .. } = load(&req(&t.path().join("big.txt")), 20) else { panic!() };
        assert_eq!(layers.all.lines().count(), 20);
        // A long line is cut.
        t.write("wide.txt", "x".repeat(100_000).as_bytes());
        let Content::Text { layers, .. } = load(&req(&t.path().join("wide.txt")), 20) else { panic!() };
        assert!(layers.all.lines().next().unwrap().chars().count() <= MAX_COLS);
        // Control and bidi characters never reach the screen.
        t.write("evil.txt", "ok\u{202e}evil\u{1b}[31m\ttab\r\n".as_bytes());
        let Content::Text { layers, .. } = load(&req(&t.path().join("evil.txt")), 20) else { panic!() };
        assert!(!layers.all.contains('\u{202e}') && !layers.all.contains('\u{1b}') && !layers.all.contains('\r'));
        assert!(layers.all.contains("ok\u{b7}evil\u{b7}[31m"));
    }

    #[test]
    fn binary_detection() {
        let t = TestDir::new("binary");
        t.write("nul.dat", b"abc\0def");
        t.write("noise.txt", &(0..4096u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8 | 0x80).collect::<Vec<u8>>());
        t.write("utf8.txt", "héllo wörld 日本語\n".repeat(50).as_bytes());
        t.write("empty.txt", b"");
        assert!(matches!(load(&req(&t.path().join("nul.dat")), 10), Content::Nothing { note: None }));
        assert!(matches!(load(&req(&t.path().join("noise.txt")), 10), Content::Nothing { note: None }));
        assert!(matches!(load(&req(&t.path().join("utf8.txt")), 10), Content::Text { .. }));
        assert!(matches!(load(&req(&t.path().join("empty.txt")), 10), Content::Nothing { note: Some(_) }));
    }

    #[test]
    fn folder_preview_counts_and_samples() {
        let t = TestDir::new("folderprev");
        for i in 0..30 {
            t.file(&format!("d/f{i:02}.txt"));
        }
        t.file("d/sub/x");
        t.file("d/.hidden");
        let Content::Folder { items, folders, sample, more } = folder(&t.path().join("d")) else { panic!() };
        assert_eq!((items, folders, more), (32, 1, false));
        assert_eq!(sample.len(), FOLDER_SHOW);
        assert_eq!(sample[0].0, "sub");
        assert!(!sample.iter().any(|s| s.0.starts_with('.')));
    }

    #[test]
    fn syntax_classes() {
        let t = text_layers("// note\nlet s = \"fn\"; /* a\nb */ let y = 1;\nit's fine\n", Some("rs"), 10);
        assert!(t.comment.contains("// note"));
        assert!(t.string.contains("\"fn\""));
        assert!(t.keyword.contains("let"));
        assert!(!t.keyword.contains("fn"));
        assert!(t.comment.contains("/* a"));
        assert!(t.comment.contains("b */"));
        // An apostrophe that never closes is not a string.
        assert!(t.plain.contains("it's fine"));
        // Layers line up: every one has the same shape as the text.
        for l in [&t.plain, &t.keyword, &t.string, &t.comment] {
            assert_eq!(l.chars().filter(|&c| c == '\n').count(), t.all.chars().filter(|&c| c == '\n').count());
            for (a, b) in l.lines().zip(t.all.lines()) {
                assert_eq!(a.chars().count(), b.chars().count());
            }
        }
        // Plain text gets no colours.
        let t = text_layers("if true { // x\n", None, 10);
        assert_eq!(t.plain, t.all);
        assert!(t.comment.trim().is_empty());
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, if x < w / 2 { 255 } else { 0 }]));
        let mut out = std::io::Cursor::new(vec![]);
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn images_fit_and_keep_alpha() {
        let (img, src) = decode_stream(std::io::Cursor::new(png(400, 100)), (200, 200)).unwrap();
        assert_eq!(src, (400, 100));
        assert_eq!((img.w, img.h), (200, 50));
        assert_eq!(img.rgba.len(), 200 * 50 * 4);
        // Left half opaque, right half clear.
        assert_eq!(img.rgba[(25 * 200) * 4 + 3], 255);
        assert_eq!(img.rgba[(25 * 200 + 190) * 4 + 3], 0);
        // Small images are never blown up.
        let (img, _) = decode_stream(std::io::Cursor::new(png(16, 16)), (200, 200)).unwrap();
        assert_eq!((img.w, img.h), (16, 16));
    }

    #[test]
    fn corrupt_and_gigantic_images_fail_cleanly() {
        let good = png(64, 64);
        // Truncations at every scale, and bit rot.
        for cut in [0, 4, 8, 16, 33, 40, 60, good.len() / 2, good.len() - 1] {
            let _ = decode_stream(std::io::Cursor::new(good[..cut].to_vec()), (64, 64));
        }
        for i in (0..good.len()).step_by(7) {
            let mut b = good.clone();
            b[i] ^= 0x5a;
            let _ = decode_stream(std::io::Cursor::new(b), (64, 64));
        }
        // A PNG header claiming 60000×60000: refused before any allocation.
        let mut huge = good[..33].to_vec();
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        let t0 = Instant::now();
        assert!(decode_stream(std::io::Cursor::new(huge), (64, 64)).is_none());
        assert!(t0.elapsed() < Duration::from_secs(1));
        // Within the side limit but not the allocation limit: a complete,
        // valid file, a few KB, that would unpack to a gigabyte.
        let bomb = {
            use std::io::Write;
            let chunk = |t: &[u8], d: &[u8]| -> Vec<u8> {
                let mut c = (d.len() as u32).to_be_bytes().to_vec();
                c.extend_from_slice(t);
                c.extend_from_slice(d);
                // The decoder is told to ignore checksums nowhere: compute it.
                let mut crc = 0xffff_ffffu32;
                for b in t.iter().chain(d) {
                    crc ^= *b as u32;
                    for _ in 0..8 {
                        crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
                    }
                }
                c.extend_from_slice(&(!crc).to_be_bytes());
                c
            };
            let mut ihdr = vec![];
            ihdr.extend_from_slice(&16_000u32.to_be_bytes());
            ihdr.extend_from_slice(&16_000u32.to_be_bytes());
            ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
            let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
            out.extend(chunk(b"IHDR", &ihdr));
            out.extend(chunk(b"IDAT", &[0x78, 0x01]));
            out.extend(chunk(b"IEND", &[]));
            let _ = out.flush();
            out
        };
        let t0 = Instant::now();
        assert!(decode_stream(std::io::Cursor::new(bomb), (64, 64)).is_none());
        assert!(t0.elapsed() < Duration::from_millis(200));
        // Not an image, and formats the decoder is not given.
        assert!(decode_stream(std::io::Cursor::new(b"GIF89a....".to_vec()), (64, 64)).is_none());
        assert!(decode_stream(std::io::Cursor::new(vec![]), (64, 64)).is_none());
    }

    #[test]
    fn child_output_is_validated() {
        let px = |w: u32, h: u32, sw: u32, sh: u32, n: usize| {
            let mut v = MAGIC.to_vec();
            for x in [w, h, sw, sh] {
                v.extend_from_slice(&x.to_le_bytes());
            }
            v.resize(20 + n, 7);
            v
        };
        assert!(parse_output(&px(2, 2, 9, 9, 16), (4, 4)).is_some());
        // Lies about size, in every direction.
        assert!(parse_output(&px(2, 2, 9, 9, 15), (4, 4)).is_none());
        assert!(parse_output(&px(5, 2, 9, 9, 40), (4, 4)).is_none());
        assert!(parse_output(&px(0, 2, 9, 9, 0), (4, 4)).is_none());
        assert!(parse_output(&px(u32::MAX, u32::MAX, 9, 9, 16), (4, 4)).is_none());
        assert!(parse_output(b"NOPE", (4, 4)).is_none());
        assert!(parse_output(&[], (4, 4)).is_none());
    }

    #[test]
    fn magic_bytes_decide_not_names() {
        assert_eq!(image_magic(b"\xff\xd8\xff\xe0abc"), Some("JPEG"));
        assert_eq!(image_magic(b"\x89PNG\r\n\x1a\nxxxx"), Some("PNG"));
        assert_eq!(image_magic(b"RIFF\0\0\0\0WEBPVP8 "), Some("WebP"));
        assert_eq!(image_magic(b"RIFF\0\0\0\0WAVEfmt "), None);
        assert_eq!(image_magic(b"just text"), None);
    }

    #[test]
    fn contain_fits() {
        assert_eq!(contain((1000, 500), (100, 100)), (100, 50));
        assert_eq!(contain((500, 1000), (100, 100)), (50, 100));
        assert_eq!(contain((10, 10), (100, 100)), (10, 10));
        assert_eq!(contain((100_000, 1), (100, 100)), (100, 1));
    }
}
