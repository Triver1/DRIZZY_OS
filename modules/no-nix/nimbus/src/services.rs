//! Logic side. Each service is a blocking thread that turns the outside world
//! into `Fact`s. Facts carry *state*, never animation: the render loop
//! decides how to move. A service only sends when its snapshot changed.

use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use calloop::channel::Sender;
use niri_ipc::{
    Action, Request, WorkspaceReferenceArg,
    socket::Socket,
    state::{EventStreamState, EventStreamStatePart},
};

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceInfo {
    pub id: u64,
    pub idx: u8,
    pub output: String,
    pub active: bool,
    pub focused: bool,
    pub urgent: bool,
    pub windows: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NiriSnapshot {
    pub workspaces: Vec<WorkspaceInfo>,
    pub title: Option<String>,
    /// Focused window id: a change means focus moved, not just a retitle.
    pub focused_id: Option<u64>,
    pub app_id: Option<String>,
    pub focused_output: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockInfo {
    pub hour: u8,
    pub minute: u8,
    pub weekday: u8,
    pub day: u8,
    pub month: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatteryInfo {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NetInfo {
    Offline,
    Ethernet,
    /// Link quality 0..=100.
    Wifi(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeInfo {
    pub percent: u8,
    pub muted: bool,
}

/// State the settings panel shows, read on demand (not polled).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SystemState {
    /// 0..1
    pub brightness: Option<f32>,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    /// 0..=100
    pub signal: u8,
    pub secure: bool,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BtDev {
    pub mac: String,
    pub name: String,
    /// freedesktop icon name from BlueZ (e.g. "input-keyboard"), if any.
    pub kind: Option<String>,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioDev {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioState {
    pub sinks: Vec<AudioDev>,
    pub sources: Vec<AudioDev>,
    pub mic: Option<VolumeInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputSummary {
    pub name: String,
    pub model: String,
    pub mode: String,
    pub scale: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerDetail {
    pub percent: u8,
    pub status: String,
    /// Remaining capacity relative to design, 0..100.
    pub health: Option<u8>,
    pub cycles: Option<u32>,
    /// Hours to empty / full at the current rate.
    pub hours: Option<f32>,
    pub limit: Option<u8>,
    pub profile: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AboutInfo {
    pub niri: Option<String>,
    pub rss_kb: u64,
    pub cpu_secs: f32,
    pub uptime_secs: f32,
    pub threads: u32,
}

#[derive(Clone)]
pub enum Fact {
    Niri(NiriSnapshot),
    Clock(ClockInfo),
    Battery(Option<BatteryInfo>),
    Net(NetInfo),
    Volume(VolumeInfo),
    System(SystemState),
    WifiList(Vec<WifiNet>),
    BtList(Vec<BtDev>),
    Audio(AudioState),
    Outputs(Vec<OutputSummary>),
    Power(PowerDetail),
    About(AboutInfo),
    /// A control command (`nimbus msg <cmd>`, or a trigger file).
    Command(String),
    /// The MPRIS player the bar shows (None: nothing playing).
    Media(Option<crate::media::MediaInfo>),
    /// A decoded cover for `Media::art_url`.
    MediaArt(std::sync::Arc<crate::media::Art>),
    /// The colour icon for an app id (None: none found).
    AppIcon(String, Option<crate::gfx::ImageData>),
    /// System tray items (StatusNotifierItem), in display order.
    Tray(std::sync::Arc<Vec<crate::tray::TrayItem>>),
    /// Night light / caffeine / recording state (owned by main).
    Toggles(crate::toggles::ToggleState),
    /// Current weather (None: unavailable).
    Weather(Option<crate::weather::Weather>),
    /// Module-private payloads (launcher, monitor, wallpaper, notifications
    /// workers); each module downcasts its own types.
    Custom(std::sync::Arc<dyn std::any::Any + Send + Sync>),
}

pub fn spawn_all(tx: Sender<Fact>) {
    spawn("niri", tx.clone(), niri_stream);
    spawn("clock", tx.clone(), clock);
    spawn("power", tx.clone(), power_and_net);
    spawn("audio", tx.clone(), audio);
    crate::media::spawn(tx.clone());
    crate::tray::spawn(tx);
}

/// Benchmark mode: same inputs as the QML twin. Real clock; battery, network
/// and volume are static; niri state is synthesised by the caller.
pub fn spawn_bench(tx: Sender<Fact>) {
    for f in [
        Fact::Battery(Some(BatteryInfo { percent: 43, charging: false })),
        Fact::Net(NetInfo::Wifi(80)),
        Fact::Volume(VolumeInfo { percent: 62, muted: false }),
    ] {
        let _ = tx.send(f);
    }
    spawn("clock", tx, clock);
}

const BENCH_TITLES: [&str; 4] = [
    "nimbus — ~/NEWFLAKE/modules/no-nix/nimbus",
    "Helium — Wayland layer shell protocol",
    "yazi",
    "Spotify Premium",
];

/// Deterministic workload: 5 workspaces, the active one walks back and forth.
pub fn bench_snapshot(n: u64, output: &str) -> Fact {
    let walk = [0u8, 1, 2, 3, 4, 3, 2, 1];
    let active = walk[(n % walk.len() as u64) as usize];
    let workspaces = (0..5u8)
        .map(|i| WorkspaceInfo {
            id: 1000 + i as u64,
            idx: i + 1,
            output: output.to_string(),
            active: i == active,
            focused: i == active,
            urgent: false,
            windows: [2, 1, 0, 3, 0][i as usize],
        })
        .collect();
    Fact::Niri(NiriSnapshot {
        workspaces,
        title: Some(BENCH_TITLES[(n % 4) as usize].to_string()),
        focused_id: Some(2000 + active as u64),
        app_id: None,
        focused_output: Some(output.to_string()),
    })
}

fn spawn(name: &str, tx: Sender<Fact>, f: fn(Sender<Fact>)) {
    thread::Builder::new()
        .name(format!("nimbus-{name}"))
        .stack_size(256 * 1024)
        .spawn(move || f(tx))
        .expect("spawn service");
}

// ── niri ────────────────────────────────────────────────────────────────

fn niri_stream(tx: Sender<Fact>) {
    loop {
        if let Err(e) = niri_stream_once(&tx) {
            eprintln!("nimbus: niri stream: {e}; retrying");
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn niri_stream_once(tx: &Sender<Fact>) -> std::io::Result<()> {
    let mut sock = Socket::connect()?;
    let _ = sock.send(Request::EventStream)?;
    let mut next = sock.read_events();
    let mut state = EventStreamState::default();
    let mut last: Option<NiriSnapshot> = None;
    loop {
        let ev = next()?;
        state.apply(ev);
        let snap = snapshot(&state);
        if last.as_ref() != Some(&snap) {
            let app = snap.app_id.clone();
            let app_changed = last.as_ref().is_none_or(|l| l.app_id != app);
            last = Some(snap.clone());
            if tx.send(Fact::Niri(snap)).is_err() {
                return Ok(());
            }
            if app_changed && let Some(a) = app {
                crate::appicons::request_app(&a, tx);
            }
        }
    }
}

fn snapshot(s: &EventStreamState) -> NiriSnapshot {
    let mut counts = std::collections::HashMap::<u64, u16>::new();
    for w in s.windows.windows.values() {
        if let Some(ws) = w.workspace_id {
            *counts.entry(ws).or_default() += 1;
        }
    }
    let mut workspaces: Vec<WorkspaceInfo> = s
        .workspaces
        .workspaces
        .values()
        .map(|w| WorkspaceInfo {
            id: w.id,
            idx: w.idx,
            output: w.output.clone().unwrap_or_default(),
            active: w.is_active,
            focused: w.is_focused,
            urgent: w.is_urgent,
            windows: counts.get(&w.id).copied().unwrap_or(0),
        })
        .collect();
    workspaces.sort_by_key(|w| (w.output.clone(), w.idx));
    let focused = s.windows.windows.values().find(|w| w.is_focused);
    NiriSnapshot {
        focused_output: workspaces.iter().find(|w| w.focused).map(|w| w.output.clone()),
        workspaces,
        title: focused.and_then(|w| w.title.clone()),
        focused_id: focused.map(|w| w.id),
        app_id: focused.and_then(|w| w.app_id.clone()),
    }
}

/// Fire-and-forget niri actions, off the render thread.
#[derive(Clone)]
pub struct NiriActions(mpsc::Sender<Action>);

impl NiriActions {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Action>();
        thread::Builder::new()
            .name("nimbus-actions".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                for a in rx {
                    if let Ok(mut s) = Socket::connect() {
                        let _ = s.send(Request::Action(a));
                    }
                }
            })
            .unwrap();
        Self(tx)
    }
    pub fn focus_workspace(&self, id: u64) {
        let _ = self.0.send(Action::FocusWorkspace { reference: WorkspaceReferenceArg::Id(id) });
    }
    pub fn workspace_up(&self) {
        let _ = self.0.send(Action::FocusWorkspaceUp {});
    }
    pub fn workspace_down(&self) {
        let _ = self.0.send(Action::FocusWorkspaceDown {});
    }
}

// ── clock ───────────────────────────────────────────────────────────────

pub fn now_local() -> (ClockInfo, u32) {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (
            ClockInfo {
                hour: tm.tm_hour as u8,
                minute: tm.tm_min as u8,
                weekday: tm.tm_wday as u8,
                day: tm.tm_mday as u8,
                month: tm.tm_mon as u8,
            },
            tm.tm_sec as u32,
        )
    }
}

fn clock(tx: Sender<Fact>) {
    let mut last = None;
    loop {
        let (now, sec) = now_local();
        if last != Some(now) {
            last = Some(now);
            if tx.send(Fact::Clock(now)).is_err() {
                return;
            }
        }
        // Wake at the minute boundary, but never sleep longer than 15 s so a
        // resume from suspend (monotonic sleep doesn't count it) is caught.
        let until_minute = 60 - sec.min(59) as u64;
        thread::sleep(Duration::from_millis((until_minute * 1000).min(15_000) + 20));
    }
}

// ── battery + network (sysfs, cheap reads) ──────────────────────────────

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn battery() -> Option<BatteryInfo> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        let base = p.to_string_lossy();
        if read(&format!("{base}/type")).as_deref() == Some("Battery") {
            let percent = read(&format!("{base}/capacity"))?.parse::<u8>().ok()?;
            let status = read(&format!("{base}/status")).unwrap_or_default();
            return Some(BatteryInfo { percent, charging: status == "Charging" || status == "Full" });
        }
    }
    None
}

fn network() -> NetInfo {
    let Ok(dir) = std::fs::read_dir("/sys/class/net") else { return NetInfo::Offline };
    let mut eth = false;
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }
        let base = e.path();
        if read(&format!("{}/operstate", base.display())).as_deref() != Some("up") {
            continue;
        }
        if base.join("wireless").exists() || base.join("phy80211").exists() {
            // /proc/net/wireless: "wlan0: 0000   54.  -56.  -256 ..."
            let q = read("/proc/net/wireless")
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.trim_start().starts_with(&format!("{name}:")))
                        .and_then(|l| l.split_whitespace().nth(2).map(|v| v.trim_end_matches('.').to_string()))
                })
                .and_then(|v| v.parse::<f32>().ok())
                .map(|v| (v / 70.0 * 100.0).clamp(0.0, 100.0) as u8)
                .unwrap_or(75);
            return NetInfo::Wifi(q);
        }
        eth = true;
    }
    if eth { NetInfo::Ethernet } else { NetInfo::Offline }
}

fn power_and_net(tx: Sender<Fact>) {
    let (mut lb, mut ln) = (None, None);
    let mut tick = 0u32;
    loop {
        let n = network();
        if ln != Some(n) {
            ln = Some(n);
            if tx.send(Fact::Net(n)).is_err() {
                return;
            }
        }
        if tick % 4 == 0 {
            let b = battery();
            if lb != Some(b) {
                lb = Some(b);
                if tx.send(Fact::Battery(b)).is_err() {
                    return;
                }
            }
        }
        tick = tick.wrapping_add(1);
        thread::sleep(Duration::from_secs(5));
    }
}

// ── audio: event driven via `pactl subscribe`, no polling ───────────────

fn read_volume() -> Option<VolumeInfo> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let v: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(VolumeInfo { percent: (v * 100.0).round().clamp(0.0, 150.0) as u8, muted: s.contains("MUTED") })
}

pub fn change_volume(delta: i32) {
    let arg = if delta >= 0 { format!("{}%+", delta) } else { format!("{}%-", -delta) };
    let _ = Command::new("wpctl")
        .args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", &arg])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut c| thread::spawn(move || c.wait()));
}

pub fn toggle_mute() {
    let _ = Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        .spawn()
        .map(|mut c| thread::spawn(move || c.wait()));
}

fn audio(tx: Sender<Fact>) {
    let mut last = None;
    let emit = |last: &mut Option<VolumeInfo>| -> bool {
        if let Some(v) = read_volume()
            && *last != Some(v)
        {
            *last = Some(v);
            return tx.send(Fact::Volume(v)).is_ok();
        }
        true
    };
    emit(&mut last);
    loop {
        let child = Command::new("pactl").arg("subscribe").stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = child else {
            thread::sleep(Duration::from_secs(30));
            continue;
        };
        let out = BufReader::new(child.stdout.take().unwrap());
        for line in out.lines() {
            let Ok(line) = line else { break };
            if (line.contains(" sink ") || line.contains(" server")) && !emit(&mut last) {
                let _ = child.kill();
                return;
            }
        }
        let _ = child.wait();
        thread::sleep(Duration::from_secs(2));
    }
}

// ── settings panel: on-demand reads, coalesced writes ───────────────────

fn read_brightness() -> Option<f32> {
    let dir = std::fs::read_dir("/sys/class/backlight").ok()?.flatten().next()?.path();
    let cur: f32 = read(&format!("{}/brightness", dir.display()))?.parse().ok()?;
    let max: f32 = read(&format!("{}/max_brightness", dir.display()))?.parse().ok()?;
    (max > 0.0).then(|| (cur / max).clamp(0.0, 1.0))
}

fn output_of(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stderr(Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Read brightness / Wi-Fi / Bluetooth once, off the render thread, after
/// an optional delay (to let a toggle settle).
pub fn read_system(tx: Sender<Fact>, delay: Duration) {
    let _ = thread::Builder::new().name("nimbus-system".into()).stack_size(256 * 1024).spawn(move || {
        thread::sleep(delay);
        let wifi = output_of("nmcli", &["radio", "wifi"]).map(|s| s.trim() == "enabled");
        let bluetooth = output_of("bluetoothctl", &["show"])
            .and_then(|s| s.lines().find(|l| l.trim_start().starts_with("Powered:")).map(|l| l.contains("yes")));
        let _ = tx.send(Fact::System(SystemState { brightness: read_brightness(), wifi, bluetooth }));
    });
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut c| thread::spawn(move || c.wait()));
}

/// Applies the latest value of a continuously changing setting (a slider
/// being dragged) without spawning a process per pointer event: values
/// queue up while a command runs and only the newest is applied next.
pub struct Applier(mpsc::Sender<u8>);

impl Applier {
    pub fn new(name: &str, apply: fn(u8)) -> Self {
        let (tx, rx) = mpsc::channel::<u8>();
        thread::Builder::new()
            .name(format!("nimbus-{name}"))
            .stack_size(128 * 1024)
            .spawn(move || {
                while let Ok(mut v) = rx.recv() {
                    while let Ok(n) = rx.try_recv() {
                        v = n;
                    }
                    apply(v);
                }
            })
            .unwrap();
        Self(tx)
    }
    pub fn set(&self, v: u8) {
        let _ = self.0.send(v);
    }
}

pub fn set_volume(pct: u8) {
    let _ = Command::new("wpctl")
        .args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", &format!("{pct}%")])
        .status();
}

pub fn set_brightness(pct: u8) {
    let _ = Command::new("brightnessctl")
        .args(["--class=backlight", "-q", "set", &format!("{}%", pct.max(1))])
        .status();
}

pub fn set_wifi(on: bool) {
    run("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
}

pub fn set_bluetooth(on: bool) {
    run("bluetoothctl", &["power", if on { "on" } else { "off" }]);
}

/// The Lock buttons: nimbus's own locker (`nimbus lock`), falling back to
/// hyprlock if it can't be started or gives up without unlocking.
pub fn lock() {
    let exe = std::env::current_exe().ok().map(|p| {
        // After a rebuild the running binary reads as "<path> (deleted)";
        // the new one sits at the same path.
        let s = p.to_string_lossy();
        std::path::PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s).to_string())
    });
    let _ = thread::Builder::new().name("nimbus-lock".into()).stack_size(64 * 1024).spawn(move || {
        let ok = exe.is_some_and(|exe| {
            Command::new(exe)
                .arg("lock")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        });
        if !ok {
            eprintln!("nimbus: lock failed, falling back to hyprlock");
            let _ = Command::new("hyprlock").stdin(Stdio::null()).status();
        }
    });
}

pub fn suspend() {
    run("systemctl", &["suspend"]);
}

pub fn reboot() {
    run("systemctl", &["reboot"]);
}

pub fn power_off() {
    run("systemctl", &["poweroff"]);
}

// ── settings panel: Wi-Fi / Bluetooth lists (on demand) ────────────────

/// Split an `nmcli -t` line on unescaped ':' (SSIDs may contain "\:").
fn nmcli_fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in line.chars() {
        match (esc, c) {
            (false, '\\') => esc = true,
            (false, ':') => out.push(String::new()),
            _ => {
                out.last_mut().unwrap().push(c);
                esc = false;
            }
        }
    }
    out
}

/// Nearby networks, strongest first (connected one on top), one entry per SSID.
pub fn scan_wifi(tx: Sender<Fact>, delay: Duration) {
    let _ = thread::Builder::new().name("nimbus-wifi".into()).stack_size(256 * 1024).spawn(move || {
        thread::sleep(delay);
        let Some(out) = output_of("nmcli", &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list"]) else {
            return;
        };
        let mut nets: Vec<WifiNet> = Vec::new();
        for line in out.lines() {
            let f = nmcli_fields(line);
            if f.len() < 4 || f[1].is_empty() {
                continue;
            }
            let net = WifiNet {
                active: f[0].trim() == "*",
                ssid: f[1].clone(),
                signal: f[2].parse().unwrap_or(0),
                secure: !f[3].is_empty() && f[3] != "--",
            };
            match nets.iter_mut().find(|n| n.ssid == net.ssid) {
                Some(n) => {
                    n.active |= net.active;
                    n.signal = n.signal.max(net.signal);
                }
                None => nets.push(net),
            }
        }
        nets.sort_by_key(|n| (!n.active, std::cmp::Reverse(n.signal)));
        let _ = tx.send(Fact::WifiList(nets));
    });
}

pub fn list_bt(tx: Sender<Fact>, delay: Duration) {
    let _ = thread::Builder::new().name("nimbus-bt".into()).stack_size(256 * 1024).spawn(move || {
        thread::sleep(delay);
        let Some(out) = output_of("bluetoothctl", &["devices", "Paired"]) else { return };
        let mut devs: Vec<BtDev> = out
            .lines()
            .filter_map(|l| {
                let mut it = l.splitn(3, ' ');
                (it.next()? == "Device").then_some(())?;
                let mac = it.next()?.to_string();
                let name = it.next().unwrap_or("").to_string();
                let info = output_of("bluetoothctl", &["info", &mac]).unwrap_or_default();
                let field = |k: &str| {
                    info.lines().find_map(|l| l.trim_start().strip_prefix(k).map(|v| v.trim().to_string()))
                };
                Some(BtDev {
                    connected: field("Connected:").is_some_and(|v| v == "yes"),
                    kind: field("Icon:"),
                    name: if name.is_empty() { mac.clone() } else { name },
                    mac,
                })
            })
            .collect();
        devs.sort_by_key(|d| !d.connected);
        let _ = tx.send(Fact::BtList(devs));
    });
}

pub fn wifi_connect(ssid: &str) {
    run("nmcli", &["device", "wifi", "connect", ssid]);
}

pub fn wifi_disconnect(ssid: &str) {
    run("nmcli", &["connection", "down", "id", ssid]);
}

pub fn bt_connect(mac: &str) {
    run("bluetoothctl", &["connect", mac]);
}

pub fn bt_disconnect(mac: &str) {
    run("bluetoothctl", &["disconnect", mac]);
}

// ── settings window: on-demand reads ────────────────────────────────────

fn spawn_once(name: &str, f: impl FnOnce() + Send + 'static) {
    let _ = thread::Builder::new().name(format!("nimbus-{name}")).stack_size(256 * 1024).spawn(f);
}

/// `wpctl status` → sinks and sources (the default is marked with '*').
fn parse_wpctl_section(status: &str, section: &str) -> Vec<AudioDev> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in status.lines() {
        let t = line.trim_start_matches(|c: char| c == ' ' || c == '│' || c == '├' || c == '└' || c == '─');
        if t.starts_with(section) {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        let t = t.trim();
        if t.is_empty() || t.ends_with(':') {
            if !out.is_empty() || t.ends_with(':') {
                break;
            }
            continue;
        }
        let default = t.starts_with('*');
        let t = t.trim_start_matches('*').trim();
        let Some((id, rest)) = t.split_once(". ") else { continue };
        let Ok(id) = id.trim().parse() else { continue };
        let name = rest.split(" [vol:").next().unwrap_or(rest).trim().to_string();
        out.push(AudioDev { id, name, default });
    }
    out
}

pub fn read_audio(tx: Sender<Fact>, delay: Duration) {
    spawn_once("audio-read", move || {
        thread::sleep(delay);
        let status = output_of("wpctl", &["status"]).unwrap_or_default();
        let audio = status.split("\nVideo").next().unwrap_or("");
        let mic = output_of("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).and_then(|s| {
            let v: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
            Some(VolumeInfo { percent: (v * 100.0).round().clamp(0.0, 150.0) as u8, muted: s.contains("MUTED") })
        });
        let _ = tx.send(Fact::Audio(AudioState {
            sinks: parse_wpctl_section(audio, "Sinks:"),
            sources: parse_wpctl_section(audio, "Sources:"),
            mic,
        }));
    });
}

pub fn set_default_sink(id: u32) {
    let _ = Command::new("wpctl").args(["set-default", &id.to_string()]).status();
}

pub fn set_mic_volume(pct: u8) {
    let _ = Command::new("wpctl")
        .args(["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SOURCE@", &format!("{pct}%")])
        .status();
}

pub fn toggle_mic_mute() {
    run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]);
}

pub fn read_outputs(tx: Sender<Fact>) {
    spawn_once("outputs", move || {
        let Ok(mut sock) = Socket::connect() else { return };
        let Ok(Ok(niri_ipc::Response::Outputs(outs))) = sock.send(Request::Outputs) else { return };
        let mut v: Vec<OutputSummary> = outs
            .into_values()
            .map(|o| {
                let mode = o
                    .current_mode
                    .and_then(|i| o.modes.get(i))
                    .map(|m| format!("{}×{} @ {:.0} Hz", m.width, m.height, m.refresh_rate as f64 / 1000.0))
                    .unwrap_or_else(|| "off".into());
                OutputSummary {
                    model: format!("{} {}", o.make, o.model).trim().to_string(),
                    scale: o.logical.map_or(1.0, |l| l.scale),
                    name: o.name,
                    mode,
                }
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        let _ = tx.send(Fact::Outputs(v));
    });
}

/// Set an output's scale through niri (applies live).
pub fn set_output_scale(output: String, scale: f64) {
    spawn_once("scale", move || {
        if let Ok(mut s) = Socket::connect() {
            let _ = s.send(Request::Output {
                output,
                action: niri_ipc::OutputAction::Scale { scale: niri_ipc::ScaleToSet::Specific(scale) },
            });
        }
    });
}

pub fn read_power(tx: Sender<Fact>) {
    spawn_once("power-read", move || {
        let Some(dir) = std::fs::read_dir("/sys/class/power_supply")
            .ok()
            .and_then(|d| d.flatten().map(|e| e.path()).find(|p| read(&format!("{}/type", p.display())).as_deref() == Some("Battery")))
        else {
            return;
        };
        let f = |k: &str| read(&format!("{}/{k}", dir.display()));
        let num = |k: &str| f(k).and_then(|v| v.parse::<f64>().ok());
        let (now, full, design) = (num("charge_now").or(num("energy_now")), num("charge_full").or(num("energy_full")), num("charge_full_design").or(num("energy_full_design")));
        let rate = num("current_now").or(num("power_now")).filter(|r| *r > 0.0);
        let status = f("status").unwrap_or_default();
        let hours = match (now, full, rate) {
            (Some(n), _, Some(r)) if status == "Discharging" => Some((n / r) as f32),
            (Some(n), Some(fl), Some(r)) if status == "Charging" => Some(((fl - n).max(0.0) / r) as f32),
            _ => None,
        };
        let _ = tx.send(Fact::Power(PowerDetail {
            percent: num("capacity").unwrap_or(0.0) as u8,
            health: full.zip(design).map(|(f, d)| ((f / d) * 100.0).round().min(100.0) as u8),
            cycles: num("cycle_count").map(|c| c as u32),
            hours,
            limit: num("charge_control_end_threshold").map(|v| v as u8),
            profile: read("/sys/firmware/acpi/platform_profile"),
            status,
        }));
    });
}

pub fn read_about(tx: Sender<Fact>, start: std::time::Instant) {
    spawn_once("about", move || {
        let niri = Socket::connect().ok().and_then(|mut s| match s.send(Request::Version) {
            Ok(Ok(niri_ipc::Response::Version(v))) => Some(v),
            _ => None,
        });
        let status = read("/proc/self/status").unwrap_or_default();
        let field = |k: &str| {
            status.lines().find_map(|l| l.strip_prefix(k)).and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        };
        let stat = read("/proc/self/stat").unwrap_or_default();
        let after = stat.rsplit_once(')').map(|(_, r)| r).unwrap_or("");
        let fields: Vec<&str> = after.split_whitespace().collect();
        let tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f32;
        let cpu = fields.get(11).and_then(|u| u.parse::<f32>().ok()).unwrap_or(0.0)
            + fields.get(12).and_then(|u| u.parse::<f32>().ok()).unwrap_or(0.0);
        let _ = tx.send(Fact::About(AboutInfo {
            niri,
            rss_kb: field("VmRSS:").unwrap_or(0),
            cpu_secs: cpu / tck.max(1.0),
            uptime_secs: start.elapsed().as_secs_f32(),
            threads: field("Threads:").unwrap_or(0) as u32,
        }));
    });
}
