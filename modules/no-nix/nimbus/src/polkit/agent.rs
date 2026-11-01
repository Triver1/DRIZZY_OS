//! The D-Bus side: `org.freedesktop.PolicyKit1.AuthenticationAgent` on the
//! system bus, and staying registered with the Authority.
//!
//! **Who may call.** The object is reachable by anything on the system bus,
//! so each method first asks the bus who owns `org.freedesktop.PolicyKit1`
//! and refuses any other sender. The owner is looked up on every call: it
//! changes when polkitd restarts, and a cached one could be a stale name.
//!
//! **Which session.** nimbus runs inside `niri.service`, a user unit, so its
//! cgroup has no logind session. `/org/freedesktop/login1/session/auto` is
//! "the caller's session, else its user's display session", which is the
//! lookup polkitd itself does for the caller; polkitd refuses any other
//! session (they must be equal for a unix-session subject).
//!
//! **Dying.** polkitd watches the agent's bus name. However this process
//! ends, including `panic = "abort"` and SIGKILL, the kernel closes the
//! socket, the bus drops the name, and polkitd removes the agent and fails
//! its pending requests; the helper sees EOF and stops. Nothing has to run
//! here for that. SIGTERM and SIGINT additionally unregister properly first,
//! then die by the same signal, so `run.sh` still sees 143 or 130.

use std::{
    collections::HashMap,
    ffi::{CStr, c_char, c_int, c_void},
    sync::{
        atomic::{AtomicI32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use zbus::{
    blocking,
    message::Header,
    names::BusName,
    zvariant::{OwnedValue, Value},
};

use super::{
    broker::{Begin, Msg, Outcome, Who, oneshot},
    helper::Cookie,
    log, sanitize,
};

pub const AUTHORITY: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY_IFACE: &str = "org.freedesktop.PolicyKit1.Authority";
pub const AGENT_PATH: &str = "/org/nimbus/PolkitAgent";

const LOGIN1: &str = "org.freedesktop.login1";
const AUTO_SESSION: &str = "/org/freedesktop/login1/session/auto";
const LOGIN1_SESSION: &str = "org.freedesktop.login1.Session";
const PROPS: &str = "org.freedesktop.DBus.Properties";

/// Identities considered per request.
const MAX_IDENTITIES: usize = 32;
/// Calls to polkitd and logind.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Registration retries: first, then doubling up to the last.
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);
/// How long a dying shell waits to unregister.
const EXIT_GRACE: Duration = Duration::from_millis(1500);

/// polkit's own error names: polkitd maps `Cancelled` to "dismissed".
#[derive(zbus::DBusError, Debug)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
pub enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
    NotAuthorized(String),
}

fn denied() -> AgentError {
    AgentError::NotAuthorized("Only the polkit daemon may call this".into())
}

/// Refuse anyone but the current owner of the Authority's name.
async fn from_authority(conn: &zbus::Connection, hdr: &Header<'_>) -> Result<(), AgentError> {
    let sender = hdr.sender().ok_or_else(denied)?;
    let dbus = zbus::fdo::DBusProxy::new(conn).await.map_err(|_| denied())?;
    let name = BusName::try_from(AUTHORITY).map_err(|_| denied())?;
    let owner = dbus.get_name_owner(name).await.map_err(|_| denied())?;
    if owner.as_str() == sender.as_str() { Ok(()) } else { Err(denied()) }
}

struct Agent {
    tx: mpsc::Sender<Msg>,
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        action_id: String,
        message: String,
        _icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), AgentError> {
        from_authority(conn, &hdr).await?;
        let cookie = Cookie::new(cookie).ok_or_else(|| AgentError::Failed("Bad cookie".into()))?;
        let users = resolve(&identities);
        let user = default_user(&users, unsafe { libc::getuid() }).ok_or_else(|| AgentError::Failed("No usable identity".into()))?;
        let (reply, rx) = oneshot();
        let b = Begin {
            message: sanitize::clean(&message, sanitize::MESSAGE),
            action: sanitize::clean(&action_id, sanitize::ACTION),
            program: subject_program(&details),
            users,
            user,
            cookie,
            reply,
        };
        self.tx.send(Msg::Begin(Box::new(b))).map_err(|_| AgentError::Cancelled("Agent is exiting".into()))?;
        match rx.await {
            Some(Outcome::Done) => Ok(()),
            Some(Outcome::Failed) => Err(AgentError::Failed("Authentication failed".into())),
            Some(Outcome::Cancelled) | None => Err(AgentError::Cancelled("Authentication was cancelled".into())),
        }
    }

    async fn cancel_authentication(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        cookie: String,
    ) -> Result<(), AgentError> {
        from_authority(conn, &hdr).await?;
        let cookie = Cookie::new(cookie).ok_or_else(|| AgentError::Failed("Bad cookie".into()))?;
        let (ack, rx) = oneshot();
        self.tx.send(Msg::Cancel { cookie, ack }).map_err(|_| AgentError::Failed("Agent is exiting".into()))?;
        match rx.await {
            Some(true) => Ok(()),
            _ => Err(AgentError::Failed("No such request".into())),
        }
    }
}

/// The command pkexec was asked to run: its argv without pkexec and options.
fn pkexec_target(argv: &[&str]) -> Option<String> {
    if argv.first()?.rsplit('/').next() != Some("pkexec") {
        return None;
    }
    let mut rest = argv[1..].iter();
    let mut out = vec![];
    while let Some(a) = rest.next() {
        match *a {
            "--user" | "-u" if out.is_empty() => {
                rest.next();
            }
            o if out.is_empty() && o.starts_with("--") => {}
            o => out.push(o),
        }
    }
    (!out.is_empty()).then(|| out.join(" "))
}

/// What this request is about, for Details. polkitd doesn't forward a
/// caller's details to the agent, only `polkit.caller-pid` (who called
/// polkitd) and `polkit.subject-pid` (who it's for), so it comes from `/proc`.
///
/// With pkexec the caller is pkexec, the subject its parent, and the point is
/// the command it will run. Its argv is only believed when `exe` is hidden,
/// which it is for a setuid-root process: anything else could just name
/// itself pkexec. Otherwise `exe` is the kernel's word for the subject.
///
/// Display only, read once, when the request arrives. Nothing depends on it.
fn subject_program(details: &HashMap<String, String>) -> Option<String> {
    let pid = |k: &str| details.get(k)?.parse::<u32>().ok();
    let exe = |p: u32| std::fs::read_link(format!("/proc/{p}/exe")).ok().map(|e| e.to_string_lossy().into_owned());
    let argv = |p: u32| std::fs::read(format!("/proc/{p}/cmdline")).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
    let words = |s: &str| s.split('\0').filter(|a| !a.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let target = pid("polkit.caller-pid").filter(|c| exe(*c).is_none()).and_then(argv).and_then(|a| {
        let w = words(&a);
        pkexec_target(&w.iter().map(String::as_str).collect::<Vec<_>>())
    });
    let shown = match target {
        Some(t) => t,
        None => {
            let s = pid("polkit.subject-pid")?;
            exe(s).or_else(|| argv(s).map(|a| words(&a).join(" ")))?
        }
    };
    Some(sanitize::clean(&shown, sanitize::PROGRAM)).filter(|p| !p.is_empty())
}

/// The user for `uid`, cleaned for display. None when unknown, or when the
/// login couldn't go down the helper's line protocol.
fn lookup(uid: u32) -> Option<Who> {
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as c_char; 16384];
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) };
    if rc != 0 || out.is_null() || pw.pw_name.is_null() {
        return None;
    }
    let field = |p: *const c_char| if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    let login = field(pw.pw_name);
    if login.is_empty() || login.len() > 128 || !login.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let gecos = field(pw.pw_gecos);
    let name = sanitize::clean(gecos.split(',').next().unwrap_or(""), sanitize::NAME);
    Some(Who { uid, name: if name.is_empty() { sanitize::clean(&login, sanitize::NAME) } else { name }, login })
}

/// polkitd expands groups to users before calling, so only `unix-user`
/// entries matter. Unknown kinds and users are skipped.
fn resolve(identities: &[(String, HashMap<String, OwnedValue>)]) -> Vec<Who> {
    let mut v: Vec<Who> = vec![];
    for (kind, map) in identities.iter().take(MAX_IDENTITIES * 4) {
        if kind != "unix-user" || v.len() >= MAX_IDENTITIES {
            continue;
        }
        let Some(uid) = map.get("uid").and_then(|u| u32::try_from(u).ok()) else { continue };
        if v.iter().all(|w| w.uid != uid)
            && let Some(w) = lookup(uid)
        {
            v.push(w);
        }
    }
    v
}

/// Whom the card starts with: you if you may answer, else the first who
/// isn't root, else whoever is listed.
fn default_user(users: &[Who], me: u32) -> Option<usize> {
    if users.is_empty() {
        return None;
    }
    Some(users.iter().position(|u| u.uid == me).or_else(|| users.iter().position(|u| u.uid != 0)).unwrap_or(0))
}

fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .map_or_else(|| "C".to_string(), |v| sanitize::clean(&v, 64))
}

/// Which bus to live on. Only tests can point it anywhere else, so a release
/// build has no way to be aimed at a stand-in Authority.
#[derive(Clone)]
pub enum Target {
    System,
    #[cfg(test)]
    Address(String),
}

fn build(target: &Target, tx: mpsc::Sender<Msg>) -> zbus::Result<blocking::Connection> {
    let b = match target {
        Target::System => blocking::connection::Builder::system()?,
        #[cfg(test)]
        Target::Address(a) => blocking::connection::Builder::address(a.as_str())?,
    };
    b.method_timeout(CALL_TIMEOUT).serve_at(AGENT_PATH, Agent { tx })?.build()
}

/// The logind session to register for.
fn session_id(conn: &blocking::Connection) -> zbus::Result<String> {
    let reply = conn.call_method(Some(LOGIN1), AUTO_SESSION, Some(PROPS), "Get", &(LOGIN1_SESSION, "Id"))?;
    let v: OwnedValue = reply.body().deserialize()?;
    let id = String::try_from(v)?;
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(zbus::Error::Failure("bad session id".into()));
    }
    Ok(id)
}

fn subject(id: &str) -> (&'static str, HashMap<&'static str, Value<'_>>) {
    ("unix-session", HashMap::from([("session-id", Value::from(id))]))
}

/// The agent's bus name, for tests that call it from outside.
#[cfg(test)]
pub static UNIQUE_NAME: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// One connection to the system bus and our registration on it.
struct Bus {
    conn: blocking::Connection,
    /// Set once registered: the session and the Authority it was with.
    session: Option<String>,
    authority: Option<String>,
}

impl Bus {
    /// Connect, and report the Authority changing hands from now on.
    fn connect(target: &Target, tx: mpsc::Sender<Msg>) -> zbus::Result<Self> {
        let conn = build(target, tx.clone())?;
        let dbus = blocking::fdo::DBusProxy::new(&conn)?;
        // Subscribed before registering, so no change can slip between.
        let name = BusName::try_from(AUTHORITY).map_err(zbus::Error::from)?;
        let changes = dbus.receive_name_owner_changed_with_args(&[(0, name.as_str())])?;
        std::thread::Builder::new()
            .name("nimbus-polkit-bus".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                for c in changes {
                    let Ok(a) = c.args() else { continue };
                    let owner = a.new_owner().as_ref().map_or(String::new(), |o| o.to_string());
                    if tx.send(Msg::Owner(owner)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(Msg::BusLost);
            })
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        #[cfg(test)]
        if let Some(n) = conn.unique_name() {
            *UNIQUE_NAME.lock().unwrap() = n.to_string();
        }
        Ok(Self { conn, session: None, authority: None })
    }

    fn register(&mut self) -> zbus::Result<()> {
        let id = session_id(&self.conn)?;
        self.conn.call_method(Some(AUTHORITY), AUTHORITY_PATH, Some(AUTHORITY_IFACE), "RegisterAuthenticationAgent", &(subject(&id), locale(), AGENT_PATH))?;
        // Registering may have started polkitd, so ask only now.
        let dbus = blocking::fdo::DBusProxy::new(&self.conn)?;
        let name = BusName::try_from(AUTHORITY).map_err(zbus::Error::from)?;
        self.authority = Some(dbus.get_name_owner(name)?.to_string());
        self.session = Some(id);
        Ok(())
    }

    fn unregister(&mut self) {
        if let Some(id) = self.session.take() {
            let _ = self.conn.call_method(Some(AUTHORITY), AUTHORITY_PATH, Some(AUTHORITY_IFACE), "UnregisterAuthenticationAgent", &(subject(&id), AGENT_PATH));
        }
    }
}

/// Keeps the agent registered: through a polkitd restart, a bus restart, and
/// another agent holding the session (it may go away, so keep trying,
/// slowly). Everything is driven by the broker's thread, so at rest this is
/// no timers and no wakeups.
pub struct Link {
    target: Target,
    tx: mpsc::Sender<Msg>,
    bus: Option<Bus>,
    retry_at: Option<Instant>,
    backoff: Duration,
    warned: bool,
}

impl Link {
    pub fn new(target: Target, tx: mpsc::Sender<Msg>) -> Self {
        Self { target, tx, bus: None, retry_at: Some(Instant::now()), backoff: RETRY_MIN, warned: false }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.retry_at
    }

    /// Try to get registered, if it's time.
    pub fn tick(&mut self, now: Instant) {
        if self.retry_at.is_none_or(|t| t > now) {
            return;
        }
        if self.bus.is_none() {
            match Bus::connect(&self.target, self.tx.clone()) {
                Ok(b) => self.bus = Some(b),
                Err(_) => return self.again("no system bus", now),
            }
        }
        let Some(bus) = self.bus.as_mut() else { return };
        match bus.register() {
            Ok(()) => {
                self.retry_at = None;
                self.backoff = RETRY_MIN;
                self.warned = false;
                log("registered as the session's authentication agent");
            }
            Err(_) => self.again("could not register, another agent may hold the session; will keep trying", now),
        }
    }

    fn again(&mut self, why: &'static str, now: Instant) {
        if !self.warned {
            self.warned = true;
            log(why);
        }
        self.retry_at = Some(now + self.backoff);
        self.backoff = (self.backoff * 2).min(RETRY_MAX);
    }

    /// The Authority's name changed hands. True when the one we registered
    /// with is gone, so its requests are dead and must be flushed.
    pub fn owner_changed(&mut self, owner: &str, now: Instant) -> bool {
        let Some(bus) = self.bus.as_mut() else { return false };
        // polkitd coming up for our own registration is not news.
        if bus.authority.as_deref() == Some(owner) {
            return false;
        }
        let gone = bus.authority.take().is_some();
        bus.session = None;
        // Registering would start polkitd again, and it may have been
        // stopped on purpose: wait for whoever takes the name next.
        self.retry_at = (!owner.is_empty()).then_some(now);
        self.backoff = RETRY_MIN;
        gone
    }

    pub fn bus_lost(&mut self, now: Instant) {
        self.bus = None;
        self.retry_at = Some(now + RETRY_MIN);
    }

    pub fn shutdown(&mut self) {
        if let Some(b) = self.bus.as_mut() {
            b.unregister();
        }
        self.retry_at = None;
    }
}

// ── exiting on a signal ─────────────────────────────────────────────────

static SIGNAL_PIPE: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(sig: c_int) {
    // Async-signal-safe only: one write, or give up and exit.
    let fd = SIGNAL_PIPE.load(Ordering::Relaxed);
    let b = sig as u8;
    if fd < 0 || unsafe { libc::write(fd, &b as *const u8 as *const c_void, 1) } != 1 {
        unsafe { libc::_exit(128 + sig) };
    }
}

/// SIGTERM and SIGINT tell the broker to unregister, then the process dies
/// of the signal it got. Exiting is never held up by more than `EXIT_GRACE`,
/// and a second signal kills at once (SA_RESETHAND).
pub fn catch_signals(tx: mpsc::Sender<Msg>) {
    let mut fds = [-1 as c_int; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return;
    }
    SIGNAL_PIPE.store(fds[1], Ordering::Relaxed);
    let read_fd = fds[0];
    let spawned = std::thread::Builder::new().name("nimbus-polkit-sig".into()).stack_size(64 * 1024).spawn(move || {
        let mut b = 0u8;
        loop {
            let n = unsafe { libc::read(read_fd, &mut b as *mut u8 as *mut c_void, 1) };
            if n == 1 {
                break;
            }
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        let (done, wait) = mpsc::channel();
        let _ = tx.send(Msg::Shutdown(done));
        let _ = wait.recv_timeout(EXIT_GRACE);
        // The handler reset itself, so this one is fatal.
        unsafe { libc::raise(c_int::from(b)) };
    });
    if spawned.is_err() {
        return;
    }
    for sig in [libc::SIGTERM, libc::SIGINT] {
        let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
        sa.sa_sigaction = on_signal as *const () as usize;
        sa.sa_flags = libc::SA_RESETHAND | libc::SA_RESTART;
        unsafe {
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::polkit::{
        broker::{Queue, Timeouts, View},
        helper::{self, testing::Seen},
        run,
    };
    use std::{
        io::{BufRead, BufReader},
        os::unix::net::UnixStream,
        process::{Child, Command, Stdio},
        sync::{Arc, Mutex, atomic::AtomicU32},
    };

    fn who(uid: u32) -> Who {
        Who { uid, login: format!("u{uid}"), name: String::new() }
    }

    #[test]
    fn pkexec_is_read_for_what_it_will_run() {
        assert_eq!(pkexec_target(&["pkexec", "/bin/id", "-u"]).as_deref(), Some("/bin/id -u"));
        assert_eq!(pkexec_target(&["/run/wrappers/bin/pkexec", "--user", "bob", "--keep-cwd", "/bin/id"]).as_deref(), Some("/bin/id"));
        assert_eq!(pkexec_target(&["pkexec"]), None);
        assert_eq!(pkexec_target(&["bash", "/bin/id"]), None);
        assert_eq!(pkexec_target(&[]), None);
    }

    #[test]
    fn the_subject_is_found_through_proc() {
        let d = |k: &str, v: &str| HashMap::from([(k.to_string(), v.to_string())]);
        let me = std::process::id().to_string();
        let got = subject_program(&d("polkit.subject-pid", &me)).unwrap();
        assert!(got.starts_with('/') && !got.contains('\0'), "{got}");
        // A caller whose exe can be read isn't a setuid pkexec, whatever its
        // argv says: it can't put words in the Program row.
        let mut both = d("polkit.subject-pid", &me);
        both.insert("polkit.caller-pid".into(), me.clone());
        assert_eq!(subject_program(&both).unwrap(), got);
        assert_eq!(subject_program(&d("polkit.subject-pid", "not a pid")), None);
        assert_eq!(subject_program(&d("polkit.subject-pid", "../../etc")), None);
        assert_eq!(subject_program(&d("polkit.subject-pid", "4194999")), None, "gone");
        assert_eq!(subject_program(&HashMap::new()), None);
        // polkitd sends this key only if a caller's details leaked through,
        // and it isn't one we'd believe.
        assert_eq!(subject_program(&d("program", "/bin/fake")), None);
    }

    #[test]
    fn the_default_identity_is_you_else_not_root() {
        assert_eq!(default_user(&[who(0), who(1000), who(1001)], 1001), Some(2));
        assert_eq!(default_user(&[who(0), who(1000), who(1001)], 5000), Some(1));
        assert_eq!(default_user(&[who(0)], 1000), Some(0));
        assert_eq!(default_user(&[], 1000), None);
    }

    fn user(uid: u32) -> (String, HashMap<String, OwnedValue>) {
        ("unix-user".into(), HashMap::from([("uid".to_string(), OwnedValue::from(uid))]))
    }

    #[test]
    fn identities_are_filtered_and_deduplicated() {
        let me = unsafe { libc::getuid() };
        let group = ("unix-group".to_string(), HashMap::from([("gid".to_string(), OwnedValue::from(0u32))]));
        let wrong_type = ("unix-user".to_string(), HashMap::from([("uid".to_string(), OwnedValue::try_from(Value::from("0")).unwrap())]));
        let missing = ("unix-user".to_string(), HashMap::new());
        let list = vec![group, wrong_type, missing, user(me), user(me), user(u32::MAX - 7)];
        let got = resolve(&list);
        assert_eq!(got.len(), 1, "only the one real, distinct user");
        assert_eq!(got[0].uid, me);
    }

    #[test]
    fn the_subject_is_a_unix_session() {
        let (kind, map) = subject("3");
        assert_eq!(kind, "unix-session");
        assert_eq!(map.get("session-id"), Some(&Value::from("3")));
    }

    // ── against a private bus and a stand-in Authority ──────────────────

    /// A private `dbus-daemon`. Nothing here ever touches the session or
    /// system bus: every connection is made to this address.
    pub struct PrivateBus {
        child: Child,
        pub address: String,
    }

    impl PrivateBus {
        pub fn start() -> Self {
            let daemon = ["/run/current-system/sw/bin/dbus-daemon"]
                .iter()
                .find(|p| std::path::Path::new(p).exists())
                .expect("these tests need dbus-daemon for a private bus");
            let mut child = Command::new(daemon)
                .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
                .env_remove("DBUS_SESSION_BUS_ADDRESS")
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
            Self { child, address: line.trim().to_string() }
        }

        pub fn client(&self) -> blocking::Connection {
            blocking::connection::Builder::address(self.address.as_str()).unwrap().method_timeout(Duration::from_secs(10)).build().unwrap()
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[derive(Default, Debug, Clone, PartialEq, Eq)]
    struct Calls {
        registered: Vec<(String, String, String, String)>,
        unregistered: Vec<(String, String)>,
        /// Who called `RegisterAuthenticationAgent`: the agent's unique name.
        agent: String,
    }

    type SubjectArg = (String, HashMap<String, OwnedValue>);

    struct FakeAuthority {
        calls: Arc<Mutex<Calls>>,
        /// Refuse registration, like an agent already holding the session.
        refuse: Arc<AtomicU32>,
    }

    fn session_of(s: &SubjectArg) -> String {
        s.1.get("session-id").and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default()
    }

    #[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
    impl FakeAuthority {
        fn register_authentication_agent(&self, #[zbus(header)] hdr: Header<'_>, subject: SubjectArg, locale: String, object_path: String) -> zbus::fdo::Result<()> {
            if self.refuse.load(Ordering::SeqCst) > 0 {
                self.refuse.fetch_sub(1, Ordering::SeqCst);
                return Err(zbus::fdo::Error::Failed("An authentication agent already exists for the given subject".into()));
            }
            let mut c = self.calls.lock().unwrap();
            c.registered.push((subject.0.clone(), session_of(&subject), locale, object_path));
            c.agent = hdr.sender().map_or(String::new(), |s| s.to_string());
            Ok(())
        }
        fn unregister_authentication_agent(&self, subject: SubjectArg, object_path: String) -> zbus::fdo::Result<()> {
            self.calls.lock().unwrap().unregistered.push((session_of(&subject), object_path));
            Ok(())
        }
    }

    struct FakeSession;

    #[zbus::interface(name = "org.freedesktop.login1.Session")]
    impl FakeSession {
        #[zbus(property)]
        fn id(&self) -> String {
            "c7".into()
        }
    }

    /// The stand-in polkitd (and logind), on its own connection.
    struct Polkitd {
        conn: blocking::Connection,
        calls: Arc<Mutex<Calls>>,
    }

    fn polkitd(bus: &PrivateBus, refuse: u32) -> Polkitd {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let fake = FakeAuthority { calls: calls.clone(), refuse: Arc::new(AtomicU32::new(refuse)) };
        let conn = blocking::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .method_timeout(Duration::from_secs(10))
            .serve_at(AUTHORITY_PATH, fake)
            .unwrap()
            .serve_at(AUTO_SESSION, FakeSession)
            .unwrap()
            .name(AUTHORITY)
            .unwrap()
            .name(LOGIN1)
            .unwrap()
            .build()
            .unwrap();
        Polkitd { conn, calls }
    }

    fn begin_args(cookie: &str) -> (&'static str, String, &'static str, HashMap<&'static str, &'static str>, String, Vec<(String, HashMap<String, OwnedValue>)>) {
        let me = unsafe { libc::getuid() };
        (
            "org.test.run",
            "Run \u{202e}thing\u{202c}\nnow".to_string(),
            "dialog-password",
            HashMap::from([("polkit.subject-pid", "1"), ("polkit.caller-pid", "2")]),
            cookie.to_string(),
            vec![user(me)],
        )
    }

    /// The agent's thread, a stand-in helper, and a robot in the UI's place.
    struct Stack {
        tx: mpsc::Sender<Msg>,
        views: mpsc::Receiver<View>,
        seen: Arc<Mutex<Vec<Seen>>>,
        name: String,
    }

    const PASSWORD: &[u8] = b"correct horse";

    fn stack(bus: &PrivateBus, target: Target) -> Stack {
        let (tx, rx) = mpsc::channel();
        let (vt, views) = mpsc::channel();
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(vec![]));
        let s2 = seen.clone();
        let connect = Box::new(move |user: &str, cookie: &Cookie| {
            let (a, b) = UnixStream::pair()?;
            helper::testing_greet(&a, user, cookie)?;
            let seen = s2.clone();
            std::thread::spawn(move || helper::testing::serve(b, PASSWORD, seen));
            Ok(a)
        });
        let ui = Box::new(move |v| {
            let _ = vt.send(v);
        });
        let q = Queue::new(ui, connect, tx.clone(), Timeouts::default());
        let link = Link::new(target, tx.clone());
        std::thread::spawn(move || run(q, rx, link));
        let _ = bus;
        Stack { tx, views, seen, name: String::new() }
    }

    fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "timed out: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    impl Stack {
        fn view(&self, what: &str, pred: impl Fn(&View) -> bool) -> View {
            let t0 = Instant::now();
            loop {
                match self.views.recv_timeout(Duration::from_millis(50)) {
                    Ok(v) if pred(&v) => return v,
                    _ => assert!(t0.elapsed() < Duration::from_secs(10), "timed out: {what}"),
                }
            }
        }
    }

    fn registered(pk: &Polkitd) -> Calls {
        wait_for("registration", || {
            let c = pk.calls.lock().unwrap().clone();
            (!c.registered.is_empty()).then_some(c)
        })
    }

    fn call(c: &blocking::Connection, to: &str, method: &str, args: &(impl zbus::export::serde::ser::Serialize + zbus::zvariant::DynamicType)) -> zbus::Result<()> {
        c.call_method(Some(to), AGENT_PATH, Some("org.freedesktop.PolicyKit1.AuthenticationAgent"), method, args).map(|_| ())
    }

    fn error_name(e: zbus::Error) -> String {
        match e {
            zbus::Error::MethodError(n, _, _) => n.to_string(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn registers_with_a_session_subject_and_serves_the_daemon_only() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        let c = registered(&pk);
        assert_eq!(c.registered.len(), 1);
        let (kind, session, _locale, path) = &c.registered[0];
        assert_eq!((kind.as_str(), session.as_str(), path.as_str()), ("unix-session", "c7", AGENT_PATH));
        st.name = c.agent.clone();

        // An impostor gets nowhere: nothing reaches the queue or the screen.
        let impostor = bus.client();
        let e = call(&impostor, &st.name, "BeginAuthentication", &begin_args("fake-1")).unwrap_err();
        assert_eq!(error_name(e), "org.freedesktop.PolicyKit1.Error.NotAuthorized");
        assert!(st.views.recv_timeout(Duration::from_millis(150)).is_err(), "a fake prompt appeared");

        // The real one: sanitised on the way in.
        let daemon = pk.conn.clone();
        let name = st.name.clone();
        let pending = std::thread::spawn(move || call(&daemon, &name, "BeginAuthentication", &begin_args("real-1")));
        let View::Show(info) = st.view("show", |v| matches!(v, View::Show(_))) else { unreachable!() };
        assert_eq!(info.message, "Run thing now");
        assert_eq!(info.action, "org.test.run");
        assert_eq!(info.users.len(), 1);
        let shown = format!("{info:?}");
        assert!(!shown.contains("real-1"), "the cookie must not reach the UI");
        st.view("prompt", |v| matches!(v, View::Prompt { .. }));

        // The impostor can't cancel it either, or even learn it's pending.
        let e = call(&impostor, &st.name, "CancelAuthentication", &("real-1",)).unwrap_err();
        assert_eq!(error_name(e), "org.freedesktop.PolicyKit1.Error.NotAuthorized");
        assert!(!pending.is_finished());

        // Authenticating answers polkitd's call.
        let mut s = crate::lock::Secret::new();
        for ch in std::str::from_utf8(PASSWORD).unwrap().chars() {
            s.push(ch);
        }
        st.tx.send(Msg::Submit { id: info.id, secret: s }).unwrap();
        st.view("done", |v| matches!(v, View::Done { ok: true, .. }));
        pending.join().unwrap().unwrap();
        let seen = st.seen.lock().unwrap();
        assert_eq!(seen[0].cookie, "real-1");
        assert_eq!(seen[0].answers, [PASSWORD.to_vec()]);
    }

    #[test]
    fn cancel_authentication_ends_begin_with_cancelled() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        st.name = registered(&pk).agent;
        let (daemon, name) = (pk.conn.clone(), st.name.clone());
        let pending = std::thread::spawn(move || call(&daemon, &name, "BeginAuthentication", &begin_args("c-9")));
        st.view("prompt", |v| matches!(v, View::Prompt { .. }));
        // An unknown cookie is an error and changes nothing.
        let e = call(&pk.conn, &st.name, "CancelAuthentication", &("nope",)).unwrap_err();
        assert_eq!(error_name(e), "org.freedesktop.PolicyKit1.Error.Failed");
        call(&pk.conn, &st.name, "CancelAuthentication", &("c-9",)).unwrap();
        st.view("done", |v| matches!(v, View::Done { ok: false, note: None, .. }));
        let e = pending.join().unwrap().unwrap_err();
        assert_eq!(error_name(e), "org.freedesktop.PolicyKit1.Error.Cancelled");
    }

    #[test]
    fn a_dismissal_reaches_polkitd_as_cancelled() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        st.name = registered(&pk).agent;
        let (daemon, name) = (pk.conn.clone(), st.name.clone());
        let pending = std::thread::spawn(move || call(&daemon, &name, "BeginAuthentication", &begin_args("d-1")));
        let View::Show(info) = st.view("show", |v| matches!(v, View::Show(_))) else { unreachable!() };
        st.tx.send(Msg::Dismiss { id: info.id }).unwrap();
        assert_eq!(error_name(pending.join().unwrap().unwrap_err()), "org.freedesktop.PolicyKit1.Error.Cancelled");
    }

    #[test]
    fn a_busy_session_is_retried_until_it_frees_up() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 1);
        let _st = stack(&bus, Target::Address(bus.address.clone()));
        assert_eq!(registered(&pk).registered.len(), 1);
    }

    #[test]
    fn unregisters_on_shutdown() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let st = stack(&bus, Target::Address(bus.address.clone()));
        registered(&pk);
        let (done, wait) = mpsc::channel();
        st.tx.send(Msg::Shutdown(done)).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let c = pk.calls.lock().unwrap().clone();
        assert_eq!(c.unregistered, [("c7".to_string(), AGENT_PATH.to_string())]);
    }

    #[test]
    fn shutdown_answers_pending_requests() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        st.name = registered(&pk).agent;
        let (daemon, name) = (pk.conn.clone(), st.name.clone());
        let pending = std::thread::spawn(move || call(&daemon, &name, "BeginAuthentication", &begin_args("s-1")));
        st.view("show", |v| matches!(v, View::Show(_)));
        let (done, wait) = mpsc::channel();
        st.tx.send(Msg::Shutdown(done)).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(error_name(pending.join().unwrap().unwrap_err()), "org.freedesktop.PolicyKit1.Error.Cancelled");
    }

    #[test]
    fn a_restarted_authority_gets_a_new_registration_and_old_requests_end() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        st.name = registered(&pk).agent;
        let (daemon, name) = (pk.conn.clone(), st.name.clone());
        let pending = std::thread::spawn(move || call(&daemon, &name, "BeginAuthentication", &begin_args("r-1")));
        st.view("prompt", |v| matches!(v, View::Prompt { .. }));
        // polkitd exits, and a new one takes the name.
        pk.conn.release_name(AUTHORITY).unwrap();
        pk.conn.release_name(LOGIN1).unwrap();
        st.view("done", |v| matches!(v, View::Done { ok: false, .. }));
        assert_eq!(error_name(pending.join().unwrap().unwrap_err()), "org.freedesktop.PolicyKit1.Error.Cancelled");
        let pk2 = polkitd(&bus, 0);
        assert_eq!(registered(&pk2).registered.len(), 1);
        // The new daemon is obeyed, and only it: a stale cached owner would
        // refuse this.
        let (daemon, name) = (pk2.conn.clone(), st.name.clone());
        let pending = std::thread::spawn(move || call(&daemon, &name, "BeginAuthentication", &begin_args("r-2")));
        let View::Show(info) = st.view("show", |v| matches!(v, View::Show(_))) else { unreachable!() };
        st.tx.send(Msg::Dismiss { id: info.id }).unwrap();
        assert_eq!(error_name(pending.join().unwrap().unwrap_err()), "org.freedesktop.PolicyKit1.Error.Cancelled");
    }

    #[test]
    fn nobody_owning_the_name_means_nobody_may_call() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        st.name = registered(&pk).agent;
        drop(pk);
        let c = bus.client();
        wait_for("name gone", || {
            let d = blocking::fdo::DBusProxy::new(&c).ok()?;
            (!d.name_has_owner(BusName::try_from(AUTHORITY).ok()?).ok()?).then_some(())
        });
        let e = call(&c, &st.name, "BeginAuthentication", &begin_args("n-1")).unwrap_err();
        assert_eq!(error_name(e), "org.freedesktop.PolicyKit1.Error.NotAuthorized");
    }

    #[test]
    fn a_bad_cookie_or_no_identity_fails_without_a_prompt() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        let mut st = stack(&bus, Target::Address(bus.address.clone()));
        st.name = registered(&pk).agent;
        let mut a = begin_args("x");
        a.4 = "bad\ncookie".to_string();
        assert_eq!(error_name(call(&pk.conn, &st.name, "BeginAuthentication", &a).unwrap_err()), "org.freedesktop.PolicyKit1.Error.Failed");
        let mut a = begin_args("x");
        a.5.clear();
        assert_eq!(error_name(call(&pk.conn, &st.name, "BeginAuthentication", &a).unwrap_err()), "org.freedesktop.PolicyKit1.Error.Failed");
        assert!(st.views.recv_timeout(Duration::from_millis(150)).is_err());
    }

    fn tids() -> Vec<String> {
        std::fs::read_dir("/proc/self/task").unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
    }

    fn switches(tid: &str) -> u64 {
        let s = std::fs::read_to_string(format!("/proc/self/task/{tid}/status")).unwrap_or_default();
        s.lines().filter(|l| l.contains("ctxt_switches")).filter_map(|l| l.split_whitespace().last()?.parse::<u64>().ok()).sum()
    }

    fn rss_kb() -> u64 {
        let s = std::fs::read_to_string("/proc/self/status").unwrap();
        s.lines().find(|l| l.starts_with("VmRSS")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()).unwrap()
    }

    /// The cost of the agent with nothing to do, in a process of its own:
    /// `<harness> --ignored --exact --nocapture polkit::agent::tests::at_rest_costs_nothing`
    #[test]
    #[ignore]
    fn at_rest_costs_nothing() {
        let bus = PrivateBus::start();
        let pk = polkitd(&bus, 0);
        std::thread::sleep(Duration::from_millis(200));
        let (before, rss0) = (tids(), rss_kb());
        let _st = stack(&bus, Target::Address(bus.address.clone()));
        registered(&pk);
        // Startup's last tasks finish first.
        std::thread::sleep(Duration::from_secs(3));
        let new: Vec<String> = tids().into_iter().filter(|t| !before.contains(t)).collect();
        let at = |ts: &[String]| ts.iter().map(|t| switches(t)).sum::<u64>();
        let s0 = at(&new);
        let per: Vec<u64> = new.iter().map(|t| switches(t)).collect();
        std::thread::sleep(Duration::from_secs(20));
        let woke = at(&new) - s0;
        for (t, p) in new.iter().zip(per) {
            let comm = std::fs::read_to_string(format!("/proc/self/task/{t}/comm")).unwrap_or_default();
            let n = switches(t) - p;
            println!("  {}: {n}", comm.trim());
            // Ours sleep with no timer. zbus's own socket reader sees the
            // occasional message on this shared private bus.
            assert!(n == 0 || comm.starts_with("zbus"), "{comm}: {n}");
        }
        println!("at rest: {} threads, {} wakeups in 20 s, {} kB RSS", new.len(), woke, rss_kb().saturating_sub(rss0));
        assert!(woke <= 2, "an idle agent must not wake up");
    }

    #[test]
    fn exiting_on_a_signal_is_not_held_up_by_a_dead_broker() {
        // The exit path's bound: nobody answers `done`, the wait still ends.
        let (done, wait) = mpsc::channel::<()>();
        drop(done);
        let t0 = Instant::now();
        let _ = wait.recv_timeout(EXIT_GRACE);
        assert!(t0.elapsed() < EXIT_GRACE);
    }
}
