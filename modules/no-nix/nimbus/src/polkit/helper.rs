//! polkit's authentication helper, as polkit 127 runs it.
//!
//! There is no setuid helper on this system: `polkit-agent-helper@.service`
//! is socket-activated on `/run/polkit/agent-helper.socket`, one root
//! instance per connection (the only fallback libpolkit-agent has is
//! `/run/wrappers/bin/polkit-agent-helper-1`, which NixOS no longer
//! installs). So nothing is spawned here: no PATH, no shell, no argv, no
//! environment. The helper runs PAM's `polkit-1` stack and, when it
//! succeeds, tells polkitd itself (`AuthenticationAgentResponse3`, with a
//! pidfd of this process that it reads from the socket). nimbus never sees
//! or decides the verdict that counts: its `SUCCESS` only closes the card.
//!
//! Wire format, see polkitagentsession.c and polkitagenthelper-pam.c:
//!
//!   → <user>\n<cookie>\n
//!   ← PAM_PROMPT_ECHO_OFF <escaped text>\n   (or _ON; answer with a line)
//!   ← PAM_ERROR_MSG <escaped text>\n
//!   ← PAM_TEXT_INFO <escaped text>\n
//!   ← SUCCESS\n or FAILURE\n, and the connection closes
//!
//! Closing the socket cancels: the helper's fgets() fails, the PAM
//! conversation errors out and the instance exits. One connection is one
//! PAM transaction, so a retry is a new connection with the same cookie.

use std::{
    io::{self, BufRead, BufReader, IoSlice, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::AsRawFd,
        net::UnixStream,
    },
    path::Path,
    time::Duration,
};

use super::sanitize;
use crate::lock::Secret;

/// The one place a password can go.
pub const SOCKET: &str = "/run/polkit/agent-helper.socket";
/// `PAM_MAX_RESP_SIZE` is 512 and the helper reads with fgets, which keeps
/// 511 bytes including the newline. Anything longer would be cut, and the
/// tail handed to the next prompt as if it were an answer.
pub const MAX_RESPONSE: usize = 510;
/// Longest helper line accepted.
const MAX_LINE: u64 = 4096;
/// A write that takes this long is a stuck helper.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// A polkit cookie: the capability that ties a helper run to one request.
/// No `Debug` or `Display`, so it can't reach a log line by accident.
pub struct Cookie(Vec<u8>);

impl Cookie {
    /// None unless it is what polkitd generates: printable ASCII, so it
    /// can't smuggle a line break into the helper's protocol.
    pub fn new(s: String) -> Option<Self> {
        let b = s.into_bytes();
        let c = Self(b);
        (!c.0.is_empty() && c.0.len() <= 512 && c.0.iter().all(|x| x.is_ascii_graphic())).then_some(c)
    }

    fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl PartialEq for Cookie {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}

impl Drop for Cookie {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

/// What the helper said.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Event {
    /// PAM wants an answer. Both styles are answered through the password
    /// field and shown as dots, including echo-on ones.
    Prompt(String),
    Info(String),
    Error(String),
    Success,
    Failure,
    /// The connection ended without a verdict.
    Closed,
    /// Something this protocol doesn't have.
    Garbled,
}

/// g_strcompress: undoes the g_strescape the helper applies to PAM text, so
/// non-ASCII comes back and no text can contain a raw line break.
fn compress(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = s.get(i) else { break };
        i += 1;
        match e {
            b'0'..=b'7' => {
                let mut v = u32::from(e - b'0');
                let mut n = 1;
                while n < 3
                    && let Some(d @ b'0'..=b'7') = s.get(i)
                {
                    v = v * 8 + u32::from(d - b'0');
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            o => out.push(o),
        }
    }
    out
}

/// One line from the helper, without its newline. Verdicts must match
/// exactly and text only ever arrives behind a `PAM_` tag, so PAM text
/// can't pose as a verdict.
pub fn parse(line: &[u8]) -> Event {
    let text = |rest: &[u8]| String::from_utf8_lossy(&compress(rest)).into_owned();
    match line {
        b"SUCCESS" => Event::Success,
        b"FAILURE" => Event::Failure,
        _ => {
            if let Some(r) = line.strip_prefix(b"PAM_PROMPT_ECHO_OFF ").or_else(|| line.strip_prefix(b"PAM_PROMPT_ECHO_ON ")) {
                Event::Prompt(sanitize::prompt(&text(r)))
            } else if let Some(r) = line.strip_prefix(b"PAM_ERROR_MSG ") {
                Event::Error(sanitize::clean(&text(r), sanitize::PAM))
            } else if let Some(r) = line.strip_prefix(b"PAM_TEXT_INFO ") {
                Event::Info(sanitize::clean(&text(r), sanitize::PAM))
            } else {
                Event::Garbled
            }
        }
    }
}

fn peer_uid(s: &UnixStream) -> io::Result<u32> {
    let mut c = libc::ucred { pid: 0, uid: u32::MAX, gid: 0 };
    let mut n = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let p = &mut c as *mut libc::ucred as *mut libc::c_void;
    if unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, p, &mut n) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(c.uid)
}

/// Connect to `path`, which `owner` must own, listen on, and hold the
/// directory of: a password must not reach a socket another user could
/// have put there.
fn connect(path: &Path, owner: u32) -> io::Result<UnixStream> {
    let denied = || io::Error::from(io::ErrorKind::PermissionDenied);
    let dir = path.parent().ok_or_else(denied)?.symlink_metadata()?;
    let sock = path.symlink_metadata()?;
    if !dir.is_dir() || dir.uid() != owner || dir.mode() & 0o022 != 0 || !sock.file_type().is_socket() || sock.uid() != owner {
        return Err(denied());
    }
    let s = UnixStream::connect(path)?;
    if peer_uid(&s)? != owner {
        return Err(denied());
    }
    s.set_write_timeout(Some(WRITE_TIMEOUT))?;
    Ok(s)
}

/// All of `parts`, without copying them anywhere first.
fn send(mut s: &UnixStream, parts: &[&[u8]]) -> io::Result<()> {
    let total: usize = parts.iter().map(|p| p.len()).sum();
    let slices: Vec<IoSlice> = parts.iter().map(|p| IoSlice::new(p)).collect();
    let mut sent = s.write_vectored(&slices)?;
    if sent == total {
        return Ok(());
    }
    // A short write: finish part by part.
    for p in parts {
        if sent >= p.len() {
            sent -= p.len();
        } else {
            s.write_all(&p[sent..])?;
            sent = 0;
        }
    }
    Ok(())
}

/// Start a PAM transaction on a connected helper.
fn greet(s: &UnixStream, user: &str, cookie: &Cookie) -> io::Result<()> {
    if user.is_empty() || user.bytes().any(|b| b.is_ascii_control()) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    send(s, &[user.as_bytes(), b"\n", cookie.bytes(), b"\n"])
}

/// A helper for `user` on this system's socket, greeted.
pub fn open(user: &str, cookie: &Cookie) -> io::Result<UnixStream> {
    let s = connect(Path::new(SOCKET), 0)?;
    greet(&s, user, cookie)?;
    Ok(s)
}

/// Can `secret` go over the protocol intact?
pub fn acceptable(secret: &Secret) -> bool {
    let b = secret.bytes();
    !b.is_empty() && b.len() <= MAX_RESPONSE && !b.iter().any(|c| matches!(c, b'\n' | b'\r' | 0))
}

/// Answer a prompt, from the secret's own page to the socket, then wipe it.
/// The buffer is wiped whether or not the write worked.
pub fn respond(s: &UnixStream, secret: &mut Secret) -> io::Result<()> {
    let r = if acceptable(secret) { send(s, &[secret.bytes(), b"\n"]) } else { Err(io::ErrorKind::InvalidInput.into()) };
    secret.clear();
    r
}

/// Read events until the connection ends, on a thread of their own (the
/// socket's shutdown ends it). `on` gets every event, the last one being
/// `Closed` unless a verdict came first.
pub fn read_events(s: UnixStream, mut on: impl FnMut(Event) + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new().name("nimbus-polkit-io".into()).stack_size(128 * 1024).spawn(move || {
        let mut r = BufReader::with_capacity(1024, s);
        let mut line = Vec::with_capacity(256);
        loop {
            line.clear();
            match r.by_ref().take(MAX_LINE).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return on(Event::Closed),
                Ok(_) => {}
            }
            if line.pop() != Some(b'\n') {
                // Over the limit, or cut off mid-line.
                return on(Event::Garbled);
            }
            let ev = parse(&line);
            let last = matches!(ev, Event::Success | Event::Failure | Event::Garbled);
            on(ev);
            if last {
                return;
            }
        }
    })?;
    Ok(())
}

/// `greet`, for test doubles of `open`.
#[cfg(test)]
pub fn testing_greet(s: &UnixStream, user: &str, cookie: &Cookie) -> io::Result<()> {
    greet(s, user, cookie)
}

#[cfg(test)]
pub mod testing {
    //! A scripted stand-in for the helper, for the tests in this module
    //! and in `broker`/`agent`. Never compiled into a release binary.
    use super::*;
    use std::io::BufRead;

    /// What the stand-in saw.
    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    pub struct Seen {
        pub user: String,
        pub cookie: String,
        pub answers: Vec<Vec<u8>>,
    }

    /// Behaves like polkit-agent-helper-1 for one connection: prompts,
    /// compares, says SUCCESS or FAILURE.
    pub fn serve(s: UnixStream, password: &[u8], seen: std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        let line = |r: &mut BufReader<UnixStream>| {
            let mut b = vec![];
            r.read_until(b'\n', &mut b).ok()?;
            (b.pop() == Some(b'\n')).then_some(b)
        };
        let (Some(user), Some(cookie)) = (line(&mut r), line(&mut r)) else { return };
        let mut got = Seen { user: String::from_utf8(user).unwrap(), cookie: String::from_utf8(cookie).unwrap(), answers: vec![] };
        let _ = w.write_all(b"PAM_PROMPT_ECHO_OFF Password: \n");
        let answer = line(&mut r);
        let ok = answer.as_deref() == Some(password);
        got.answers.extend(answer);
        seen.lock().unwrap().push(got);
        let _ = w.write_all(if ok { b"SUCCESS\n" } else { b"FAILURE\n" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn verdicts_are_exact() {
        assert_eq!(parse(b"SUCCESS"), Event::Success);
        assert_eq!(parse(b"FAILURE"), Event::Failure);
        assert_eq!(parse(b"SUCCESSFUL"), Event::Garbled);
        assert_eq!(parse(b"SUCCESS "), Event::Garbled);
        assert_eq!(parse(b" SUCCESS"), Event::Garbled);
        assert_eq!(parse(b""), Event::Garbled);
        assert_eq!(parse(b"PAM_PROMPT_ECHO_MAYBE x"), Event::Garbled);
    }

    #[test]
    fn prompts_and_notes_are_unescaped_and_clean() {
        assert_eq!(parse(b"PAM_PROMPT_ECHO_OFF Password: "), Event::Prompt("Password".into()));
        assert_eq!(parse(b"PAM_PROMPT_ECHO_ON Login: "), Event::Prompt("Login".into()));
        assert_eq!(parse(b"PAM_TEXT_INFO Place your finger"), Event::Info("Place your finger".into()));
        assert_eq!(parse(b"PAM_ERROR_MSG Account locked"), Event::Error("Account locked".into()));
        // g_strescape's octal for UTF-8 and its backslash sequences.
        assert_eq!(parse(b"PAM_PROMPT_ECHO_OFF Mot de passe\\302\\240: "), Event::Prompt("Mot de passe".into()));
        assert_eq!(parse(b"PAM_TEXT_INFO caf\\303\\251"), Event::Info("café".into()));
        assert_eq!(parse(b"PAM_TEXT_INFO one\\ntwo\\ttab \\\"q\\\" b\\\\s"), Event::Info("one two tab \"q\" b\\s".into()));
    }

    #[test]
    fn pam_text_cannot_pose_as_a_verdict() {
        // A module printing "SUCCESS" arrives tagged, even after unescaping
        // and even with an escaped line break in front of it.
        assert_eq!(parse(b"PAM_TEXT_INFO SUCCESS"), Event::Info("SUCCESS".into()));
        assert_eq!(parse(b"PAM_ERROR_MSG \\nSUCCESS"), Event::Error("SUCCESS".into()));
    }

    #[test]
    fn compress_is_g_strcompress() {
        assert_eq!(compress(b"a\\tb\\nc\\\\d\\\"e"), b"a\tb\nc\\d\"e");
        assert_eq!(compress(b"\\101\\7x\\1234"), b"A\x07xS4");
        assert_eq!(compress(b"trailing\\"), b"trailing");
        assert_eq!(compress(b"\\q"), b"q");
    }

    fn collect(s: UnixStream) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        read_events(s, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        rx
    }

    #[test]
    fn framing_handles_split_and_joined_writes() {
        let (mut a, b) = UnixStream::pair().unwrap();
        let rx = collect(b);
        a.write_all(b"PAM_TEXT_INFO on").unwrap();
        std::thread::sleep(Duration::from_millis(30));
        a.write_all(b"e\nPAM_ERROR_MSG two\nPAM_PROMPT_ECHO_OFF Pass").unwrap();
        std::thread::sleep(Duration::from_millis(30));
        a.write_all(b"word: \nSUCCESS\nPAM_TEXT_INFO after the verdict\n").unwrap();
        let got: Vec<_> = rx.iter().collect();
        assert_eq!(got, [Event::Info("one".into()), Event::Error("two".into()), Event::Prompt("Password".into()), Event::Success]);
    }

    #[test]
    fn eof_without_a_verdict_is_closed() {
        let (a, b) = UnixStream::pair().unwrap();
        let rx = collect(b);
        drop(a);
        assert_eq!(rx.recv().unwrap(), Event::Closed);
    }

    #[test]
    fn a_partial_line_is_garbled_not_a_verdict() {
        let (mut a, b) = UnixStream::pair().unwrap();
        let rx = collect(b);
        a.write_all(b"SUCCESS").unwrap();
        drop(a);
        assert_eq!(rx.recv().unwrap(), Event::Garbled);
        assert!(rx.recv().is_err());
    }

    #[test]
    fn an_endless_line_is_garbled() {
        let (mut a, b) = UnixStream::pair().unwrap();
        let rx = collect(b);
        let t = std::thread::spawn(move || {
            let _ = a.write_all(&vec![b'x'; 1 << 20]);
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Garbled);
        drop(rx);
        t.join().unwrap();
    }

    #[test]
    fn shutdown_ends_the_reader() {
        let (a, b) = UnixStream::pair().unwrap();
        let rx = collect(b.try_clone().unwrap());
        b.shutdown(std::net::Shutdown::Both).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Closed);
        drop(a);
    }

    fn secret(s: &str) -> Secret {
        let mut x = Secret::new();
        for c in s.chars() {
            assert!(x.push(c));
        }
        x
    }

    fn read_all(mut s: UnixStream) -> Vec<u8> {
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut v = vec![];
        let mut b = [0u8; 1024];
        while let Ok(n) = s.read(&mut b) {
            if n == 0 {
                break;
            }
            v.extend_from_slice(&b[..n]);
        }
        v
    }

    #[test]
    fn the_greeting_is_user_then_cookie() {
        let (a, b) = UnixStream::pair().unwrap();
        let c = Cookie::new("1-abc-2-def".into()).unwrap();
        greet(&a, "drizzy", &c).unwrap();
        assert_eq!(read_all(b), b"drizzy\n1-abc-2-def\n");
    }

    #[test]
    fn newlines_cannot_be_smuggled_in() {
        for bad in ["a\nb", "a b", "", "a\0", "é"] {
            assert!(Cookie::new(bad.into()).is_none(), "{bad:?}");
        }
        assert!(Cookie::new("x".repeat(513)).is_none());
        let (a, _b) = UnixStream::pair().unwrap();
        let c = Cookie::new("c".into()).unwrap();
        assert!(greet(&a, "root\nevil", &c).is_err());
        assert!(greet(&a, "", &c).is_err());
    }

    #[test]
    fn respond_sends_one_line_and_wipes_the_page() {
        let (a, b) = UnixStream::pair().unwrap();
        let mut s = secret("hunter2 ü");
        let p = s.raw();
        let n = s.bytes().len();
        assert_eq!(unsafe { std::slice::from_raw_parts(p, n) }, "hunter2 ü".as_bytes());
        respond(&a, &mut s).unwrap();
        assert_eq!(read_all(b), "hunter2 ü\n".as_bytes());
        assert!(s.is_empty());
        assert_eq!(unsafe { std::slice::from_raw_parts(p, n) }, vec![0u8; n].as_slice(), "the page still holds the password");
    }

    #[test]
    fn a_failed_write_still_wipes() {
        let (a, b) = UnixStream::pair().unwrap();
        drop(b);
        let mut s = secret("hunter2");
        let p = s.raw();
        assert!(respond(&a, &mut s).is_err());
        assert_eq!(unsafe { std::slice::from_raw_parts(p, 7) }, &[0u8; 7]);
    }

    #[test]
    fn what_the_protocol_cannot_carry_is_refused_and_wiped() {
        let (a, b) = UnixStream::pair().unwrap();
        for s in ["a\nb", "a\rb", "a\0b"] {
            let mut x = secret(s);
            assert!(!acceptable(&x));
            assert!(respond(&a, &mut x).is_err());
            assert!(x.is_empty());
        }
        let mut long = secret(&"x".repeat(MAX_RESPONSE + 1));
        assert!(!acceptable(&long));
        assert!(respond(&a, &mut long).is_err());
        assert!(!acceptable(&Secret::new()));
        assert!(acceptable(&secret(&"x".repeat(MAX_RESPONSE))));
        assert_eq!(read_all(b), b"", "nothing may reach the helper");
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            // sun_path is short.
            let base = Some(std::env::temp_dir()).filter(|b| b.as_os_str().len() < 60).unwrap_or_else(|| "/tmp".into());
            let p = base.join(format!("nimbus-polkit-test-{}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_the_expected_owner_gets_a_password() {
        let me = unsafe { libc::getuid() };
        let d = TempDir::new();
        let path = d.path().join("helper.socket");
        let _l = std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(d.path(), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(connect(&path, me).is_ok());
        // The real call site's owner: root. A socket of ours is refused.
        assert_eq!(connect(&path, 0).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        // A group-writable directory could have the socket swapped.
        std::fs::set_permissions(d.path(), std::os::unix::fs::PermissionsExt::from_mode(0o775)).unwrap();
        assert_eq!(connect(&path, me).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        std::fs::set_permissions(d.path(), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        // A symlink or a plain file is not a socket.
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(connect(&link, me).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        let file = d.path().join("file");
        std::fs::write(&file, "").unwrap();
        assert_eq!(connect(&file, me).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(connect(&d.path().join("absent"), me).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn the_socket_path_is_fixed_and_absolute() {
        assert!(SOCKET.starts_with('/'));
        assert_eq!(SOCKET, "/run/polkit/agent-helper.socket");
    }
}
