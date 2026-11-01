//! The home index behind the launcher's file search.
//!
//! A breadth-first walk of `$HOME`, `MAX_DEPTH` folders deep, on a low
//! priority background thread. Nothing here runs at startup: the first
//! launcher open starts the first walk, later opens refresh it once it is
//! older than `REFRESH_AFTER`, and the old index keeps answering meanwhile.
//! Paths live in one byte arena (raw bytes: names need not be UTF-8) so
//! the index costs little more than the text of the paths.
//!
//! Being breadth-first, the caps cut the deepest entries. Nothing here
//! follows a symlink, leaves the home filesystem, or logs a name.

use std::{
    collections::VecDeque,
    ffi::OsStr,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use super::{
    fuzzy::{self, Query, Scratch, Span},
    kind::{self, Kind},
};
use crate::services::Fact;

/// How many folders below `$HOME` are read: `~` itself is level 0, so with
/// 3 the deepest entry is `~/a/b/c/file`.
pub const MAX_DEPTH: usize = 3;
/// Total entries, and entries taken from any one folder.
pub const MAX_ENTRIES: usize = 150_000;
pub const MAX_PER_DIR: usize = 20_000;
/// A walk stops here (cold disks, slow filesystems) and keeps what it has.
const MAX_WALK: Duration = Duration::from_secs(20);
/// An index older than this is rebuilt when the launcher opens.
const REFRESH_AFTER: Duration = Duration::from_secs(60);
/// Results handed to the launcher.
pub const MAX_HITS: usize = 30;

/// Folders that are never interesting and often enormous.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "bower_components",
    "target",
    "__pycache__",
    "__MACOSX",
    "venv",
    "virtualenv",
    "site-packages",
    "vendor",
    "build",
    "_build",
    "dist",
    "dist-newstyle",
    "CMakeFiles",
    "lost+found",
];
/// Files that mark their whole folder as generated.
const SKIP_MARKERS: &[&str] = &["pyvenv.cfg", "CACHEDIR.TAG"];

/// The rule for one name, before looking at the disk. `dir`: it is a real
/// directory; `link`: a symlink.
pub fn skipped(name: &[u8], dir: bool, link: bool) -> bool {
    if name.first() == Some(&b'.') {
        return true;
    }
    if link {
        // `nix build` droppings, and bazel's.
        return name == b"result" || name.starts_with(b"result-") || name.starts_with(b"bazel-");
    }
    if dir {
        return SKIP_DIRS.iter().any(|d| d.as_bytes() == name) || name.starts_with(b"cmake-build-");
    }
    name.ends_with(b"~") || name.ends_with(b".pyc") || name.ends_with(b".pyo") || name.ends_with(b".o")
}

/// One indexed path.
#[derive(Clone, Copy)]
struct Item {
    off: u32,
    len: u16,
    /// Basename offset within the path.
    base: u16,
    mask: u32,
    /// Hours since the epoch (a day's accuracy is plenty for ranking).
    mtime: u32,
    flags: u8,
}

const F_DIR: u8 = 1;
const F_LINK: u8 = 2;

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub depth: usize,
    pub entries: usize,
    pub per_dir: usize,
}

pub const LIMITS: Limits = Limits { depth: MAX_DEPTH, entries: MAX_ENTRIES, per_dir: MAX_PER_DIR };

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub entries: usize,
    pub dirs_read: usize,
    pub unreadable: usize,
    pub capped: bool,
    pub elapsed: Duration,
}

pub struct Index {
    root: PathBuf,
    arena: Vec<u8>,
    items: Vec<Item>,
    built: Instant,
    pub stats: Stats,
}

impl Index {
    /// Walk `root`. Blocking and quiet: errors just mean fewer entries.
    pub fn build(root: &Path, lim: Limits) -> Index {
        let t0 = Instant::now();
        let mut ix = Index { root: root.to_path_buf(), arena: vec![], items: vec![], built: t0, stats: Stats::default() };
        let Ok(top) = std::fs::metadata(root) else { return ix };
        let dev = top.dev();
        let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
        queue.push_back((root.to_path_buf(), 0));
        let mut batch: Vec<(Vec<u8>, usize, u32, u8)> = vec![];
        'walk: while let Some((dir, level)) = queue.pop_front() {
            if t0.elapsed() > MAX_WALK {
                ix.stats.capped = true;
                break;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                ix.stats.unreadable += 1;
                continue;
            };
            ix.stats.dirs_read += 1;
            let rel = dir.strip_prefix(root).unwrap_or(&dir).as_os_str().as_bytes().to_vec();
            batch.clear();
            let mut subdirs: Vec<PathBuf> = vec![];
            let mut generated = false;
            for e in rd.flatten() {
                if batch.len() >= lim.per_dir {
                    ix.stats.capped = true;
                    break;
                }
                let name = e.file_name();
                let nb = name.as_bytes();
                if SKIP_MARKERS.iter().any(|m| m.as_bytes() == nb) {
                    generated = true;
                    break;
                }
                // The dirent's type: no stat, and a symlink stays a symlink.
                let Ok(ft) = e.file_type() else { continue };
                let (is_dir, is_link) = (ft.is_dir(), ft.is_symlink());
                if skipped(nb, is_dir, is_link) {
                    continue;
                }
                let meta = e.metadata().ok();
                let mtime = meta.as_ref().map_or(0, |m| (m.mtime().max(0) / 3600) as u32);
                let flags = if is_dir { F_DIR } else { 0 } | if is_link { F_LINK } else { 0 };
                let mut path = rel.clone();
                if !path.is_empty() {
                    path.push(b'/');
                }
                let base = path.len();
                path.extend_from_slice(nb);
                if path.len() > u16::MAX as usize {
                    continue;
                }
                if is_dir && level < lim.depth && meta.is_some_and(|m| m.dev() == dev) {
                    subdirs.push(e.path());
                }
                batch.push((path, base, mtime, flags));
            }
            if generated {
                continue;
            }
            for (path, base, mtime, flags) in batch.drain(..) {
                if ix.items.len() >= lim.entries || ix.arena.len() + path.len() >= u32::MAX as usize {
                    ix.stats.capped = true;
                    break 'walk;
                }
                ix.items.push(Item {
                    off: ix.arena.len() as u32,
                    len: path.len() as u16,
                    base: base as u16,
                    mask: fuzzy::mask_of(&path[base..]) | fuzzy::mask_of(&path[..base]),
                    mtime,
                    flags,
                });
                ix.arena.extend_from_slice(&path);
            }
            queue.extend(subdirs.into_iter().map(|d| (d, level + 1)));
        }
        ix.arena.shrink_to_fit();
        ix.items.shrink_to_fit();
        ix.stats.entries = ix.items.len();
        ix.stats.elapsed = t0.elapsed();
        ix
    }

    fn rel(&self, it: &Item) -> &[u8] {
        &self.arena[it.off as usize..it.off as usize + it.len as usize]
    }

    /// Roughly what the index holds on the heap.
    #[cfg(test)]
    pub fn heap_bytes(&self) -> usize {
        self.arena.capacity() + self.items.capacity() * std::mem::size_of::<Item>()
    }

    /// The best `limit` entries for `q` and whether more matched.
    pub fn search(&self, q: &Query, limit: usize, scratch: &mut Scratch) -> (Vec<Hit>, bool) {
        let want = q.mask();
        let now_h = (SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()) / 3600) as u32;
        let mut scored: Vec<(i32, u32)> = vec![];
        for (i, it) in self.items.iter().enumerate() {
            if it.mask & want != want {
                continue;
            }
            let age = now_h.saturating_sub(it.mtime) / 24;
            if let Some(s) = fuzzy::score_path(q, self.rel(it), it.base as usize, age, scratch) {
                scored.push((s, i as u32));
            }
        }
        // Once there is a real hit, weak scatter matches go.
        if let Some(top) = scored.iter().map(|e| e.0).max() {
            let floor = if top > 0 { top * 2 / 5 } else { top };
            scored.retain(|e| e.0 >= floor);
        }
        let more = scored.len() > limit;
        if more {
            scored.select_nth_unstable_by(limit, |a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            scored.truncate(limit);
        }
        scored.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let hits = scored
            .iter()
            .map(|&(_, i)| {
                let it = self.items[i as usize];
                let (rel, base) = (self.rel(&it), it.base as usize);
                let (name_hl, dir_hl) = fuzzy::path_spans(q, rel, base, scratch);
                let raw = &rel[base..];
                let name = kind::shown(OsStr::from_bytes(raw));
                let dir_raw = &rel[..base.saturating_sub(1)];
                let dir = kind::shown(OsStr::from_bytes(dir_raw));
                let is_dir = it.flags & F_DIR != 0;
                Hit {
                    path: self.root.join(OsStr::from_bytes(rel)),
                    // Spans index the raw bytes: valid only where the safe
                    // display form kept them.
                    name_hl: if name.len() == raw.len() { name_hl } else { vec![] },
                    dir_hl: if dir.len() == dir_raw.len() { dir_hl } else { vec![] },
                    dir: if dir.is_empty() { "~".into() } else { format!("~/{dir}") },
                    kind: if is_dir { Kind::Folder } else { kind::of_name(OsStr::from_bytes(raw)) },
                    is_dir,
                    is_link: it.flags & F_LINK != 0,
                    name,
                }
            })
            .collect();
        (hits, more)
    }

    #[cfg(test)]
    pub fn paths(&self) -> Vec<String> {
        self.items.iter().map(|i| String::from_utf8_lossy(self.rel(i)).into_owned()).collect()
    }
}

/// One search result, ready to show.
#[derive(Clone, Debug)]
pub struct Hit {
    pub path: PathBuf,
    /// Display-safe name.
    pub name: String,
    /// Display-safe `~/…` folder.
    pub dir: String,
    pub kind: Kind,
    pub is_dir: bool,
    #[allow(dead_code)]
    pub is_link: bool,
    /// Matched spans: bytes of `name`, and of `dir` after its `~/`.
    pub name_hl: Vec<Span>,
    pub dir_hl: Vec<Span>,
}

// ── the shared index ────────────────────────────────────────────────────

static CURRENT: Mutex<Option<Arc<Index>>> = Mutex::new(None);
static WALKING: AtomicBool = AtomicBool::new(false);

pub fn current() -> Option<Arc<Index>> {
    CURRENT.lock().ok()?.clone()
}

/// Sent when a walk has replaced the index: a query on screen runs again.
pub struct Refreshed;

/// Install an index directly (the headless demo's fixture).
pub fn install(ix: Index) {
    if let Ok(mut g) = CURRENT.lock() {
        *g = Some(Arc::new(ix));
    }
}

/// Start a background walk if the index is missing or old. Never blocks.
pub fn refresh(tx: calloop::channel::Sender<Fact>) {
    let fresh = current().is_some_and(|i| i.built.elapsed() < REFRESH_AFTER);
    if fresh || WALKING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name("nimbus-index".into()).spawn(move || {
        // Background work: never compete with anything on screen.
        // SAFETY: plain syscall, 0 = this thread (Linux semantics).
        unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 15) };
        if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            let ix = Arc::new(Index::build(Path::new(&home), LIMITS));
            if let Ok(mut g) = CURRENT.lock() {
                *g = Some(ix);
            }
            let _ = tx.send(Fact::Custom(Arc::new(Refreshed)));
        }
        WALKING.store(false, Ordering::Release);
    });
    if spawned.is_err() {
        WALKING.store(false, Ordering::Release);
    }
}

// ── search worker ───────────────────────────────────────────────────────

/// One query for the search thread.
pub struct Ask {
    pub owner: u64,
    pub generation: u64,
    pub query: String,
}

/// The answer, as `Fact::Custom`.
pub struct Hits {
    pub owner: u64,
    pub generation: u64,
    pub hits: Mutex<Vec<Hit>>,
    pub more: bool,
}

/// Answer one query (runs on the launcher's search thread).
pub fn answer(ask: Ask, tx: &calloop::channel::Sender<Fact>, scratch: &mut Scratch) {
    let q = Query::new(&ask.query);
    // No index yet: nothing for now, `Refreshed` asks again when it lands.
    let (hits, more) = match current() {
        Some(ix) if !q.is_empty() => ix.search(&q, MAX_HITS, scratch),
        _ => (vec![], false),
    };
    let _ = tx.send(Fact::Custom(Arc::new(Hits { owner: ask.owner, generation: ask.generation, hits: Mutex::new(hits), more })));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testdir::TestDir;
    use std::os::unix::fs::symlink;

    fn lim(depth: usize) -> Limits {
        Limits { depth, entries: 1000, per_dir: 1000 }
    }

    #[test]
    fn depth_limit() {
        let t = TestDir::new("depth");
        t.file("zero.txt");
        t.file("a/one.txt");
        t.file("a/b/two.txt");
        t.file("a/b/c/three.txt");
        t.file("a/b/c/d/four.txt");
        t.file("a/b/c/d/e/five.txt");
        let p = Index::build(t.path(), lim(3)).paths();
        assert!(p.contains(&"zero.txt".to_string()));
        assert!(p.contains(&"a/b/c/three.txt".to_string()), "{p:?}");
        // The folder at the edge is listed, but not read.
        assert!(p.contains(&"a/b/c/d".to_string()));
        assert!(!p.iter().any(|x| x.ends_with("four.txt")), "{p:?}");
        assert!(!p.iter().any(|x| x.ends_with("five.txt")));
        let p = Index::build(t.path(), lim(1)).paths();
        assert!(p.contains(&"a/one.txt".to_string()));
        assert!(!p.iter().any(|x| x.ends_with("two.txt")));
        assert!(!p.iter().any(|x| x.starts_with("a/b/")));
    }

    #[test]
    fn skip_rules() {
        let t = TestDir::new("skip");
        t.file("keep/visible.md");
        t.file(".hidden-file");
        t.file(".config/app.toml");
        t.file(".git/config");
        for d in ["node_modules", "target", "__pycache__", "venv", "build", "dist", "site-packages", "cmake-build-debug"] {
            t.file(&format!("proj/{d}/junk.txt"));
        }
        t.file("proj/main.c");
        t.file("proj/main.o");
        t.file("proj/mod.pyc");
        t.file("proj/notes.txt~");
        // An oddly named venv, and a tagged cache: found by their markers.
        t.file("proj/my-env/pyvenv.cfg");
        t.file("proj/my-env/lib.py");
        t.file("proj/cachedir/CACHEDIR.TAG");
        t.file("proj/cachedir/blob");
        let p = Index::build(t.path(), lim(5)).paths();
        assert!(p.contains(&"keep/visible.md".to_string()));
        assert!(p.contains(&"proj/main.c".to_string()));
        for bad in ["hidden", ".config", ".git", "junk", "main.o", ".pyc", "txt~", "lib.py", "blob"] {
            assert!(!p.iter().any(|x| x.contains(bad)), "{bad}: {p:?}");
        }
        // The rule is for folders: a plain file with that name stays.
        let t = TestDir::new("skip-files");
        t.file("proj/target.txt");
        t.file("proj/build");
        t.file("proj/vendor");
        let p = Index::build(t.path(), lim(5)).paths();
        assert!(p.contains(&"proj/target.txt".to_string()));
        assert!(p.contains(&"proj/build".to_string()));
        assert!(p.contains(&"proj/vendor".to_string()));
    }

    #[test]
    fn symlinks_are_not_followed() {
        let t = TestDir::new("links");
        let outside = TestDir::new("links-outside");
        outside.file("secret/inner.txt");
        t.file("real/file.txt");
        symlink(outside.path(), t.path().join("escape")).unwrap();
        symlink(t.path().join("real"), t.path().join("again")).unwrap();
        // A loop: must terminate, must not repeat.
        symlink(t.path(), t.path().join("real/loop")).unwrap();
        symlink(outside.path().join("secret"), t.path().join("result")).unwrap();
        symlink(outside.path().join("secret"), t.path().join("result-1")).unwrap();
        let p = Index::build(t.path(), lim(6)).paths();
        assert!(!p.iter().any(|x| x.contains("inner.txt")), "{p:?}");
        assert!(!p.iter().any(|x| x.contains("loop/")), "{p:?}");
        // The links themselves are entries (except `result`), as leaves.
        assert!(p.contains(&"escape".to_string()));
        assert!(p.contains(&"real/loop".to_string()));
        assert!(!p.iter().any(|x| x.starts_with("result")), "{p:?}");
        // A real folder called `result` is the user's.
        t.file("sub/result/kept.txt");
        let p = Index::build(t.path(), lim(6)).paths();
        assert!(p.contains(&"sub/result/kept.txt".to_string()), "{p:?}");
    }

    #[test]
    fn caps_hold_and_prefer_shallow() {
        let t = TestDir::new("caps");
        for i in 0..40 {
            t.file(&format!("top{i}.txt"));
            t.file(&format!("sub/deep{i}.txt"));
        }
        let ix = Index::build(t.path(), Limits { depth: 3, entries: 30, per_dir: 1000 });
        assert_eq!(ix.stats.entries, 30);
        assert!(ix.stats.capped);
        // Breadth first: the level that was cut is the deepest one read.
        assert!(!ix.paths().iter().any(|x| x.starts_with("sub/")));
        let ix = Index::build(t.path(), Limits { depth: 3, entries: 1000, per_dir: 10 });
        assert!(ix.stats.capped);
        assert!((10..=20).contains(&ix.stats.entries), "{}", ix.stats.entries);
    }

    #[test]
    fn unreadable_dirs_are_tolerated() {
        use std::os::unix::fs::PermissionsExt;
        let t = TestDir::new("perm");
        t.file("open/a.txt");
        t.file("shut/b.txt");
        let shut = t.path().join("shut");
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();
        let ix = Index::build(t.path(), lim(3));
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = ix.paths();
        assert!(p.contains(&"open/a.txt".to_string()));
        assert!(p.contains(&"shut".to_string()));
        assert!(!p.contains(&"shut/b.txt".to_string()));
        assert_eq!(ix.stats.unreadable, 1);
        // A root that does not exist is an empty index, not an error.
        let none = Index::build(&t.path().join("nope"), lim(3));
        assert_eq!(none.stats.entries, 0);
    }

    #[test]
    fn awkward_names_survive() {
        let t = TestDir::new("names");
        let raw = OsStr::from_bytes(b"caf\xe9-not-utf8.txt");
        std::fs::write(t.path().join(raw), "x").unwrap();
        t.file("line\nbreak.txt");
        t.file("spa ce.txt");
        let ix = Index::build(t.path(), lim(2));
        let (hits, _) = ix.search(&Query::new("break"), 5, &mut Scratch::default());
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].name.contains('\n'));
        assert!(hits[0].path.exists());
        let (hits, _) = ix.search(&Query::new("utf8"), 5, &mut Scratch::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path.file_name().unwrap(), raw);
    }

    #[test]
    fn search_ranks_and_highlights() {
        let t = TestDir::new("search");
        t.file("Documents/report.pdf");
        t.file("Documents/archive/2019/report-old.pdf");
        t.file("report.txt");
        t.file("repos/other/main.rs");
        let ix = Index::build(t.path(), lim(4));
        let (hits, _) = ix.search(&Query::new("report"), 10, &mut Scratch::default());
        assert_eq!(hits[0].name, "report.txt");
        assert_eq!(hits[0].dir, "~");
        assert_eq!(hits[1].name, "report.pdf");
        assert_eq!(hits[1].dir, "~/Documents");
        assert_eq!(hits[1].name_hl, vec![(0, 6)]);
        assert_eq!(hits[2].name, "report-old.pdf");
        // The limit reports that more matched.
        let (hits, more) = ix.search(&Query::new("report"), 2, &mut Scratch::default());
        assert_eq!(hits.len(), 2);
        assert!(more);
        // A folder match is a folder.
        let (hits, _) = ix.search(&Query::new("documents"), 3, &mut Scratch::default());
        assert!(hits[0].is_dir);
        assert_eq!(hits[0].kind, Kind::Folder);
    }

    /// Read-only timing of the real home: prints counts and times, never a
    /// path or a name.
    /// `HOME=… cargo test --release index_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn index_timing() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let rss = || {
            std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split(' ').nth(1)?.parse::<u64>().ok()).unwrap_or(0) * 4096
        };
        for run in 0..3 {
            let before = rss();
            let ix = Index::build(&home, LIMITS);
            let s = ix.stats;
            println!(
                "run {run}: entries={} dirs={} unreadable={} capped={} walk={:?} heap={:.2}MiB rss+={:.2}MiB",
                s.entries,
                s.dirs_read,
                s.unreadable,
                s.capped,
                s.elapsed,
                ix.heap_bytes() as f64 / 1048576.0,
                rss().saturating_sub(before) as f64 / 1048576.0
            );
            let mut sc = Scratch::default();
            for (i, q) in ["a", "e", "re", "not", "main", "config", "readme", "xq", "zzzqqq", "doc rep", "src/ma"].iter().enumerate() {
                let q = Query::new(q);
                let t = Instant::now();
                let (hits, more) = ix.search(&q, MAX_HITS, &mut sc);
                println!("  query #{i}: {:?} hits={} more={more}", t.elapsed(), hits.len());
            }
        }
    }
}
