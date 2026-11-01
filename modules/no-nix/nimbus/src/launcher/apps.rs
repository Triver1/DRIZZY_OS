//! The app catalog: freedesktop `.desktop` entries, an icon-file index and
//! launch counts. Everything here runs on worker threads; the result is
//! cached in a module-level static across opens and only rebuilt when one
//! of the scanned directories changed (mtime, or where its path resolves
//! to — NixOS swaps `/run/current-system` symlinks into mtime-1 stores).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[derive(Debug, Clone)]
pub struct App {
    /// Desktop file id (`org.gnome.Nautilus.desktop`).
    pub id: String,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub icon: String,
    /// Argv with field codes stripped.
    pub exec: Vec<String>,
    pub terminal: bool,
    pub path: Option<String>,
    /// Lower-cased search fields: name, generic, keywords, exec basename.
    pub hay: [String; 4],
}

pub struct Catalog {
    /// Sorted by name.
    pub apps: Vec<App>,
    /// Icon name → best file.
    icons: HashMap<String, PathBuf>,
    stamps: Vec<(PathBuf, Option<(PathBuf, SystemTime)>)>,
}

impl Catalog {
    /// A copy without the first and last app (demo: a rebuild that
    /// removes apps while the window is open).
    pub fn trimmed(&self) -> Catalog {
        let n = self.apps.len();
        let apps = self.apps.iter().enumerate().filter(|(i, _)| *i != 0 && *i + 1 != n).map(|(_, a)| a.clone()).collect();
        Catalog { apps, icons: self.icons.clone(), stamps: self.stamps.clone() }
    }

    pub fn icon_path(&self, name: &str) -> Option<PathBuf> {
        if name.starts_with('/') {
            let p = Path::new(name);
            return p.exists().then(|| p.to_path_buf());
        }
        let n = name.trim_end_matches(".png").trim_end_matches(".svg").trim_end_matches(".xpm");
        self.icons.get(n).cloned()
    }
}

static CACHE: Mutex<Option<Arc<Catalog>>> = Mutex::new(None);

/// The catalog from an earlier open, if any (may be stale; see `fresh`).
pub fn cached() -> Option<Arc<Catalog>> {
    CACHE.lock().ok()?.clone()
}

/// A catalog that matches the disk: the cached one when nothing changed,
/// else a rescan (stored for the next open). Blocking: call from a worker.
/// Returns `None` when the cached catalog `have` is still current.
pub fn fresh(have: Option<&Arc<Catalog>>) -> Option<Arc<Catalog>> {
    load_launches();
    if let Some(c) = have
        && c.stamps.iter().all(|(d, s)| stamp(d) == *s)
    {
        return None;
    }
    let c = Arc::new(scan());
    if let Ok(mut g) = CACHE.lock() {
        *g = Some(c.clone());
    }
    Some(c)
}

fn stamp(d: &Path) -> Option<(PathBuf, SystemTime)> {
    let m = std::fs::metadata(d).ok()?;
    Some((std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf()), m.modified().ok()?))
}

fn data_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(h) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    {
        v.push(h);
    }
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in sys.split(':').filter(|d| !d.is_empty()) {
        let p = PathBuf::from(d);
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

fn desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_else(|_| "niri".into())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn scan() -> Catalog {
    let dirs = data_dirs();
    let current = desktops();
    let lang = std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .map(|l| l.split(['.', '@']).next().unwrap_or("").to_string())
        .filter(|l| !l.is_empty() && l != "C" && l != "POSIX");
    let mut seen: HashMap<String, Option<App>> = HashMap::new();
    let mut stamps = vec![];
    for base in &dirs {
        let root = base.join("applications");
        let mut stack = vec![(root.clone(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            stamps.push((dir.clone(), stamp(&dir)));
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if p.is_dir() {
                    stack.push((p, format!("{prefix}{name}-")));
                    continue;
                }
                if !name.ends_with(".desktop") {
                    continue;
                }
                let id = format!("{prefix}{name}");
                if seen.contains_key(&id) {
                    continue; // an earlier dir (higher priority) owns this id
                }
                let app = std::fs::read_to_string(&p).ok().and_then(|s| parse(&s, &id, &current, lang.as_deref()));
                seen.insert(id, app);
            }
        }
    }
    let mut apps: Vec<App> = seen.into_values().flatten().filter(|a| !replaced(a)).collect();
    apps.extend(builtins());
    // Same name + same command from two packages: keep one.
    apps.sort_by(|a, b| a.hay[0].cmp(&b.hay[0]).then(a.id.cmp(&b.id)));
    apps.dedup_by(|a, b| a.hay[0] == b.hay[0] && a.exec == b.exec);
    Catalog { apps, icons: icon_index(&dirs), stamps }
}

/// Apps a nimbus window replaces: GNOME Files stays installed (it is the
/// fallback when nimbus is down) but never shows up next to our Files.
fn replaced(a: &App) -> bool {
    a.id.to_ascii_lowercase().contains("nautilus") || a.exec.iter().take(3).any(|x| x.rsplit('/').next() == Some("nautilus"))
}

/// nimbus's own windows, launched like apps (`nimbus msg <window>`).
const BUILTINS: [(&str, &str, &str, &str, &str); 6] = [
    // (id, name, comment, keywords, icon svg)
    ("wallpapers", "Wallpapers", "Pick the desktop wallpaper", "background desktop picture theme", include_str!("../../assets/apps/nimbus-wallpapers.svg")),
    ("files", "Files", "Browse your home folder", "file manager browser explorer finder folders documents nautilus", include_str!("../../assets/apps/nimbus-files.svg")),
    ("monitor", "System Monitor", "Processes, CPU and memory", "task manager processes cpu memory ram top htop kill", include_str!("../../assets/apps/nimbus-monitor.svg")),
    ("prefs", "Settings", "Appearance, sound, display, network and power", "preferences control center configuration", include_str!("../../assets/apps/nimbus-settings.svg")),
    ("notifications", "Notifications", "Notification history", "alerts messages history", include_str!("../../assets/apps/nimbus-notifications.svg")),
    ("lock", "Lock Screen", "Lock the session", "lock screen security away", include_str!("../../assets/apps/nimbus-lock.svg")),
];

fn builtins() -> Vec<App> {
    let Some(exe) = std::env::current_exe().ok().map(|p| {
        let s = p.to_string_lossy().into_owned();
        s.strip_suffix(" (deleted)").map(str::to_string).unwrap_or(s)
    }) else {
        return vec![];
    };
    // Icons live in the binary; the launcher loads icons from files.
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|d| d.join("nimbus/apps"));
    if let Some(d) = &dir {
        let _ = std::fs::create_dir_all(d);
    }
    BUILTINS
        .iter()
        .map(|&(cmd, name, comment, keywords, svg)| {
            let icon = dir.as_ref().map_or(String::new(), |d| {
                let f = d.join(format!("nimbus-{cmd}.svg"));
                if std::fs::read_to_string(&f).ok().as_deref() != Some(svg) {
                    let _ = std::fs::write(&f, svg);
                }
                f.to_string_lossy().into_owned()
            });
            let exec = if cmd == "lock" { vec![exe.clone(), "lock".into()] } else { vec![exe.clone(), "msg".into(), cmd.into()] };
            App {
                id: format!("nimbus-{cmd}"),
                name: name.into(),
                generic: "nimbus".into(),
                comment: comment.into(),
                icon,
                exec,
                terminal: false,
                path: None,
                hay: [name.to_lowercase(), "nimbus".into(), keywords.into(), format!("nimbus {cmd}")],
            }
        })
        .collect()
}

fn parse(src: &str, id: &str, current: &[String], lang: Option<&str>) -> Option<App> {
    let mut in_entry = false;
    let mut f: HashMap<&str, &str> = HashMap::new();
    let (mut lname, mut lcomment, mut lgeneric, mut lkeys) = (None, None, None, None);
    let short = lang.map(|l| l.split('_').next().unwrap_or(l));
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if in_entry {
                break;
            }
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if let Some((key, loc)) = k.split_once('[') {
            let loc = loc.trim_end_matches(']');
            let hit = lang == Some(loc) || (short == Some(loc) && lang != Some(loc));
            if hit {
                match key {
                    "Name" => lname = Some(v),
                    "Comment" => lcomment = Some(v),
                    "GenericName" => lgeneric = Some(v),
                    "Keywords" => lkeys = Some(v),
                    _ => {}
                }
            }
            continue;
        }
        f.entry(k).or_insert(v);
    }
    let yes = |k: &str| f.get(k).is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if f.get("Type").copied() != Some("Application") || yes("NoDisplay") || yes("Hidden") {
        return None;
    }
    let list = |k: &str| -> Vec<String> {
        f.get(k).map(|v| v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_ascii_lowercase()).collect()).unwrap_or_default()
    };
    let only = list("OnlyShowIn");
    if !only.is_empty() && !only.iter().any(|d| current.contains(d)) {
        return None;
    }
    if list("NotShowIn").iter().any(|d| current.contains(d)) {
        return None;
    }
    if let Some(t) = f.get("TryExec")
        && !in_path(&unescape(t))
    {
        return None;
    }
    let name = unescape(lname.or(f.get("Name").copied())?);
    let exec = split_exec(&unescape(f.get("Exec")?), &name);
    if exec.is_empty() {
        return None;
    }
    let generic = lgeneric.or(f.get("GenericName").copied()).map(unescape).unwrap_or_default();
    let comment = lcomment.or(f.get("Comment").copied()).map(unescape).unwrap_or_default();
    let keywords: Vec<String> = lkeys
        .or(f.get("Keywords").copied())
        .map(|k| k.split(';').filter(|s| !s.is_empty()).map(unescape).collect())
        .unwrap_or_default();
    let bin = exec
        .iter()
        .find(|a| !a.contains('=') && *a != "env")
        .map(|a| a.rsplit('/').next().unwrap_or(a).to_lowercase())
        .unwrap_or_default();
    let stem = id.trim_end_matches(".desktop").rsplit('.').next().unwrap_or("").to_lowercase();
    let hay = [
        name.to_lowercase(),
        generic.to_lowercase(),
        keywords.join(" ").to_lowercase(),
        if bin == stem { bin } else { format!("{bin} {stem}") },
    ];
    Some(App {
        id: id.to_string(),
        name,
        generic,
        comment,
        icon: f.get("Icon").map(|s| unescape(s)).unwrap_or_default(),
        exec,
        terminal: yes("Terminal"),
        path: f.get("Path").map(|s| unescape(s)).filter(|s| !s.is_empty()),
        hay,
    })
}

fn in_path(bin: &str) -> bool {
    if bin.contains('/') {
        return Path::new(bin).exists();
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).exists()))
}

/// Desktop-entry string escapes (`\s \n \t \r \\`).
fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('s') => o.push(' '),
            Some('n') => o.push('\n'),
            Some('t') => o.push('\t'),
            Some('r') => o.push('\r'),
            Some('\\') => o.push('\\'),
            Some(x) => {
                o.push('\\');
                o.push(x);
            }
            None => o.push('\\'),
        }
    }
    o
}

/// Split an Exec value into argv (spec quoting) and drop field codes.
pub fn split_exec(s: &str, name: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![];
    let mut cur = String::new();
    let mut have = false;
    let mut quoted = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                have = true;
            }
            '\\' if quoted => {
                if let Some(&n) = it.peek() {
                    it.next();
                    cur.push(n);
                }
            }
            '\\' => {
                // Unquoted escapes are leftovers of the string-level pass.
                if let Some(&n) = it.peek() {
                    it.next();
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if have || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => cur.push(c),
        }
    }
    if have || !cur.is_empty() {
        args.push(cur);
    }
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        match a.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%i" | "%k" | "%v" | "%m" => continue,
            "%c" => {
                out.push(name.to_string());
                continue;
            }
            _ => {}
        }
        // Field codes inside an argument: drop them, keep `%%` as `%`.
        let mut o = String::with_capacity(a.len());
        let mut cs = a.chars();
        while let Some(c) = cs.next() {
            if c == '%' {
                match cs.next() {
                    Some('%') => o.push('%'),
                    Some('c') => o.push_str(name),
                    _ => {}
                }
            } else {
                o.push(c);
            }
        }
        if !o.is_empty() {
            out.push(o);
        }
    }
    out
}

const THEMES: [&str; 3] = ["hicolor", "Adwaita", "breeze"];
const CATS: [&str; 6] = ["apps", "devices", "status", "places", "categories", "mimetypes"];

/// Name → best icon file, built from directory listings (a few hundred
/// `readdir`s instead of thousands of `stat`s per icon).
fn icon_index(dirs: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut best: HashMap<String, (i32, PathBuf)> = HashMap::new();
    let mut offer = |name: &str, rank: i32, p: PathBuf| {
        let Some(stem) = name.strip_suffix(".svg").or_else(|| name.strip_suffix(".png")) else { return };
        match best.get(stem) {
            Some((r, _)) if *r >= rank => {}
            _ => {
                best.insert(stem.to_string(), (rank, p));
            }
        }
    };
    for base in dirs {
        for theme in THEMES {
            let Ok(sizes) = std::fs::read_dir(base.join("icons").join(theme)) else { continue };
            // hicolor is the fallback theme for app icons: rank it highest.
            let tb = if theme == "hicolor" { 2 } else { 0 };
            for sz in sizes.flatten() {
                let sname = sz.file_name().to_string_lossy().into_owned();
                let rank = match sname.as_str() {
                    "scalable" => 4096,
                    "symbolic" => -100,
                    s => s.split(['x', '@']).next().and_then(|n| n.parse().ok()).unwrap_or(0),
                } * 4
                    + tb;
                for cat in CATS {
                    let Ok(files) = std::fs::read_dir(sz.path().join(cat)) else { continue };
                    for f in files.flatten() {
                        offer(&f.file_name().to_string_lossy(), rank, f.path());
                    }
                }
            }
        }
        if let Ok(files) = std::fs::read_dir(base.join("pixmaps")) {
            for f in files.flatten() {
                offer(&f.file_name().to_string_lossy(), 1, f.path());
            }
        }
    }
    best.into_iter().map(|(k, (_, p))| (k, p)).collect()
}

// ── launch counts ───────────────────────────────────────────────────────

/// id → (launches, last launch unix secs).
static LAUNCHES: Mutex<Option<HashMap<String, (u32, u64)>>> = Mutex::new(None);

fn launches_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("nimbus").join("launches"))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn load_launches() {
    let Ok(mut g) = LAUNCHES.lock() else { return };
    if g.is_some() {
        return;
    }
    let mut m = HashMap::new();
    if let Some(s) = launches_file().and_then(|p| std::fs::read_to_string(p).ok()) {
        for l in s.lines() {
            let mut it = l.splitn(3, '\t');
            if let (Some(c), Some(t), Some(id)) = (it.next(), it.next(), it.next())
                && let (Ok(c), Ok(t)) = (c.parse(), t.parse())
            {
                m.insert(id.to_string(), (c, t));
            }
        }
    }
    *g = Some(m);
}

/// Frecency weight per app (0 = never launched): log count, boosted when
/// recent.
pub fn frecency(apps: &[App]) -> Vec<f32> {
    let g = LAUNCHES.lock().ok();
    let Some(m) = g.as_ref().and_then(|g| g.as_ref()) else { return vec![0.0; apps.len()] };
    let now = now_secs();
    apps.iter()
        .map(|a| match m.get(&a.id) {
            Some(&(c, t)) => {
                let age_days = now.saturating_sub(t) as f32 / 86400.0;
                (1.0 + c as f32).ln() * (1.0 + 1.5 / (1.0 + age_days * 0.5))
            }
            None => 0.0,
        })
        .collect()
}

/// Count a launch and write the file (blocking: call from a worker).
pub fn record(id: &str) {
    load_launches();
    let body = {
        let Ok(mut g) = LAUNCHES.lock() else { return };
        let m = g.get_or_insert_with(HashMap::new);
        let e = m.entry(id.to_string()).or_insert((0, 0));
        e.0 = e.0.saturating_add(1);
        e.1 = now_secs();
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
        v.iter().map(|(id, (c, t))| format!("{c}\t{t}\t{id}\n")).collect::<String>()
    };
    let Some(p) = launches_file() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// The argv to run: through the terminal for `Terminal=true`, and via
/// `cd` when the entry names a working directory.
pub fn argv(app: &App) -> Vec<String> {
    let mut v = if app.terminal {
        let mut t = vec!["ghostty".to_string(), "-e".to_string()];
        t.extend(app.exec.iter().cloned());
        t
    } else {
        app.exec.clone()
    };
    if let Some(dir) = &app.path {
        let mut w = vec!["sh".into(), "-c".into(), "cd \"$0\" && exec \"$@\"".into(), dir.clone()];
        w.append(&mut v);
        v = w;
    }
    v
}

/// Spawn through niri (so the app is niri's child, gets activation, …);
/// fall back to a detached shell. Blocking: call from a worker.
pub fn spawn(argv: Vec<String>) {
    use niri_ipc::{Action, Request, Response, socket::Socket};
    let ok = Socket::connect()
        .ok()
        .and_then(|mut s| s.send(Request::Action(Action::Spawn { command: argv.clone() })).ok())
        .is_some_and(|r| matches!(r, Ok(Response::Handled)));
    if !ok {
        let _ = std::process::Command::new("setsid")
            .args(["-f", "sh", "-c", "exec \"$@\"", "sh"])
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| c.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_field_codes() {
        assert_eq!(split_exec("firefox %u", "Firefox"), vec!["firefox"]);
        assert_eq!(split_exec("\"/opt/my app/run\" --x=%f -y", "A"), vec!["/opt/my app/run", "--x=", "-y"]);
        assert_eq!(split_exec("sh -c \"echo \\\"hi\\\" 100%%\"", "A"), vec!["sh", "-c", "echo \"hi\" 100%"]);
        assert_eq!(split_exec("app --name %c %i", "Name"), vec!["app", "--name", "Name"]);
    }

    fn entry(id: &str, exec: &str) -> Option<App> {
        parse(&format!("[Desktop Entry]\nType=Application\nName=Files\nExec={exec}\n"), id, &[], None)
    }

    #[test]
    fn nautilus_is_replaced() {
        assert!(replaced(&entry("org.gnome.Nautilus.desktop", "nautilus --new-window %U").unwrap()));
        // By its command alone, under any id, however it is spelled.
        assert!(replaced(&entry("my-files.desktop", "/nix/store/abc-nautilus-50/bin/nautilus %U").unwrap()));
        assert!(replaced(&entry("x.desktop", "env GDK_BACKEND=wayland nautilus").unwrap()));
        assert!(!replaced(&entry("org.kde.dolphin.desktop", "dolphin %u").unwrap()));
        // An app merely named after it, with a file of that name, stays.
        assert!(!replaced(&entry("gimp.desktop", "gimp --new a b c nautilus").unwrap()));
    }

    #[test]
    fn our_files_answers_its_keywords() {
        let (cmd, name, _, keywords, _) = BUILTINS.iter().find(|b| b.0 == "files").copied().unwrap();
        let app = App {
            id: format!("nimbus-{cmd}"),
            name: name.into(),
            generic: "nimbus".into(),
            comment: String::new(),
            icon: String::new(),
            exec: vec!["nimbus".into()],
            terminal: false,
            path: None,
            hay: [name.to_lowercase(), "nimbus".into(), keywords.into(), format!("nimbus {cmd}")],
        };
        for q in ["files", "file", "fil", "nautilus", "explorer", "finder", "folders", "documents"] {
            assert_eq!(super::super::fuzzy::rank(std::slice::from_ref(&app), &[0.0], q), vec![0], "{q}");
        }
    }
}
