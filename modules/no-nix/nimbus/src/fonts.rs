//! Load only the faces we use. Scanning every system font (this machine
//! has the whole Google Fonts collection) would cost hundreds of ms and MBs.

use std::process::Command;

use glyphon::fontdb::Database;

/// Fallbacks for window titles in other scripts, and emoji.
const FALLBACKS: &[&str] = &["Noto Sans CJK JP", "Noto Color Emoji", "sans-serif"];

/// Load `families` (the look's UI face, the design's faces) plus fallbacks,
/// via fontconfig.
pub fn load(db: &mut Database, families: &[&str]) {
    let mut loaded = 0;
    for pat in families.iter().chain(FALLBACKS) {
        let Ok(out) = Command::new("fc-match").args(["-f", "%{file}", pat]).output() else {
            continue;
        };
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() && db.load_font_file(&path).is_ok() {
            loaded += 1;
        }
    }
    if loaded == 0 {
        db.load_system_fonts();
    }
}
