//! System tray: StatusNotifierItem host (and watcher, when nobody else is).
//!
//! One thread, `nimbus-tray`, owns a session-bus connection. It tries to
//! become `org.kde.StatusNotifierWatcher`; apps register their items with
//! it (the interface below, served on zbus's executor, records them and
//! emits `StatusNotifierItemRegistered`). If another watcher already runs
//! (waybar, a desktop shell) nimbus registers as a host with it instead.
//! Either way the thread learns about items from the watcher's signals and
//! about icon / status changes from the items' own signals, blocking on
//! the bus; nothing polls.
//!
//! Icons arrive as theme names (resolved like app icons, with the item's
//! IconThemePath first) or as ARGB32 pixmaps, which are untrusted: sizes
//! are bounded and checked against the data before converting. Each icon
//! is sent in colour and as a desaturated variant for the resting bar.
//!
//! Clicks: left → Activate (or the menu for menu-only items), middle →
//! SecondaryActivate, right → the item's com.canonical.dbusmenu menu,
//! shown by nimbus (`traymenu.rs`), else ContextMenu.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use calloop::channel::Sender;
use zbus::{
    MatchRule,
    blocking::{Connection, MessageIterator, fdo::DBusProxy},
    fdo::{RequestNameFlags, RequestNameReply},
    message::Type as MsgType,
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Value},
};

use crate::{gfx::ImageData, services::Fact};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const MENU: &str = "com.canonical.dbusmenu";
/// Icon edge in pixels (drawn at 16 logical px on 1–2× outputs).
pub const ICON_PX: u32 = 32;
const MAX_ITEMS: usize = 32;

/// What the bar draws for one item.
#[derive(Clone)]
pub struct TrayItem {
    pub id: String,
    pub title: String,
    pub attention: bool,
    pub icon: Option<ImageData>,
    /// Desaturated, for the bar at rest.
    pub grey: Option<ImageData>,
    /// Changes whenever the icon does.
    pub rev: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Primary,
    Secondary,
    Menu,
}

// ── the watcher interface (only when nobody else provides it) ───────────

struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
        service: &str,
    ) {
        let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();
        // A bare object path means "on the caller's connection".
        let id = if service.starts_with('/') { format!("{sender}{service}") } else { service.to_string() };
        if id.is_empty() || id.len() > 512 {
            return;
        }
        let new = match self.items.lock() {
            Ok(mut v) if !v.contains(&id) && v.len() < MAX_ITEMS => {
                v.push(id.clone());
                true
            }
            _ => false,
        };
        if new {
            let _ = Self::status_notifier_item_registered(&e, &id).await;
        }
    }

    async fn register_status_notifier_host(&self, #[zbus(signal_emitter)] e: SignalEmitter<'_>, _service: &str) {
        let _ = Self::status_notifier_host_registered(&e).await;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().map(|v| v.clone()).unwrap_or_default()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(e: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(e: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

// ── the host ────────────────────────────────────────────────────────────

/// Where an item lives, for clicks and menus.
#[derive(Clone, Debug)]
struct Addr {
    bus: String,
    path: String,
    menu: Option<String>,
    item_is_menu: bool,
}

struct Item {
    addr: Addr,
    /// Unique name that owns the item (signals come from it).
    owner: String,
    view: TrayItem,
    passive: bool,
}

static CONN: Mutex<Option<Connection>> = Mutex::new(None);
static ADDRS: Mutex<Option<HashMap<String, Addr>>> = Mutex::new(None);

pub fn spawn(tx: Sender<Fact>) {
    let _ = thread::Builder::new().name("nimbus-tray".into()).stack_size(512 * 1024).spawn(move || {
        loop {
            match run(&tx, None) {
                Ok(()) => return,
                Err(e) => eprintln!("nimbus: tray: {e}; retrying in 10 s"),
            }
            thread::sleep(Duration::from_secs(10));
        }
    });
}

/// Split an item id into (bus name, object path).
fn split(id: &str) -> (String, String) {
    match id.find('/') {
        Some(i) => (id[..i].to_string(), id[i..].to_string()),
        None => (id.to_string(), "/StatusNotifierItem".to_string()),
    }
}

/// `until`: stop after this long (tray-dump); `None`: run forever.
fn run(tx: &Sender<Fact>, until: Option<std::time::Instant>) -> zbus::Result<()> {
    let conn = zbus::blocking::connection::Builder::session()?.method_timeout(Duration::from_secs(2)).build()?;
    let dbus = DBusProxy::new(&conn)?;
    for rule in [
        MatchRule::builder().msg_type(MsgType::Signal).sender("org.freedesktop.DBus")?.member("NameOwnerChanged")?.build(),
        MatchRule::builder().msg_type(MsgType::Signal).interface(ITEM)?.build(),
        MatchRule::builder().msg_type(MsgType::Signal).interface(WATCHER)?.build(),
    ] {
        dbus.add_match_rule(rule)?;
    }
    let iter = MessageIterator::from(&conn);

    // Be the watcher if nobody is; otherwise register as a host.
    let shared = Arc::new(Mutex::new(Vec::<String>::new()));
    conn.object_server().at(WATCHER_PATH, Watcher { items: shared.clone() })?;
    let own = conn.request_name_with_flags(WATCHER, RequestNameFlags::DoNotQueue.into());
    let watcher = matches!(own, Ok(RequestNameReply::PrimaryOwner));
    let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    let _ = conn.request_name(host.as_str());
    let mut items: HashMap<String, Item> = HashMap::new();
    if watcher {
        eprintln!("nimbus: tray: acting as the StatusNotifierWatcher");
    } else {
        let _ = conn.object_server().remove::<Watcher, _>(WATCHER_PATH);
        let _ = conn.call_method(Some(WATCHER), WATCHER_PATH, Some(WATCHER), "RegisterStatusNotifierHost", &(host.as_str(),));
        let r = conn.call_method(Some(WATCHER), WATCHER_PATH, Some(PROPS), "Get", &(WATCHER, "RegisteredStatusNotifierItems"));
        if let Ok(m) = r
            && let Ok(v) = m.body().deserialize::<OwnedValue>()
        {
            for id in v_strs(&v).into_iter().take(MAX_ITEMS) {
                add(&conn, &dbus, &mut items, id);
            }
        }
    }
    *CONN.lock().map_err(|_| zbus::Error::Failure("lock".into()))? = Some(conn.clone());
    let mut sent: Vec<(String, u64, String, bool)> = vec![];
    if !publish(tx, &items, &mut sent) {
        return Ok(());
    }

    for msg in iter {
        if until.is_some_and(|u| std::time::Instant::now() > u) {
            return Ok(());
        }
        let Ok(msg) = msg else { continue };
        let h = msg.header();
        if h.message_type() != MsgType::Signal {
            continue;
        }
        let member = h.member().map(|m| m.to_string()).unwrap_or_default();
        let iface = h.interface().map(|m| m.to_string()).unwrap_or_default();
        let sender = h.sender().map(|s| s.to_string()).unwrap_or_default();
        let path = h.path().map(|p| p.to_string()).unwrap_or_default();
        match (iface.as_str(), member.as_str()) {
            (_, "NameOwnerChanged") => {
                let Ok((name, old, new)) = msg.body().deserialize::<(String, String, String)>() else { continue };
                if !new.is_empty() || old.is_empty() {
                    continue;
                }
                // An owner went away: its items go with it.
                let gone: Vec<String> = items.iter().filter(|(_, it)| it.owner == old || it.addr.bus == name).map(|(k, _)| k.clone()).collect();
                if gone.is_empty() {
                    continue;
                }
                for id in gone {
                    items.remove(&id);
                    if watcher {
                        if let Ok(mut v) = shared.lock() {
                            v.retain(|x| x != &id);
                        }
                        let _ = conn.emit_signal(None::<&str>, WATCHER_PATH, WATCHER, "StatusNotifierItemUnregistered", &(id.as_str(),));
                    }
                }
            }
            (WATCHER, "StatusNotifierItemRegistered") => {
                let Ok(id) = msg.body().deserialize::<String>() else { continue };
                if items.len() < MAX_ITEMS {
                    add(&conn, &dbus, &mut items, id);
                }
            }
            (WATCHER, "StatusNotifierItemUnregistered") => {
                let Ok(id) = msg.body().deserialize::<String>() else { continue };
                items.remove(&id);
            }
            (ITEM, _) => {
                // NewIcon / NewStatus / NewTitle / …: re-read that item.
                let ids: Vec<String> = items.iter().filter(|(_, it)| it.owner == sender && it.addr.path == path).map(|(k, _)| k.clone()).collect();
                for id in ids {
                    refresh(&conn, &mut items, &id);
                }
            }
            _ => continue,
        }
        if !publish(tx, &items, &mut sent) {
            return Ok(());
        }
    }
    Err(zbus::Error::Failure("session bus closed".into()))
}

fn add(conn: &Connection, dbus: &DBusProxy, items: &mut HashMap<String, Item>, id: String) {
    if items.contains_key(&id) {
        return;
    }
    let (bus, path) = split(&id);
    let owner = if bus.starts_with(':') {
        bus.clone()
    } else {
        match bus.as_str().try_into().ok().and_then(|n| dbus.get_name_owner(n).ok()) {
            Some(o) => o.to_string(),
            None => return,
        }
    };
    let view = TrayItem { id: id.clone(), title: String::new(), attention: false, icon: None, grey: None, rev: 0 };
    items.insert(id.clone(), Item { addr: Addr { bus, path, menu: None, item_is_menu: false }, owner, view, passive: false });
    refresh(conn, items, &id);
}

fn refresh(conn: &Connection, items: &mut HashMap<String, Item>, id: &str) {
    let Some(it) = items.get_mut(id) else { return };
    let r = conn.call_method(Some(it.addr.bus.as_str()), it.addr.path.as_str(), Some(PROPS), "GetAll", &(ITEM,));
    let Ok(m) = r else { return };
    let Ok(p) = m.body().deserialize::<HashMap<String, OwnedValue>>() else { return };
    let s = |k: &str| p.get(k).and_then(|v| v_str(v)).unwrap_or_default();
    let status = s("Status");
    it.passive = status == "Passive";
    it.view.attention = status == "NeedsAttention";
    it.view.title = { let t = s("Title"); if t.is_empty() { s("Id") } else { t } };
    it.addr.menu = p.get("Menu").and_then(|v| v_str(v)).filter(|m| m.starts_with('/') && m != "/");
    it.addr.item_is_menu = p.get("ItemIsMenu").and_then(|v| match &**v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }) == Some(true);
    let name = if it.view.attention { Some(s("AttentionIconName")).filter(|n| !n.is_empty()) } else { None }.unwrap_or_else(|| s("IconName"));
    let theme = s("IconThemePath");
    let icon = p
        .get(if it.view.attention && p.contains_key("AttentionIconPixmap") { "AttentionIconPixmap" } else { "IconPixmap" })
        .and_then(|v| pixmap(v))
        .or_else(|| named(&name, &theme));
    // Prefer a theme icon over a pixmap when both exist (crisper); last,
    // look next to the app's own executable (Nix keeps icons there).
    let icon = named(&name, &theme).or(icon).or_else(|| beside_exe(conn, &it.owner, &name));
    let rev = icon.as_ref().map_or(0, |i| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        i.rgba.hash(&mut h);
        h.finish()
    });
    if rev != it.view.rev {
        it.view.grey = icon.as_ref().map(desaturate);
        it.view.icon = icon;
        it.view.rev = rev;
    }
    if let Ok(mut a) = ADDRS.lock() {
        a.get_or_insert_with(HashMap::new).insert(id.to_string(), it.addr.clone());
    }
}

/// Send the visible items (in a stable order) if anything changed.
fn publish(tx: &Sender<Fact>, items: &HashMap<String, Item>, sent: &mut Vec<(String, u64, String, bool)>) -> bool {
    // Passive items are shown too: many apps never leave that state.
    let mut v: Vec<&Item> = items.values().filter(|i| i.view.icon.is_some()).collect();
    v.sort_by(|a, b| a.view.id.cmp(&b.view.id));
    let sig: Vec<(String, u64, String, bool)> = v.iter().map(|i| (i.view.id.clone(), i.view.rev, i.view.title.clone(), i.view.attention)).collect();
    if &sig == sent {
        return true;
    }
    *sent = sig;
    tx.send(Fact::Tray(Arc::new(v.into_iter().map(|i| i.view.clone()).collect()))).is_ok()
}

// ── icons ───────────────────────────────────────────────────────────────

/// ARGB32 (network order) pixmaps: pick the smallest ≥ ICON_PX, else the
/// largest; reject anything whose data doesn't match its size.
fn pixmap(v: &Value) -> Option<ImageData> {
    let Value::Array(a) = unwrap_v(v) else { return None };
    let mut best: Option<(u32, u32, Vec<u8>)> = None;
    for e in a.iter().take(16) {
        let Value::Structure(s) = unwrap_v(e) else { continue };
        let f = s.fields();
        let (Some(Value::I32(w)), Some(Value::I32(h)), Some(Value::Array(d))) = (f.first(), f.get(1), f.get(2)) else { continue };
        let (w, h) = (*w, *h);
        if !(1..=512).contains(&w) || !(1..=512).contains(&h) {
            continue;
        }
        if d.len() != (w * h * 4) as usize {
            continue;
        }
        let bytes: Vec<u8> = d.iter().filter_map(|b| if let Value::U8(b) = b { Some(*b) } else { None }).collect();
        if bytes.len() != d.len() {
            continue;
        }
        let better = match &best {
            None => true,
            Some((bw, _, _)) => {
                let (w, bw) = (w as u32, *bw);
                (w >= ICON_PX && (bw < ICON_PX || w < bw)) || (bw < ICON_PX && w > bw)
            }
        };
        if better {
            best = Some((w as u32, h as u32, bytes));
        }
    }
    let (w, h, d) = best?;
    let mut rgba = Vec::with_capacity(d.len());
    for p in d.chunks_exact(4) {
        rgba.extend_from_slice(&[p[1], p[2], p[3], p[0]]);
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    Some(fit(&img))
}

/// Scale into an ICON_PX square, aspect kept, centred.
fn fit(img: &image::RgbaImage) -> ImageData {
    let (w, h) = img.dimensions();
    let k = ICON_PX as f32 / w.max(h) as f32;
    let (nw, nh) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
    let small = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    let mut out = image::RgbaImage::new(ICON_PX, ICON_PX);
    image::imageops::overlay(&mut out, &small, ((ICON_PX - nw) / 2) as i64, ((ICON_PX - nh) / 2) as i64);
    ImageData { w: ICON_PX, h: ICON_PX, rgba: Arc::new(out.into_raw()) }
}

/// A theme icon by name, the item's own theme path first.
fn named(name: &str, theme: &str) -> Option<ImageData> {
    if name.is_empty() || name.len() > 256 || name.contains("..") {
        return None;
    }
    let p = std::path::Path::new(name);
    if p.is_absolute() {
        return crate::appicons::load(p, ICON_PX);
    }
    if !theme.is_empty() {
        let root = std::path::Path::new(theme);
        if let Some(f) = find_under(root, name, 0, &mut 0) {
            return crate::appicons::load(&f, ICON_PX);
        }
    }
    crate::appicons::find(name).and_then(|f| crate::appicons::load(&f, ICON_PX))
}

/// `<prefix>/share/icons` of the process that owns the item.
fn beside_exe(conn: &Connection, owner: &str, name: &str) -> Option<ImageData> {
    if name.is_empty() || name.len() > 256 || name.contains('/') || name.contains("..") {
        return None;
    }
    let m = conn
        .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "GetConnectionUnixProcessID", &(owner,))
        .ok()?;
    let pid = m.body().deserialize::<u32>().ok()?;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let icons = exe.parent()?.parent()?.join("share/icons");
    let f = find_under(&icons, name, 0, &mut 0)?;
    crate::appicons::load(&f, ICON_PX)
}

/// Bounded search for `<name>.{svg,png}` below `dir` (theme layouts or
/// flat folders): an SVG if there is one, else the largest PNG.
fn find_under(dir: &std::path::Path, name: &str, _depth: u32, _seen: &mut u32) -> Option<std::path::PathBuf> {
    let mut found: Vec<std::path::PathBuf> = vec![];
    collect(dir, name, 0, &mut 0, &mut found);
    let size = |p: &std::path::Path| -> u32 {
        // "48x48" / "spotify-linux-48" style hints; unknown sorts low.
        p.components().filter_map(|c| c.as_os_str().to_str()).flat_map(|c| c.split(|ch: char| !ch.is_ascii_digit())).filter_map(|n| n.parse::<u32>().ok()).filter(|n| *n <= 512).max().unwrap_or(1)
    };
    found.iter().find(|p| p.extension().is_some_and(|e| e == "svg")).cloned().or_else(|| found.iter().max_by_key(|p| size(p).min(256)).cloned())
}

fn collect(dir: &std::path::Path, name: &str, depth: u32, seen: &mut u32, found: &mut Vec<std::path::PathBuf>) {
    for ext in ["svg", "png"] {
        let f = dir.join(format!("{name}.{ext}"));
        if f.is_file() && found.len() < 32 {
            found.push(f);
        }
    }
    if depth >= 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        *seen += 1;
        if *seen > 3000 {
            return;
        }
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            collect(&e.path(), name, depth + 1, seen, found);
        }
    }
}

/// Calm resting variant: mostly grey, a little of the colour left.
pub(crate) fn desaturate(i: &ImageData) -> ImageData {
    let mut v = (*i.rgba).clone();
    for p in v.chunks_exact_mut(4) {
        let l = 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32;
        // Lift dark glyphs so they read on the black bar.
        let l = 70.0 + l * 0.75;
        for c in &mut p[..3] {
            *c = (l * 0.85 + *c as f32 * 0.15).clamp(0.0, 255.0) as u8;
        }
    }
    ImageData { w: i.w, h: i.h, rgba: Arc::new(v) }
}

// ── clicks and menus ────────────────────────────────────────────────────

/// One dbusmenu entry (labels without mnemonics).
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    /// Checkbox / radio state.
    pub toggle: Option<bool>,
    pub children: Vec<MenuEntry>,
}

/// A menu to show, for `traymenu` (via `Fact::Custom` and `take_menu`).
pub struct Menu {
    pub item: String,
    pub title: String,
    /// Where it was clicked (output-logical x of the icon's centre).
    pub x: f32,
    pub entries: Vec<MenuEntry>,
}

static PENDING: Mutex<Option<Arc<Menu>>> = Mutex::new(None);

/// The menu a new tray-menu window should show.
pub fn take_menu() -> Option<Arc<Menu>> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Handle a click on an item (off the UI thread). `x` is the icon's
/// centre in output-logical px (for the menu's placement).
pub fn click(id: &str, title: &str, c: Click, x: f32, tx: Sender<Fact>) {
    let Some(conn) = CONN.lock().ok().and_then(|c| c.clone()) else { return };
    let Some(addr) = ADDRS.lock().ok().and_then(|a| a.as_ref().and_then(|a| a.get(id).cloned())) else { return };
    let (id, title) = (id.to_string(), title.to_string());
    let _ = thread::Builder::new().name("nimbus-tray-click".into()).stack_size(256 * 1024).spawn(move || {
        let call = |m: &str| conn.call_method(Some(addr.bus.as_str()), addr.path.as_str(), Some(ITEM), m, &(x as i32, 0i32)).is_ok();
        let menu = || {
            let Some(path) = &addr.menu else { return false };
            match layout(&conn, &addr.bus, path) {
                Some(entries) if !entries.is_empty() => {
                    let m = Arc::new(Menu { item: id.clone(), title: title.clone(), x, entries });
                    if let Ok(mut p) = PENDING.lock() {
                        *p = Some(m.clone());
                    }
                    let _ = tx.send(Fact::Custom(m));
                    true
                }
                _ => false,
            }
        };
        let _ = match c {
            Click::Primary if addr.item_is_menu => menu() || call("ContextMenu"),
            Click::Primary => call("Activate") || menu(),
            Click::Secondary => call("SecondaryActivate"),
            Click::Menu => menu() || call("ContextMenu"),
        };
    });
}

/// Tell the item's menu that `entry` was clicked.
pub fn menu_event(item: &str, entry: i32) {
    let Some(conn) = CONN.lock().ok().and_then(|c| c.clone()) else { return };
    let Some(addr) = ADDRS.lock().ok().and_then(|a| a.as_ref().and_then(|a| a.get(item).cloned())) else { return };
    let Some(path) = addr.menu else { return };
    let _ = thread::Builder::new().name("nimbus-tray-menu".into()).stack_size(128 * 1024).spawn(move || {
        let data = Value::from(0i32);
        let r = conn.call_method(Some(addr.bus.as_str()), path.as_str(), Some(MENU), "Event", &(entry, "clicked", data, 0u32));
        if let Err(e) = r {
            eprintln!("nimbus: tray menu event: {e}");
        }
    });
}

fn layout(conn: &Connection, bus: &str, path: &str) -> Option<Vec<MenuEntry>> {
    let _ = conn.call_method(Some(bus), path, Some(MENU), "AboutToShow", &(0i32,));
    let names: Vec<&str> = vec![];
    let m = conn.call_method(Some(bus), path, Some(MENU), "GetLayout", &(0i32, -1i32, names)).ok()?;
    let (_rev, root) = m.body().deserialize::<(u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>))>().ok()?;
    let mut budget = 300;
    Some(root.2.iter().filter_map(|c| node(c, 0, &mut budget)).collect())
}

fn node(v: &Value, depth: u32, budget: &mut i32) -> Option<MenuEntry> {
    *budget -= 1;
    if *budget < 0 || depth > 4 {
        return None;
    }
    let Value::Structure(s) = unwrap_v(v) else { return None };
    let f = s.fields();
    let (Some(Value::I32(id)), Some(Value::Dict(props)), Some(Value::Array(kids))) = (f.first(), f.get(1), f.get(2)) else { return None };
    let mut label = String::new();
    let mut enabled = true;
    let mut visible = true;
    let mut separator = false;
    let mut toggle_type = String::new();
    let mut toggle_state = -1i32;
    for (k, v) in props.iter() {
        let Some(k) = v_str(k) else { continue };
        let v = unwrap_v(v);
        match (k.as_str(), v) {
            ("label", Value::Str(s)) => label = s.to_string(),
            ("enabled", Value::Bool(b)) => enabled = *b,
            ("visible", Value::Bool(b)) => visible = *b,
            ("type", Value::Str(s)) => separator = s.as_str() == "separator",
            ("toggle-type", Value::Str(s)) => toggle_type = s.to_string(),
            ("toggle-state", Value::I32(i)) => toggle_state = *i,
            _ => {}
        }
    }
    if !visible {
        return None;
    }
    let children = kids.iter().filter_map(|c| node(c, depth + 1, budget)).collect();
    Some(MenuEntry {
        id: *id,
        label: strip_mnemonic(&label).chars().take(120).collect(),
        enabled,
        separator,
        toggle: (!toggle_type.is_empty()).then_some(toggle_state == 1),
        children,
    })
}

/// "_Quit" → "Quit", "Save __as" → "Save _as".
fn strip_mnemonic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '_' {
            if it.peek() == Some(&'_') {
                out.push('_');
                it.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `nimbus tray-dump [secs]`: run the host for a while and print the items
/// it sees (and their menus). Becomes the watcher only if none exists.
pub fn dump(secs: f32) {
    let mut ev: calloop::EventLoop<'static, ()> = calloop::EventLoop::try_new().expect("event loop");
    let (tx, rx) = calloop::channel::channel::<Fact>();
    ev.handle()
        .insert_source(rx, move |e, _, _| {
            let calloop::channel::Event::Msg(Fact::Tray(v)) = e else { return };
            println!("tray: {} item(s)", v.len());
            for i in v.iter() {
                let a = ADDRS.lock().ok().and_then(|a| a.as_ref().and_then(|a| a.get(&i.id).cloned()));
                println!(
                    "  {:<50} {:?} attention={} icon={} menu={:?}",
                    i.id,
                    i.title,
                    i.attention,
                    i.icon.as_ref().map_or("-".to_string(), |x| format!("{}×{}", x.w, x.h)),
                    a.and_then(|a| a.menu)
                );
            }
        })
        .expect("channel");
    let until = std::time::Instant::now() + Duration::from_secs_f32(secs);
    let tx2 = tx.clone();
    let _ = thread::Builder::new().name("nimbus-tray".into()).spawn(move || {
        if let Err(e) = run(&tx2, Some(until)) {
            eprintln!("tray-dump: {e}");
        }
    });
    while std::time::Instant::now() < until {
        let _ = ev.dispatch(Some(until.saturating_duration_since(std::time::Instant::now())), &mut ());
    }
    // Menus, read-only (GetLayout): nothing is clicked.
    if let (Some(conn), Some(addrs)) = (CONN.lock().ok().and_then(|c| c.clone()), ADDRS.lock().ok().and_then(|a| a.clone())) {
        for (id, a) in addrs {
            if let Some(p) = &a.menu {
                let n = layout(&conn, &a.bus, p).map(|v| v.iter().map(|e| if e.separator { "—".to_string() } else { e.label.clone() }).collect::<Vec<_>>());
                println!("  menu of {id}: {n:?}");
            }
        }
    }
    drop(tx);
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
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_split() {
        assert_eq!(split(":1.42/org/ayatana/NotificationItem/x"), (":1.42".into(), "/org/ayatana/NotificationItem/x".into()));
        assert_eq!(split("org.kde.StatusNotifierItem-1-1"), ("org.kde.StatusNotifierItem-1-1".into(), "/StatusNotifierItem".into()));
    }

    #[test]
    fn mnemonics() {
        assert_eq!(strip_mnemonic("_Quit"), "Quit");
        assert_eq!(strip_mnemonic("Save __as"), "Save _as");
    }

    #[test]
    fn pixmaps_are_checked() {
        // 2×2 ARGB, opaque red.
        let px: Vec<u8> = [255u8, 255, 0, 0].repeat(4);
        let good = Value::from(vec![(2i32, 2i32, px.clone())]);
        let img = pixmap(&good).expect("valid pixmap");
        assert_eq!((img.w, img.h), (ICON_PX, ICON_PX));
        assert_eq!(&img.rgba[(16 * ICON_PX as usize + 16) * 4..][..4], &[255, 0, 0, 255]);
        // Claims 64×64 with 16 bytes of data: rejected.
        let bad = Value::from(vec![(64i32, 64i32, px)]);
        assert!(pixmap(&bad).is_none());
    }
}
