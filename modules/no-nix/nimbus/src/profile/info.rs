//! What the Profile page says about the machine. Everything is read from
//! the machine, once, off the UI thread; what can't be read is left out,
//! never guessed. `gather` takes the filesystem root so tests can give it a
//! made-up one.

use std::{
    ffi::CString,
    os::unix::ffi::OsStrExt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Vendors' placeholders for "nobody filled this in".
const DMI_JUNK: [&str; 9] =
    ["to be filled by o.e.m.", "to be filled by o.e.m", "default string", "system product name", "system manufacturer", "not specified", "not applicable", "none", "unknown"];

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAY: u64 = 86_400;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub login: String,
    pub host: Option<String>,
    pub device: Option<String>,
    pub os: Option<String>,
    /// NixOS system generation.
    pub generation: Option<u32>,
    pub kernel: Option<String>,
    pub cpu: Option<String>,
    pub gpu: Option<String>,
    pub memory: Option<String>,
    pub disk: Option<String>,
    pub uptime: Option<u64>,
    /// "14 July 2025 · 475 days".
    pub since: Option<String>,
    pub days_here: Option<u64>,
    /// When today is the install's anniversary: its age in years.
    pub birthday: Option<u32>,
    pub wallpaper: Option<String>,
    /// Local hour.
    pub hour: Option<u32>,
}

fn read(root: &Path, rel: &str) -> Option<String> {
    let s = std::fs::read_to_string(root.join(rel)).ok()?;
    let s = s.trim().trim_matches('\0').trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn dmi(root: &Path, field: &str) -> Option<String> {
    read(root, &format!("sys/class/dmi/id/{field}")).filter(|v| !DMI_JUNK.contains(&v.to_ascii_lowercase().as_str()))
}

/// "M5606UA_M5606UA" → "M5606UA": some firmware repeats the model.
fn drop_echo(s: &str) -> String {
    let (head, last) = s.rsplit_once(' ').unwrap_or(("", s));
    match last.split_once('_') {
        Some((a, b)) if a == b && head.is_empty() => a.to_string(),
        Some((a, b)) if a == b => format!("{head} {a}"),
        _ => s.to_string(),
    }
}

/// "LENOVO" → "Lenovo", "Dell Inc." → "Dell", leaving "HP" be.
fn vendor_word(v: &str) -> String {
    let w = v.split_whitespace().next().unwrap_or("").trim_end_matches([',', '.']);
    if w.len() > 3 && w.chars().all(|c| c.is_ascii_uppercase()) {
        let mut c = w.chars();
        c.next().map_or(String::new(), |f| f.to_string() + &c.as_str().to_ascii_lowercase())
    } else {
        w.to_string()
    }
}

fn device(root: &Path) -> Option<String> {
    let product = dmi(root, "product_name").map(|p| drop_echo(&p));
    let family = dmi(root, "product_family");
    // Lenovo keeps the marketing name in the version, and a code in the name.
    let code = |s: &str| !s.contains(' ');
    let friendly = |s: &String| s.contains(' ');
    let name = match product {
        Some(p) if code(&p) => dmi(root, "product_version").filter(friendly).or_else(|| family.filter(friendly)).or(Some(p)),
        Some(p) => Some(p),
        None => family,
    };
    let Some(name) = name else { return read(root, "proc/device-tree/model") };
    let vendor = dmi(root, "sys_vendor").map(|v| vendor_word(&v)).unwrap_or_default();
    let first = name.split_whitespace().next().unwrap_or("");
    let known = vendor.is_empty() || vendor.to_ascii_lowercase().starts_with(&first.to_ascii_lowercase()) || first.to_ascii_lowercase().starts_with(&vendor.to_ascii_lowercase());
    Some(if known { name } else { format!("{vendor} {name}") })
}

fn os_release(root: &Path) -> Option<String> {
    let text = read(root, "etc/os-release").or_else(|| read(root, "usr/lib/os-release"))?;
    let field = |k: &str| {
        text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).filter(|v| !v.is_empty())
    };
    field("PRETTY_NAME").or_else(|| field("NAME"))
}

/// The number in /nix/var/nix/profiles/system → system-437-link.
fn generation(root: &Path) -> Option<u32> {
    let link = std::fs::read_link(root.join("nix/var/nix/profiles/system")).ok()?;
    link.file_name()?.to_str()?.strip_prefix("system-")?.strip_suffix("-link")?.parse().ok()
}

fn tidy_cpu(model: &str) -> String {
    let mut s = model.replace("(R)", "").replace("(TM)", "").replace("(tm)", "");
    // Everything after these says nothing the name doesn't.
    for cut in [" w/ ", " with ", " @ ", " CPU", " Processor"] {
        if let Some(i) = s.find(cut) {
            s.truncate(i);
        }
    }
    let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // "AMD Ryzen 7 5800X 8-Core"
    if let Some((head, last)) = s.rsplit_once(' ')
        && last.ends_with("-Core")
    {
        s = head.to_string();
    }
    s
}

fn cpu(root: &Path) -> Option<String> {
    let text = read(root, "proc/cpuinfo")?;
    let value = |k: &str| {
        text.lines().find_map(|l| {
            let (a, b) = l.split_once(':')?;
            (a.trim() == k).then(|| b.trim().to_string()).filter(|v| !v.is_empty())
        })
    };
    let model = value("model name").or_else(|| value("Model")).or_else(|| value("Hardware"))?;
    let threads = text.lines().filter(|l| l.split(':').next().is_some_and(|k| k.trim() == "processor")).count();
    let name = tidy_cpu(&model);
    Some(if threads > 1 { format!("{name} · {threads} threads") } else { name })
}

/// GiB with one decimal below 100.
fn gib(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if g >= 99.5 { format!("{g:.0} GiB") } else { format!("{g:.1} GiB") }
}

fn memory(root: &Path) -> Option<String> {
    let text = read(root, "proc/meminfo")?;
    let kb: u64 = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.split_whitespace().next()?.parse().ok()?;
    Some(gib(kb * 1024))
}

fn disk(root: &Path) -> Option<String> {
    let c = CString::new(root.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let block = s.f_frsize as u64;
    let total = s.f_blocks as u64 * block;
    if total == 0 {
        return None;
    }
    let free = s.f_bavail as u64 * block;
    let used = (s.f_blocks as u64).saturating_sub(s.f_bfree as u64) * block;
    Some(format!("{} used · {} free", gib(used), gib(free)))
}

fn on_tmpfs(p: &Path) -> bool {
    const TMPFS_MAGIC: i64 = 0x0102_1994;
    let Ok(c) = CString::new(p.as_os_str().as_bytes()) else { return false };
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    unsafe { libc::statfs(c.as_ptr(), &mut s) == 0 && s.f_type as i64 == TMPFS_MAGIC }
}

/// When this install began, seconds since the epoch: the root filesystem's
/// birth, else /nix's or the machine id's where / is rebuilt on every boot.
fn born(root: &Path) -> Option<u64> {
    ["", "nix", "etc/machine-id"].iter().find_map(|rel| {
        let p = root.join(rel);
        let t = std::fs::metadata(&p).ok()?.created().ok()?;
        let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
        (secs > 365 * DAY && !on_tmpfs(&p)).then_some(secs)
    })
}

/// (year, month 1..=12, day, hour) in local time.
fn local(secs: u64) -> Option<(i32, u32, u32, u32)> {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    Some((tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32, tm.tm_hour as u32))
}

fn date(y: i32, m: u32, d: u32) -> String {
    format!("{d} {} {y}", MONTHS.get(m.saturating_sub(1) as usize).copied().unwrap_or(""))
}

fn plural(n: u64, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// "3 days, 4 hours" / "4 hours, 12 min" / "12 min".
pub fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / DAY, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 {
        return if h > 0 { format!("{}, {}", plural(d, "day"), plural(h, "hour")) } else { plural(d, "day") };
    }
    if h > 0 {
        return if m > 0 { format!("{}, {m} min", plural(h, "hour")) } else { plural(h, "hour") };
    }
    format!("{m} min")
}

/// What the wallpaper's file is called, spaced out.
pub fn wallpaper_name(p: &Path) -> Option<String> {
    let s = p.file_stem()?.to_string_lossy();
    let s: String = s.chars().map(|c| if c == '-' || c == '_' { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// Read everything under `root`, leaving out what isn't there.
pub fn gather(root: &Path, wallpaper: Option<&Path>, gpu: Option<&str>) -> Info {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let today = local(now);
    let mut i = Info {
        host: read(root, "proc/sys/kernel/hostname").or_else(|| read(root, "etc/hostname")),
        device: device(root),
        os: os_release(root),
        generation: generation(root),
        kernel: read(root, "proc/sys/kernel/osrelease"),
        cpu: cpu(root),
        // wgpu's name, less the driver's note: "AMD Radeon 780M Graphics".
        gpu: gpu.map(|g| g.split(" (").next().unwrap_or(g).trim().to_string()).filter(|g| !g.is_empty()),
        memory: memory(root),
        disk: disk(root),
        uptime: read(root, "proc/uptime").and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok()).map(|u| u as u64),
        wallpaper: wallpaper.and_then(wallpaper_name),
        hour: today.map(|t| t.3),
        ..Default::default()
    };
    if let Some(b) = born(root)
        && let Some((y, m, d, _)) = local(b)
    {
        let days = now.saturating_sub(b) / DAY;
        i.since = Some(format!("{} · {}", date(y, m, d), plural(days, "day")));
        i.days_here = Some(days);
        i.birthday = today.filter(|t| (t.1, t.2) == (m, d) && t.0 > y).map(|t| (t.0 - y) as u32);
    }
    i
}

impl Info {
    /// The friendly lines under the name. The first one fits the moment;
    /// the rest are for whoever clicks it.
    pub fn lines(&self, name: &str) -> Vec<String> {
        let first = name.split_whitespace().next().unwrap_or(name);
        let up_days = self.uptime.unwrap_or(0) / DAY;
        let opener = if let Some(y) = self.birthday {
            format!("This install turns {y} today. Happy birthday.")
        } else if up_days >= 60 {
            format!("{up_days} days without a reboot. The kernel sends its regards.")
        } else if up_days >= 14 {
            format!("Up {up_days} days straight, {first}. Not bad.")
        } else {
            match self.hour {
                Some(0..=4) => format!("It's late, {first}. The machine doesn't mind."),
                Some(5..=11) => format!("Good morning, {first}."),
                Some(12..=17) => format!("Good afternoon, {first}."),
                Some(_) => format!("Good evening, {first}."),
                None => format!("Hello, {first}."),
            }
        };
        let mut v = vec![opener];
        if let Some(g) = self.generation.filter(|g| *g > 1) {
            v.push(format!("{g} generations and counting."));
        }
        if let Some(d) = self.days_here.filter(|d| *d > 0) {
            v.push(format!("Together for {}.", plural(d, "day")));
        }
        if let Some(u) = self.uptime.filter(|u| *u >= 60) {
            v.push(format!("Awake for {}.", fmt_uptime(u)));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("nimbus-info-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn put(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn an_empty_root_gives_no_made_up_facts() {
        let d = dir("empty");
        let i = gather(&d.0, None, None);
        assert_eq!(i.host, None);
        assert_eq!(i.device, None);
        assert_eq!(i.os, None);
        assert_eq!(i.generation, None);
        assert_eq!(i.kernel, None);
        assert_eq!(i.cpu, None);
        assert_eq!(i.gpu, None);
        assert_eq!(i.memory, None);
        assert_eq!(i.uptime, None);
        assert_eq!(i.wallpaper, None);
    }

    #[test]
    fn a_root_that_does_not_exist_is_fine_too() {
        let d = dir("gone");
        let i = gather(&d.0.join("nothing"), None, None);
        assert_eq!(i.disk, None);
        assert_eq!(i.since, None);
    }

    #[test]
    fn reads_a_made_up_machine() {
        let d = dir("full");
        let r = &d.0;
        put(r, "proc/sys/kernel/hostname", "kettle\n");
        put(r, "proc/sys/kernel/osrelease", "6.1.0-test\n");
        put(r, "proc/uptime", "93784.21 1000.00\n");
        put(r, "proc/meminfo", "MemTotal:       16000000 kB\nMemFree:  1 kB\n");
        put(r, "proc/cpuinfo", "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-1185G7 @ 3.00GHz\n\nprocessor\t: 1\nmodel name\t: Intel(R) Core(TM) i7-1185G7 @ 3.00GHz\n");
        put(r, "etc/os-release", "NAME=NixOS\nPRETTY_NAME=\"NixOS 25.05 (Warbler)\"\nID=nixos\n");
        put(r, "sys/class/dmi/id/product_name", "XPS 13 9310\n");
        put(r, "sys/class/dmi/id/sys_vendor", "Dell Inc.\n");
        std::fs::create_dir_all(r.join("nix/var/nix/profiles")).unwrap();
        std::os::unix::fs::symlink("system-42-link", r.join("nix/var/nix/profiles/system")).unwrap();
        let i = gather(r, Some(Path::new("/w/dark-lake_2.jpg")), Some("Mesa Intel Xe (RADV FAKE)"));
        assert_eq!(i.host.as_deref(), Some("kettle"));
        assert_eq!(i.kernel.as_deref(), Some("6.1.0-test"));
        assert_eq!(i.uptime, Some(93_784));
        assert_eq!(i.memory.as_deref(), Some("15.3 GiB"));
        assert_eq!(i.cpu.as_deref(), Some("Intel Core i7-1185G7 · 2 threads"));
        assert_eq!(i.os.as_deref(), Some("NixOS 25.05 (Warbler)"));
        assert_eq!(i.device.as_deref(), Some("Dell XPS 13 9310"));
        assert_eq!(i.generation, Some(42));
        assert_eq!(i.gpu.as_deref(), Some("Mesa Intel Xe"));
        assert_eq!(i.wallpaper.as_deref(), Some("dark lake 2"));
        assert!(i.disk.as_deref().is_some_and(|s| s.contains("used") && s.contains("free")));
    }

    #[test]
    fn broken_files_are_treated_as_missing() {
        let d = dir("broken");
        let r = &d.0;
        put(r, "proc/uptime", "soon\n");
        put(r, "proc/meminfo", "MemTotal: lots\n");
        put(r, "proc/cpuinfo", "flags: abc\n");
        put(r, "etc/os-release", "ID=x\n");
        put(r, "proc/sys/kernel/hostname", "   \n");
        // Not text at all.
        std::fs::write(r.join("proc/sys/kernel/osrelease"), [0xff, 0xfe, 0x00]).unwrap();
        let i = gather(r, None, None);
        assert_eq!((i.uptime, i.memory.clone(), i.cpu.clone(), i.os.clone(), i.host.clone(), i.kernel.clone()), (None, None, None, None, None, None));
    }

    #[test]
    fn device_names_are_cleaned_up_not_invented() {
        let d = dir("dmi");
        let r = &d.0;
        let dmi_files = |name: &str, vendor: &str, family: &str, version: &str| {
            for (f, v) in [("product_name", name), ("sys_vendor", vendor), ("product_family", family), ("product_version", version)] {
                put(r, &format!("sys/class/dmi/id/{f}"), v);
            }
            device(r)
        };
        assert_eq!(dmi_files("Kettle 16 K5606UA_K5606UA", "KETTLECO COMPUTER INC.", "Kettle 16", "1.0").as_deref(), Some("Kettle 16 K5606UA"));
        assert_eq!(dmi_files("21HM0012GE", "LENOVO", "ThinkPad T14 Gen 3", "ThinkPad T14 Gen 3").as_deref(), Some("Lenovo ThinkPad T14 Gen 3"));
        assert_eq!(dmi_files("Vivobook S 16 M5606UA_M5606UA", "ASUSTeK COMPUTER INC.", "", "").as_deref(), Some("ASUSTeK Vivobook S 16 M5606UA"));
        assert_eq!(dmi_files("ASUS Vivobook 16", "ASUSTeK COMPUTER INC.", "", "").as_deref(), Some("ASUS Vivobook 16"));
        assert_eq!(dmi_files("To Be Filled By O.E.M.", "To Be Filled By O.E.M.", "Default string", "None"), None);
    }

    #[test]
    fn cpu_names_lose_the_marketing() {
        assert_eq!(tidy_cpu("AMD Ryzen 7 8845HS w/ Radeon 780M Graphics"), "AMD Ryzen 7 8845HS");
        assert_eq!(tidy_cpu("AMD Ryzen 9 5900X 12-Core Processor"), "AMD Ryzen 9 5900X");
        assert_eq!(tidy_cpu("Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz"), "Intel Core i5-8250U");
        assert_eq!(tidy_cpu("Apple M2"), "Apple M2");
    }

    #[test]
    fn units_and_times_read_well() {
        assert_eq!(gib(1 << 30), "1.0 GiB");
        assert_eq!(gib(120 * (1 << 30)), "120 GiB");
        assert_eq!(fmt_uptime(59), "0 min");
        assert_eq!(fmt_uptime(12 * 60), "12 min");
        assert_eq!(fmt_uptime(3600), "1 hour");
        assert_eq!(fmt_uptime(2 * 3600 + 5 * 60), "2 hours, 5 min");
        assert_eq!(fmt_uptime(DAY), "1 day");
        assert_eq!(fmt_uptime(3 * DAY + 4 * 3600), "3 days, 4 hours");
        assert_eq!(date(2025, 7, 14), "14 July 2025");
    }

    #[test]
    fn the_install_date_is_a_real_past_date() {
        let d = dir("born");
        let i = gather(&d.0, None, None);
        // Whatever filesystem the temp dir is on: a date, or nothing.
        if let Some(s) = i.since {
            assert!(s.contains('·'));
            assert_eq!(i.days_here, Some(0));
        }
    }

    fn at(hour: u32) -> Info {
        Info { hour: Some(hour), ..Default::default() }
    }

    #[test]
    fn the_friendly_line_fits_the_moment() {
        assert_eq!(at(9).lines("Ada Lovelace")[0], "Good morning, Ada.");
        assert_eq!(at(14).lines("Ada")[0], "Good afternoon, Ada.");
        assert_eq!(at(21).lines("Ada")[0], "Good evening, Ada.");
        assert!(at(3).lines("Ada")[0].starts_with("It's late"));
        assert_eq!(Info::default().lines("Ada")[0], "Hello, Ada.");
        let up = Info { uptime: Some(20 * DAY), hour: Some(9), ..Default::default() };
        assert!(up.lines("Ada")[0].contains("20 days"));
        let up = Info { uptime: Some(90 * DAY), hour: Some(9), ..Default::default() };
        assert!(up.lines("Ada")[0].contains("90 days"));
        let bday = Info { birthday: Some(3), uptime: Some(90 * DAY), ..Default::default() };
        assert!(bday.lines("Ada")[0].contains("turns 3"), "a birthday beats everything");
    }

    #[test]
    fn the_extra_lines_only_say_what_is_known() {
        assert_eq!(at(9).lines("Ada").len(), 1);
        let all = Info { hour: Some(9), generation: Some(437), days_here: Some(10), uptime: Some(7200), ..Default::default() };
        assert_eq!(all.lines("Ada")[1..], ["437 generations and counting.", "Together for 10 days.", "Awake for 2 hours."]);
    }
}
