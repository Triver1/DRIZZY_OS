//! A one-line text field: what the key does to the text. Drawing is the
//! page's; there is no selection, just a caret.

use crate::window::{KeyInput, keys};

const CTRL_U: u32 = 0x75;
const CTRL_V: u32 = 0x76;
const CTRL_A: u32 = 0x61;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edit {
    /// Not for this field.
    Ignored,
    Changed,
    /// Return.
    Submit,
    /// Escape.
    Cancel,
    /// Ctrl+V: the host fetches the clipboard.
    Paste,
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub text: String,
    /// Byte offset of the caret, on a character boundary.
    pub caret: usize,
    /// Most characters it takes.
    pub max: usize,
}

impl Line {
    pub fn new(text: &str, max: usize) -> Self {
        let mut l = Self { text: String::new(), caret: 0, max };
        l.insert(text);
        l
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    /// Up to `max`; control characters (a pasted newline) are dropped.
    pub fn insert(&mut self, s: &str) {
        let room = self.max.saturating_sub(self.text.chars().count());
        let add: String = s.chars().filter(|c| !c.is_control()).take(room).collect();
        self.text.insert_str(self.caret, &add);
        self.caret += add.len();
    }

    fn prev(&self) -> usize {
        self.text[..self.caret].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.caret..].chars().next().map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// Start of the word before the caret.
    fn word_start(&self) -> usize {
        let head = self.text[..self.caret].trim_end();
        head.char_indices().rev().find(|(_, c)| c.is_whitespace() || *c == '/').map_or(0, |(i, c)| i + c.len_utf8())
    }

    /// The text before the caret (to measure where to draw it).
    pub fn head(&self) -> &str {
        &self.text[..self.caret]
    }

    pub fn key(&mut self, k: &KeyInput) -> Edit {
        let lower = if (0x41..=0x5a).contains(&k.sym) { k.sym + 0x20 } else { k.sym };
        match k.sym {
            keys::ESCAPE => return Edit::Cancel,
            keys::RETURN | keys::KP_ENTER => return Edit::Submit,
            keys::BACKSPACE => {
                let to = if k.ctrl { self.word_start() } else { self.prev() };
                self.text.replace_range(to..self.caret, "");
                self.caret = to;
            }
            keys::DELETE => {
                let to = self.next();
                self.text.replace_range(self.caret..to, "");
            }
            keys::LEFT => self.caret = self.prev(),
            keys::RIGHT => self.caret = self.next(),
            keys::HOME => self.caret = 0,
            keys::END => self.caret = self.text.len(),
            _ if k.ctrl => match lower {
                CTRL_V => return Edit::Paste,
                CTRL_U => {
                    self.text.replace_range(..self.caret, "");
                    self.caret = 0;
                }
                CTRL_A => self.caret = 0,
                _ => return Edit::Ignored,
            },
            _ => match k.text.as_deref() {
                Some(t) if !t.is_empty() && !k.alt && !t.chars().any(char::is_control) => self.insert(t),
                _ => return Edit::Ignored,
            },
        }
        Edit::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(sym: u32) -> KeyInput {
        KeyInput { sym, text: None, ctrl: false, shift: false, alt: false }
    }

    fn typed(t: &str) -> KeyInput {
        KeyInput { sym: 0x20, text: Some(t.into()), ctrl: false, shift: false, alt: false }
    }

    fn ctrl(sym: u32) -> KeyInput {
        KeyInput { ctrl: true, ..press(sym) }
    }

    #[test]
    fn typing_and_moving_handle_multibyte_text() {
        let mut l = Line::new("", 10);
        l.key(&typed("é"));
        l.key(&typed("🙂"));
        l.key(&press(keys::LEFT));
        l.key(&typed("x"));
        assert_eq!(l.text, "éx🙂");
        l.key(&press(keys::BACKSPACE));
        l.key(&press(keys::BACKSPACE));
        assert_eq!(l.text, "🙂");
        assert_eq!(l.caret, 0);
        l.key(&press(keys::DELETE));
        assert_eq!(l.text, "");
        // Nothing left: these do nothing rather than panic.
        l.key(&press(keys::BACKSPACE));
        l.key(&press(keys::DELETE));
        l.key(&press(keys::LEFT));
        l.key(&press(keys::RIGHT));
    }

    #[test]
    fn the_cap_holds_for_typing_and_pasting() {
        let mut l = Line::new("abc", 4);
        l.key(&typed("d"));
        l.key(&typed("e"));
        assert_eq!(l.text, "abcd");
        let mut l = Line::new("", 3);
        l.insert("one\ntwo");
        assert_eq!(l.text, "one", "newlines dropped, then capped");
    }

    #[test]
    fn return_escape_and_paste_are_for_the_host() {
        let mut l = Line::new("x", 8);
        assert_eq!(l.key(&press(keys::RETURN)), Edit::Submit);
        assert_eq!(l.key(&press(keys::KP_ENTER)), Edit::Submit);
        assert_eq!(l.key(&press(keys::ESCAPE)), Edit::Cancel);
        assert_eq!(l.key(&ctrl(CTRL_V)), Edit::Paste);
        assert_eq!(l.key(&KeyInput { shift: true, ..ctrl(0x56) }), Edit::Paste, "Ctrl+Shift+V");
        assert_eq!(l.text, "x");
    }

    #[test]
    fn word_and_line_deletion() {
        let mut l = Line::new("~/Pictures/me.png", 64);
        l.key(&ctrl(keys::BACKSPACE));
        assert_eq!(l.text, "~/Pictures/");
        l.key(&ctrl(CTRL_U));
        assert_eq!(l.text, "");
        // A separator wider than a byte must not split a character.
        let mut l = Line::new("a\u{a0}bé", 64);
        l.key(&ctrl(keys::BACKSPACE));
        assert_eq!(l.text, "a\u{a0}");
    }

    #[test]
    fn stray_keys_change_nothing() {
        let mut l = Line::new("x", 8);
        assert_eq!(l.key(&press(0xffe1)), Edit::Ignored, "shift alone");
        assert_eq!(l.key(&KeyInput { text: Some("\u{1b}".into()), ..press(0x20) }), Edit::Ignored);
        assert_eq!(l.key(&KeyInput { ctrl: true, text: Some("q".into()), ..press(0x71) }), Edit::Ignored);
        assert_eq!(l.text, "x");
    }
}
