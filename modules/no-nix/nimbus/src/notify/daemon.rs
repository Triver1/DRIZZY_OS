//! org.freedesktop.Notifications on the session bus (zbus, blocking API).
//!
//! One thread, `nimbus-notify`, owns the connection and blocks on a job
//! queue: D-Bus method calls (served on zbus's own executor) only parse
//! their arguments and enqueue a job, so `Notify` returns immediately.
//! The thread then decodes the icon / image at the size the card draws it,
//! records the notification in the history and hands it to the UI as a
//! `Fact::Custom`. UI requests (close, invoke action) come back through
//! the same queue and are answered with the spec's signals.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
};

use calloop::channel::Sender;
use zbus::zvariant::OwnedValue;

use super::{Msg, Note};
use crate::{gfx::ImageData, services::Fact};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// Why a notification closed (spec values).
pub(crate) mod reason {
    pub const EXPIRED: u32 = 1;
    pub const DISMISSED: u32 = 2;
    pub const CLOSED: u32 = 3;
}

pub(super) enum Job {
    New(Box<Raw>),
    /// CloseNotification from the client.
    AppClose(u32),
    /// The UI closed it (expired / dismissed): (id, seq, reason). Ignored
    /// when `seq` no longer is the live note under that id (replaced).
    UiClose(u32, u64, u32),
    /// (id, seq, action key, xdg-activation token for the app).
    Invoke(u32, u64, String, Option<String>),
}

pub(super) struct Raw {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub urgency: u8,
    pub timeout: i32,
    pub image_data: Option<RawImage>,
    pub image_path: Option<String>,
    pub desktop_entry: Option<String>,
    pub transient: bool,
    pub resident: bool,
}

/// The `image-data` hint: (width, height, rowstride, has_alpha,
/// bits_per_sample, channels, data).
pub(super) struct RawImage {
    pub w: i32,
    pub h: i32,
    pub stride: i32,
    pub alpha: bool,
    pub bps: i32,
    pub channels: i32,
    pub data: Vec<u8>,
}

static JOBS: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);

pub(super) fn send(j: Job) {
    if let Ok(g) = JOBS.lock()
        && let Some(tx) = g.as_ref()
    {
        let _ = tx.send(j);
    }
}

pub(super) fn start(tx: Sender<Fact>) {
    let (jtx, jrx) = mpsc::channel();
    if let Ok(mut g) = JOBS.lock() {
        *g = Some(jtx);
    }
    let _ = thread::Builder::new().name("nimbus-notify".into()).stack_size(512 * 1024).spawn(move || run(tx, jrx));
}

struct Server {
    next: AtomicU32,
}

fn hint_str(h: &HashMap<String, OwnedValue>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| h.get(*k).and_then(|v| v.downcast_ref::<String>().ok())).filter(|s| !s.is_empty())
}

fn hint_bool(h: &HashMap<String, OwnedValue>, k: &str) -> bool {
    h.get(k).and_then(|v| v.downcast_ref::<bool>().ok()).unwrap_or(false)
}

fn hint_image(h: &HashMap<String, OwnedValue>) -> Option<RawImage> {
    let v = ["image-data", "image_data", "icon_data"].iter().find_map(|k| h.get(*k))?;
    let (w, h, stride, alpha, bps, channels, data) =
        <(i32, i32, i32, bool, i32, i32, Vec<u8>)>::try_from(v.try_clone().ok()?).ok()?;
    Some(RawImage { w, h, stride, alpha, bps, channels, data })
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 { replaces_id } else { self.next.fetch_add(1, Ordering::Relaxed) };
        let urgency = hints.get("urgency").and_then(|v| v.downcast_ref::<u8>().ok()).unwrap_or(1);
        send(Job::New(Box::new(Raw {
            id,
            app_name,
            app_icon,
            summary,
            body,
            actions,
            urgency,
            timeout: expire_timeout,
            image_data: hint_image(&hints),
            image_path: hint_str(&hints, &["image-path", "image_path"]),
            desktop_entry: hint_str(&hints, &["desktop-entry"]),
            transient: hint_bool(&hints, "transient"),
            resident: hint_bool(&hints, "resident"),
        })));
        id
    }

    fn close_notification(&self, id: u32) {
        send(Job::AppClose(id));
    }

    fn get_capabilities(&self) -> Vec<String> {
        // body-markup: clients then escape their plain text, so the
        // body can be stripped safely (the summary is never markup).
        ["body", "body-markup", "actions", "icon-static", "persistence"].map(String::from).to_vec()
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("nimbus".into(), "nimbus".into(), "0.1".into(), "1.2".into())
    }

    // Declared for introspection; emitted from the worker via emit_signal.
    #[zbus(signal)]
    async fn notification_closed(e: &zbus::object_server::SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn activation_token(e: &zbus::object_server::SignalEmitter<'_>, id: u32, token: String) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(e: &zbus::object_server::SignalEmitter<'_>, id: u32, action_key: String) -> zbus::Result<()>;
}

fn run(tx: Sender<Fact>, jobs: mpsc::Receiver<Job>) {
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.serve_at(PATH, Server { next: AtomicU32::new(1) }))
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.build());
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!("nimbus: notification daemon unavailable: {e}");
            if let Ok(mut g) = JOBS.lock() {
                *g = None;
            }
            return;
        }
    };
    let closed = |id: u32, why: u32| {
        let _ = conn.emit_signal(None::<&str>, PATH, NAME, "NotificationClosed", &(id, why));
    };
    // Live notifications (shown or in the history, not yet closed).
    let mut live: HashMap<u32, Arc<Note>> = HashMap::new();
    let mut icons = IconCache::default();
    let mut seq = 0u64;
    for job in jobs {
        match job {
            Job::New(raw) => {
                seq += 1;
                let note = Arc::new(build(*raw, seq, &mut icons));
                if let Some(old) = live.insert(note.id, note.clone()) {
                    old.live.store(false, Ordering::Relaxed);
                }
                // Old entries pushed out of the history are done for good.
                for id in super::history_push(&note) {
                    if let Some(n) = live.remove(&id) {
                        n.live.store(false, Ordering::Relaxed);
                        closed(id, reason::EXPIRED);
                    }
                }
                if tx.send(Fact::Custom(Arc::new(Msg::New(note)))).is_err() {
                    return;
                }
            }
            Job::AppClose(id) => {
                // Signal only what was still open (no second signal for an
                // id the app already saw closed); the UI drops it anyway.
                if let Some(n) = live.remove(&id) {
                    n.live.store(false, Ordering::Relaxed);
                    closed(id, reason::CLOSED);
                }
                super::history_remove(id);
                let _ = tx.send(Fact::Custom(Arc::new(Msg::Closed(id))));
            }
            Job::UiClose(id, seq, why) => {
                if live.get(&id).is_some_and(|n| n.seq == seq)
                    && let Some(n) = live.remove(&id)
                {
                    n.live.store(false, Ordering::Relaxed);
                    closed(id, why);
                }
            }
            Job::Invoke(id, seq, key, token) => {
                if let Some(n) = live.get(&id).filter(|n| n.seq == seq) {
                    if let Some(t) = token {
                        let _ = conn.emit_signal(None::<&str>, PATH, NAME, "ActivationToken", &(id, t.as_str()));
                    }
                    let _ = conn.emit_signal(None::<&str>, PATH, NAME, "ActionInvoked", &(id, key.as_str()));
                    if !n.resident {
                        n.live.store(false, Ordering::Relaxed);
                        live.remove(&id);
                        closed(id, reason::DISMISSED);
                    }
                }
            }
        }
    }
}

/// App icons decoded once per (name, size); notifications from the same
/// app share the pixels.
#[derive(Default)]
struct IconCache {
    map: HashMap<(String, u32), Option<ImageData>>,
}

impl IconCache {
    fn get(&mut self, name: &str, px: u32) -> Option<ImageData> {
        let k = (name.to_string(), px);
        if let Some(v) = self.map.get(&k) {
            return v.clone();
        }
        if self.map.len() > 64 {
            self.map.clear();
        }
        let v = crate::appicons::icon(name, px);
        self.map.insert(k, v.clone());
        v
    }
}

fn build(r: Raw, seq: u64, icons: &mut IconCache) -> Note {
    let px = super::icon_px();
    let image = r
        .image_data
        .as_ref()
        .and_then(|i| decode_raw(i, px))
        .or_else(|| r.image_path.as_deref().and_then(|p| load_path(p, px, icons)))
        .or_else(|| {
            let mut names: Vec<&str> = vec![];
            for n in [Some(r.app_icon.as_str()), r.desktop_entry.as_deref()].into_iter().flatten() {
                if !n.is_empty() {
                    names.push(n);
                }
            }
            let app = r.app_name.to_lowercase().replace(' ', "-");
            if !app.is_empty() {
                names.push(&app);
            }
            names.iter().find_map(|n| load_path(n, px, icons))
        });
    let mut actions = vec![];
    let mut default_action = false;
    for kv in r.actions.chunks(2) {
        if let [k, label] = kv {
            if k == "default" {
                default_action = true;
            } else if actions.len() < 3 {
                let label = if label.is_empty() { k } else { label };
                actions.push((k.clone(), collapse(clip_chars(label, LABEL_MAX)).replace('\n', " ")));
            }
        }
    }
    let critical = r.urgency >= 2;
    let app = clip_chars(&r.app_name, 64);
    let app = if app.is_empty() { "Notification".to_string() } else { app.to_string() };
    // Caps keep shaping cheap on the UI thread whatever a client sends.
    let body = strip_markup(clip_chars(&r.body, BODY_MAX));
    let timeout = if critical || r.timeout == 0 {
        None
    } else if r.timeout < 0 {
        Some(super::DEFAULT_TIMEOUT)
    } else {
        Some((r.timeout as f32 / 1000.0).clamp(1.0, 60.0))
    };
    Note {
        id: r.id,
        seq,
        label: app.to_uppercase(),
        initial: app.chars().find(|c| c.is_alphanumeric()).map_or_else(|| "•".into(), |c| c.to_uppercase().collect()),
        line: clip_chars(&body, LINE_MAX).replace('\n', " "),
        summary: collapse(clip_chars(&r.summary, SUMMARY_MAX)),
        body,
        actions,
        default_action,
        critical,
        timeout,
        image,
        time: clock_now(),
        transient: r.transient,
        resident: r.resident,
        live: AtomicBool::new(true),
    }
}

/// An image path or icon name from a client. Paths must be plain, modest
/// files (a FIFO or device would block this thread, a huge file would eat
/// memory); raster files decode under strict limits.
fn load_path(p: &str, px: u32, icons: &mut IconCache) -> Option<ImageData> {
    let p = p.strip_prefix("file://").unwrap_or(p);
    if p.is_empty() || p.len() > NAME_MAX || p.contains('\0') {
        return None;
    }
    if !p.starts_with('/') {
        // A theme icon name: no directories.
        return (!p.contains('/')).then(|| icons.get(p, px)).flatten();
    }
    let meta = std::fs::metadata(p).ok()?;
    // Size 0: empty, or a /proc-style file that streams on read.
    if !meta.is_file() || meta.len() == 0 || meta.len() > FILE_MAX {
        return None;
    }
    if p.ends_with(".svg") {
        return crate::appicons::load(std::path::Path::new(p), px);
    }
    let mut rd = image::ImageReader::open(p).ok()?.with_guessed_format().ok()?;
    let mut lim = image::Limits::default();
    lim.max_image_width = Some(MAX_DIM as u32);
    lim.max_image_height = Some(MAX_DIM as u32);
    lim.max_alloc = Some(256 << 20);
    rd.limits(lim);
    let img = rd.decode().ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    cover(img.into_raw(), w, h, px)
}

/// `image-data` → RGBA at `px`×`px` (centre-cropped like an avatar).
/// Anything a client sends is checked before a single cast or index: a
/// bad hint must never take the shell down (release builds abort on panic).
fn decode_raw(i: &RawImage, px: u32) -> Option<ImageData> {
    let dims = 1..=MAX_DIM;
    if !dims.contains(&i.w) || !dims.contains(&i.h) || i.stride <= 0 || i.bps != 8 || !(3..=4).contains(&i.channels) {
        return None;
    }
    let (w, h, ch, stride) = (i.w as usize, i.h as usize, i.channels as usize, i.stride as usize);
    let row = w.checked_mul(ch)?;
    let need = stride.checked_mul(h - 1)?.checked_add(row)?;
    if stride < row || i.data.len() < need {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &i.data[y * stride..y * stride + w * ch];
        for p in row.chunks_exact(ch) {
            rgba.extend_from_slice(&[p[0], p[1], p[2], if ch == 4 && i.alpha { p[3] } else { 255 }]);
        }
    }
    cover(rgba, w as u32, h as u32, px)
}

fn cover(rgba: Vec<u8>, w: u32, h: u32, px: u32) -> Option<ImageData> {
    let px = px.clamp(8, 256);
    let s = w.min(h);
    if s == 0 {
        return None;
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let crop = image::imageops::crop_imm(&img, (w - s) / 2, (h - s) / 2, s, s).to_image();
    let out = if s == px { crop } else { image::imageops::resize(&crop, px, px, image::imageops::FilterType::Triangle) };
    Some(ImageData { w: px, h: px, rgba: Arc::new(out.into_raw()) })
}

const SUMMARY_MAX: usize = 200;
const BODY_MAX: usize = 1024;
const LINE_MAX: usize = 200;
const LABEL_MAX: usize = 48;
/// Largest `image-data` / image file side accepted.
const MAX_DIM: i32 = 4096;
/// Icon names / paths, and image files on disk.
const NAME_MAX: usize = 1024;
const FILE_MAX: u64 = 16 << 20;

/// The first `n` chars of `s`.
pub(super) fn clip_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Tags of the spec's markup subset (plus the common `br`).
const TAGS: [&str; 6] = ["b", "i", "u", "a", "img", "br"];

/// Length of a known tag at the start of `s` (which starts with '<'), and
/// whether it breaks the line. Anything else is literal text.
fn tag_at(s: &str) -> Option<(usize, bool)> {
    let b = s.as_bytes();
    let mut i = 1;
    if b.get(i) == Some(&b'/') {
        i += 1;
    }
    let st = i;
    while i < b.len() && b[i].is_ascii_alphabetic() && i - st < 4 {
        i += 1;
    }
    let name = s[st..i].to_ascii_lowercase();
    if !TAGS.contains(&name.as_str()) || !matches!(b.get(i), Some(b'>' | b'/' | b' ' | b'\t' | b'\n')) {
        return None;
    }
    let end = s[i..].find('>')?;
    Some((i + end + 1, name == "br"))
}

/// The entity at the start of `s` (which starts with '&'): its char and
/// length. Only `&name;` / `&#n;` / `&#xh;` count.
fn entity_at(s: &str) -> Option<(char, usize)> {
    let (n, c) = s[1..].char_indices().take(10).find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '#'))?;
    if c != ';' {
        return None;
    }
    let e = &s[1..1 + n];
    let r = match e {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let num = e.strip_prefix('#')?;
            let v = match num.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => num.parse().ok()?,
            };
            char::from_u32(v)?
        }
    };
    Some((r, n + 2))
}

/// Plain text from the spec's tiny markup subset (b, i, u, a, img) and
/// its entities; a '<' or '&' that isn't one stays literal. Whitespace
/// runs collapse; newlines survive.
pub(super) fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '<'
            && let Some((n, br)) = tag_at(rest)
        {
            if br {
                out.push('\n');
            }
            rest = &rest[n..];
            continue;
        }
        if c == '&'
            && let Some((r, n)) = entity_at(rest)
        {
            out.push(r);
            rest = &rest[n..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    collapse(&out)
}

/// Collapse spaces within lines, drop blank lines.
pub(super) fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for l in s.lines() {
        let mut first = true;
        for w in l.split_whitespace() {
            if first {
                if !out.is_empty() {
                    out.push('\n');
                }
                first = false;
            } else {
                out.push(' ');
            }
            out.push_str(w);
        }
    }
    out
}

fn clock_now() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return String::new();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

/// Used by the demo: run raw notifications through the real decoding path.
pub(super) fn build_demo(r: Raw, seq: u64) -> Note {
    build(r, seq, &mut IconCache::default())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[test]
    fn markup() {
        assert_eq!(strip_markup("<b>Hi</b> &amp; <i>bye</i>"), "Hi & bye");
        assert_eq!(strip_markup("a  b\n\n c &#65;&#x42;"), "a b\nc AB");
        assert_eq!(strip_markup("x & y"), "x & y");
        assert_eq!(strip_markup("I <3 this, see you at 8"), "I <3 this, see you at 8");
        assert_eq!(strip_markup("a < b > c"), "a < b > c");
        assert_eq!(strip_markup("Tom & Jerry <b>x</b>"), "Tom & Jerry x");
        assert_eq!(strip_markup("<a href=\"x\">link</a> <img src=\"y\"/>!"), "link !");
        assert_eq!(strip_markup("one<br>two"), "one\ntwo");
        assert_eq!(collapse("Re: <Vec> bug"), "Re: <Vec> bug");
        assert_eq!(clip_chars("héllo", 2), "hé");
    }

    #[test]
    fn hostile_images() {
        let bad = |w, h, stride, channels, len| RawImage { w, h, stride, alpha: false, bps: 8, channels, data: vec![0; len] };
        assert!(decode_raw(&bad(2, 2, -1, 3, 6), 36).is_none());
        assert!(decode_raw(&bad(2, 2, 0, 3, 12), 36).is_none());
        assert!(decode_raw(&bad(i32::MAX, 2, i32::MAX, 4, 16), 36).is_none());
        assert!(decode_raw(&bad(2, i32::MAX, 8, 4, 16), 36).is_none());
        assert!(decode_raw(&bad(2, 2, 6, 3, 11), 36).is_none());
        assert!(decode_raw(&bad(2, 2, 6, 3, 12), 36).is_some());
        assert!(decode_raw(&bad(-2, 2, 6, 3, 12), 36).is_none());
    }

    /// xorshift64*: deterministic noise for the fuzz tests.
    pub(crate) struct Rng(pub u64);

    impl Rng {
        pub fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        pub fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
        pub fn pick<T: Copy>(&mut self, v: &[T]) -> T {
            v[self.below(v.len())]
        }
        /// Text built from markup-ish, unicode and whitespace fragments.
        pub fn text(&mut self, max: usize) -> String {
            const F: &[&str] = &[
                "<", ">", "&", ";", "#", "x", "/", "b", "i", "u", "a", "img", "br", "B", "A", " ", "  ", "\n", "\t", "\r", "amp", "lt", "gt",
                "#65", "#x1F600", "#xD800", "#99999999999", "#x", "&#;", "nbsp", "é", "🎉", "\u{301}", "\u{200b}", "\u{202e}", "\u{0}", "ß",
                "href=\"", "\"", "<b>", "</b>", "<br/>", "<img src=\"x\"/>", "<a href='y'>", "</a>", "<3", "a<b", "&amp;", "&lt;b&gt;",
                "Lorem ipsum dolor sit amet", "\u{fffd}", "\u{10ffff}", "ﷺ", "𒐫", "ـ",
            ];
            let n = self.below(max + 1);
            (0..n).map(|_| self.pick(F)).collect()
        }
    }

    /// Hostile `image-data`: every field at its edges or random, the data
    /// a little short / exact / long. Never panics; anything accepted
    /// comes out at the requested size.
    #[test]
    fn fuzz_image_data() {
        const EDGE: [i32; 22] = [
            i32::MIN, i32::MIN + 1, -65536, -4097, -4, -2, -1, 0, 1, 2, 3, 4, 5, 7, 8, 16, 255, 4096, 4097, 65535, i32::MAX - 1, i32::MAX,
        ];
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        let mut ok = 0;
        for _ in 0..40_000 {
            let f = |r: &mut Rng, small: i32| if r.below(3) == 0 { r.pick(&EDGE) } else if r.below(4) == 0 { r.next() as i32 } else { 1 + r.below(small as usize) as i32 };
            let (w, h) = (f(&mut r, 48), f(&mut r, 48));
            let channels = f(&mut r, 5);
            let bps = if r.below(5) == 0 { f(&mut r, 16) } else { 8 };
            let row = (w as i64).saturating_mul(channels as i64);
            let stride = match r.below(4) {
                0 => r.pick(&EDGE),
                1 => row as i32,
                2 => (row + r.below(9) as i64 - 4) as i32,
                _ => r.next() as i32,
            };
            let need = (stride as i64).saturating_mul(h as i64 - 1).saturating_add(row);
            let len = match r.below(4) {
                0 => r.below(1 << 14),
                _ => (need + r.below(5) as i64 - 2).clamp(0, 1 << 20) as usize,
            };
            let img = RawImage { w, h, stride, alpha: r.below(2) == 0, bps, channels, data: vec![0x5a; len] };
            let px = r.below(600) as u32;
            if let Some(d) = decode_raw(&img, px) {
                let p = px.clamp(8, 256);
                assert!(d.w == p && d.h == p && d.rgba.len() == (p * p * 4) as usize);
                assert!(w > 0 && h > 0 && stride > 0 && bps == 8 && (3..=4).contains(&channels));
                ok += 1;
            }
        }
        // Valid images do get through.
        assert!(ok > 100, "only {ok} accepted");
    }

    /// Hostile markup: never panics, plain text survives untouched,
    /// whitespace is normalised, the subset's tags never leak.
    #[test]
    fn fuzz_markup() {
        let mut r = Rng(0xdead_beef_cafe_f00d);
        for _ in 0..20_000 {
            let s = r.text(40);
            let out = strip_markup(&s);
            assert!(!out.contains("\n\n") && !out.starts_with('\n') && !out.ends_with('\n'));
            assert!(out.lines().all(|l| l == l.trim() && !l.contains("  ")));
            // Never longer than the input (tags / entities only shrink).
            assert!(out.chars().count() <= s.chars().count());
            if !s.contains('<') && !s.contains('&') {
                assert_eq!(out, collapse(&s));
            }
            let _ = clip_chars(&s, r.below(50));
        }
    }

    /// Huge / hostile fields through the real `build`: caps hold, nothing
    /// blocks or panics (paths to devices, dirs, /proc, traversal).
    #[test]
    fn fuzz_build() {
        let mut r = Rng(0x0123_4567_89ab_cdef);
        let huge = |unit: &str, n: usize| unit.repeat(n);
        let paths = [
            "", "/", "/dev/zero", "/dev/stdin", "/proc/self/mem", "/proc/self/pagemap", "/tmp", "../../../../etc/passwd", "file:///dev/zero",
            "file://", "a\u{0}b", "/nonexistent/x.png", "/etc/hostname", "dialog-information",
        ];
        let mut icons = IconCache::default();
        for i in 0..300u64 {
            let big = i % 10 == 0;
            let raw = Raw {
                id: r.next() as u32,
                app_name: if big { huge("Ꙃpp ", 50_000) } else { r.text(30) },
                app_icon: if big { huge("x/", 100_000) } else { r.pick(&paths).to_string() },
                summary: if big { huge("<b>sum</b> &amp; ", 80_000) } else { r.text(80) },
                body: if big { huge("line <3 &lt;\n", 100_000) } else { r.text(400) },
                actions: (0..if big { 20_000 } else { r.below(9) }).map(|_| if big { huge("label\n", 5000) } else { r.text(20) }).collect(),
                urgency: r.below(256) as u8,
                timeout: r.next() as i32,
                image_data: None,
                image_path: Some(r.pick(&paths).to_string()),
                desktop_entry: Some(if big { huge("e", 1 << 16) } else { r.text(10) }),
                transient: false,
                resident: false,
            };
            let n = build(raw, i, &mut icons);
            assert!(n.summary.chars().count() <= SUMMARY_MAX);
            assert!(n.body.chars().count() <= BODY_MAX);
            assert!(n.line.chars().count() <= LINE_MAX && !n.line.contains('\n'));
            assert!(n.label.chars().count() <= 64 * 3 && !n.label.is_empty());
            assert!(n.actions.len() <= 3 && n.actions.iter().all(|(_, l)| l.chars().count() <= LABEL_MAX));
            assert!(n.timeout.is_none_or(|t| (1.0..=60.0).contains(&t)));
            assert!(n.image.is_none() || n.image.as_ref().is_some_and(|i| i.rgba.len() == (i.w * i.h * 4) as usize));
        }
    }

    /// Talks to the daemon over a private bus: `NIMBUS_TEST_BUS=<address>`
    /// (e.g. from `dbus-daemon --session --print-address --fork`). Never
    /// runs against the real session bus, and never in a plain `cargo
    /// test` (it starts the daemon): `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn dbus_roundtrip() {
        let Ok(addr) = std::env::var("NIMBUS_TEST_BUS") else { return };
        unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &addr) };
        let mut ev: calloop::EventLoop<'static, Vec<Fact>> = calloop::EventLoop::try_new().unwrap();
        let (tx, rx) = calloop::channel::channel::<Fact>();
        ev.handle()
            .insert_source(rx, |e, _, q: &mut Vec<Fact>| {
                if let calloop::channel::Event::Msg(f) = e {
                    q.push(f);
                }
            })
            .unwrap();
        start(tx);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let c = zbus::blocking::Connection::session().unwrap();
        let p = zbus::blocking::Proxy::new(&c, NAME, PATH, NAME).unwrap();
        let caps: Vec<String> = p.call("GetCapabilities", &()).unwrap();
        assert!(caps.contains(&"actions".to_string()));
        let info: (String, String, String, String) = p.call("GetServerInformation", &()).unwrap();
        assert_eq!(info.3, "1.2");
        let mut hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        hints.insert("urgency", zbus::zvariant::Value::U8(2));
        let img = (2i32, 2i32, 6i32, false, 8i32, 3i32, vec![255u8; 12]);
        hints.insert("image-data", zbus::zvariant::Value::from(zbus::zvariant::Structure::from(img)));
        let id: u32 = p
            .call("Notify", &("test", 0u32, "", "Hello there", "<b>body</b> &amp; soul", vec!["default", "Open", "x", "Do X"], hints, -1i32))
            .unwrap();
        assert!(id > 0);
        let mut q = vec![];
        for _ in 0..20 {
            let _ = ev.dispatch(Some(std::time::Duration::from_millis(100)), &mut q);
            if !q.is_empty() {
                break;
            }
        }
        let Some(Fact::Custom(a)) = q.first() else { panic!("no fact") };
        let Some(Msg::New(n)) = a.downcast_ref::<Msg>() else { panic!("wrong fact") };
        assert_eq!(n.id, id);
        assert_eq!(n.summary, "Hello there");
        assert_eq!(n.body, "body & soul");
        assert!(n.critical && n.timeout.is_none() && n.default_action);
        assert_eq!(n.actions, vec![("x".to_string(), "Do X".to_string())]);
        assert!(n.image.as_ref().is_some_and(|i| i.w == super::super::icon_px()));
        // Signals.
        let sigs = zbus::blocking::MessageIterator::for_match_rule(
            zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(NAME).unwrap().build(),
            &c,
            None,
        )
        .unwrap();
        send(Job::Invoke(id, n.seq, "x".into(), None));
        let mut got = vec![];
        for m in sigs.take(2) {
            let m = m.unwrap();
            got.push(m.header().member().unwrap().to_string());
        }
        assert_eq!(got, vec!["ActionInvoked", "NotificationClosed"]);
        let _: () = p.call("CloseNotification", &(id,)).unwrap();
        // A hostile image (negative rowstride) is dropped, not a crash.
        let mut hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        let img = (2i32, 2i32, -1i32, false, 8i32, 3i32, vec![0u8; 6]);
        hints.insert("image-data", zbus::zvariant::Value::from(zbus::zvariant::Structure::from(img)));
        let id2: u32 = p.call("Notify", &("test", 0u32, "", "I <3 this", "a < b", Vec::<&str>::new(), hints, -1i32)).unwrap();
        let mut q = vec![];
        for _ in 0..20 {
            let _ = ev.dispatch(Some(std::time::Duration::from_millis(100)), &mut q);
            if q.iter().any(|f| matches!(f, Fact::Custom(a) if matches!(a.downcast_ref::<Msg>(), Some(Msg::New(n)) if n.id == id2))) {
                break;
            }
        }
        let n = q
            .iter()
            .find_map(|f| match f {
                Fact::Custom(a) => match a.downcast_ref::<Msg>() {
                    Some(Msg::New(n)) if n.id == id2 => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            })
            .expect("second note");
        assert!(n.image.is_none());
        assert_eq!(n.summary, "I <3 this");
        assert_eq!(n.body, "a < b");
    }
}
