//! The session's polkit authentication agent: the prompt apps get when they
//! ask for admin rights.
//!
//! - `agent`: the D-Bus object and the registration with polkitd
//! - `broker`: the request queue, on its own thread; owns all state
//! - `helper`: polkit's root helper, reached over its socket; PAM runs there
//! - `sanitize`: what untrusted text has to go through to be shown
//! - `scene`: the card, a `WinScene` on the shared layer-surface host
//!
//! nimbus never checks a password. It relays PAM's questions to the user and
//! the answers to polkit's helper; the helper tells polkitd the verdict.
//!
//! The rules this module keeps:
//! - The password lives in a `Secret` (mlocked, no core dump, zeroed), moves
//!   by value from the UI to the helper's socket, and is wiped there. It
//!   never goes into argv, the environment, a `String` or a log.
//! - `log` takes `&'static str`, so no name, cookie or text can reach a log.
//! - Only the owner of `org.freedesktop.PolicyKit1` is listened to.
//! - Every path ends in a reply to polkitd, and every wait has a deadline.
//! - There is no way to feed it a password or point it elsewhere from
//!   outside: test doubles exist under `cfg(test)` only.

mod agent;
mod broker;
mod helper;
mod sanitize;
mod scene;
#[cfg(test)]
mod vm;

use std::{
    io::Write,
    sync::{Arc, OnceLock, mpsc},
    time::Instant,
};

use calloop::channel::Sender;

use crate::services::Fact;
use agent::{Link, Target};
use broker::{Msg, Queue, Timeouts, Ui};

pub use scene::{caps_fact, demo, focus_fact, no_output, open, wants_window, wipe_text};

/// How the card reaches the broker. Unset except in the real bar.
static BROKER: OnceLock<mpsc::Sender<Msg>> = OnceLock::new();

/// One outcome line. Static text only, by type: no identifier, cookie,
/// message or PAM text can be logged from here, and a failed write to a
/// closed stderr is ignored.
fn log(line: &'static str) {
    let _ = writeln!(std::io::stderr().lock(), "nimbus polkit: {line}");
}

/// The broker's thread: at rest it sleeps in `recv` with no timer.
fn run(mut q: Queue, rx: mpsc::Receiver<Msg>, mut link: Link) {
    loop {
        let now = Instant::now();
        link.tick(now);
        q.tick(now);
        let wait = [q.deadline(), link.deadline()].into_iter().flatten().min();
        let msg = match wait {
            Some(t) => match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => return,
            },
        };
        let now = Instant::now();
        match msg {
            Msg::Owner(o) => {
                if link.owner_changed(&o, now) {
                    q.flush();
                }
            }
            Msg::BusLost => {
                q.flush();
                link.bus_lost(now);
            }
            Msg::Shutdown(done) => {
                q.flush();
                link.shutdown();
                let _ = done.send(());
                return;
            }
            m => q.handle(m, now),
        }
    }
}

/// Register and serve for the life of the bar. A quiet no-op without a
/// system bus, beyond a line in the log.
pub fn start(facts: Sender<Fact>) {
    let (tx, rx) = mpsc::channel();
    if BROKER.set(tx.clone()).is_err() {
        return;
    }
    let ui: Ui = Box::new(move |v| {
        let _ = facts.send(Fact::Custom(Arc::new(v)));
    });
    let q = Queue::new(ui, Box::new(|user, cookie| helper::open(user, cookie)), tx.clone(), Timeouts::default());
    let link = Link::new(Target::System, tx.clone());
    agent::catch_signals(tx);
    let _ = std::thread::Builder::new().name("nimbus-polkit".into()).stack_size(256 * 1024).spawn(move || run(q, rx, link));
}

#[cfg(test)]
mod tests {
    /// A guard on the rule rather than on one instance of it: this module
    /// may only log through `log`, and nothing here reads a password from
    /// the environment, argv or a file.
    #[test]
    fn nothing_here_can_print_or_take_a_password_from_outside() {
        let sources = [
            include_str!("polkit.rs"),
            include_str!("polkit/agent.rs"),
            include_str!("polkit/broker.rs"),
            include_str!("polkit/helper.rs"),
            include_str!("polkit/sanitize.rs"),
            include_str!("polkit/scene.rs"),
        ];
        for s in sources {
            // Only what ships: everything from the test modules on is cut.
            let shipped = s.split("#[cfg(test)]\nmod tests").next().unwrap().split("#[cfg(test)]\npub mod t").next().unwrap();
            // `/proc` of the subject is the one thing read, for Details.
            let lines: String = shipped.lines().filter(|l| !l.contains("/proc/")).collect::<Vec<_>>().join("\n");
            for banned in ["eprint", "println", "dbg!", "env::args", "env::var_os", "Command::new", "/bin/sh", "File::open", "fs::read"] {
                assert!(!lines.contains(banned), "{banned} in shipped polkit code");
            }
        }
    }
}
