//! End to end against the real thing: polkitd, the socket-activated helper,
//! PAM and `pkexec`. It runs only inside the throw-away NixOS VM
//! (`scripts/polkit-vm.nix`), as a test user logged in on a tty, and refuses
//! to do anything anywhere else. Test builds only.
//!
//! A robot stands in for the card and sends what the card would send, so
//! this covers everything below the UI: registration, the D-Bus object as
//! polkitd really calls it, the helper's protocol, retries, cancellation
//! and what happens to pkexec when the agent dies.

use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use zbus::{blocking, zvariant::OwnedValue};

use super::{
    agent::{self, Link, Target},
    broker::{Info, Msg, Queue, Timeouts, Ui, View},
    helper, run,
};
use crate::lock::Secret;

const PKEXEC: &str = "/run/wrappers/bin/pkexec";
const ID: &str = "/run/current-system/sw/bin/id";
const ALICE: &str = "alice-vm-password";
const BOB: &str = "bob-vm-password";
const MARKER: &str = "/etc/nimbus-polkit-vm";

fn in_the_vm() -> bool {
    let vendor = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
    std::fs::read_to_string(MARKER).is_ok_and(|m| m.trim() == "throw-away test VM") && vendor.trim() == "QEMU"
}

struct Robot {
    tx: mpsc::Sender<Msg>,
    views: mpsc::Receiver<View>,
}

fn robot() -> Robot {
    let (tx, rx) = mpsc::channel();
    let (vt, views) = mpsc::channel();
    let ui: Ui = Box::new(move |v| {
        println!("view: {}", describe(&v));
        let _ = vt.send(v);
    });
    let q = Queue::new(ui, Box::new(|user, cookie| helper::open(user, cookie)), tx.clone(), Timeouts::default());
    let link = Link::new(Target::System, tx.clone());
    std::thread::spawn(move || run(q, rx, link));
    // Registration happens on that thread; give it a moment before asking
    // polkitd for an agent.
    std::thread::sleep(Duration::from_secs(1));
    Robot { tx, views }
}

/// Without the cookie (`Info` has none), for the log.
fn describe(v: &View) -> String {
    match v {
        View::Show(i) => format!("show id={} action={} program={:?} users={:?}", i.id, i.action, i.program, i.users.iter().map(|u| u.login.as_str()).collect::<Vec<_>>()),
        o => format!("{o:?}"),
    }
}

impl Robot {
    fn wait(&self, what: &str, pred: impl Fn(&View) -> bool) -> View {
        let t0 = Instant::now();
        loop {
            match self.views.recv_timeout(Duration::from_millis(100)) {
                Ok(v) if pred(&v) => return v,
                Ok(_) => {}
                Err(_) => assert!(t0.elapsed() < Duration::from_secs(30), "timed out waiting for {what}"),
            }
        }
    }
    fn show(&self) -> Arc<Info> {
        let View::Show(i) = self.wait("the card", |v| matches!(v, View::Show(_))) else { unreachable!() };
        i
    }
    fn prompt(&self) {
        self.wait("a prompt", |v| matches!(v, View::Prompt { .. }));
    }
    fn type_password(&self, id: u64, p: &str) {
        let mut s = Secret::new();
        for c in p.chars() {
            s.push(c);
        }
        self.tx.send(Msg::Submit { id, secret: s }).unwrap();
    }
    fn done(&self) -> View {
        self.wait("the end of the request", |v| matches!(v, View::Done { .. }))
    }
}

fn pkexec() -> Child {
    Command::new(PKEXEC).args([ID, "-u"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
}

/// pkexec succeeded, as root.
fn ran_as_root(c: Child) {
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "pkexec: {:?} {}", o.status, String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "0\n");
}

/// pkexec was refused and ran nothing.
fn refused(c: Child) {
    let o = c.wait_with_output().unwrap();
    assert!(!o.status.success(), "pkexec must not have succeeded");
    assert_eq!(o.stdout, b"", "the command must not have run");
    println!("pkexec refused: {:?}", o.status);
}

fn correct(r: &Robot) {
    let c = pkexec();
    let i = r.show();
    assert_eq!(i.action, "org.freedesktop.policykit.exec");
    assert_eq!(i.program.as_deref(), Some(format!("{ID} -u").as_str()));
    assert!(i.message.contains("as the super user"), "{}", i.message);
    r.prompt();
    r.type_password(i.id, ALICE);
    assert!(matches!(r.done(), View::Done { ok: true, .. }));
    ran_as_root(c);
    println!("PASS correct password");
}

fn wrong_then_correct(r: &Robot) {
    let c = pkexec();
    let i = r.show();
    r.prompt();
    r.type_password(i.id, "not the password");
    let w = r.wait("wrong", |v| matches!(v, View::Wrong { .. }));
    assert!(matches!(w, View::Wrong { fails: 1, .. }));
    r.prompt();
    r.type_password(i.id, ALICE);
    assert!(matches!(r.done(), View::Done { ok: true, .. }));
    ran_as_root(c);
    println!("PASS wrong then correct");
}

fn dismissed(r: &Robot) {
    let c = pkexec();
    let i = r.show();
    r.prompt();
    r.tx.send(Msg::Dismiss { id: i.id }).unwrap();
    assert!(matches!(r.done(), View::Done { ok: false, note: None, .. }));
    let o = c.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(126), "pkexec's code for a dismissed dialog");
    assert_eq!(o.stdout, b"");
    println!("PASS dismissed");
}

fn cancelled_mid_prompt(r: &Robot) {
    let mut c = pkexec();
    r.show();
    r.prompt();
    // The asking process goes away: polkitd calls CancelAuthentication.
    c.kill().unwrap();
    assert!(matches!(r.done(), View::Done { ok: false, note: None, more: false, .. }));
    let _ = c.wait();
    println!("PASS CancelAuthentication mid-prompt");
}

fn picker(r: &Robot) {
    let c = pkexec();
    let i = r.show();
    let logins: Vec<&str> = i.users.iter().map(|u| u.login.as_str()).collect();
    assert!(logins.contains(&"alice") && logins.contains(&"bob"), "{logins:?}");
    assert_eq!(i.users[i.user].login, "alice", "starts with the user at the keyboard");
    r.prompt();
    let bob = i.users.iter().position(|u| u.login == "bob").unwrap();
    r.tx.send(Msg::Pick { id: i.id, user: bob }).unwrap();
    r.prompt();
    // Alice's password is not bob's.
    r.type_password(i.id, ALICE);
    r.wait("wrong", |v| matches!(v, View::Wrong { .. }));
    r.prompt();
    r.type_password(i.id, BOB);
    assert!(matches!(r.done(), View::Done { ok: true, .. }));
    ran_as_root(c);
    println!("PASS identity picker");
}

fn queued(r: &Robot) {
    let (a, b) = (pkexec(), pkexec());
    let first = r.show();
    r.prompt();
    r.tx.send(Msg::Dismiss { id: first.id }).unwrap();
    let second = r.show();
    assert_ne!(first.id, second.id);
    r.prompt();
    r.type_password(second.id, ALICE);
    assert!(matches!(r.done(), View::Done { ok: true, .. }));
    // Either may have been first; one ran, one was dismissed.
    let (oa, ob) = (a.wait_with_output().unwrap(), b.wait_with_output().unwrap());
    assert_eq!(u32::from(oa.status.success()) + u32::from(ob.status.success()), 1, "{:?} {:?}", oa.status, ob.status);
    println!("PASS two requests queue");
}

fn impostor(r: &Robot) {
    let c = pkexec();
    let i = r.show();
    r.prompt();
    let me = unsafe { libc::getuid() };
    let conn = blocking::Connection::system().unwrap();
    let agent = agent::UNIQUE_NAME.lock().unwrap().clone();
    let ids = vec![("unix-user".to_string(), std::collections::HashMap::from([("uid".to_string(), OwnedValue::from(me))]))];
    let details: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let iface = "org.freedesktop.PolicyKit1.AuthenticationAgent";
    let e = conn.call_method(Some(agent.as_str()), agent::AGENT_PATH, Some(iface), "BeginAuthentication", &("a.fake.action", "Fake prompt", "", details, "fake-cookie", ids));
    println!("impostor begin: {:?}", e.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    assert!(e.is_err(), "a stranger's BeginAuthentication must fail");
    let e = conn.call_method(Some(agent.as_str()), agent::AGENT_PATH, Some(iface), "CancelAuthentication", &("anything",));
    assert!(e.is_err(), "a stranger's CancelAuthentication must fail");
    // Nothing moved: the real request is still there and still works.
    assert!(r.views.recv_timeout(Duration::from_millis(300)).is_err(), "no new card");
    r.type_password(i.id, ALICE);
    assert!(matches!(r.done(), View::Done { ok: true, .. }));
    ran_as_root(c);
    println!("PASS impostor refused on the real system bus");
}

/// The helper's socket is stopped, by the driver, before this runs.
fn helper_down(r: &Robot) {
    let c = pkexec();
    let i = r.show();
    let d = r.done();
    assert!(matches!(&d, View::Done { id, ok: false, note: Some(_), .. } if *id == i.id), "{d:?}");
    refused(c);
    println!("PASS helper unavailable fails closed");
}

/// Dies with `abort`, as `panic = "abort"` would, mid-prompt. The driver
/// checks what became of pkexec.
fn abort(r: &Robot) -> ! {
    let _ = Command::new("/bin/sh")
        .args(["-c", &format!("{PKEXEC} {ID} -u > /tmp/pk-abort.out 2>&1; echo $? > /tmp/pk-abort.rc")])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    r.show();
    r.prompt();
    println!("aborting with a prompt open");
    std::process::abort();
}

/// Dies of SIGTERM with a prompt open, through the real handler.
fn sigterm(r: &Robot) -> ! {
    let _ = Command::new("/bin/sh")
        .args(["-c", &format!("{PKEXEC} {ID} -u > /tmp/pk-term.out 2>&1; echo $? > /tmp/pk-term.rc")])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    agent::catch_signals(r.tx.clone());
    r.show();
    r.prompt();
    println!("SIGTERM with a prompt open");
    unsafe { libc::raise(libc::SIGTERM) };
    std::thread::sleep(Duration::from_secs(5));
    panic!("still alive after SIGTERM");
}

/// One agent, kept up while the driver restarts polkitd: it has to come
/// back by itself, with no restart of its own. A prompt open at that moment
/// must end.
fn polkitd_restarts(r: &Robot) {
    correct(r);
    let c = pkexec();
    r.show();
    r.prompt();
    std::fs::write("/tmp/ready-for-restart", "").unwrap();
    assert!(matches!(r.done(), View::Done { ok: false, .. }), "the old daemon's request must end");
    refused(c);
    let t0 = Instant::now();
    while !std::path::Path::new("/tmp/restarted").exists() {
        assert!(t0.elapsed() < Duration::from_secs(30), "no restart");
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(2));
    correct(r);
    println!("PASS survives a polkitd restart");
}

#[test]
#[ignore]
fn e2e() {
    assert!(in_the_vm(), "this test only runs inside the throw-away VM");
    let stage = std::env::var("NIMBUS_POLKIT_VM").expect("which stage");
    let r = robot();
    match stage.as_str() {
        "main" => {
            correct(&r);
            wrong_then_correct(&r);
            dismissed(&r);
            cancelled_mid_prompt(&r);
            picker(&r);
            queued(&r);
            impostor(&r);
        }
        "correct" => correct(&r),
        "helper-down" => helper_down(&r),
        "restart" => polkitd_restarts(&r),
        "abort" => abort(&r),
        "sigterm" => sigterm(&r),
        s => panic!("no stage {s}"),
    }
    println!("STAGE OK");
}
