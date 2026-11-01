//! Everything shown on the prompt comes from outside: polkit's `message`
//! embeds the caller's command line (`pkexec` puts its argv there), and
//! `details`, GECOS names and PAM prompts are just strings. The prompt asks
//! for a root password, so a string must not be able to rearrange what the
//! user reads: no line breaks, no bidi overrides that flip a command, no
//! invisible characters, no walls of combining marks, and a length limit.
//! Text is rendered as plain glyphs, so there is no markup to escape.

/// Longest `message`, in chars.
pub const MESSAGE: usize = 400;
pub const ACTION: usize = 128;
pub const PROGRAM: usize = 256;
pub const NAME: usize = 64;
/// PAM prompts and PAM's info and error lines.
pub const PAM: usize = 120;

/// Combining marks kept on one base character.
const MAX_MARKS: usize = 2;
/// Input looked at per output char: a string can't make us scan megabytes.
const SCAN: usize = 8;

/// Dropped outright: control and format characters, bidi controls,
/// zero-width and invisible characters, variation selectors, tags,
/// private use and noncharacters.
fn hidden(c: char) -> bool {
    let u = c as u32;
    c.is_control()
        || matches!(
            u,
            0xAD                  // soft hyphen
            | 0x34F               // combining grapheme joiner
            | 0x61C               // Arabic letter mark
            | 0x115F..=0x1160     // Hangul fillers
            | 0x17B4..=0x17B5     // Khmer invisible vowels
            | 0x180B..=0x180F     // Mongolian selectors and separator
            | 0x200B..=0x200F     // zero-width, LRM, RLM
            | 0x202A..=0x202E     // bidi embeddings and overrides
            | 0x2060..=0x206F     // word joiner, bidi isolates, deprecated formats
            | 0x3164              // Hangul filler
            | 0xE000..=0xF8FF     // private use
            | 0xFDD0..=0xFDEF     // noncharacters
            | 0xFE00..=0xFE0F     // variation selectors
            | 0xFEFF              // BOM, zero-width no-break space
            | 0xFFA0              // halfwidth Hangul filler
            | 0xFFF0..=0xFFFB     // specials, interlinear annotation
            | 0x1D173..=0x1D17A   // musical format controls
            | 0xE0000..=0xE0FFF   // tags, variation selectors supplement
            | 0xF0000..=0x10FFFF  // private use planes
        )
        || (u & 0xFFFE) == 0xFFFE
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x300..=0x36F | 0x483..=0x489 | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// `s` as one line of plain, visible text of at most `max` chars (an
/// ellipsis marks a cut). Runs of whitespace, including line breaks and
/// tabs, become one space.
pub fn clean(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max * 4));
    let (mut n, mut marks, mut space, mut cut) = (0usize, 0usize, false, false);
    for (seen, c) in s.chars().enumerate() {
        if seen >= max.saturating_mul(SCAN).max(64) {
            cut = true;
            break;
        }
        if c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}') {
            space = n > 0;
            marks = 0;
            continue;
        }
        if hidden(c) {
            continue;
        }
        if is_mark(c) {
            marks += 1;
            if n == 0 || marks > MAX_MARKS {
                continue;
            }
        } else {
            marks = 0;
        }
        if n + usize::from(space) >= max {
            cut = true;
            break;
        }
        if space {
            out.push(' ');
            n += 1;
            space = false;
        }
        out.push(c);
        n += 1;
    }
    if cut {
        while out.ends_with(' ') {
            out.pop();
        }
        if n >= max {
            out.pop();
        }
        out.push('…');
    }
    out
}

/// The PAM prompt as a field placeholder: "Password: " is "Password".
pub fn prompt(s: &str) -> String {
    let c = clean(s, PAM);
    let t = c.trim_end_matches([':', ' ']);
    if t.is_empty() { "Password".to_string() } else { t.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_untouched() {
        let s = "Authentication is needed to run `/usr/bin/true' as the super user";
        assert_eq!(clean(s, MESSAGE), s);
        assert_eq!(clean("Überprüfung für Größe 検証", MESSAGE), "Überprüfung für Größe 検証");
    }

    #[test]
    fn line_breaks_cannot_fake_a_layout() {
        assert_eq!(clean("run true\n\n\nPassword for root:\r\n", 100), "run true Password for root:");
        assert_eq!(clean("a\tb\u{2028}c\u{2029}d\u{85}e\u{b}f\u{c}g", 100), "a b c d e f g");
        assert_eq!(clean("   leading and trailing   ", 100), "leading and trailing");
        assert_eq!(clean("a\u{a0}\u{2003}\u{3000}b", 100), "a b");
    }

    #[test]
    fn bidi_overrides_are_gone() {
        // "rm -rf /" hidden behind a right-to-left override.
        let s = "true \u{202e}/ fr- mr\u{202c} \u{2066}x\u{2069}\u{200f}\u{200e}\u{61c}";
        assert_eq!(clean(s, 100), "true / fr- mr x");
        for c in (0x202A..=0x202E).chain(0x2066..=0x2069).chain([0x200E, 0x200F, 0x61C]) {
            let bad = format!("a{}b", char::from_u32(c).unwrap());
            assert_eq!(clean(&bad, 10), "ab", "U+{c:04X}");
        }
    }

    #[test]
    fn invisible_and_control_characters_are_gone() {
        for c in [
            '\0', '\u{1}', '\u{7f}', '\u{80}', '\u{9b}', '\u{1b}', '\u{ad}', '\u{34f}', '\u{115f}', '\u{200b}', '\u{200d}', '\u{2060}',
            '\u{3164}', '\u{feff}', '\u{fff9}', '\u{e000}', '\u{f8ff}', '\u{fe0f}', '\u{e0041}', '\u{e0001}', '\u{fffe}', '\u{1fffe}',
            '\u{10ffff}',
        ] {
            assert_eq!(clean(&format!("a{c}b"), 10), "ab", "U+{:04X}", c as u32);
        }
        // Terminal escapes lose their introducer; what is left is inert text.
        assert_eq!(clean("\x1b[31mred\x1b[0m", 40), "[31mred[0m");
    }

    #[test]
    fn markup_stays_literal_text() {
        let s = "<b>bold</b> &amp; <a href='x'>link</a>";
        assert_eq!(clean(s, 100), s);
    }

    #[test]
    fn truncation_is_visible_and_on_char_boundaries() {
        let s = "é".repeat(1000);
        let c = clean(&s, 10);
        assert_eq!(c.chars().count(), 10);
        assert!(c.ends_with('…'));
        // A string of exactly `max` chars is not cut.
        assert_eq!(clean(&"x".repeat(10), 10), "x".repeat(10));
        assert_eq!(clean(&"x".repeat(11), 10).chars().count(), 10);
        assert!(clean("word word word word", 12).ends_with('…'));
        assert!(clean("word word word word", 12).chars().count() <= 12);
    }

    #[test]
    fn huge_input_is_cut_without_scanning_all_of_it() {
        // Nothing but hidden characters, then a payload past the scan limit.
        let s = format!("{}visible", "\u{200b}".repeat(1 << 20));
        assert_eq!(clean(&s, 10), "…");
    }

    #[test]
    fn combining_floods_are_capped() {
        let zalgo = format!("a{}b", "\u{301}".repeat(200));
        assert_eq!(clean(&zalgo, 100), "a\u{301}\u{301}b");
        // No mark can sit on the card's edge with nothing under it.
        assert_eq!(clean("\u{301}a", 10), "a");
        assert_eq!(clean("ré", 10), "ré");
    }

    #[test]
    fn prompts_become_placeholders() {
        assert_eq!(prompt("Password: "), "Password");
        assert_eq!(prompt("Verification code:"), "Verification code");
        assert_eq!(prompt(""), "Password");
        assert_eq!(prompt(": :"), "Password");
    }
}
