//! `org.freedesktop.FileManager1` on the session bus.
//!
//! This is how browsers and Electron apps "show in folder": they call
//! `ShowItems` here and only fall back to `inode/directory` when nothing
//! answers. While nimbus runs it owns the name; when it is not running, the
//! bus activates whoever ships the service file (Nautilus), which makes it
//! the natural fallback. The name is queued, not fought over: a Nautilus
//! already holding it keeps it until it exits, and then it comes to us.

use std::sync::Mutex;

use calloop::channel::Sender;

use crate::services::Fact;

const NAME: &str = "org.freedesktop.FileManager1";
const PATH: &str = "/org/freedesktop/FileManager1";

struct Server {
    tx: Mutex<Sender<Fact>>,
}

impl Server {
    /// One window: the first local URI wins.
    fn show(&self, uris: &[String]) {
        let Some(p) = uris.iter().find_map(|u| super::local_path(u.as_bytes())) else { return };
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(Fact::Command(super::reveal_line(&p, None)));
        }
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl Server {
    async fn show_folders(&self, uris: Vec<String>, _startup_id: String) {
        self.show(&uris);
    }
    async fn show_items(&self, uris: Vec<String>, _startup_id: String) {
        self.show(&uris);
    }
    /// There is no properties dialog: reveal it, the preview shows them.
    async fn show_item_properties(&self, uris: Vec<String>, _startup_id: String) {
        self.show(&uris);
    }
}

/// Claim the name for the life of the process (a quiet no-op without a
/// session bus).
pub fn start(tx: Sender<Fact>) {
    let _ = std::thread::Builder::new().name("nimbus-fm1".into()).stack_size(256 * 1024).spawn(move || {
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.serve_at(PATH, Server { tx: Mutex::new(tx) }))
            .and_then(|b| b.build());
        let Ok(conn) = conn else { return };
        // Queued when taken; ReplaceExisting takes over from an owner that
        // allows it. Without DoNotQueue the bus hands it over later.
        if conn.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::ReplaceExisting.into()).is_err() {
            return;
        }
        // The connection serves from its own executor; it just has to live.
        loop {
            std::thread::park();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parsed(uri: &str) -> Option<PathBuf> {
        super::super::local_path(uri.as_bytes())
    }

    #[test]
    fn uris_from_the_bus() {
        assert_eq!(parsed("file:///home/a/b%20c.txt"), Some(PathBuf::from("/home/a/b c.txt")));
        assert_eq!(parsed("file://localhost/x"), Some(PathBuf::from("/x")));
        assert_eq!(parsed("https://example.org/x"), None);
        assert_eq!(parsed("smb://host/share"), None);
    }

    /// Against a private bus only, never the session's:
    /// `dbus-run-session -- <test binary> fm1_bus --ignored`
    #[test]
    #[ignore]
    fn fm1_bus() {
        assert!(std::env::var("NIMBUS_TEST_PRIVATE_BUS").is_ok(), "run under dbus-run-session with NIMBUS_TEST_PRIVATE_BUS=1");
        let mut ev: calloop::EventLoop<'static, Vec<Fact>> = calloop::EventLoop::try_new().unwrap();
        let (tx, rx) = calloop::channel::channel::<Fact>();
        ev.handle()
            .insert_source(rx, |e, _, q: &mut Vec<Fact>| {
                if let calloop::channel::Event::Msg(f) = e {
                    q.push(f);
                }
            })
            .unwrap();
        start(tx);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let c = zbus::blocking::Connection::session().unwrap();
        let p = zbus::blocking::Proxy::new(&c, NAME, PATH, NAME).unwrap();
        let empty = "";
        let calls: [(&str, Vec<&str>, &str); 3] = [
            ("ShowItems", vec!["file:///tmp/a%20b/c.txt", "file:///ignored"], "/tmp/a b/c.txt"),
            ("ShowFolders", vec!["file:///tmp"], "/tmp"),
            ("ShowItemProperties", vec!["file:///tmp/x"], "/tmp/x"),
        ];
        for (method, uris, want) in calls {
            let _: () = p.call(method, &(uris, empty)).unwrap();
            let mut q = vec![];
            let _ = ev.dispatch(Some(std::time::Duration::from_millis(400)), &mut q);
            let Some(Fact::Command(line)) = q.first() else { panic!("{method}: nothing arrived") };
            let Some(Some(req)) = super::super::parse_command(line) else { panic!("{line}") };
            assert_eq!(req.path, PathBuf::from(want));
            assert_eq!(q.len(), 1, "one window for many uris");
        }
    }
}
