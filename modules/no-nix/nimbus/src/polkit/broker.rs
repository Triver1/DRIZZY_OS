//! The request queue: one thread owns every pending authentication, so a
//! request has exactly one state and one place that can finish it.
//!
//! polkitd's `BeginAuthentication` call has no timeout, so an agent that
//! stops answering hangs the app forever. Everything here therefore ends in
//! a reply: `Queue` holds each request's reply handle until `finish`, the
//! handle answers `Cancelled` if it is ever dropped unused, and every wait
//! has a deadline that runs on this thread, independent of the UI.
//!
//! Only the head of the queue is on screen and talking to a helper. The
//! password arrives here by value from the UI, goes straight to the helper
//! (or waits in its `Secret` for the helper's prompt) and is wiped by
//! whichever of those ends it.

use std::{
    collections::VecDeque,
    future::Future,
    io,
    os::unix::net::UnixStream,
    pin::Pin,
    sync::{Arc, Mutex, mpsc},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use super::{
    helper::{self, Cookie, Event},
    log,
};
use crate::lock::Secret;

/// A user who may answer, as shown on the card (sanitised).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    pub uid: u32,
    pub login: String,
    pub name: String,
}

/// What the card shows. Sanitised, and free of cookies: this is all that
/// ever crosses to the UI thread.
#[derive(Debug, PartialEq, Eq)]
pub struct Info {
    pub id: u64,
    pub message: String,
    pub action: String,
    pub program: Option<String>,
    pub users: Vec<Who>,
    /// Index into `users` to start with.
    pub user: usize,
}

/// Broker → UI, delivered as `Fact::Custom`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum View {
    /// A request reached the head of the queue.
    Show(Arc<Info>),
    /// The helper asked. `again`: a further prompt of the same attempt, so
    /// the field goes back to taking input.
    Prompt { id: u64, text: String, again: bool },
    /// A line from PAM.
    Note { id: u64, text: String, error: bool },
    /// That password was wrong; the field takes another.
    Wrong { id: u64, note: Option<String>, fails: u32 },
    /// This request is over. `more`: another `Show` follows at once.
    Done { id: u64, ok: bool, note: Option<String>, more: bool },
}

/// What a request ends with, as polkitd sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// polkitd decides from the helper's own response; a stale `Done` with
    /// no such response grants nothing.
    Done,
    Failed,
    Cancelled,
}

/// A single-use reply, awaited on zbus's executor. `Rx` resolves to None if
/// the `Tx` was dropped without sending, which callers treat as cancelled.
pub struct Tx<T>(Arc<Mutex<Slot<T>>>);
pub struct Rx<T>(Arc<Mutex<Slot<T>>>);

struct Slot<T> {
    val: Option<T>,
    closed: bool,
    waker: Option<Waker>,
}

pub fn oneshot<T>() -> (Tx<T>, Rx<T>) {
    let s = Arc::new(Mutex::new(Slot { val: None, closed: false, waker: None }));
    (Tx(s.clone()), Rx(s))
}

impl<T> Tx<T> {
    pub fn send(self, v: T) {
        if let Ok(mut s) = self.0.lock() {
            s.val = Some(v);
        }
    }
}

impl<T> Drop for Tx<T> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.lock() {
            s.closed = true;
            if let Some(w) = s.waker.take() {
                w.wake();
            }
        }
    }
}

impl<T> Future for Rx<T> {
    type Output = Option<T>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let Ok(mut s) = self.0.lock() else { return Poll::Ready(None) };
        if s.val.is_some() || s.closed {
            return Poll::Ready(s.val.take());
        }
        s.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// A new request from the (sender-checked) D-Bus side.
pub struct Begin {
    pub message: String,
    pub action: String,
    pub program: Option<String>,
    pub users: Vec<Who>,
    pub user: usize,
    pub cookie: Cookie,
    pub reply: Tx<Outcome>,
}

/// Everything the broker thread reacts to.
pub enum Msg {
    Begin(Box<Begin>),
    /// polkitd's `CancelAuthentication`; `ack` says whether it was pending.
    Cancel { cookie: Cookie, ack: Tx<bool> },
    /// The password for the current prompt of request `id`.
    Submit { id: u64, secret: Secret },
    /// Cancel button, Escape.
    Dismiss { id: u64 },
    /// Another identity was chosen.
    Pick { id: u64, user: usize },
    /// The window showing `id` is gone (output removed, surface closed).
    Gone { id: u64 },
    Helper { sid: u64, ev: Event },
    /// `org.freedesktop.PolicyKit1` changed hands; its new unique name.
    Owner(String),
    /// The bus connection died.
    BusLost,
    /// SIGTERM/SIGINT: tear down, then `done` lets the process exit.
    Shutdown(mpsc::Sender<()>),
}

/// How long anything may wait.
#[derive(Clone, Copy)]
pub struct Timeouts {
    /// A card nobody answers.
    pub prompt: Duration,
    /// A helper that doesn't come back with a verdict.
    pub verify: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self { prompt: Duration::from_secs(300), verify: Duration::from_secs(30) }
    }
}

/// Requests pending at once. polkitd is the only sender, but any app can
/// make it send: beyond this they are refused, not queued.
pub const MAX_QUEUE: usize = 16;
/// Wrong passwords per request.
pub const MAX_FAILS: u32 = 5;

const UNAVAILABLE: &str = "Authentication is unavailable";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Queued,
    /// On screen, the field is the user's.
    Asking,
    /// A password is on its way or being checked.
    Verifying,
}

/// One PAM transaction. Dropping it closes the socket, which cancels it.
struct Session {
    sid: u64,
    stream: UnixStream,
    /// The helper is waiting for an answer.
    asked: bool,
    /// A prompt of this transaction has been answered.
    answered: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

struct Request {
    info: Arc<Info>,
    cookie: Cookie,
    /// Taken exactly once, by `finish` or `flush`. If this struct is ever
    /// dropped without that, `Tx`'s drop still resolves the caller.
    reply: Option<Tx<Outcome>>,
    user: usize,
    stage: Stage,
    session: Option<Session>,
    /// Submitted before the helper asked.
    pending: Option<Secret>,
    fails: u32,
    /// PAM's last error line for this attempt.
    note: Option<String>,
    deadline: Option<Instant>,
}

pub type Connect = Box<dyn FnMut(&str, &Cookie) -> io::Result<UnixStream> + Send>;
pub type Ui = Box<dyn Fn(View) + Send>;

pub struct Queue {
    ui: Ui,
    connect: Connect,
    tx: mpsc::Sender<Msg>,
    t: Timeouts,
    queue: VecDeque<Request>,
    next_id: u64,
    next_sid: u64,
}

impl Queue {
    pub fn new(ui: Ui, connect: Connect, tx: mpsc::Sender<Msg>, t: Timeouts) -> Self {
        Self { ui, connect, tx, t, queue: VecDeque::new(), next_id: 1, next_sid: 1 }
    }

    /// When `tick` next needs to run.
    pub fn deadline(&self) -> Option<Instant> {
        self.queue.front().and_then(|r| r.deadline)
    }

    /// Handle one message for the queue. Bus and shutdown messages are the
    /// caller's, apart from calling `flush`.
    pub fn handle(&mut self, m: Msg, now: Instant) {
        match m {
            Msg::Begin(b) => self.begin(*b, now),
            Msg::Cancel { cookie, ack } => {
                let known = self.queue.iter().position(|r| r.cookie == cookie);
                ack.send(known.is_some());
                match known {
                    Some(0) => self.finish(Outcome::Cancelled, None, now),
                    Some(i) => {
                        if let Some(mut r) = self.queue.remove(i)
                            && let Some(tx) = r.reply.take()
                        {
                            tx.send(Outcome::Cancelled);
                        }
                    }
                    None => {}
                }
            }
            Msg::Submit { id, secret } => self.submit(id, secret, now),
            Msg::Dismiss { id } | Msg::Gone { id } => {
                if self.head_is(id) {
                    self.finish(Outcome::Cancelled, None, now);
                }
            }
            Msg::Pick { id, user } => self.pick(id, user, now),
            Msg::Helper { sid, ev } => self.helper(sid, ev, now),
            Msg::Owner(_) | Msg::BusLost | Msg::Shutdown(_) => {}
        }
    }

    fn head_is(&self, id: u64) -> bool {
        self.queue.front().is_some_and(|r| r.info.id == id)
    }

    fn begin(&mut self, b: Begin, now: Instant) {
        if self.queue.len() >= MAX_QUEUE {
            log("too many pending requests: refusing one");
            b.reply.send(Outcome::Cancelled);
            return;
        }
        if self.queue.iter().any(|r| r.cookie == b.cookie) {
            b.reply.send(Outcome::Failed);
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let info = Info { id, message: b.message, action: b.action, program: b.program, user: b.user, users: b.users };
        self.queue.push_back(Request {
            info: Arc::new(info),
            cookie: b.cookie,
            reply: Some(b.reply),
            user: b.user,
            stage: Stage::Queued,
            session: None,
            pending: None,
            fails: 0,
            note: None,
            deadline: None,
        });
        if self.queue.len() == 1 {
            self.present(now);
        }
    }

    /// Put the head on screen and start its first PAM transaction.
    fn present(&mut self, now: Instant) {
        let Some(r) = self.queue.front_mut() else { return };
        r.stage = Stage::Asking;
        r.deadline = Some(now + self.t.prompt);
        (self.ui)(View::Show(r.info.clone()));
        self.open_session(now);
    }

    fn open_session(&mut self, now: Instant) {
        let Some(r) = self.queue.front_mut() else { return };
        let sid = self.next_sid;
        self.next_sid += 1;
        // The head's identity list is non-empty and `user` indexes it.
        let opened = match r.info.users.get(r.user) {
            Some(u) => (self.connect)(&u.login, &r.cookie).and_then(|s| Ok((s.try_clone()?, s))),
            None => Err(io::ErrorKind::InvalidInput.into()),
        };
        let started = opened.and_then(|(reader, stream)| {
            let tx = self.tx.clone();
            helper::read_events(reader, move |ev| {
                let _ = tx.send(Msg::Helper { sid, ev });
            })?;
            Ok(stream)
        });
        match started {
            Ok(stream) => r.session = Some(Session { sid, stream, asked: false, answered: false }),
            Err(_) => {
                log("could not reach the polkit helper");
                self.finish(Outcome::Failed, Some(UNAVAILABLE.into()), now);
            }
        }
    }

    fn submit(&mut self, id: u64, mut secret: Secret, now: Instant) {
        // Anything not for the current prompt is dropped, which wipes it.
        let Some(r) = self.queue.front_mut() else { return };
        if r.info.id != id || r.stage != Stage::Asking {
            return;
        }
        if !helper::acceptable(&secret) {
            secret.clear();
            let id = r.info.id;
            (self.ui)(View::Wrong { id, note: Some("That password can't be used here".into()), fails: r.fails });
            return;
        }
        r.stage = Stage::Verifying;
        r.deadline = Some(now + self.t.verify);
        if r.session.as_ref().is_some_and(|s| s.asked) {
            self.answer(secret, now);
        } else {
            r.pending = Some(secret);
        }
    }

    /// Hand the password to the helper. Whatever happens it's wiped here.
    fn answer(&mut self, mut secret: Secret, now: Instant) {
        let Some(s) = self.queue.front_mut().and_then(|r| r.session.as_mut()) else { return };
        s.asked = false;
        s.answered = true;
        let r = helper::respond(&s.stream, &mut secret);
        drop(secret);
        if r.is_err() {
            log("could not write to the polkit helper");
            self.finish(Outcome::Failed, Some(UNAVAILABLE.into()), now);
        }
    }

    fn pick(&mut self, id: u64, user: usize, now: Instant) {
        let Some(r) = self.queue.front_mut() else { return };
        if r.info.id != id || r.stage != Stage::Asking || user >= r.info.users.len() || user == r.user {
            return;
        }
        r.user = user;
        r.note = None;
        r.session = None;
        r.deadline = Some(now + self.t.prompt);
        self.open_session(now);
    }

    fn helper(&mut self, sid: u64, ev: Event, now: Instant) {
        // A stale session's last words, or a request already gone.
        let Some(r) = self.queue.front_mut() else { return };
        if r.session.as_ref().is_none_or(|s| s.sid != sid) {
            return;
        }
        let id = r.info.id;
        match ev {
            Event::Prompt(text) => {
                let Some(s) = r.session.as_mut() else { return };
                s.asked = true;
                let pending = r.pending.take();
                let again = s.answered && pending.is_none();
                if again {
                    // A second question in one transaction (a code after a
                    // password): the field is the user's again.
                    r.stage = Stage::Asking;
                    r.deadline = Some(now + self.t.prompt);
                }
                (self.ui)(View::Prompt { id, text, again });
                if let Some(secret) = pending {
                    self.answer(secret, now);
                }
            }
            Event::Info(text) => (self.ui)(View::Note { id, text, error: false }),
            Event::Error(text) => {
                r.note = Some(text.clone());
                (self.ui)(View::Note { id, text, error: true });
            }
            Event::Success => self.finish(Outcome::Done, None, now),
            Event::Failure => {
                if r.session.as_ref().is_some_and(|s| s.answered) {
                    self.wrong(now);
                } else {
                    // It gave up without asking, so this isn't a wrong
                    // password and another try wouldn't help.
                    let note = r.note.take().unwrap_or_else(|| UNAVAILABLE.into());
                    self.finish(Outcome::Failed, Some(note), now);
                }
            }
            Event::Closed | Event::Garbled => {
                log("the polkit helper went away");
                self.finish(Outcome::Failed, Some(UNAVAILABLE.into()), now);
            }
        }
    }

    fn wrong(&mut self, now: Instant) {
        let Some(r) = self.queue.front_mut() else { return };
        r.fails += 1;
        r.session = None;
        r.pending = None;
        let note = r.note.take();
        if r.fails >= MAX_FAILS {
            self.finish(Outcome::Failed, Some("Too many attempts".into()), now);
            return;
        }
        r.stage = Stage::Asking;
        r.deadline = Some(now + self.t.prompt);
        (self.ui)(View::Wrong { id: r.info.id, note, fails: r.fails });
        self.open_session(now);
    }

    /// End the head request and move on to the next.
    fn finish(&mut self, o: Outcome, note: Option<String>, now: Instant) {
        let Some(mut r) = self.queue.pop_front() else { return };
        // Closes the helper connection and wipes a waiting password before
        // anyone hears about it.
        r.session = None;
        r.pending = None;
        if let Some(tx) = r.reply.take() {
            tx.send(o);
        }
        (self.ui)(View::Done { id: r.info.id, ok: o == Outcome::Done, note, more: !self.queue.is_empty() });
        self.present(now);
    }

    /// Cancel everything: the authority went away, or the shell is exiting,
    /// or nobody is at the machine.
    pub fn flush(&mut self) {
        let shown = self.queue.front().map(|r| r.info.id);
        for mut r in self.queue.drain(..) {
            r.session = None;
            r.pending = None;
            if let Some(tx) = r.reply.take() {
                tx.send(Outcome::Cancelled);
            }
        }
        if let Some(id) = shown {
            (self.ui)(View::Done { id, ok: false, note: None, more: false });
        }
    }

    /// Enforce the deadlines.
    pub fn tick(&mut self, now: Instant) {
        let Some(r) = self.queue.front() else { return };
        if r.deadline.is_none_or(|d| d > now) {
            return;
        }
        if r.stage == Stage::Verifying {
            log("the polkit helper did not answer in time");
            self.finish(Outcome::Failed, Some("Authentication timed out".into()), now);
        } else {
            // Nobody is answering: the ones behind it won't fare better.
            log("nobody answered: cancelling");
            self.flush();
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polkit::helper::testing::{Seen, serve};
    use std::sync::atomic::{AtomicU32, Ordering};

    const PASSWORD: &[u8] = b"correct horse";

    fn who(uid: u32, login: &str) -> Who {
        Who { uid, login: login.into(), name: login.into() }
    }

    fn begin(cookie: &str) -> (Msg, Rx<Outcome>) {
        begin_as(cookie, vec![who(1000, "alice")])
    }

    fn begin_as(cookie: &str, users: Vec<Who>) -> (Msg, Rx<Outcome>) {
        let (tx, rx) = oneshot();
        let b = Begin {
            message: "Run a thing".into(),
            action: "org.test.run".into(),
            program: Some("/bin/true".into()),
            users,
            user: 0,
            cookie: Cookie::new(cookie.into()).unwrap(),
            reply: tx,
        };
        (Msg::Begin(Box::new(b)), rx)
    }

    fn outcome(rx: Rx<Outcome>) -> Option<Option<Outcome>> {
        let mut rx = rx;
        let w = Waker::noop();
        match Pin::new(&mut rx).poll(&mut Context::from_waker(w)) {
            Poll::Ready(v) => Some(v),
            Poll::Pending => None,
        }
    }

    fn secret(s: &str) -> Secret {
        let mut x = Secret::new();
        for c in s.chars() {
            x.push(c);
        }
        x
    }

    /// A queue wired to a scripted helper. Drives everything on the test's
    /// thread: messages are pumped by hand.
    struct Rig {
        q: Queue,
        rx: mpsc::Receiver<Msg>,
        views: mpsc::Receiver<View>,
        seen: Arc<Mutex<Vec<Seen>>>,
        connects: Arc<AtomicU32>,
        stuck: Arc<Mutex<Vec<UnixStream>>>,
    }

    #[derive(Clone, Copy)]
    enum Mode {
        Normal,
        /// Connection refused.
        Refuse,
        /// Accepts, then says nothing.
        Silent,
        /// Prompts, takes the answer, then says nothing.
        Hang,
        /// Connects, then closes without a verdict.
        Die,
    }

    fn rig(mode: Mode, t: Timeouts) -> Rig {
        let (tx, rx) = mpsc::channel();
        let (vt, views) = mpsc::channel();
        let seen = Arc::new(Mutex::new(vec![]));
        let connects = Arc::new(AtomicU32::new(0));
        let stuck = Arc::new(Mutex::new(vec![]));
        let (s2, c2, st2) = (seen.clone(), connects.clone(), stuck.clone());
        let connect: Connect = Box::new(move |user, cookie| {
            c2.fetch_add(1, Ordering::SeqCst);
            if matches!(mode, Mode::Refuse) {
                return Err(io::ErrorKind::ConnectionRefused.into());
            }
            let (a, b) = UnixStream::pair()?;
            helper::testing_greet(&a, user, cookie)?;
            match mode {
                Mode::Normal => {
                    let seen = s2.clone();
                    std::thread::spawn(move || serve(b, PASSWORD, seen));
                }
                Mode::Silent => st2.lock().unwrap().push(b),
                Mode::Hang => {
                    use std::io::{BufRead, BufReader, Write};
                    let st = st2.clone();
                    std::thread::spawn(move || {
                        let mut w = b.try_clone().unwrap();
                        let mut r = BufReader::new(b);
                        let mut l = String::new();
                        let _ = r.read_line(&mut l);
                        let _ = r.read_line(&mut l);
                        let _ = w.write_all(b"PAM_PROMPT_ECHO_OFF Password: \n");
                        let _ = r.read_line(&mut l);
                        st.lock().unwrap().push(w);
                    });
                }
                Mode::Die => drop(b),
                Mode::Refuse => unreachable!(),
            }
            Ok(a)
        });
        let ui: Ui = Box::new(move |v| {
            let _ = vt.send(v);
        });
        Rig { q: Queue::new(ui, connect, tx, t), rx, views, seen, connects, stuck }
    }

    fn normal() -> Rig {
        rig(Mode::Normal, Timeouts::default())
    }

    impl Rig {
        fn send(&mut self, m: Msg) {
            self.q.handle(m, Instant::now());
        }
        /// Pump helper events until `pred` holds for a view.
        fn until(&mut self, pred: impl Fn(&View) -> bool) -> View {
            let t0 = Instant::now();
            loop {
                while let Ok(v) = self.views.try_recv() {
                    if pred(&v) {
                        return v;
                    }
                }
                assert!(t0.elapsed() < Duration::from_secs(5), "timed out waiting for a view");
                if let Ok(m) = self.rx.recv_timeout(Duration::from_millis(5)) {
                    self.q.handle(m, Instant::now());
                }
            }
        }
        fn prompted(&mut self) -> View {
            self.until(|v| matches!(v, View::Prompt { .. }))
        }
        fn done(&mut self) -> View {
            self.until(|v| matches!(v, View::Done { .. }))
        }
    }

    #[test]
    fn correct_password() {
        let mut r = normal();
        let (m, out) = begin("c-1");
        r.send(m);
        assert!(matches!(r.views.recv().unwrap(), View::Show(i) if i.message == "Run a thing"));
        assert_eq!(r.prompted(), View::Prompt { id: 1, text: "Password".into(), again: false });
        r.send(Msg::Submit { id: 1, secret: secret("correct horse") });
        assert_eq!(r.done(), View::Done { id: 1, ok: true, note: None, more: false });
        assert_eq!(outcome(out), Some(Some(Outcome::Done)));
        assert_eq!(r.q.len(), 0);
        let s = r.seen.lock().unwrap();
        assert_eq!(s[0], Seen { user: "alice".into(), cookie: "c-1".into(), answers: vec![PASSWORD.to_vec()] });
    }

    #[test]
    fn wrong_then_correct_uses_a_new_transaction_and_the_same_cookie() {
        let mut r = normal();
        let (m, out) = begin("c-2");
        r.send(m);
        r.prompted();
        r.send(Msg::Submit { id: 1, secret: secret("nope") });
        assert_eq!(r.until(|v| matches!(v, View::Wrong { .. })), View::Wrong { id: 1, note: None, fails: 1 });
        r.prompted();
        assert_eq!(outcome_pending(&out), true, "a retry must not answer polkitd");
        r.send(Msg::Submit { id: 1, secret: secret("correct horse") });
        assert!(matches!(r.done(), View::Done { ok: true, .. }));
        assert_eq!(outcome(out), Some(Some(Outcome::Done)));
        assert_eq!(r.connects.load(Ordering::SeqCst), 2);
        let s = r.seen.lock().unwrap();
        assert_eq!((s[0].cookie.as_str(), s[1].cookie.as_str()), ("c-2", "c-2"));
    }

    fn outcome_pending(rx: &Rx<Outcome>) -> bool {
        rx.0.lock().unwrap().val.is_none()
    }

    #[test]
    fn too_many_wrong_passwords_end_the_request() {
        let mut r = normal();
        let (m, out) = begin("c-3");
        r.send(m);
        for i in 1..MAX_FAILS {
            r.prompted();
            r.send(Msg::Submit { id: 1, secret: secret("nope") });
            assert!(matches!(r.until(|v| matches!(v, View::Wrong { .. })), View::Wrong { fails, .. } if fails == i));
        }
        r.prompted();
        r.send(Msg::Submit { id: 1, secret: secret("nope") });
        assert!(matches!(r.done(), View::Done { ok: false, note: Some(_), .. }));
        assert_eq!(outcome(out), Some(Some(Outcome::Failed)));
        assert_eq!(r.connects.load(Ordering::SeqCst), MAX_FAILS);
    }

    #[test]
    fn dismissing_cancels_and_closes_the_helper() {
        let mut r = rig(Mode::Silent, Timeouts::default());
        let (m, out) = begin("c-4");
        r.send(m);
        r.send(Msg::Dismiss { id: 1 });
        assert_eq!(r.done(), View::Done { id: 1, ok: false, note: None, more: false });
        assert_eq!(outcome(out), Some(Some(Outcome::Cancelled)));
        // The helper's side reads EOF: its PAM conversation fails.
        let mut b = r.stuck.lock().unwrap().pop().unwrap();
        b.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut v = vec![];
        std::io::Read::read_to_end(&mut b, &mut v).unwrap();
        assert!(v.starts_with(b"alice\nc-4\n"));
    }

    #[test]
    fn cancel_authentication_mid_prompt_closes_at_once() {
        let mut r = normal();
        let (m, out) = begin("c-5");
        r.send(m);
        r.prompted();
        let (ack, known) = oneshot();
        r.send(Msg::Cancel { cookie: Cookie::new("c-5".into()).unwrap(), ack });
        assert_eq!(known.0.lock().unwrap().val, Some(true));
        assert_eq!(r.done(), View::Done { id: 1, ok: false, note: None, more: false });
        assert_eq!(outcome(out), Some(Some(Outcome::Cancelled)));
    }

    #[test]
    fn cancelling_an_unknown_cookie_changes_nothing() {
        let mut r = normal();
        let (m, out) = begin("c-6");
        r.send(m);
        let (ack, known) = oneshot();
        r.send(Msg::Cancel { cookie: Cookie::new("other".into()).unwrap(), ack });
        assert_eq!(known.0.lock().unwrap().val, Some(false));
        assert_eq!(r.q.len(), 1);
        assert!(outcome_pending(&out));
    }

    #[test]
    fn requests_queue_and_run_in_order() {
        let mut r = normal();
        let (m1, o1) = begin("a");
        let (m2, o2) = begin("b");
        let (m3, o3) = begin("c");
        r.send(m1);
        r.send(m2);
        r.send(m3);
        assert_eq!(r.connects.load(Ordering::SeqCst), 1, "only the head talks to a helper");
        assert!(matches!(r.views.recv().unwrap(), View::Show(i) if i.id == 1));
        r.prompted();
        r.send(Msg::Submit { id: 1, secret: secret("correct horse") });
        assert_eq!(r.done(), View::Done { id: 1, ok: true, note: None, more: true });
        assert!(matches!(r.until(|v| matches!(v, View::Show(_))), View::Show(i) if i.id == 2));
        // Cancelling one behind the head shows nothing and skips it.
        let (ack, known) = oneshot();
        r.send(Msg::Cancel { cookie: Cookie::new("c".into()).unwrap(), ack });
        assert_eq!(known.0.lock().unwrap().val, Some(true));
        assert_eq!(outcome(o3), Some(Some(Outcome::Cancelled)));
        r.prompted();
        r.send(Msg::Dismiss { id: 2 });
        assert_eq!(r.done(), View::Done { id: 2, ok: false, note: None, more: false });
        assert_eq!(outcome(o1), Some(Some(Outcome::Done)));
        assert_eq!(outcome(o2), Some(Some(Outcome::Cancelled)));
        assert_eq!(r.q.len(), 0);
    }

    #[test]
    fn the_queue_is_bounded_and_duplicate_cookies_are_refused() {
        let mut r = rig(Mode::Silent, Timeouts::default());
        let mut keep = vec![];
        let (m, o) = begin("q0");
        r.send(m);
        keep.push(o);
        let (m, o) = begin("q0");
        r.send(m);
        assert_eq!(outcome(o), Some(Some(Outcome::Failed)));
        for i in 1..MAX_QUEUE {
            let (m, o) = begin(&format!("q{i}"));
            r.send(m);
            keep.push(o);
        }
        assert_eq!(r.q.len(), MAX_QUEUE);
        let (m, o) = begin("over");
        r.send(m);
        assert_eq!(outcome(o), Some(Some(Outcome::Cancelled)));
        assert_eq!(r.q.len(), MAX_QUEUE);
    }

    #[test]
    fn stale_ids_cannot_touch_the_head() {
        let mut r = normal();
        let (m, out) = begin("c-7");
        r.send(m);
        r.prompted();
        r.send(Msg::Dismiss { id: 99 });
        r.send(Msg::Gone { id: 99 });
        r.send(Msg::Submit { id: 99, secret: secret("correct horse") });
        r.send(Msg::Pick { id: 99, user: 0 });
        assert_eq!(r.q.len(), 1);
        assert!(outcome_pending(&out));
        assert_eq!(r.connects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stale_helper_lines_are_ignored() {
        let mut r = normal();
        let (m, out) = begin("c-8");
        r.send(m);
        r.prompted();
        // A previous session's verdict must not finish this request.
        r.send(Msg::Helper { sid: 77, ev: Event::Success });
        r.send(Msg::Helper { sid: 77, ev: Event::Failure });
        r.send(Msg::Helper { sid: 77, ev: Event::Closed });
        assert_eq!(r.q.len(), 1);
        assert!(outcome_pending(&out));
    }

    #[test]
    fn a_password_sent_early_waits_for_the_prompt() {
        let mut r = normal();
        let (m, out) = begin("c-9");
        r.send(m);
        // Submitted before the helper's prompt has been read.
        r.send(Msg::Submit { id: 1, secret: secret("correct horse") });
        assert!(matches!(r.done(), View::Done { ok: true, .. }));
        assert_eq!(outcome(out), Some(Some(Outcome::Done)));
    }

    #[test]
    fn a_double_submit_is_dropped() {
        let mut r = rig(Mode::Hang, Timeouts::default());
        let (m, _out) = begin("c-10");
        r.send(m);
        r.prompted();
        r.send(Msg::Submit { id: 1, secret: secret("one") });
        let probe = secret("two");
        r.send(Msg::Submit { id: 1, secret: probe });
        assert_eq!(r.q.queue[0].stage, Stage::Verifying);
        assert!(r.q.queue[0].pending.is_none());
    }

    #[test]
    fn helper_unreachable_fails_closed() {
        let mut r = rig(Mode::Refuse, Timeouts::default());
        let (m, out) = begin("c-11");
        r.send(m);
        assert!(matches!(r.done(), View::Done { ok: false, note: Some(_), more: false, .. }));
        assert_eq!(outcome(out), Some(Some(Outcome::Failed)));
        assert_eq!(r.q.len(), 0);
    }

    #[test]
    fn helper_failure_moves_on_to_the_next_request() {
        let mut r = rig(Mode::Refuse, Timeouts::default());
        let (m1, o1) = begin("x");
        let (m2, o2) = begin("y");
        r.send(m1);
        // The first is already gone by the time the second arrives.
        r.send(m2);
        assert_eq!(outcome(o1), Some(Some(Outcome::Failed)));
        assert_eq!(outcome(o2), Some(Some(Outcome::Failed)));
    }

    #[test]
    fn helper_dying_mid_prompt_fails_closed() {
        let mut r = rig(Mode::Die, Timeouts::default());
        let (m, out) = begin("c-12");
        r.send(m);
        assert!(matches!(r.done(), View::Done { ok: false, note: Some(_), .. }));
        assert_eq!(outcome(out), Some(Some(Outcome::Failed)));
    }

    #[test]
    fn failure_without_an_answer_is_not_a_wrong_password() {
        let mut r = rig(Mode::Silent, Timeouts::default());
        let (m, out) = begin("c-13");
        r.send(m);
        let sid = r.q.queue[0].session.as_ref().unwrap().sid;
        r.send(Msg::Helper { sid, ev: Event::Error("Account locked".into()) });
        r.send(Msg::Helper { sid, ev: Event::Failure });
        assert!(matches!(r.done(), View::Done { ok: false, note: Some(n), .. } if n == "Account locked"));
        assert_eq!(outcome(out), Some(Some(Outcome::Failed)));
        assert_eq!(r.connects.load(Ordering::SeqCst), 1, "no retry loop");
    }

    #[test]
    fn a_second_prompt_goes_back_to_the_user() {
        let mut r = rig(Mode::Silent, Timeouts::default());
        let (m, _out) = begin("c-14");
        r.send(m);
        let sid = r.q.queue[0].session.as_ref().unwrap().sid;
        r.send(Msg::Helper { sid, ev: Event::Prompt("Password".into()) });
        assert_eq!(r.prompted(), View::Prompt { id: 1, text: "Password".into(), again: false });
        r.send(Msg::Submit { id: 1, secret: secret("first") });
        r.send(Msg::Helper { sid, ev: Event::Prompt("Code".into()) });
        assert_eq!(r.prompted(), View::Prompt { id: 1, text: "Code".into(), again: true });
        assert_eq!(r.q.queue[0].stage, Stage::Asking);
        r.send(Msg::Submit { id: 1, secret: secret("123456") });
        let mut b = r.stuck.lock().unwrap().pop().unwrap();
        b.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let mut v = vec![0u8; 256];
        let n = std::io::Read::read(&mut b, &mut v).unwrap();
        assert_eq!(&v[..n], b"alice\nc-14\nfirst\n123456\n");
    }

    #[test]
    fn nobody_answering_cancels_the_whole_queue() {
        let t = Timeouts { prompt: Duration::from_millis(20), verify: Duration::from_secs(30) };
        let mut r = rig(Mode::Silent, t);
        let (m1, o1) = begin("t1");
        let (m2, o2) = begin("t2");
        r.send(m1);
        r.send(m2);
        assert!(r.q.deadline().is_some());
        r.q.tick(Instant::now());
        assert_eq!(r.q.len(), 2, "not yet");
        std::thread::sleep(Duration::from_millis(30));
        r.q.tick(Instant::now());
        assert_eq!(r.q.len(), 0);
        assert_eq!(outcome(o1), Some(Some(Outcome::Cancelled)));
        assert_eq!(outcome(o2), Some(Some(Outcome::Cancelled)));
        assert!(r.q.deadline().is_none());
        assert!(matches!(r.done(), View::Done { id: 1, ok: false, more: false, .. }));
    }

    #[test]
    fn a_hung_helper_times_out_and_the_next_request_runs() {
        let t = Timeouts { prompt: Duration::from_secs(300), verify: Duration::from_millis(20) };
        let mut r = rig(Mode::Hang, t);
        let (m1, o1) = begin("h1");
        let (m2, o2) = begin("h2");
        r.send(m1);
        r.send(m2);
        r.prompted();
        r.send(Msg::Submit { id: 1, secret: secret("whatever") });
        std::thread::sleep(Duration::from_millis(30));
        r.q.tick(Instant::now());
        assert_eq!(outcome(o1), Some(Some(Outcome::Failed)));
        assert_eq!(r.q.len(), 1);
        assert!(outcome_pending(&o2));
    }

    #[test]
    fn a_secret_waiting_for_a_prompt_is_wiped_on_cancel() {
        let mut r = rig(Mode::Silent, Timeouts::default());
        let (m, _out) = begin("w1");
        r.send(m);
        let s = secret("hunter2");
        let (p, n) = (s.raw(), s.bytes().len());
        r.send(Msg::Submit { id: 1, secret: s });
        assert_eq!(unsafe { std::slice::from_raw_parts(p, n) }, b"hunter2");
        // Reach into the queue before it is dropped: `Secret::drop` unmaps
        // the page, so check the wipe by clearing on the same path it takes.
        r.q.queue[0].pending.as_mut().unwrap().clear();
        assert_eq!(unsafe { std::slice::from_raw_parts(p, n) }, &[0u8; 7]);
        r.send(Msg::Dismiss { id: 1 });
        assert!(r.q.queue.is_empty());
    }

    #[test]
    fn a_request_dropped_unanswered_resolves_cancelled() {
        // The last line of defence: even a bug that loses the request
        // can't leave polkitd's call hanging.
        let (tx, rx) = oneshot::<Outcome>();
        drop(tx);
        assert_eq!(outcome(rx), Some(None));
    }

    #[test]
    fn flush_answers_everyone() {
        let mut r = rig(Mode::Silent, Timeouts::default());
        let mut rs = vec![];
        for i in 0..3 {
            let (m, o) = begin(&format!("f{i}"));
            r.send(m);
            rs.push(o);
        }
        r.q.flush();
        for o in rs {
            assert_eq!(outcome(o), Some(Some(Outcome::Cancelled)));
        }
    }

    #[test]
    fn picking_another_user_restarts_as_them() {
        let mut r = normal();
        let (m, out) = begin_as("p1", vec![who(1000, "alice"), who(1001, "bob")]);
        r.send(m);
        r.prompted();
        r.send(Msg::Pick { id: 1, user: 1 });
        r.prompted();
        r.send(Msg::Submit { id: 1, secret: secret("correct horse") });
        assert!(matches!(r.done(), View::Done { ok: true, .. }));
        assert_eq!(outcome(out), Some(Some(Outcome::Done)));
        let t0 = Instant::now();
        while r.seen.lock().unwrap().len() < 2 && t0.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let s = r.seen.lock().unwrap();
        let of = |who: &str| s.iter().find(|x| x.user == who).unwrap().answers.clone();
        assert!(of("alice").is_empty(), "alice's session must not see the password");
        assert_eq!(of("bob"), [PASSWORD.to_vec()]);
    }

    #[test]
    fn out_of_range_picks_are_ignored() {
        let mut r = normal();
        let (m, _o) = begin_as("p2", vec![who(1000, "alice")]);
        r.send(m);
        r.prompted();
        r.send(Msg::Pick { id: 1, user: 5 });
        assert_eq!(r.q.queue[0].user, 0);
        assert_eq!(r.connects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn nothing_for_the_ui_ever_holds_the_cookie() {
        let mut r = normal();
        let (m, _o) = begin("secret-cookie-value");
        r.send(m);
        let v = r.prompted();
        let shown = format!("{v:?}");
        assert!(!shown.contains("secret-cookie-value"));
        r.send(Msg::Dismiss { id: 1 });
        let d = format!("{:?}", r.done());
        assert!(!d.contains("secret-cookie-value"));
    }
}
