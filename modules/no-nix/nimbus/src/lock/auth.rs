//! Password handling and PAM.
//!
//! The password lives only in a [`Secret`]: a fixed-capacity byte buffer
//! on its own mlocked, MADV_DONTDUMP page that never reallocates and is overwritten with zeros whenever
//! bytes leave it (backspace, clear) and on drop. It has no `Debug` and is
//! never formatted, logged or printed.
//!
//! Authentication runs on one worker thread (so the UI keeps animating and
//! attempts can never overlap): the UI hands it a `Secret` by value, the
//! worker runs `pam_authenticate` and drops (wipes) it, and the verdict
//! comes back over a calloop channel.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    sync::mpsc,
};

/// Longest password accepted, in bytes. The buffer is allocated once at
/// this size so typing never reallocates (which would leave copies behind).
const CAP: usize = 1024;

/// Overwrite `len` bytes at `p` in a way the optimiser can't elide.
fn wipe(p: *mut u8, len: usize) {
    for i in 0..len {
        unsafe { std::ptr::write_volatile(p.add(i), 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// One page of its own: `mlock` works on whole pages and doesn't nest, so
/// a buffer sharing a page with anything else could be unlocked (and
/// swapped out) when that neighbour is unlocked.
const PAGE: usize = 4096;

static MLOCK_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A zeroizing password buffer (UTF-8 bytes) on its own mlocked,
/// never-dumped anonymous page.
pub struct Secret {
    ptr: *mut u8,
    len: usize,
    mapped: bool,
}

// The buffer is owned exclusively; it moves to the PAM worker by value.
unsafe impl Send for Secret {}

impl Secret {
    pub fn new() -> Self {
        let p = unsafe {
            libc::mmap(std::ptr::null_mut(), PAGE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0)
        };
        if p == libc::MAP_FAILED {
            // Out of memory for one page: still work, just unlocked.
            let b: Box<[u8; PAGE]> = Box::new([0; PAGE]);
            return Self { ptr: Box::into_raw(b) as *mut u8, len: 0, mapped: false };
        }
        unsafe {
            libc::madvise(p, PAGE, libc::MADV_DONTDUMP);
            if libc::mlock(p, PAGE) != 0 && !MLOCK_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                super::log(format_args!("mlock failed ({}); the password buffer may be swapped", std::io::Error::last_os_error()));
            }
        }
        Self { ptr: p as *mut u8, len: 0, mapped: true }
    }

    /// Append a character. Returns false (and stores nothing) when full.
    pub fn push(&mut self, c: char) -> bool {
        let mut tmp = [0u8; 4];
        let n = c.encode_utf8(&mut tmp).len();
        let ok = self.len + n <= CAP;
        if ok {
            unsafe { std::ptr::copy_nonoverlapping(tmp.as_ptr(), self.ptr.add(self.len), n) };
            self.len += n;
        }
        wipe(tmp.as_mut_ptr(), tmp.len());
        ok
    }

    /// Remove the last character. Returns false when already empty.
    pub fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        let b = self.bytes();
        let mut n = b.len() - 1;
        while n > 0 && (b[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        let old = self.len;
        self.len = n;
        wipe(unsafe { self.ptr.add(n) }, old - n);
        true
    }

    pub fn clear(&mut self) {
        wipe(self.ptr, self.len);
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of characters (what the dots show).
    pub fn chars(&self) -> usize {
        self.bytes().iter().filter(|b| (**b & 0xC0) != 0x80).count()
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// The page itself, for tests that check it was wiped.
    #[cfg(test)]
    pub(crate) fn raw(&self) -> *const u8 {
        self.ptr
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(self.ptr, PAGE);
        unsafe {
            if self.mapped {
                libc::munlock(self.ptr as *const c_void, PAGE);
                libc::munmap(self.ptr as *mut c_void, PAGE);
            } else {
                drop(Box::from_raw(self.ptr as *mut [u8; PAGE]));
            }
        }
    }
}

/// The user being unlocked.
#[derive(Clone)]
pub struct User {
    pub login: String,
    /// GECOS full name, or the login when unset.
    pub name: String,
    pub home: String,
}

impl User {
    pub fn current() -> Self {
        let uid = unsafe { libc::getuid() };
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buf = vec![0 as c_char; 16384];
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) };
        let field = |p: *const c_char| {
            if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
        };
        let (login, gecos, home) = if rc == 0 && !out.is_null() {
            (field(pw.pw_name), field(pw.pw_gecos), field(pw.pw_dir))
        } else {
            (std::env::var("USER").unwrap_or_default(), String::new(), std::env::var("HOME").unwrap_or_default())
        };
        let full = gecos.split(',').next().unwrap_or("").trim().to_string();
        let name = if full.is_empty() { login.clone() } else { full };
        Self { login, name, home }
    }
}

/// The first PAM service that exists: a dedicated `nimbus`, then the
/// minimal locker stacks (`swaylock`, `hyprlock`), then `login`.
pub fn service() -> &'static str {
    ["nimbus", "swaylock", "hyprlock"].into_iter().find(|s| Path::new("/etc/pam.d").join(s).exists()).unwrap_or("login")
}

// ── libpam FFI (Linux-PAM) ──────────────────────────────────────────────

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
/// Fail empty passwords even where a stack says `nullok`.
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 1;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<ConvFn>,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, pamh: *mut *mut c_void) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut c_void, errnum: c_int) -> *const c_char;
}

struct ConvData<'a> {
    password: &'a [u8],
    login: &'a CStr,
    /// The last error/info message a module sent ("account locked", ...).
    note: std::cell::RefCell<Option<String>>,
}

/// malloc'd NUL-terminated copy (PAM frees it, after wiping it).
fn c_dup(bytes: &[u8]) -> *mut c_char {
    unsafe {
        let p = libc::malloc(bytes.len() + 1) as *mut u8;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            *p.add(bytes.len()) = 0;
        }
        p as *mut c_char
    }
}

extern "C" fn conversation(n: c_int, msgs: *mut *const PamMessage, resp: *mut *mut PamResponse, data: *mut c_void) -> c_int {
    if n <= 0 || msgs.is_null() || resp.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }
    let data = unsafe { &*(data as *const ConvData) };
    let n = n as usize;
    let out = unsafe { libc::calloc(n, std::mem::size_of::<PamResponse>()) as *mut PamResponse };
    if out.is_null() {
        return PAM_BUF_ERR;
    }
    let mut ok = true;
    for i in 0..n {
        // Linux-PAM: `msgs` is an array of pointers.
        let m = unsafe { *msgs.add(i) };
        if m.is_null() {
            ok = false;
            break;
        }
        let m = unsafe { &*m };
        let r = unsafe { &mut *out.add(i) };
        match m.msg_style {
            PAM_PROMPT_ECHO_OFF => r.resp = c_dup(data.password),
            PAM_PROMPT_ECHO_ON => r.resp = c_dup(data.login.to_bytes()),
            PAM_ERROR_MSG | PAM_TEXT_INFO => {
                if !m.msg.is_null() {
                    // Module messages ("account locked", ...), never the password.
                    let text = unsafe { CStr::from_ptr(m.msg) }.to_string_lossy().into_owned();
                    super::log(format_args!("pam: {}", text.trim()));
                    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
                    if !line.is_empty() {
                        let short: String = line.chars().take(64).collect();
                        *data.note.borrow_mut() = Some(short);
                    }
                }
            }
            _ => ok = false,
        }
        if matches!(m.msg_style, PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON) && r.resp.is_null() {
            ok = false;
        }
        if !ok {
            break;
        }
    }
    if !ok {
        for i in 0..n {
            let r = unsafe { &mut *out.add(i) };
            if !r.resp.is_null() {
                let len = unsafe { libc::strlen(r.resp) };
                wipe(r.resp as *mut u8, len);
                unsafe { libc::free(r.resp as *mut c_void) };
            }
        }
        unsafe { libc::free(out as *mut c_void) };
        return PAM_CONV_ERR;
    }
    unsafe { *resp = out };
    PAM_SUCCESS
}

/// One full PAM transaction. `ok` only on PAM_SUCCESS; `note` is the last
/// message a module wanted shown.
fn authenticate(service: &CStr, login: &CStr, secret: &Secret) -> (bool, Option<String>) {
    if secret.is_empty() {
        return (false, None);
    }
    let data = ConvData { password: secret.bytes(), login, note: std::cell::RefCell::new(None) };
    let conv = PamConv { conv: Some(conversation), appdata_ptr: &data as *const ConvData as *mut c_void };
    let mut h: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { pam_start(service.as_ptr(), login.as_ptr(), &conv, &mut h) };
    if rc != PAM_SUCCESS || h.is_null() {
        super::log(format_args!("pam_start failed ({rc})"));
        return (false, Some("Authentication unavailable".into()));
    }
    let rc = unsafe { pam_authenticate(h, PAM_DISALLOW_NULL_AUTHTOK) };
    if rc != PAM_SUCCESS {
        let why = unsafe { pam_strerror(h, rc) };
        if !why.is_null() {
            super::log(format_args!("authentication failed: {}", unsafe { CStr::from_ptr(why) }.to_string_lossy()));
        }
    }
    unsafe { pam_end(h, rc) };
    (rc == PAM_SUCCESS, data.note.into_inner())
}

/// What the worker reports for attempt `id`.
pub struct Verdict {
    pub id: u64,
    pub ok: bool,
    pub note: Option<String>,
}

/// The auth worker. `submit` hands over the password; the verdict arrives
/// on the channel given to `spawn`. Only one attempt runs at a time. A
/// worker stuck inside a PAM module is abandoned (see `session`), and a
/// fresh one spawned; the attempt ids tell their verdicts apart.
pub struct Authenticator {
    tx: mpsc::Sender<(u64, Secret)>,
}

impl Authenticator {
    pub fn spawn(login: &str, service: &str, done: calloop::channel::Sender<Verdict>) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<(u64, Secret)>();
        let login = CString::new(login).unwrap_or_default();
        let service = CString::new(service).unwrap_or_default();
        std::thread::Builder::new()
            .name("nimbus-pam".into())
            .spawn(move || {
                for (id, secret) in rx {
                    let (ok, note) = authenticate(&service, &login, &secret);
                    drop(secret); // wiped here, before the UI hears back
                    if done.send(Verdict { id, ok, note }).is_err() {
                        return;
                    }
                }
            })
            .ok()?;
        Some(Self { tx })
    }

    /// False if the worker is gone (the secret is wiped here then).
    pub fn submit(&self, id: u64, secret: Secret) -> bool {
        self.tx.send((id, secret)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_edits_in_place() {
        let mut s = Secret::new();
        let p = s.ptr;
        assert_eq!(p as usize % PAGE, 0);
        for c in "pässwörd€".chars() {
            assert!(s.push(c));
        }
        assert_eq!(s.chars(), 9);
        assert!(s.pop());
        assert_eq!(s.chars(), 8);
        assert_eq!(std::str::from_utf8(s.bytes()).unwrap(), "pässwörd");
        // Popped bytes were wiped.
        let tail = unsafe { std::slice::from_raw_parts(p.add(s.len), 3) };
        assert_eq!(tail, &[0, 0, 0]);
        s.clear();
        assert!(s.is_empty());
        let head = unsafe { std::slice::from_raw_parts(p, 4) };
        assert_eq!(head, &[0, 0, 0, 0]);
    }

    #[test]
    fn secret_is_bounded() {
        let mut s = Secret::new();
        let mut n = 0;
        while s.push('x') {
            n += 1;
        }
        assert_eq!(n, CAP);
    }

    #[test]
    fn secrets_have_their_own_pages() {
        let a = Secret::new();
        let b = Secret::new();
        assert_ne!(a.ptr as usize / PAGE, b.ptr as usize / PAGE);
    }
}
