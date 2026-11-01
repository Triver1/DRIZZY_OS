//! Control commands: `nimbus msg <command>` talks to the running bar over
//! `$XDG_RUNTIME_DIR/nimbus.sock`. Commands arrive as `Fact::Command`.
//!
//! The legacy trigger files written by existing niri binds
//! (`/tmp/qs-launcher-trigger`, `/tmp/qs-settings-trigger`) are watched
//! with inotify too, so Win+Space works without touching niri's config.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
};

use calloop::channel::Sender;

use crate::services::Fact;

pub const COMMANDS: [&str; 7] = ["launcher", "monitor", "wallpapers", "settings", "prefs", "notifications", "files"];

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("nimbus.sock")
}

/// Client side: send one command and exit.
pub fn send(cmd: &str) -> std::io::Result<()> {
    let mut s = UnixStream::connect(socket_path())?;
    s.write_all(cmd.as_bytes())?;
    s.write_all(b"\n")
}

pub fn serve(tx: Sender<Fact>) {
    let path = socket_path();
    // A stale socket from a crashed run would make bind fail.
    if UnixStream::connect(&path).is_err() {
        let _ = std::fs::remove_file(&path);
    }
    let Ok(listener) = UnixListener::bind(&path) else {
        eprintln!("nimbus: control socket {} busy", path.display());
        return;
    };
    let tx2 = tx.clone();
    let _ = thread::Builder::new().name("nimbus-ipc".into()).stack_size(128 * 1024).spawn(move || {
        for conn in listener.incoming().flatten() {
            for line in BufReader::new(conn).lines().map_while(Result::ok) {
                let cmd = line.trim().to_string();
                if !cmd.is_empty() && tx2.send(Fact::Command(cmd)).is_err() {
                    return;
                }
            }
        }
    });
    watch_triggers(tx);
}

/// inotify on /tmp for the legacy trigger files; no polling.
fn watch_triggers(tx: Sender<Fact>) {
    let _ = thread::Builder::new().name("nimbus-triggers".into()).stack_size(128 * 1024).spawn(move || unsafe {
        let fd = libc::inotify_init1(libc::IN_CLOEXEC);
        if fd < 0 {
            return;
        }
        let dir = c"/tmp";
        if libc::inotify_add_watch(fd, dir.as_ptr(), libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO) < 0 {
            return;
        }
        let mut buf = [0u8; 4096];
        loop {
            let n = libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
            if n <= 0 {
                return;
            }
            let mut off = 0usize;
            while off + std::mem::size_of::<libc::inotify_event>() <= n as usize {
                let ev = &*(buf.as_ptr().add(off) as *const libc::inotify_event);
                let name_start = off + std::mem::size_of::<libc::inotify_event>();
                let name = &buf[name_start..name_start + ev.len as usize];
                let name = std::str::from_utf8(name).unwrap_or("").trim_end_matches('\0');
                let cmd = match name {
                    "qs-launcher-trigger" => Some("launcher"),
                    "qs-settings-trigger" => Some("prefs"),
                    _ => None,
                };
                if let Some(c) = cmd
                    && tx.send(Fact::Command(c.into())).is_err()
                {
                    return;
                }
                off = name_start + ev.len as usize;
            }
        }
    });
}
