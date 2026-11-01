//! The logic behind Settings' Profile page, none of it GPU or Wayland: the
//! circular crop (`crop`), reading and saving the picture (`avatar`), what
//! the page says about the machine (`info`) and the small text fields
//! (`field`). The page itself is `prefs::profile_page`.
//!
//! Everything slow runs on a one-shot worker thread and comes back as a
//! `Fact::Custom(Msg)` addressed to the window that asked.

pub mod avatar;
pub mod crop;
pub mod field;
pub mod info;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{gfx::ImageData, services::Fact};

/// Config key of the name shown instead of the login.
pub const NAME_KEY: &str = "display_name";
/// Longest display name, in characters.
pub const NAME_MAX: usize = 48;
/// The pixels kept for the page's own avatar, square.
pub const SHOW_PX: u32 = 256;

/// The name set in Settings, if any.
pub fn display_name() -> Option<String> {
    crate::config::get(NAME_KEY).map(|n| clean_name(&n)).filter(|n| !n.is_empty())
}

/// What goes in the config: one line, no control characters, capped.
pub fn clean_name(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    s.trim().to_string()
}

pub fn save_display_name(name: &str) {
    crate::config::set(NAME_KEY, &clean_name(name));
    published();
}

/// The picture or the name changed. The lock reads both each time it
/// starts, so only copies made for other users could be stale: this is
/// where the login screen's are refreshed.
pub fn published() {}

/// A picture chosen for cropping, downscaled for the screen.
pub struct Preview {
    pub img: ImageData,
    /// Size of the picture itself, upright.
    pub size: (u32, u32),
}

pub enum Body {
    Info(Box<info::Info>),
    /// What the lock shows now, and whether nimbus could remove it.
    Avatar { img: Option<ImageData>, ours: bool },
    Scan(Vec<PathBuf>),
    Picked { path: PathBuf, result: Result<Preview, String> },
    /// The new picture as the lock will read it.
    Saved(Result<(Option<ImageData>, bool), String>),
    Removed(Result<(Option<ImageData>, bool), String>),
    Pasted(String),
}

/// A worker's result on its way to the window that asked.
pub struct Msg {
    pub owner: u64,
    /// Taken by the window.
    pub body: Mutex<Option<Body>>,
}

fn send(tx: &calloop::channel::Sender<Fact>, owner: u64, body: Body) {
    let _ = tx.send(Fact::Custom(Arc::new(Msg { owner, body: Mutex::new(Some(body)) })));
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(f);
}

fn current_avatar(home: &Path, login: &str) -> (Option<ImageData>, bool) {
    let h = home.to_string_lossy();
    (crate::lock::avatar(&h, login, SHOW_PX), avatar::has_face(home))
}

/// One type of the clipboard, or None within a couple of seconds.
fn clipboard(mime: &str) -> Option<String> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    const CAP: u64 = 8192;
    let mut child = Command::new("wl-paste").args(["--no-newline", "--type", mime]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            _ => break None,
        }
    };
    if !status.is_some_and(|s| s.success()) {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let mut out = String::new();
    child.stdout.take()?.take(CAP).read_to_string(&mut out).ok()?;
    (!out.is_empty()).then_some(out)
}

/// The worker behind everything the page does: `Prefs` owns one.
#[derive(Clone)]
pub struct Workers {
    pub tx: calloop::channel::Sender<Fact>,
    pub owner: u64,
    pub home: PathBuf,
    pub login: String,
}

impl Workers {
    /// What the lock shows right now.
    pub fn read_avatar(&self) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            let (img, ours) = current_avatar(&w.home, &w.login);
            send(&w.tx, w.owner, Body::Avatar { img, ours });
        });
    }

    /// Facts about the machine, read from `root` (`/` outside tests).
    pub fn gather(&self, gpu: String) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            let wall = crate::lock::wallpaper_path(&w.home.to_string_lossy());
            let mut i = info::gather(Path::new("/"), wall.as_deref(), Some(&gpu));
            i.login = w.login.clone();
            send(&w.tx, w.owner, Body::Info(Box::new(i)));
        });
    }

    /// Recent pictures under ~/Pictures.
    pub fn scan(&self) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            let pics = avatar::scan(&w.home.join("Pictures"));
            send(&w.tx, w.owner, Body::Scan(pics));
        });
    }

    /// Open `path` for cropping.
    pub fn pick(&self, path: PathBuf) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            let result = avatar::preview(&path).map_err(|e| e.to_string());
            send(&w.tx, w.owner, Body::Picked { path, result });
        });
    }

    /// Crop `path` and make it the picture.
    pub fn save(&self, path: PathBuf, crop: crop::Crop) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            let r = avatar::save(&w.home, &path, &crop).map(|()| {
                published();
                current_avatar(&w.home, &w.login)
            });
            send(&w.tx, w.owner, Body::Saved(r.map_err(|e| e.to_string())));
        });
    }

    /// Back to the initial.
    pub fn remove(&self) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            let r = avatar::remove_face(&w.home).map(|()| {
                published();
                current_avatar(&w.home, &w.login)
            });
            send(&w.tx, w.owner, Body::Removed(r.map_err(|e| e.to_string())));
        });
    }

    /// The clipboard, through wl-paste (nimbus has no data device). A file
    /// copied in a file manager arrives as a URI list. No wl-paste, or an
    /// owner that never answers: nothing happens, typing still works.
    pub fn paste(&self) {
        let w = self.clone();
        spawn("nimbus-profile", move || {
            if let Some(s) = ["text/uri-list", "text/plain"].iter().find_map(|t| clipboard(t)) {
                send(&w.tx, w.owner, Body::Pasted(s));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_on_one_line_and_within_the_cap() {
        assert_eq!(clean_name("  Ada Lovelace \n"), "Ada Lovelace");
        assert_eq!(clean_name("a\u{7}b\tc"), "abc");
        assert_eq!(clean_name(&"x".repeat(NAME_MAX * 2)).chars().count(), NAME_MAX);
        assert_eq!(clean_name("   "), "");
        // A value that looks like config syntax survives a round trip.
        assert_eq!(clean_name("a = b"), "a = b");
    }
}
