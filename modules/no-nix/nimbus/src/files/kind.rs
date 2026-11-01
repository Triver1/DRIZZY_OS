//! What a file is, for people: type buckets, labels, safe display forms
//! of names, sizes and times. No I/O.

use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

use crate::icons::IconSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Folder,
    Image,
    Video,
    Audio,
    Text,
    Code,
    Document,
    Sheet,
    Slides,
    Pdf,
    Archive,
    Font,
    Binary,
    Other,
}

fn ext_of(name: &OsStr) -> Option<String> {
    let b = name.as_bytes();
    let dot = b.iter().rposition(|&c| c == b'.').filter(|&d| d > 0)?;
    let e = &b[dot + 1..];
    (!e.is_empty() && e.len() <= 12 && e.is_ascii()).then(|| String::from_utf8_lossy(e).to_ascii_lowercase())
}

const CODE: &[&str] = &[
    "rs", "c", "h", "cc", "cpp", "hpp", "cxx", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "java", "kt", "swift", "rb", "php", "cs", "lua", "sh", "bash", "zsh", "fish",
    "nix", "zig", "hs", "ml", "ex", "exs", "erl", "clj", "scala", "dart", "r", "jl", "pl", "sql", "html", "htm", "css", "scss", "sass", "less", "vue", "svelte", "qml", "wgsl",
    "glsl", "vert", "frag", "toml", "yaml", "yml", "json", "jsonc", "xml", "kdl", "ini", "conf", "cfg", "cmake", "mk", "gradle", "tex",
];
const TEXT: &[&str] = &["txt", "md", "markdown", "rst", "org", "log", "csv", "tsv", "srt", "vtt", "diff", "patch", "lock", "env"];

pub fn of_name(name: &OsStr) -> Kind {
    let Some(e) = ext_of(name) else {
        return match name.as_bytes() {
            b"Makefile" | b"Dockerfile" | b"Justfile" | b"Cargo.lock" | b"flake.lock" => Kind::Code,
            b"LICENSE" | b"README" | b"NOTICE" | b"CHANGELOG" | b"TODO" => Kind::Text,
            _ => Kind::Other,
        };
    };
    let e = e.as_str();
    match e {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "tif" | "tiff" | "heic" | "heif" | "avif" | "ico" | "raw" | "cr2" | "nef" | "dng" | "psd" | "xcf" | "kra" => Kind::Image,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg" => Kind::Video,
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma" | "aiff" | "mid" | "midi" => Kind::Audio,
        "pdf" | "epub" | "djvu" => Kind::Pdf,
        "doc" | "docx" | "odt" | "rtf" | "pages" => Kind::Document,
        "xls" | "xlsx" | "ods" | "numbers" => Kind::Sheet,
        "ppt" | "pptx" | "odp" | "key" => Kind::Slides,
        "zip" | "tar" | "gz" | "tgz" | "xz" | "zst" | "bz2" | "7z" | "rar" | "deb" | "rpm" | "iso" | "img" | "dmg" | "appimage" | "jar" => Kind::Archive,
        "ttf" | "otf" | "woff" | "woff2" => Kind::Font,
        "bin" | "exe" | "dll" | "so" | "a" | "o" | "wasm" | "dat" | "db" | "sqlite" | "npy" | "npz" | "pt" | "onnx" | "safetensors" | "h5" | "parquet" => Kind::Binary,
        _ if CODE.contains(&e) => Kind::Code,
        _ if TEXT.contains(&e) => Kind::Text,
        _ => Kind::Other,
    }
}

pub fn of_path(p: &Path) -> Kind {
    p.file_name().map_or(Kind::Other, of_name)
}

/// The symbolic icon for a file type (monochrome, tinted when drawn).
pub fn icon(k: Kind, set: &IconSet) -> u16 {
    let n = match k {
        Kind::Folder => "folder-symbolic",
        Kind::Image => "image-x-generic-symbolic",
        Kind::Video => "video-x-generic-symbolic",
        Kind::Audio => "audio-x-generic-symbolic",
        Kind::Document | Kind::Pdf => "x-office-document-symbolic",
        Kind::Sheet => "x-office-spreadsheet-symbolic",
        Kind::Slides => "x-office-presentation-symbolic",
        Kind::Archive => "package-x-generic-symbolic",
        Kind::Font => "font-x-generic-symbolic",
        Kind::Binary => "application-x-executable-symbolic",
        Kind::Code | Kind::Text | Kind::Other => "text-x-generic-symbolic",
    };
    set.id(&[n])
}

/// "PNG image", "Rust source", …
pub fn label(k: Kind, name: &OsStr) -> String {
    let e = ext_of(name).unwrap_or_default();
    let special = match e.as_str() {
        "rs" => "Rust source",
        "py" => "Python source",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "c" | "h" => "C source",
        "cc" | "cpp" | "cxx" | "hpp" => "C++ source",
        "go" => "Go source",
        "sh" | "bash" | "zsh" | "fish" => "Shell script",
        "nix" => "Nix expression",
        "md" | "markdown" => "Markdown",
        "txt" => "Plain text",
        "log" => "Log",
        "json" | "jsonc" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "html" | "htm" => "HTML",
        "pdf" => "PDF document",
        "epub" => "EPUB book",
        "jpg" | "jpeg" => "JPEG image",
        "svg" => "SVG image",
        "docx" | "doc" => "Word document",
        "xlsx" | "xls" => "Excel sheet",
        "pptx" | "ppt" => "PowerPoint",
        _ => "",
    };
    if !special.is_empty() {
        return special.to_string();
    }
    let up = e.to_ascii_uppercase();
    let suffix = match k {
        Kind::Image => "image",
        Kind::Video => "video",
        Kind::Audio => "audio",
        Kind::Archive => "archive",
        Kind::Font => "font",
        Kind::Sheet => "sheet",
        Kind::Slides => "slides",
        Kind::Document | Kind::Pdf => "document",
        Kind::Folder => return "Folder".into(),
        Kind::Text | Kind::Code | Kind::Binary | Kind::Other => "file",
    };
    if up.is_empty() { "File".into() } else { format!("{up} {suffix}") }
}

/// A name as it may be put on screen: never control characters (a newline
/// would break the line), and never the invisible formatting marks that
/// reorder or hide text. Each one becomes a visible middle dot.
pub fn shown(name: &OsStr) -> String {
    let lossy = String::from_utf8_lossy(name.as_bytes());
    if lossy.chars().all(|c| !unsafe_char(c)) {
        return lossy.into_owned();
    }
    lossy.chars().map(|c| if unsafe_char(c) { '\u{b7}' } else { c }).collect()
}

/// Control and invisible-format characters.
pub fn unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}')
}

pub fn human_size(b: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if b < 1000 {
        return format!("{b} B");
    }
    let (mut v, mut u) = (b as f64 / 1000.0, 0);
    while v >= 999.5 && u + 1 < UNITS.len() {
        v /= 1000.0;
        u += 1;
    }
    if v < 9.95 { format!("{v:.1} {}", UNITS[u]) } else { format!("{v:.0} {}", UNITS[u]) }
}

fn tm_of(secs: i64) -> libc::tm {
    // SAFETY: tm is plain data; localtime_r only fills it in.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&(secs as libc::time_t), &mut tm);
        tm
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "Today 14:02", "Yesterday", "3 days ago", "12 Oct", "12 Oct 2023".
pub fn human_time(secs: i64, now: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let (t, n) = (tm_of(secs), tm_of(now));
    let days = (n.tm_year - t.tm_year) * 366 + n.tm_yday - t.tm_yday;
    if secs > now + 60 {
        format!("{} {} {}", t.tm_mday, MONTHS[t.tm_mon.clamp(0, 11) as usize], t.tm_year + 1900)
    } else if days == 0 {
        format!("Today {:02}:{:02}", t.tm_hour, t.tm_min)
    } else if days == 1 || (n.tm_year != t.tm_year && (now - secs) < 86_400 * 2) {
        "Yesterday".into()
    } else if (2..7).contains(&days) {
        format!("{days} days ago")
    } else if t.tm_year == n.tm_year {
        format!("{} {}", t.tm_mday, MONTHS[t.tm_mon.clamp(0, 11) as usize])
    } else {
        format!("{} {} {}", t.tm_mday, MONTHS[t.tm_mon.clamp(0, 11) as usize], t.tm_year + 1900)
    }
}

/// Natural, case-insensitive order: `file2` before `file10`.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    let (a2, b2) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a2.len() && j < b2.len() {
        let (x, y) = (a2[i], b2[j]);
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let (s, t) = (i, j);
            while i < a2.len() && a2[i].is_ascii_digit() {
                i += 1;
            }
            while j < b2.len() && b2[j].is_ascii_digit() {
                j += 1;
            }
            let trim = |d: &[u8]| {
                let z = d.iter().take_while(|&&c| c == b'0').count();
                d[z..].to_vec()
            };
            let (p, q) = (trim(&a2[s..i]), trim(&b2[t..j]));
            let c = p.len().cmp(&q.len()).then_with(|| p.cmp(&q));
            if c != Equal {
                return c;
            }
            continue;
        }
        let c = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
        if c != Equal {
            return c;
        }
        i += 1;
        j += 1;
    }
    (a2.len() - i).cmp(&(b2.len() - j)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_safety() {
        let s = |b: &[u8]| shown(OsStr::from_bytes(b));
        assert_eq!(s(b"plain name.txt"), "plain name.txt");
        assert_eq!(s(b"two\nlines\ttab.txt"), "two\u{b7}lines\u{b7}tab.txt");
        // Bidi override: the "gpj" must not read as a reversed extension.
        let spoof = s("photo\u{202e}gpj.exe".as_bytes());
        assert!(!spoof.chars().any(|c| ('\u{202a}'..='\u{202e}').contains(&c)));
        assert_eq!(spoof, "photo\u{b7}gpj.exe");
        for c in ['\u{2066}', '\u{2069}', '\u{200f}', '\u{61c}', '\u{feff}', '\u{200b}', '\u{2028}'] {
            assert!(unsafe_char(c), "{c:?}");
        }
        // Invalid UTF-8 shows the replacement character, valid text stays.
        assert!(s(b"caf\xe9.txt").contains('\u{fffd}'));
        assert_eq!(s("日本語 ü.txt".as_bytes()), "日本語 ü.txt");
    }

    #[test]
    fn kinds_and_labels() {
        let k = |n: &str| of_name(OsStr::new(n));
        assert_eq!(k("a.PNG"), Kind::Image);
        assert_eq!(k("a.rs"), Kind::Code);
        assert_eq!(k("archive.tar.gz"), Kind::Archive);
        assert_eq!(k(".bashrc"), Kind::Other);
        assert_eq!(k("Makefile"), Kind::Code);
        assert_eq!(label(Kind::Image, OsStr::new("a.png")), "PNG image");
        assert_eq!(label(Kind::Code, OsStr::new("a.rs")), "Rust source");
        assert_eq!(label(Kind::Other, OsStr::new("noext")), "File");
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_000), "1.0 KB");
        assert_eq!(human_size(2_400_000), "2.4 MB");
        assert_eq!(human_size(120_000_000), "120 MB");
        assert_eq!(human_size(999_999_999), "1.0 GB");
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["file10", "File2", "file1", "a", "file02x", "Z", "z"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, vec!["a", "file1", "File2", "file02x", "file10", "Z", "z"]);
        assert_eq!(natural("same", "same"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn times() {
        let now = 1_790_000_000;
        assert!(human_time(now - 60, now).starts_with("Today") || human_time(now - 60, now) == "Yesterday");
        assert_eq!(human_time(0, now), "");
        assert!(human_time(now - 86_400 * 3, now).contains("days ago"));
        assert!(human_time(now - 86_400 * 800, now).len() >= 10);
    }
}
