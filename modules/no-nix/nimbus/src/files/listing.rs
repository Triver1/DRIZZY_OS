//! Folder contents and the rail of places, read on worker threads, and the
//! small latest-wins mailbox every worker of the browser hangs off.

use std::{
    ffi::{OsStr, OsString},
    os::unix::{ffi::OsStrExt, fs::{FileTypeExt, MetadataExt}},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::{
    fuzzy,
    kind::{self, Kind},
};
use crate::services::Fact;

/// Entries taken from one folder; the rest is counted as "more".
pub const MAX_LIST: usize = 400_000;

/// A worker stuck this long (a dead network mount, a `/proc` file that
/// never answers) no longer holds up new requests.
const STUCK: Duration = Duration::from_secs(2);
const MAX_WORKERS: usize = 4;

struct Slot<T> {
    next: Option<T>,
    workers: usize,
    since: Instant,
    closed: bool,
}

/// One slot for the newest request and worker threads that exist only
/// while there is work: asking again replaces what has not started, and
/// when the slot is empty the thread is gone.
pub struct Mailbox<T> {
    slot: Arc<Mutex<Slot<T>>>,
    run: Arc<dyn Fn(T) + Send + Sync>,
    name: &'static str,
}

impl<T: Send + 'static> Mailbox<T> {
    pub fn new(name: &'static str, run: impl Fn(T) + Send + Sync + 'static) -> Self {
        Self { slot: Arc::new(Mutex::new(Slot { next: None, workers: 0, since: Instant::now(), closed: false })), run: Arc::new(run), name }
    }

    pub fn push(&self, req: T) {
        let Ok(mut s) = self.slot.lock() else { return };
        s.next = Some(req);
        if s.workers == 0 || (s.since.elapsed() > STUCK && s.workers < MAX_WORKERS) {
            s.workers += 1;
            s.since = Instant::now();
            let (slot, run) = (self.slot.clone(), self.run.clone());
            let ok = std::thread::Builder::new().name(self.name.into()).spawn(move || Self::work(slot, run)).is_ok();
            if !ok {
                s.workers -= 1;
            }
        }
    }

    fn work(slot: Arc<Mutex<Slot<T>>>, run: Arc<dyn Fn(T) + Send + Sync>) {
        loop {
            let req = {
                let Ok(mut s) = slot.lock() else { return };
                match s.next.take().filter(|_| !s.closed) {
                    Some(r) => {
                        s.since = Instant::now();
                        r
                    }
                    None => {
                        s.workers -= 1;
                        return;
                    }
                }
            };
            run(req);
        }
    }

    #[cfg(test)]
    pub fn workers(&self) -> usize {
        self.slot.lock().unwrap().workers
    }
}

impl<T> Drop for Mailbox<T> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.slot.lock() {
            s.next = None;
            s.closed = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    No,
    Link,
    BrokenLink,
    /// FIFO, socket, device: listed, never opened.
    Device,
}

pub struct Entry {
    pub name: OsString,
    /// Safe to display.
    pub shown: Box<str>,
    pub kind: Kind,
    pub special: Special,
    pub is_dir: bool,
    pub hidden: bool,
    pub size: u64,
    pub mtime: i64,
    pub mask: u32,
}

impl Entry {
    pub fn openable(&self) -> bool {
        !matches!(self.special, Special::Device | Special::BrokenLink)
    }
}

pub struct ListReq {
    pub owner: u64,
    pub generation: u64,
    pub path: PathBuf,
    /// `path` may be a file (reveal it) or gone (nearest folder). Resolved
    /// here: a stat on a dead mount must not stop the main thread.
    pub reveal: bool,
    pub home: PathBuf,
    pub tx: calloop::channel::Sender<Fact>,
}

/// A folder's contents, as `Fact::Custom`.
pub struct Listing {
    pub owner: u64,
    pub generation: u64,
    pub dir: PathBuf,
    /// What a reveal asked to select.
    pub select: Option<OsString>,
    pub entries: Mutex<Vec<Entry>>,
    /// Stopped at `MAX_LIST`.
    pub more: bool,
    pub error: Option<String>,
}

fn entry(e: &std::fs::DirEntry) -> Option<Entry> {
    let name = e.file_name();
    let ft = e.file_type().ok()?;
    let lmeta = e.metadata().ok();
    let (mut is_dir, mut special) = (ft.is_dir(), Special::No);
    let (mut size, mut mtime) = lmeta.as_ref().map_or((0, 0), |m| (m.len(), m.mtime()));
    if ft.is_symlink() {
        special = Special::Link;
        match std::fs::metadata(e.path()) {
            Ok(m) => {
                is_dir = m.is_dir();
                if m.is_file() {
                    size = m.len();
                    mtime = m.mtime();
                } else if !is_dir {
                    special = Special::Device;
                }
            }
            Err(_) => special = Special::BrokenLink,
        }
    } else if ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device() {
        special = Special::Device;
    }
    let kind = if is_dir { Kind::Folder } else { kind::of_name(&name) };
    let bytes = name.as_bytes();
    Some(Entry {
        hidden: bytes.first() == Some(&b'.'),
        shown: kind::shown(&name).into_boxed_str(),
        mask: fuzzy::mask_of(bytes),
        kind,
        special,
        is_dir,
        size,
        mtime,
        name,
    })
}

/// Read and sort one folder (folders first, natural order within).
pub fn read(dir: &Path, cap: usize) -> Result<(Vec<Entry>, bool), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => "You don't have permission to read this folder".to_string(),
        std::io::ErrorKind::NotFound => "This folder no longer exists".to_string(),
        _ => "This folder can't be read".to_string(),
    })?;
    let (mut v, mut more) = (Vec::new(), false);
    for e in rd.flatten() {
        if v.len() >= cap {
            more = true;
            break;
        }
        if let Some(x) = entry(&e) {
            v.push(x);
        }
    }
    v.sort_unstable_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| kind::natural(&a.shown, &b.shown)));
    Ok((v, more))
}

pub fn run_list(r: ListReq) {
    let (dir, select) = if r.reveal { target(&r.path, &r.home) } else { (r.path, None) };
    let (entries, more, error) = match read(&dir, MAX_LIST) {
        Ok((e, m)) => (e, m, None),
        Err(e) => (vec![], false, Some(e)),
    };
    let msg = Listing { owner: r.owner, generation: r.generation, dir, select, entries: Mutex::new(entries), more, error };
    let _ = r.tx.send(Fact::Custom(Arc::new(msg)));
}

// ── typed paths ─────────────────────────────────────────────────────────

/// Does this text in the bar mean a path rather than a filter?
pub fn is_path(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("~/")
}

/// `~/x` to a real path.
pub fn expand(s: &str, home: &Path) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(r) => home.join(r),
        None => PathBuf::from(s),
    }
}

pub struct CompleteReq {
    pub owner: u64,
    pub generation: u64,
    pub text: String,
    pub home: PathBuf,
    pub tx: calloop::channel::Sender<Fact>,
}

/// A longer path for the bar, as `Fact::Custom`.
pub struct Completed {
    pub owner: u64,
    pub generation: u64,
    pub text: String,
}

/// Tab: extend `input` as far as every match agrees; a single folder gets
/// its slash. Only names that can be typed and shown as they are count.
pub fn complete(input: &str, home: &Path) -> Option<String> {
    let cut = input.rfind('/')? + 1;
    let (head, stem) = input.split_at(cut);
    let dir = expand(head, home);
    let mut names: Vec<String> = vec![];
    for e in std::fs::read_dir(dir).ok()?.flatten().take(MAX_LIST) {
        let Ok(n) = e.file_name().into_string() else { continue };
        if n.starts_with(stem) && (!stem.is_empty() || !n.starts_with('.')) && kind::shown(OsStr::new(&n)) == n {
            names.push(n);
        }
    }
    let first = names.first()?;
    let mut common = first.len();
    for n in &names[1..] {
        common = common.min(first.bytes().zip(n.bytes()).take_while(|(a, b)| a == b).count());
    }
    while !first.is_char_boundary(common) {
        common -= 1;
    }
    let mut out = format!("{head}{}", &first[..common]);
    if names.len() == 1 && expand(&out, home).is_dir() {
        out.push('/');
    }
    (out != input).then_some(out)
}

pub fn run_complete(r: CompleteReq) {
    if let Some(text) = complete(&r.text, &r.home) {
        let _ = r.tx.send(Fact::Custom(Arc::new(Completed { owner: r.owner, generation: r.generation, text })));
    }
}

// ── places ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    pub icon: &'static str,
}

/// The rail, as `Fact::Custom`.
pub struct Places {
    pub owner: u64,
    pub places: Mutex<Vec<Place>>,
    pub projects: Mutex<Vec<Place>>,
}

/// `XDG_DOCUMENTS_DIR="$HOME/Docs"` style overrides.
fn user_dirs(home: &Path) -> Vec<(String, PathBuf)> {
    let cfg = std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
    let Ok(s) = std::fs::read_to_string(cfg.join("user-dirs.dirs")) else { return vec![] };
    s.lines()
        .filter_map(|l| {
            let (k, v) = l.trim().split_once('=')?;
            let v = v.trim().trim_matches('"');
            let p = match v.strip_prefix("$HOME/") {
                Some(r) => home.join(r),
                None => PathBuf::from(v),
            };
            Some((k.to_string(), p))
        })
        .collect()
}

const PROJECT_MARKS: [&str; 7] = [".git", "flake.nix", "Cargo.toml", "package.json", "pyproject.toml", "go.mod", "Makefile"];
const MAX_PROJECTS: usize = 6;

/// Home, the standard folders that exist, and top-level folders of `home`
/// that are clearly projects (most recently touched first).
pub fn places(home: &Path) -> (Vec<Place>, Vec<Place>) {
    let over = user_dirs(home);
    let std_dir = |key: &str, name: &str| over.iter().find(|(k, _)| k == key).map_or_else(|| home.join(name), |(_, p)| p.clone());
    let mut v = vec![Place { name: "Home".into(), path: home.to_path_buf(), icon: "user-home-symbolic" }];
    for (key, name, icon) in [
        ("XDG_DOCUMENTS_DIR", "Documents", "folder-documents-symbolic"),
        ("XDG_DOWNLOAD_DIR", "Downloads", "folder-download-symbolic"),
        ("XDG_PICTURES_DIR", "Pictures", "folder-pictures-symbolic"),
        ("XDG_MUSIC_DIR", "Music", "folder-music-symbolic"),
        ("XDG_VIDEOS_DIR", "Videos", "folder-videos-symbolic"),
    ] {
        let path = std_dir(key, name);
        if path.is_dir() && path != home {
            v.push(Place { name: name.into(), path, icon });
        }
    }
    let mut found: Vec<(i64, Place)> = vec![];
    if let Ok(rd) = std::fs::read_dir(home) {
        for e in rd.flatten().take(2000) {
            let name = e.file_name();
            if name.as_bytes().first() == Some(&b'.') || !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = e.path();
            if v.iter().any(|p| p.path == path) || !PROJECT_MARKS.iter().any(|m| path.join(m).exists()) {
                continue;
            }
            let t = e.metadata().map_or(0, |m| m.mtime());
            found.push((t, Place { name: kind::shown(&name), path, icon: "folder-symbolic" }));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    (v, found.into_iter().take(MAX_PROJECTS).map(|x| x.1).collect())
}

/// The nearest folder that exists at or above `p`, for paths that are gone.
pub fn nearest_dir(p: &Path, home: &Path) -> PathBuf {
    let mut cur = p;
    loop {
        if cur.is_dir() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(up) if !up.as_os_str().is_empty() => cur = up,
            _ => return home.to_path_buf(),
        }
    }
}

/// What to open and select for a `files <path>` request: a file reveals in
/// its folder, a folder opens.
pub fn target(p: &Path, home: &Path) -> (PathBuf, Option<OsString>) {
    match std::fs::symlink_metadata(p) {
        Ok(_) if !p.is_dir() => match p.parent() {
            Some(d) if !d.as_os_str().is_empty() => (d.to_path_buf(), p.file_name().map(OsStr::to_os_string)),
            _ => (home.to_path_buf(), None),
        },
        Ok(_) => (p.to_path_buf(), None),
        Err(_) => (nearest_dir(p, home), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testdir::TestDir;
    use std::os::unix::fs::symlink;

    #[test]
    fn read_sorts_and_classifies() {
        let t = TestDir::new("list");
        t.file("zeta.txt");
        t.file("Alpha/inner.txt");
        t.file("beta2.rs");
        t.file("beta10.rs");
        t.file(".hidden");
        symlink(t.path().join("Alpha"), t.path().join("link-dir")).unwrap();
        symlink(t.path().join("zeta.txt"), t.path().join("link-file")).unwrap();
        symlink(t.path().join("gone"), t.path().join("broken")).unwrap();
        let cpath = std::ffi::CString::new(t.path().join("pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let (v, more) = read(t.path(), 100).unwrap();
        assert!(!more);
        let names: Vec<&str> = v.iter().map(|e| &*e.shown).collect();
        assert_eq!(&names[..2], &["Alpha", "link-dir"]);
        assert!(names.iter().position(|n| *n == "beta2.rs") < names.iter().position(|n| *n == "beta10.rs"));
        let get = |n: &str| v.iter().find(|e| &*e.shown == n).unwrap();
        assert!(get(".hidden").hidden);
        assert!(get("link-dir").is_dir);
        assert_eq!(get("link-dir").special, Special::Link);
        assert_eq!(get("link-file").special, Special::Link);
        assert_eq!(get("broken").special, Special::BrokenLink);
        assert_eq!(get("pipe").special, Special::Device);
        assert!(!get("pipe").openable());
        assert!(!get("broken").openable());
    }

    #[test]
    fn read_caps_and_reports_errors() {
        let t = TestDir::new("listcap");
        for i in 0..20 {
            t.file(&format!("f{i}"));
        }
        let (v, more) = read(t.path(), 5).unwrap();
        assert_eq!(v.len(), 5);
        assert!(more);
        assert!(read(&t.path().join("missing"), 5).is_err());
        assert!(read(&t.path().join("f1"), 5).is_err());
    }

    #[test]
    fn places_and_projects() {
        let t = TestDir::new("places");
        t.file("Documents/a");
        t.file("Music/b");
        t.file("site/flake.nix");
        t.file("other/plain.txt");
        t.file("repo/.git/HEAD");
        let (p, pr) = places(t.path());
        let names: Vec<&str> = p.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Home", "Documents", "Music"]);
        let mut names: Vec<&str> = pr.iter().map(|x| x.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["repo", "site"]);
    }

    #[test]
    fn targets() {
        let t = TestDir::new("target");
        t.file("d/file.txt");
        let h = t.path();
        assert_eq!(target(&h.join("d"), h), (h.join("d"), None));
        assert_eq!(target(&h.join("d/file.txt"), h), (h.join("d"), Some("file.txt".into())));
        // Gone: the nearest folder that is still there.
        assert_eq!(target(&h.join("d/x/y"), h), (h.join("d"), None));
    }

    #[test]
    fn typed_paths() {
        let t = TestDir::new("typed");
        t.file("Documents/a.txt");
        t.file("Downloads/b.txt");
        t.file("Music/c.txt");
        t.file(".secret/d.txt");
        let h = t.path();
        assert!(is_path("/etc") && is_path("~/x") && is_path("~/"));
        assert!(!is_path("~") && !is_path("notes") && !is_path(""));
        assert_eq!(expand("~/Music", h), h.join("Music"));
        assert_eq!(expand("/etc", h), PathBuf::from("/etc"));
        // A single folder completes, with its slash.
        assert_eq!(complete("~/Mu", h).as_deref(), Some("~/Music/"));
        // Several: as far as they agree, then nothing more to add.
        assert_eq!(complete("~/D", h).as_deref(), Some("~/Do"));
        assert_eq!(complete("~/Do", h), None, "Documents and Downloads diverge here");
        // Files have no slash; absolute paths work the same.
        assert_eq!(complete("~/Music/c", h).as_deref(), Some("~/Music/c.txt"));
        let abs = format!("{}/Mu", h.display());
        assert_eq!(complete(&abs, h), Some(format!("{}/Music/", h.display())));
        // Nothing to do, nothing there, hidden only when asked for.
        assert_eq!(complete("~/zzz", h), None);
        assert_eq!(complete("~/nope/x", h), None);
        assert_eq!(complete("no-slash", h), None);
        assert_eq!(complete("~/.se", h).as_deref(), Some("~/.secret/"));
        assert!(!complete("~/", h).unwrap_or_default().contains('.'));
    }

    #[test]
    fn mailbox_keeps_only_the_newest_and_goes_away() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let seen = Arc::new(Mutex::new(vec![]));
        let gate = Arc::new(AtomicUsize::new(0));
        let (s2, g2) = (seen.clone(), gate.clone());
        let m: Mailbox<u32> = Mailbox::new("test-mailbox", move |r| {
            while g2.load(Ordering::Acquire) == 0 {
                std::thread::yield_now();
            }
            s2.lock().unwrap().push(r);
        });
        m.push(1); // running, blocked on the gate
        std::thread::sleep(Duration::from_millis(30));
        m.push(2);
        m.push(3);
        m.push(4);
        gate.store(1, Ordering::Release);
        for _ in 0..500 {
            if m.workers() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(m.workers(), 0);
        assert_eq!(*seen.lock().unwrap(), vec![1, 4]);
    }
}
