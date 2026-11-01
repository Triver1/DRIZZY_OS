//! Now playing: every MPRIS player on the session bus (Spotify, browsers,
//! mpv …), reduced to the one worth showing.
//!
//! One thread, `nimbus-media`, owns a session-bus connection and blocks on
//! its message stream: PropertiesChanged / Seeked from players and
//! NameOwnerChanged for players coming and going. Nothing polls. Position
//! is sent as (position, stamp, rate) whenever it is known exactly (status
//! change, seek, new track); the UI extrapolates between.
//!
//! Album art is fetched off-thread (file:// read, or https:// through
//! `curl` with a strict time and size cap), decoded with limits, cropped
//! square, downscaled to a small thumbnail and sampled for a dominant
//! colour pair. The last few decoded covers are cached.

use std::{
    collections::HashMap,
    io::Read,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use calloop::channel::Sender;
use zbus::{
    MatchRule,
    blocking::{Connection, MessageIterator, fdo::DBusProxy},
    message::Type as MsgType,
    zvariant::{OwnedValue, Value},
};

use crate::{gfx::ImageData, services::Fact, theme::Rgba};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPS: &str = "org.freedesktop.DBus.Properties";

/// Thumbnail edge in pixels (drawn at ~22–26 logical px on 1–2× outputs).
pub const ART_PX: u32 = 72;
/// Untrusted download cap.
const MAX_BYTES: usize = 2 << 20;
const MAX_LOCAL: u64 = 8 << 20;

/// The player the bar shows.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    /// Short player name ("spotify", "firefox.instance_1_23").
    pub player: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    /// Seconds; 0 when unknown (streams).
    pub length: f32,
    /// Seconds at `stamp`.
    pub position: f32,
    pub stamp: Instant,
    pub rate: f32,
    pub playing: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
}

impl MediaInfo {
    /// Same track, same transport state (ignores the position estimate).
    fn same(&self, o: &MediaInfo) -> bool {
        self.player == o.player
            && self.title == o.title
            && self.artist == o.artist
            && self.album == o.album
            && self.art_url == o.art_url
            && self.length == o.length
            && self.rate == o.rate
            && self.playing == o.playing
            && self.can_next == o.can_next
            && self.can_prev == o.can_prev
            && self.can_seek == o.can_seek
    }
}

/// A decoded cover: straight-alpha RGBA thumbnail plus its colours.
pub struct Art {
    pub url: String,
    pub img: ImageData,
    /// Dominant and secondary colour, brightened for use on black; `None`
    /// for (near) greyscale covers.
    pub tint: Option<(Rgba, Rgba)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ctl {
    PlayPause,
    Next,
    Prev,
    /// Relative seek in seconds.
    Seek(f32),
}

// ── service ─────────────────────────────────────────────────────────────

pub fn spawn(tx: Sender<Fact>) {
    let _ = thread::Builder::new().name("nimbus-media".into()).stack_size(512 * 1024).spawn(move || {
        loop {
            match run(&tx) {
                Ok(()) => return,
                Err(e) => eprintln!("nimbus: media: {e}; retrying in 10 s"),
            }
            thread::sleep(Duration::from_secs(10));
        }
    });
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum Status {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Clone, Debug)]
struct Player {
    /// Well-known name without the MPRIS prefix.
    name: String,
    status: Status,
    title: String,
    artist: String,
    album: String,
    art_url: Option<String>,
    length: f32,
    position: f32,
    stamp: Instant,
    rate: f32,
    can_next: bool,
    can_prev: bool,
    can_seek: bool,
    /// Last time it started or stopped playing (most recent wins).
    changed: Instant,
}

impl Player {
    fn new(name: String) -> Self {
        let now = Instant::now();
        Self {
            name,
            status: Status::Stopped,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            art_url: None,
            length: 0.0,
            position: 0.0,
            stamp: now,
            rate: 1.0,
            can_next: false,
            can_prev: false,
            can_seek: false,
            changed: now,
        }
    }

    /// Apply a property map; returns true if the position must be re-read.
    fn apply(&mut self, props: &HashMap<String, OwnedValue>) -> bool {
        let mut reread = false;
        // Metadata first: a new track resets the position, which a
        // Position in the same map must then override.
        if let Some(v) = props.get("Metadata") {
            self.metadata(v);
            reread = true;
        }
        for (k, v) in props {
            let v: &Value = v;
            match k.as_str() {
                "PlaybackStatus" => {
                    let s = match v_str(v).as_deref() {
                        Some("Playing") => Status::Playing,
                        Some("Paused") => Status::Paused,
                        _ => Status::Stopped,
                    };
                    if s != self.status {
                        // Freeze the estimate where it is before switching.
                        self.position = self.estimate();
                        self.stamp = Instant::now();
                        self.status = s;
                        self.changed = Instant::now();
                        reread = true;
                    }
                }
                "Rate" => {
                    if let Some(r) = v_f64(v) {
                        self.position = self.estimate();
                        self.stamp = Instant::now();
                        // Rate 0 is not allowed while playing; treat it as 1.
                        self.rate = if r > 0.0 { r as f32 } else { 1.0 };
                    }
                }
                "Position" => {
                    if let Some(p) = v_f64(v) {
                        self.position = (p / 1e6) as f32;
                        self.stamp = Instant::now();
                    }
                }
                "CanGoNext" => self.can_next = v_bool(v).unwrap_or(false),
                "CanGoPrevious" => self.can_prev = v_bool(v).unwrap_or(false),
                "CanSeek" => self.can_seek = v_bool(v).unwrap_or(false),
                _ => {}
            }
        }
        reread
    }

    fn metadata(&mut self, v: &Value) {
        let mut title = String::new();
        let mut artist = String::new();
        let mut album = String::new();
        let mut art = None;
        let mut length = 0.0;
        if let Value::Dict(d) = unwrap_v(v) {
            for (k, v) in d.iter() {
                let Some(k) = v_str(k) else { continue };
                match k.as_str() {
                    "xesam:title" => title = v_str(v).unwrap_or_default(),
                    "xesam:artist" => artist = v_strs(v).join(", "),
                    "xesam:album" => album = v_str(v).unwrap_or_default(),
                    "mpris:artUrl" => art = v_str(v).filter(|s| !s.is_empty()),
                    "mpris:length" => length = v_f64(v).map_or(0.0, |l| (l / 1e6) as f32),
                    _ => {}
                }
            }
        }
        let new_track = title != self.title || artist != self.artist || album != self.album;
        self.title = title.trim().to_string();
        self.artist = artist.trim().to_string();
        self.album = album;
        self.art_url = art;
        self.length = length.max(0.0);
        if new_track {
            self.position = 0.0;
            self.stamp = Instant::now();
        }
    }

    fn estimate(&self) -> f32 {
        let p = if self.status == Status::Playing {
            self.position + self.stamp.elapsed().as_secs_f32() * self.rate
        } else {
            self.position
        };
        if self.length > 0.0 { p.clamp(0.0, self.length) } else { p.max(0.0) }
    }

    fn info(&self) -> MediaInfo {
        MediaInfo {
            player: self.name.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            art_url: self.art_url.clone(),
            length: self.length,
            position: self.estimate(),
            stamp: Instant::now(),
            rate: self.rate,
            playing: self.status == Status::Playing,
            can_next: self.can_next,
            can_prev: self.can_prev,
            can_seek: self.can_seek,
        }
    }
}

/// Where controls go: the connection and the shown player's unique name.
static TARGET: Mutex<Option<(Connection, String)>> = Mutex::new(None);

struct Svc {
    conn: Connection,
    /// unique name → player
    players: HashMap<String, Player>,
    shown: Option<(String, MediaInfo)>,
    art_sent: Option<String>,
    tx: Sender<Fact>,
}

fn run(tx: &Sender<Fact>) -> zbus::Result<()> {
    let conn = zbus::blocking::connection::Builder::session()?.method_timeout(Duration::from_secs(2)).build()?;
    let dbus = DBusProxy::new(&conn)?;
    for rule in [
        MatchRule::builder().msg_type(MsgType::Signal).interface(PROPS)?.member("PropertiesChanged")?.path(PATH)?.build(),
        MatchRule::builder().msg_type(MsgType::Signal).interface(PLAYER)?.member("Seeked")?.path(PATH)?.build(),
        MatchRule::builder()
            .msg_type(MsgType::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns("org.mpris.MediaPlayer2")?
            .build(),
    ] {
        dbus.add_match_rule(rule)?;
    }
    // Subscribe before listing so nothing that starts in between is lost.
    let iter = MessageIterator::from(&conn);
    let mut svc = Svc { conn: conn.clone(), players: HashMap::new(), shown: None, art_sent: None, tx: tx.clone() };
    for name in dbus.list_names()? {
        let name = name.to_string();
        if let Some(short) = name.strip_prefix(PREFIX)
            && let Ok(owner) = dbus.get_name_owner(name.as_str().try_into()?)
        {
            svc.add(owner.to_string(), short.to_string());
        }
    }
    if !svc.publish() {
        return Ok(());
    }
    for msg in iter {
        let Ok(msg) = msg else { continue };
        let h = msg.header();
        if h.message_type() != MsgType::Signal {
            continue;
        }
        let member = h.member().map(|m| m.to_string()).unwrap_or_default();
        let sender = h.sender().map(|s| s.to_string()).unwrap_or_default();
        match member.as_str() {
            "NameOwnerChanged" => {
                let Ok((name, old, new)) = msg.body().deserialize::<(String, String, String)>() else { continue };
                let Some(short) = name.strip_prefix(PREFIX) else { continue };
                if !old.is_empty() {
                    svc.players.remove(&old);
                }
                if !new.is_empty() {
                    svc.add(new, short.to_string());
                }
            }
            "PropertiesChanged" => {
                let Ok((iface, changed, invalidated)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() else {
                    continue;
                };
                if iface != PLAYER {
                    continue;
                }
                let Some(p) = svc.players.get_mut(&sender) else { continue };
                let reread = p.apply(&changed) || !invalidated.is_empty();
                if !invalidated.is_empty() {
                    svc.refresh(&sender);
                } else if reread {
                    svc.read_position(&sender);
                }
            }
            "Seeked" => {
                let Ok(pos) = msg.body().deserialize::<i64>() else { continue };
                if let Some(p) = svc.players.get_mut(&sender) {
                    p.position = pos as f32 / 1e6;
                    p.stamp = Instant::now();
                }
            }
            _ => continue,
        }
        if !svc.publish() {
            return Ok(());
        }
    }
    Err(zbus::Error::Failure("session bus closed".into()))
}

impl Svc {
    fn add(&mut self, unique: String, short: String) {
        self.players.insert(unique.clone(), Player::new(short));
        self.refresh(&unique);
    }

    /// Read every Player property (GetAll), then the position.
    fn refresh(&mut self, unique: &str) {
        let r = self.conn.call_method(Some(unique), PATH, Some(PROPS), "GetAll", &(PLAYER,));
        if let Ok(m) = r
            && let Ok(props) = m.body().deserialize::<HashMap<String, OwnedValue>>()
            && let Some(p) = self.players.get_mut(unique)
        {
            p.apply(&props);
            p.changed = Instant::now();
        }
    }

    fn read_position(&mut self, unique: &str) {
        let r = self.conn.call_method(Some(unique), PATH, Some(PROPS), "Get", &(PLAYER, "Position"));
        if let Ok(m) = r
            && let Ok(v) = m.body().deserialize::<OwnedValue>()
            && let Some(us) = v_f64(&v)
            && let Some(p) = self.players.get_mut(unique)
        {
            p.position = (us / 1e6) as f32;
            p.stamp = Instant::now();
        }
    }

    /// Pick the player to show and send it if anything changed. Returns
    /// false once the UI is gone.
    fn publish(&mut self) -> bool {
        let pick = pick(&self.players);
        let next = pick.map(|(u, p)| (u.clone(), p.info()));
        let changed = match (&self.shown, &next) {
            (None, None) => false,
            (Some((ua, a)), Some((ub, b))) => {
                // Re-send on a jump of the estimate (a seek) too.
                let drift = (a.position + a.stamp.elapsed().as_secs_f32() * if a.playing { a.rate } else { 0.0 } - b.position).abs();
                ua != ub || !a.same(b) || drift > 0.75
            }
            _ => true,
        };
        if let Ok(mut t) = TARGET.lock() {
            *t = next.as_ref().map(|(u, _)| (self.conn.clone(), u.clone()));
        }
        if !changed {
            return true;
        }
        self.shown = next.clone();
        let info = next.map(|(_, i)| i);
        let art = info.as_ref().and_then(|i| i.art_url.clone());
        if self.tx.send(Fact::Media(info)).is_err() {
            return false;
        }
        if art != self.art_sent {
            self.art_sent = art.clone();
            if let Some(url) = art {
                fetch_art(url, self.tx.clone());
            }
        }
        true
    }
}

/// Playing beats paused; among equals the most recently changed wins.
/// Stopped players and players without a title are not shown.
fn pick(players: &HashMap<String, Player>) -> Option<(&String, &Player)> {
    players
        .iter()
        .filter(|(_, p)| p.status != Status::Stopped && !p.title.is_empty())
        .max_by_key(|(_, p)| (p.status == Status::Playing, p.changed))
}

/// Send a transport command to the shown player (off the UI thread).
pub fn control(c: Ctl) {
    let Some((conn, dest)) = TARGET.lock().ok().and_then(|t| t.clone()) else { return };
    let _ = thread::Builder::new().name("nimbus-media-ctl".into()).stack_size(128 * 1024).spawn(move || {
        let d = Some(dest.as_str());
        let r = match c {
            Ctl::PlayPause => conn.call_method(d, PATH, Some(PLAYER), "PlayPause", &()),
            Ctl::Next => conn.call_method(d, PATH, Some(PLAYER), "Next", &()),
            Ctl::Prev => conn.call_method(d, PATH, Some(PLAYER), "Previous", &()),
            Ctl::Seek(s) => conn.call_method(d, PATH, Some(PLAYER), "Seek", &((s as f64 * 1e6) as i64,)),
        };
        if let Err(e) = r {
            eprintln!("nimbus: media {c:?}: {e}");
        }
    });
}

/// `nimbus media-dump --watch <secs>`: run the real service and print the
/// facts it sends (read-only: no control calls).
pub fn watch(secs: f32) {
    let mut ev: calloop::EventLoop<'static, ()> = calloop::EventLoop::try_new().expect("event loop");
    let (tx, rx) = calloop::channel::channel::<Fact>();
    let t0 = Instant::now();
    ev.handle()
        .insert_source(rx, move |e, _, _| {
            let calloop::channel::Event::Msg(f) = e else { return };
            let t = t0.elapsed().as_secs_f32();
            match f {
                Fact::Media(Some(m)) => println!(
                    "{t:6.2}s media {} {:?} — {:?} {} {:.1}/{:.1}s",
                    m.player,
                    m.title,
                    m.artist,
                    if m.playing { "playing" } else { "paused" },
                    m.position,
                    m.length
                ),
                Fact::Media(None) => println!("{t:6.2}s media none"),
                Fact::MediaArt(a) => println!("{t:6.2}s art {}×{} tint {:?} {}", a.img.w, a.img.h, a.tint.map(|(x, y)| (hex(x), hex(y))), a.url),
                _ => {}
            }
        })
        .expect("channel");
    spawn(tx);
    let end = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < end {
        let _ = ev.dispatch(Some(end.saturating_duration_since(Instant::now())), &mut ());
    }
}

/// `nimbus media-dump`: print what the bar would see, once (read-only).
pub fn dump() {
    let r = (|| -> zbus::Result<()> {
        let conn = zbus::blocking::connection::Builder::session()?.method_timeout(Duration::from_secs(2)).build()?;
        let dbus = DBusProxy::new(&conn)?;
        let (tx, _rx) = calloop::channel::channel::<Fact>();
        let mut svc = Svc { conn: conn.clone(), players: HashMap::new(), shown: None, art_sent: Some(String::new()), tx };
        for name in dbus.list_names()? {
            let name = name.to_string();
            if let Some(short) = name.strip_prefix(PREFIX) {
                let owner = dbus.get_name_owner(name.as_str().try_into()?)?;
                svc.add(owner.to_string(), short.to_string());
            }
        }
        if svc.players.is_empty() {
            println!("no MPRIS players on the session bus");
        }
        for (u, p) in &svc.players {
            println!(
                "{u} {:<24} {:?} {:?} — {:?} [{}] {:.1}/{:.1}s rate {} next={} prev={} seek={}\n    art: {}",
                p.name,
                p.status,
                p.title,
                p.artist,
                p.album,
                p.estimate(),
                p.length,
                p.rate,
                p.can_next,
                p.can_prev,
                p.can_seek,
                p.art_url.as_deref().unwrap_or("-"),
            );
        }
        match pick(&svc.players) {
            Some((u, p)) => {
                println!("shown: {} ({u})", p.name);
                if let Some(url) = &p.art_url {
                    let t0 = Instant::now();
                    match load_art(url) {
                        Some(a) => println!(
                            "art: {}×{} in {:.0} ms, tint {:?}",
                            a.img.w,
                            a.img.h,
                            t0.elapsed().as_secs_f32() * 1e3,
                            a.tint.map(|(x, y)| (hex(x), hex(y)))
                        ),
                        None => println!("art: failed to load"),
                    }
                }
            }
            None => println!("shown: nothing"),
        }
        Ok(())
    })();
    if let Err(e) = r {
        eprintln!("media-dump: {e}");
        std::process::exit(1);
    }
}

fn hex(c: Rgba) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.0), b(c.1), b(c.2))
}

// ── album art ───────────────────────────────────────────────────────────

static CACHE: Mutex<Vec<Arc<Art>>> = Mutex::new(Vec::new());
const CACHE_N: usize = 4;

fn fetch_art(url: String, tx: Sender<Fact>) {
    if let Some(a) = CACHE.lock().ok().and_then(|c| c.iter().find(|a| a.url == url).cloned()) {
        let _ = tx.send(Fact::MediaArt(a));
        return;
    }
    let _ = thread::Builder::new().name("nimbus-art".into()).stack_size(512 * 1024).spawn(move || {
        let Some(art) = load_art(&url) else { return };
        let art = Arc::new(art);
        if let Ok(mut c) = CACHE.lock() {
            c.retain(|a| a.url != url);
            c.insert(0, art.clone());
            c.truncate(CACHE_N);
        }
        let _ = tx.send(Fact::MediaArt(art));
    });
}

/// Fetch and decode a cover. `None` on any failure (or a hostile file).
pub fn load_art(url: &str) -> Option<Art> {
    let bytes = if let Some(path) = url.strip_prefix("file://") {
        let path = percent_decode(path);
        let f = std::fs::File::open(&path).ok()?;
        if f.metadata().ok()?.len() > MAX_LOCAL {
            return None;
        }
        let mut v = Vec::new();
        f.take(MAX_LOCAL).read_to_end(&mut v).ok()?;
        v
    } else if url.starts_with("https://") || url.starts_with("http://") {
        download(url)?
    } else {
        return None;
    };
    let (img, tint) = decode(&bytes)?;
    Some(Art { url: url.to_string(), img, tint })
}

/// `curl` with no config file, a hard timeout and a byte cap on what we
/// read; redirects may only go to https.
pub(crate) fn download(url: &str) -> Option<Vec<u8>> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let mut child = Command::new("curl")
        .args(["-q", "-sSfL", "--max-time", "8", "--max-filesize", &MAX_BYTES.to_string(), "--max-redirs", "3"])
        .args(["--proto", "=https,http", "--proto-redir", "=https", "--url", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut v = Vec::new();
    let ok = child.stdout.take().map(|o| o.take(MAX_BYTES as u64 + 1).read_to_end(&mut v));
    if v.len() > MAX_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let status = child.wait().ok()?;
    (status.success() && ok.is_some_and(|r| r.is_ok()) && !v.is_empty()).then_some(v)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decode with limits, centre-crop square, downscale, sample colours.
pub fn decode(bytes: &[u8]) -> Option<(ImageData, Option<(Rgba, Rgba)>)> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut lim = image::Limits::default();
    lim.max_image_width = Some(6000);
    lim.max_image_height = Some(6000);
    lim.max_alloc = Some(128 << 20);
    r.limits(lim);
    let img = r.decode().ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let s = w.min(h).max(1);
    let sq = image::imageops::crop_imm(&img, (w - s) / 2, (h - s) / 2, s, s).to_image();
    let small = image::imageops::resize(&sq, ART_PX, ART_PX, image::imageops::FilterType::CatmullRom);
    let tint = dominant(small.as_raw());
    Some((ImageData { w: ART_PX, h: ART_PX, rgba: Arc::new(small.into_raw()) }, tint))
}

/// Dominant and secondary hue of an RGBA buffer, as bright UI colours.
pub fn dominant(px: &[u8]) -> Option<(Rgba, Rgba)> {
    const BINS: usize = 18;
    let mut w = [0f32; BINS];
    let mut rgb = [[0f32; 3]; BINS];
    let mut n = 0f32;
    for p in px.chunks_exact(4).step_by(2) {
        if p[3] < 128 {
            continue;
        }
        n += 1.0;
        let (r, g, b) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
        let (h, s, v) = hsv(r, g, b);
        if v < 0.14 || s < 0.12 {
            continue;
        }
        let k = s * s * v.sqrt();
        let i = ((h / 360.0 * BINS as f32) as usize).min(BINS - 1);
        w[i] += k;
        rgb[i][0] += r * k;
        rgb[i][1] += g * k;
        rgb[i][2] += b * k;
    }
    // Neighbouring bins blur together so a hue on a bin edge isn't split.
    let score = |i: usize| w[i] + 0.5 * (w[(i + 1) % BINS] + w[(i + BINS - 1) % BINS]);
    let best = (0..BINS).max_by(|a, b| score(*a).total_cmp(&score(*b)))?;
    if n == 0.0 || score(best) / n < 0.02 {
        return None;
    }
    let col = |i: usize| {
        let k = w[i].max(1e-6);
        let (h, s, _) = hsv(rgb[i][0] / k, rgb[i][1] / k, rgb[i][2] / k);
        (h, s)
    };
    let (h1, s1) = col(best);
    // Secondary: the strongest bin at least 50° away, else a neighbour hue.
    let far = |i: usize| {
        let d = (i as i32 - best as i32).rem_euclid(BINS as i32);
        d.min(BINS as i32 - d) >= 3
    };
    let second = (0..BINS).filter(|i| far(*i) && w[*i] > 0.25 * w[best]).max_by(|a, b| w[*a].total_cmp(&w[*b]));
    let (h2, s2) = match second {
        Some(i) => col(i),
        None => ((h1 + 28.0) % 360.0, s1 * 0.85),
    };
    let ui = |h: f32, s: f32| {
        let (r, g, b) = rgb_of(h, s.clamp(0.38, 0.72), 0.97);
        Rgba(r, g, b, 1.0)
    };
    Some((ui(h1, s1), ui(h2, s2)))
}

fn hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d <= 1e-6 {
        0.0
    } else if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if mx > 0.0 { d / mx } else { 0.0 }, mx)
}

fn rgb_of(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

// ── textures ────────────────────────────────────────────────────────────

static NEXT_KEY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0xDEC1_0000_0000_0001);

/// A UI texture (a cover, an app icon) that cross-fades to its successor.
/// Uploads and frees ride on the next draw list; `keys` hands back what
/// is still on the GPU when the owner goes (for `gfx::free_later`).
pub struct TexSwap {
    pub cur: Option<(u64, String)>,
    pub prev: Option<(u64, String)>,
    pending: Option<(u64, ImageData)>,
    /// Cross-fade progress, 0 → 1 (snaps to 1 when there is no `prev`).
    pub t: crate::anim::Spring,
    frees: Vec<u64>,
}

impl Default for TexSwap {
    fn default() -> Self {
        Self { cur: None, prev: None, pending: None, t: crate::anim::Spring::new(1.0, crate::anim::SpringCfg::new(0.5, 1.0)), frees: vec![] }
    }
}

impl TexSwap {
    /// Show `img` (identified by `id`); a different id cross-fades.
    pub fn want(&mut self, id: &str, img: &ImageData) {
        if self.cur.as_ref().is_some_and(|c| c.1 == id) {
            return;
        }
        if let Some((k, _)) = self.prev.take() {
            self.frees.push(k);
        }
        self.prev = self.cur.take();
        let key = NEXT_KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.cur = Some((key, id.to_string()));
        self.pending = Some((key, img.clone()));
        if self.prev.is_some() {
            self.t.snap(0.0);
            self.t.set(1.0);
        } else {
            self.t.snap(1.0);
        }
    }
    pub fn clear(&mut self) {
        self.frees.extend(self.cur.take().map(|c| c.0));
        self.frees.extend(self.prev.take().map(|c| c.0));
        self.pending = None;
        self.t.snap(1.0);
    }
    /// Push pending uploads / frees into this frame's list.
    pub fn flush(&mut self, out: &mut crate::gfx::DrawList) {
        if self.t.at_rest()
            && let Some((k, _)) = self.prev.take()
        {
            self.frees.push(k);
        }
        out.uploads.extend(self.pending.take());
        out.frees.append(&mut self.frees);
    }
    pub fn keys(&mut self) -> Vec<u64> {
        let mut v = std::mem::take(&mut self.frees);
        v.extend(self.cur.take().map(|c| c.0));
        v.extend(self.prev.take().map(|c| c.0));
        v
    }
}

// ── zvariant helpers ────────────────────────────────────────────────────

fn unwrap_v<'a, 'b>(v: &'a Value<'b>) -> &'a Value<'b> {
    match v {
        Value::Value(b) => unwrap_v(b),
        _ => v,
    }
}

fn v_str(v: &Value) -> Option<String> {
    match unwrap_v(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn v_strs(v: &Value) -> Vec<String> {
    match unwrap_v(v) {
        Value::Array(a) => a.iter().filter_map(v_str).collect(),
        other => v_str(other).into_iter().collect(),
    }
}

fn v_f64(v: &Value) -> Option<f64> {
    Some(match unwrap_v(v) {
        Value::I64(x) => *x as f64,
        Value::U64(x) => *x as f64,
        Value::I32(x) => *x as f64,
        Value::U32(x) => *x as f64,
        Value::I16(x) => *x as f64,
        Value::U16(x) => *x as f64,
        Value::U8(x) => *x as f64,
        Value::F64(x) => *x,
        _ => return None,
    })
}

fn v_bool(v: &Value) -> Option<bool> {
    match unwrap_v(v) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_picks_the_saturated_hue() {
        // Mostly dark grey with a red patch.
        let mut px = vec![];
        for i in 0..1000 {
            if i % 4 == 0 { px.extend_from_slice(&[220, 40, 40, 255]) } else { px.extend_from_slice(&[30, 30, 30, 255]) }
        }
        let (a, _) = dominant(&px).expect("tint");
        assert!(a.0 > a.1 && a.0 > a.2, "{a:?}");
        // Pure greyscale has no tint.
        let grey: Vec<u8> = (0..1000).flat_map(|i| [(i % 255) as u8; 3].into_iter().chain([255])).collect();
        assert!(dominant(&grey).is_none());
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%20b/c%C3%A9.jpg"), "/a b/cé.jpg");
        assert_eq!(percent_decode("/x%2"), "/x%2");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not an image at all").is_none());
        assert!(load_art("ftp://x/y.png").is_none());
    }
}
