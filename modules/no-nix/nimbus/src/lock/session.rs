//! Wayland side of `nimbus lock`: ext-session-lock-v1, one lock surface
//! per output (outputs added while locked get one too), keyboard with
//! repeat, PAM verdicts, and frame pacing like the bar (wl_surface.frame +
//! wp_presentation, fractional scale + viewporter).
//!
//! Safety rules: the lock is requested before anything else; the only
//! `unlock_and_destroy` is in `unlock`, reached only from a successful
//! PAM verdict. Every error path exits without unlocking (the compositor
//! keeps the session locked when a locker dies).
//!
//! Never a lockout: the GPU is optional (no Vulkan, a failed swapchain or
//! a lost device switch every output to the CPU-drawn `soft` screen); a
//! panic, or a main thread that stops making progress for 30 s (watchdog),
//! re-executes the locker in software mode, which takes over the lock
//! (both niri and sway accept a new lock client once the old one is
//! gone); a PAM module that never returns is abandoned after 30 s.

use std::{
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use calloop::{
    EventLoop, LoopHandle, channel,
    ping::make_ping,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_dispatch2, delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    presentation_time::{PresentTime, PresentationTimeHandler, PresentationTimeState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager},
    },
    session_lock::{SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure},
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use wayland_client::{
    Connection, Proxy, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};
use wayland_protocols::wp::{
    cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape as CursorShape, WpCursorShapeDeviceV1},
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, wp_fractional_scale_v1},
    presentation_time::client::wp_presentation_feedback,
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

use super::{
    assets::{self, Backdrop},
    auth::{Authenticator, Secret, User, Verdict},
    log,
    scene::{Edit, SUCCESS_TOTAL, Scene},
    soft,
};
use crate::{
    FrameClock,
    gfx::{DrawList, Gpu, ImageData, Icons, SurfaceGfx},
    icons::IconSet,
    monotonic_ns,
    services::{BatteryInfo, ClockInfo},
    theme::{self, Rgba},
};

/// Minimum time between the starts of two PAM attempts (PAM adds its own
/// delay after a failure; this also covers stacks that don't).
const MIN_GAP: Duration = Duration::from_millis(1000);
/// A PAM attempt that hasn't answered by then is abandoned.
const PAM_TIMEOUT: Duration = Duration::from_secs(30);
/// The watchdog restarts the locker after this long without a loop turn
/// (the clock timer guarantees one at least every ~15 s).
const STALL: Duration = Duration::from_secs(30);
/// Locked and untouched this long: the screen goes to sleep (any input
/// wakes it).
const SLEEP_AFTER_MS: u32 = 20_000;
/// Self-restarts allowed before giving up (each one re-locks first).
const MAX_RESPAWNS: u32 = 4;

/// Bumped on every event-loop turn; watched by the watchdog thread.
static PROGRESS: AtomicU64 = AtomicU64::new(0);
/// The session was unlocked: never re-lock from here on.
static UNLOCKED: AtomicBool = AtomicBool::new(false);
/// A `--daemonize` parent was told; its pipe is closed.
static NOTIFIED: AtomicBool = AtomicBool::new(false);

enum Loaded {
    Wall((u32, u32), Backdrop),
    Avatar(Option<ImageData>),
}

struct Surf {
    // Field order matters: the swapchain must go before the wl_surface.
    gfx: Option<SurfaceGfx>,
    /// Software-mode buffers (at most two in flight).
    bufs: Vec<Buffer>,
    viewport: Option<WpViewport>,
    fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    lock_surface: SessionLockSurface,
    output: wl_output::WlOutput,
    /// None in software mode.
    scene: Option<Scene>,
    size: (u32, u32),
    scale120: u32,
    bg_size: Option<(u32, u32)>,
    /// A frame has been shown, so the scale is final: decode then (not
    /// at the integer scale of the very first configure).
    shown: bool,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    animating: bool,
    /// Consecutive frames that couldn't be presented (retry backoff).
    misses: u32,
    clock: FrameClock,
}

impl Surf {
    fn scale(&self) -> f32 {
        self.scale120 as f32 / 120.0
    }
    fn physical(&self) -> (u32, u32) {
        let s = self.scale();
        ((self.size.0 as f32 * s).round() as u32, (self.size.1 as f32 * s).round() as u32)
    }
    fn scene(&mut self) -> Option<&mut Scene> {
        self.scene.as_mut()
    }
}

struct LockApp {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    _lock_state: SessionLockState,
    lock: SessionLock,
    locked: bool,
    unlock_wanted: bool,
    conn: Connection,
    fractional_mgr: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    presentation: PresentationTimeState,
    cursor_shapes: Option<CursorShapeManager>,
    pointer: Option<wl_pointer::WlPointer>,
    shape_device: Option<WpCursorShapeDeviceV1>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    idle_mgr: Option<ExtIdleNotifierV1>,
    idle: Option<ExtIdleNotificationV1>,

    /// None in software mode (no Vulkan, or the device was lost).
    gpu: Option<Gpu>,
    icons: Option<Icons>,
    shm: Option<Shm>,
    pool: Option<SlotPool>,
    retry_pending: bool,
    iconset: Rc<IconSet>,
    surfs: Vec<Surf>,
    primary: Option<wl_output::WlOutput>,
    list: DrawList,
    next_key: u64,

    secret: Secret,
    auth: Option<Authenticator>,
    auth_tx: channel::Sender<Verdict>,
    service: &'static str,
    /// Id of the attempt in flight (or the last one).
    attempt: u64,
    busy: bool,
    success: bool,
    last_attempt: Option<Instant>,
    /// Software mode: the last attempt failed (until the next keystroke).
    soft_wrong: bool,

    user: User,
    wallpaper: Option<PathBuf>,
    accent: (Rgba, Rgba),
    requested: Vec<(u32, u32)>,
    avatar: Option<ImageData>,
    load_tx: channel::Sender<Loaded>,

    clock: Option<ClockInfo>,
    battery: Option<BatteryInfo>,
    handle: LoopHandle<'static, LockApp>,
    qh: QueueHandle<LockApp>,
    start_ns: u64,
    wake_pending: Option<f32>,
    exit: Option<i32>,
}

fn battery() -> Option<BatteryInfo> {
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    for e in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let p = e.path();
        if read(&p.join("type")).as_deref() == Some("Battery") {
            let percent = read(&p.join("capacity"))?.parse::<u8>().ok()?;
            let status = read(&p.join("status")).unwrap_or_default();
            return Some(BatteryInfo { percent: percent.min(100), charging: status == "Charging" || status == "Full" });
        }
    }
    None
}

/// Set by `--daemonize` for the child: a pipe to report "locked" on.
const NOTIFY_ENV: &str = "NIMBUS_LOCK_NOTIFY_FD";
/// Set on a self-restart: how many restarts so far.
const GEN_ENV: &str = "NIMBUS_LOCK_GEN";

/// `nimbus lock --daemonize`: start the locker as a detached child and
/// return once the compositor has confirmed the lock (0), or non-zero if
/// the child failed first. For `swayidle -w before-sleep`.
fn daemonize() -> i32 {
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        log(format_args!("pipe failed"));
        return 1;
    }
    let (rd, wr) = (fds[0], fds[1]);
    // The child inherits only the write end.
    unsafe { libc::fcntl(wr, libc::F_SETFD, 0) };
    let mut cmd = self_command();
    cmd.arg("lock").env(NOTIFY_ENV, wr.to_string()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
    if std::env::args().any(|a| a == "--software") {
        cmd.arg("--software");
    }
    // Our stderr (a terminal, swayidle's pipe) may vanish while the child
    // lives on: log to the runtime dir instead.
    cmd.stderr(child_log());
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd.spawn();
    unsafe { libc::close(wr) };
    if let Err(e) = child {
        log(format_args!("spawn failed: {e}"));
        return 1;
    }
    let mut b = [0u8; 1];
    let n = loop {
        let n = unsafe { libc::read(rd, b.as_mut_ptr() as *mut _, 1) };
        if n >= 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break n;
        }
    };
    unsafe { libc::close(rd) };
    if n == 1 { 0 } else { 1 }
}

/// This very binary (even if it was replaced on disk since), keeping our
/// argv[0] so `ps` and `pkill nimbus` still see a nimbus.
fn self_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("/proc/self/exe");
    std::os::unix::process::CommandExt::arg0(&mut cmd, std::env::args_os().next().unwrap_or_else(|| "nimbus".into()));
    cmd
}

/// `$XDG_RUNTIME_DIR/nimbus-lock.log` (0600), else /dev/null.
fn child_log() -> std::process::Stdio {
    use std::os::unix::fs::OpenOptionsExt;
    std::env::var_os("XDG_RUNTIME_DIR")
        .and_then(|d| {
            std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(PathBuf::from(d).join("nimbus-lock.log")).ok()
        })
        .map_or_else(std::process::Stdio::null, Into::into)
}

/// Put the monitors to sleep, in the background so a slow compositor can't
/// hold up the lock screen. Without niri the screen just stays on.
fn screen_off() {
    let cmd = std::process::Command::new("niri")
        .args(["msg", "action", "power-off-monitors"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match cmd {
        Ok(mut c) => {
            let _ = std::thread::Builder::new().name("nimbus-sleep".into()).spawn(move || c.wait());
        }
        Err(e) => log(format_args!("screen off: {e}")),
    }
}

/// Tell a `--daemonize` parent that the session is locked.
fn notify_locked() {
    if NOTIFIED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(fd) = std::env::var(NOTIFY_ENV).ok().and_then(|v| v.parse::<i32>().ok()) {
        unsafe {
            libc::write(fd, b"1".as_ptr() as *const _, 1);
            libc::close(fd);
        }
    }
}

fn generation() -> u32 {
    std::env::var(GEN_ENV).ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Replace this process with a fresh software-mode locker. The old lock
/// object dies with our Wayland connection (the session stays locked) and
/// the new process takes the lock over. Never returns.
fn respawn(why: &str) -> ! {
    if UNLOCKED.load(Ordering::SeqCst) {
        // Already unlocked: nothing to protect, don't lock again.
        log(format_args!("{why} after unlocking; exiting"));
        std::process::exit(0);
    }
    let next = generation() + 1;
    if next > MAX_RESPAWNS {
        log(format_args!("{why}; giving up after {MAX_RESPAWNS} restarts (the session stays locked)"));
        std::process::abort();
    }
    log(format_args!("{why}; restarting in software mode ({next})"));
    let mut cmd = self_command();
    cmd.args(["lock", "--software"]).env(GEN_ENV, next.to_string());
    if NOTIFIED.load(Ordering::SeqCst) {
        cmd.env_remove(NOTIFY_ENV);
    }
    let e = std::os::unix::process::CommandExt::exec(&mut cmd);
    log(format_args!("restart failed: {e}"));
    std::process::abort();
}

/// Restart the locker if the main thread stops turning the event loop
/// (a hung driver call, a blocking roundtrip): a hung client would also
/// keep niri from accepting a replacement locker.
fn spawn_watchdog() {
    let _ = std::thread::Builder::new().name("nimbus-watchdog".into()).spawn(|| {
        let mut last = PROGRESS.load(Ordering::SeqCst);
        let mut since = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = PROGRESS.load(Ordering::SeqCst);
            if now != last {
                last = now;
                since = Instant::now();
            } else if since.elapsed() >= STALL {
                respawn("main thread stalled");
            }
        }
    });
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.iter().any(|a| a == "--daemonize" || a == "-f") {
        return daemonize();
    }
    let software = args.iter().any(|a| a == "--software") || std::env::var_os("NIMBUS_LOCK_SOFTWARE").is_some();
    let generation = generation();
    // No core dumps or same-user ptrace of a process holding a password.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    // Never unwind (which could run destructors mid-lock): restart in
    // software mode, which re-locks before anything else.
    std::panic::set_hook(Box::new(|info| {
        log(format_args!("fatal: {info}"));
        respawn("panic");
    }));
    if generation > 0 {
        // Let the compositor notice the previous locker is gone.
        std::thread::sleep(Duration::from_millis(150 * generation as u64));
    }

    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            log(format_args!("no Wayland display: {e}"));
            return 1;
        }
    };
    let Ok((globals, queue)) = registry_queue_init::<LockApp>(&conn) else {
        log(format_args!("registry failed"));
        return 1;
    };
    let qh = queue.handle();

    // Lock first, before any (slow) GPU or font setup.
    let lock_state = SessionLockState::new(&globals, &qh);
    let lock = match lock_state.lock(&qh) {
        Ok(l) => l,
        Err(e) => {
            log(format_args!("compositor lacks ext-session-lock-v1: {e}"));
            return 1;
        }
    };
    let _ = conn.flush();
    spawn_watchdog();

    let Ok(compositor) = CompositorState::bind(&globals, &qh) else {
        log(format_args!("no wl_compositor"));
        return 1;
    };
    let fractional_mgr = globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, LFracMgr).ok();
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, LViewporter).ok();
    let idle_mgr = globals.bind::<ExtIdleNotifierV1, _, _>(&qh, 1..=1, LIdleMgr).ok();
    let presentation = PresentationTimeState::bind(&globals, &qh);
    let cursor_shapes = CursorShapeManager::bind(&globals, &qh).ok();
    let shm = Shm::bind(&globals, &qh).ok();

    let mut ev: EventLoop<'static, LockApp> = match EventLoop::try_new() {
        Ok(e) => e,
        Err(e) => {
            log(format_args!("event loop: {e}"));
            return 1;
        }
    };
    let handle = ev.handle();

    // The GPU is optional: without it every output gets the soft screen.
    let gpu = if software {
        None
    } else {
        match Gpu::try_new(Some(conn.backend().display_ptr() as *mut _)) {
            Ok(g) => Some(g),
            Err(e) => {
                log(format_args!("{e}; drawing without the GPU"));
                None
            }
        }
    };
    PROGRESS.fetch_add(1, Ordering::SeqCst);
    let lost = if let Some(g) = &gpu {
        let flag = std::sync::Arc::new(AtomicBool::new(false));
        match make_ping() {
            Ok((ping, src)) => {
                let f = flag.clone();
                g.survive_errors(move || {
                    f.store(true, Ordering::SeqCst);
                    ping.ping();
                });
                let _ = handle.insert_source(src, |_, _, app: &mut LockApp| app.go_soft("GPU device lost"));
            }
            Err(_) => g.survive_errors(|| {}),
        }
        Some(flag)
    } else {
        None
    };
    let mut gpu = gpu;
    let iconset = Rc::new(IconSet::discover());
    let icons = gpu.as_mut().map(|g| {
        crate::fonts::load(g.fonts.db_mut(), &["Geist"]);
        Icons::load(&iconset.paths)
    });

    let user = User::current();
    let service = super::auth::service();
    log(format_args!("PAM service \"{service}\", user {}{}", user.login, if gpu.is_none() { ", software mode" } else { "" }));

    let (auth_tx, auth_rx) = channel::channel::<Verdict>();
    let (load_tx, load_rx) = channel::channel::<Loaded>();
    let auth = Authenticator::spawn(&user.login, service, auth_tx.clone());

    let wallpaper = assets::wallpaper_path(&user.home);
    if gpu.is_some() {
        let (tx, home, login) = (load_tx.clone(), user.home.clone(), user.login.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Loaded::Avatar(assets::avatar(&home, &login, 192)));
        });
    }

    let mut app = LockApp {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        compositor,
        _lock_state: lock_state,
        lock,
        locked: false,
        unlock_wanted: false,
        conn: conn.clone(),
        fractional_mgr,
        viewporter,
        presentation,
        cursor_shapes,
        pointer: None,
        shape_device: None,
        keyboard: None,
        idle_mgr,
        idle: None,
        mods: Modifiers::default(),
        gpu,
        icons,
        shm,
        pool: None,
        retry_pending: false,
        iconset,
        surfs: vec![],
        primary: None,
        list: DrawList::default(),
        next_key: 0x10c,
        secret: Secret::new(),
        auth,
        auth_tx,
        service,
        attempt: 0,
        busy: false,
        success: false,
        last_attempt: None,
        soft_wrong: false,
        user,
        wallpaper,
        accent: theme::accent(),
        requested: vec![],
        avatar: None,
        load_tx,
        clock: None,
        battery: battery(),
        handle: handle.clone(),
        qh: qh.clone(),
        start_ns: monotonic_ns(),
        wake_pending: None,
        exit: None,
    };

    if WaylandSource::new(conn.clone(), queue).insert(handle.clone()).is_err() {
        log(format_args!("wayland source"));
        return 1;
    }
    let ok = handle
        .insert_source(auth_rx, |e, _, app: &mut LockApp| {
            if let channel::Event::Msg(v) = e {
                app.verdict(v);
            }
        })
        .is_ok()
        && handle
            .insert_source(load_rx, |e, _, app: &mut LockApp| {
                if let channel::Event::Msg(l) = e {
                    app.loaded(l);
                }
            })
            .is_ok()
        // Clock: wake at each minute boundary (at most every 15 s, so a
        // resume from suspend is caught quickly; also feeds the watchdog).
        && handle
            .insert_source(Timer::immediate(), |_, _, app: &mut LockApp| TimeoutAction::ToDuration(app.tick_clock()))
            .is_ok();
    if !ok {
        log(format_args!("event sources"));
        return 1;
    }

    loop {
        if let Err(e) = ev.dispatch(None, &mut app) {
            match conn.protocol_error() {
                Some(pe) => log(format_args!(
                    "event loop failed: {e}; protocol error {} on {}@{}: {}",
                    pe.code, pe.object_interface, pe.object_id, pe.message
                )),
                None => log(format_args!("event loop failed: {e}")),
            }
            return 1;
        }
        PROGRESS.fetch_add(1, Ordering::SeqCst);
        if lost.as_ref().is_some_and(|f| f.swap(false, Ordering::SeqCst)) {
            app.go_soft("GPU device lost");
        }
        if let Some(code) = app.exit {
            if code == RELOCK {
                respawn("the compositor refused the lock");
            }
            return code;
        }
    }
}

/// `exit` value meaning: respawn and try to take the lock again.
const RELOCK: i32 = -1;

impl LockApp {
    fn surf_of(&self, s: &wl_surface::WlSurface) -> Option<usize> {
        self.surfs.iter().position(|x| x.lock_surface.wl_surface() == s)
    }

    fn primary_index(&self) -> Option<usize> {
        let p = self.primary.as_ref()?;
        self.surfs.iter().position(|s| &s.output == p)
    }

    fn new_scene(&mut self, primary: bool) -> Option<Scene> {
        let gpu = self.gpu.as_mut()?;
        self.next_key += 1;
        let mut scene = Scene::new(&mut gpu.fonts, self.iconset.clone(), &self.user, primary, self.next_key);
        if let Some(c) = self.clock {
            scene.set_clock(c);
        }
        scene.set_battery(self.battery);
        if let Some(a) = &self.avatar {
            scene.set_avatar(a.clone());
        }
        if primary {
            scene.set_caps(self.mods.caps_lock);
        }
        Some(scene)
    }

    fn add_output(&mut self, output: wl_output::WlOutput) {
        if self.surfs.iter().any(|s| s.output == output) || self.success {
            return;
        }
        let qh = self.qh.clone();
        let surface = self.compositor.create_surface(&qh);
        // Fractional scaling needs the viewport to size the surface; without
        // one, stay on integer buffer scales (a buffer that doesn't match
        // the configured size is a protocol error for lock surfaces).
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, &qh, LViewport));
        let fractional = if viewport.is_some() { self.fractional_mgr.as_ref().map(|m| m.get_fractional_scale(&surface, &qh, LFracScale)) } else { None };
        let lock_surface = self.lock.create_lock_surface(surface, &output, &qh);
        let int_scale = self.outputs.info(&output).map_or(1, |i| i.scale_factor.max(1)) as u32;
        if self.primary.is_none() {
            self.primary = Some(output.clone());
        }
        let primary = self.primary.as_ref() == Some(&output);
        let scene = self.new_scene(primary);
        self.surfs.push(Surf {
            gfx: None,
            bufs: vec![],
            viewport,
            fractional,
            lock_surface,
            output,
            scene,
            size: (0, 0),
            scale120: int_scale * 120,
            bg_size: None,
            shown: false,
            configured: false,
            frame_pending: false,
            dirty: true,
            animating: false,
            misses: 0,
            clock: FrameClock::default(),
        });
    }

    fn reconfigure(&mut self, i: usize) {
        let s = &mut self.surfs[i];
        if !s.configured {
            return;
        }
        let soft = self.gpu.is_none();
        let (pw, ph) = s.physical();
        match &s.viewport {
            Some(vp) => vp.set_destination(s.size.0 as i32, s.size.1 as i32),
            None => s.lock_surface.wl_surface().set_buffer_scale(if soft { 1 } else { (s.scale120 / 120).max(1) as i32 }),
        }
        let scale = s.scale();
        let size = s.size;
        if let Some(sc) = s.scene() {
            sc.resize(size.0 as f32, size.1 as f32, scale);
        }
        s.dirty = true;
        if soft {
            return;
        }
        let gpu = self.gpu.as_mut().unwrap();
        let err = match &mut s.gfx {
            Some(g) => {
                gpu.resize(g, pw, ph);
                None
            }
            None => {
                let ptr = s.lock_surface.wl_surface().id().as_ptr() as *mut std::ffi::c_void;
                match gpu.try_create_surface(ptr, pw, ph) {
                    Ok(g) => {
                        s.gfx = Some(g);
                        None
                    }
                    Err(e) => Some(e),
                }
            }
        };
        if let Some(e) = err {
            self.go_soft(&e);
            return;
        }
        if self.surfs[i].shown {
            self.want_backdrop(i);
        }
    }

    /// Drop the GPU for good: every output switches to the soft screen.
    fn go_soft(&mut self, why: &str) {
        if self.gpu.is_none() {
            return;
        }
        log(format_args!("{why}; switching to the software screen"));
        for s in &mut self.surfs {
            // Swapchains go before the device and instance.
            s.gfx = None;
            s.scene = None;
            s.frame_pending = false;
            s.animating = false;
            s.dirty = true;
        }
        self.list.clear();
        self.icons = None;
        self.gpu = None;
        for i in 0..self.surfs.len() {
            self.reconfigure(i);
        }
        self.redraw_all();
    }

    /// Decode the wallpaper for this output's physical size (once per size).
    fn want_backdrop(&mut self, i: usize) {
        if self.gpu.is_none() {
            return;
        }
        let phys = self.surfs[i].physical();
        if self.surfs[i].bg_size == Some(phys) {
            return;
        }
        self.surfs[i].bg_size = Some(phys);
        if self.requested.contains(&phys) {
            return;
        }
        self.requested.push(phys);
        let (tx, path, accent) = (self.load_tx.clone(), self.wallpaper.clone(), self.accent);
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let b = assets::backdrop(path.as_deref(), phys.0, phys.1, accent);
            log(format_args!("wallpaper {}×{} ready in {:.0} ms", phys.0, phys.1, t0.elapsed().as_secs_f32() * 1e3));
            let _ = tx.send(Loaded::Wall(phys, b));
        });
    }

    fn loaded(&mut self, l: Loaded) {
        match l {
            Loaded::Wall(phys, b) => {
                self.requested.retain(|p| *p != phys);
                for i in 0..self.surfs.len() {
                    if self.surfs[i].bg_size == Some(phys)
                        && let Some(sc) = self.surfs[i].scene()
                    {
                        sc.set_backdrop(b.clone());
                        self.request_redraw(i);
                    }
                }
                // `b` drops here: the pixels live on only until uploaded.
            }
            Loaded::Avatar(a) => {
                if let Some(a) = a {
                    for i in 0..self.surfs.len() {
                        if let Some(sc) = self.surfs[i].scene() {
                            sc.set_avatar(a.clone());
                            self.request_redraw(i);
                        }
                    }
                    self.avatar = Some(a);
                }
            }
        }
    }

    fn tick_clock(&mut self) -> Duration {
        let (c, sec) = crate::services::now_local();
        if self.clock != Some(c) {
            self.clock = Some(c);
            self.battery = battery();
            for i in 0..self.surfs.len() {
                let b = self.battery;
                if let Some(sc) = self.surfs[i].scene() {
                    sc.set_clock(c);
                    sc.set_battery(b);
                    self.request_redraw(i);
                }
            }
        }
        Duration::from_millis(((60 - sec.min(59)) as u64 * 1000).min(15_000) + 20)
    }

    fn set_primary(&mut self, output: &wl_output::WlOutput) {
        if self.primary.as_ref() == Some(output) || self.busy || self.success {
            return;
        }
        self.primary = Some(output.clone());
        let n = self.secret.chars();
        let caps = self.mods.caps_lock;
        for i in 0..self.surfs.len() {
            let on = &self.surfs[i].output == output;
            if let Some(sc) = self.surfs[i].scene() {
                sc.set_primary(on, n);
                if on {
                    sc.set_caps(caps);
                }
            }
            self.surfs[i].dirty = true;
            self.request_redraw(i);
        }
    }

    fn request_redraw(&mut self, i: usize) {
        let s = &mut self.surfs[i];
        if s.frame_pending {
            s.dirty = true;
        } else {
            self.draw(i);
        }
    }

    fn redraw_all(&mut self) {
        for i in 0..self.surfs.len() {
            self.request_redraw(i);
        }
    }

    fn draw(&mut self, i: usize) {
        if self.gpu.is_none() {
            self.draw_soft(i);
            return;
        }
        let qh = self.qh.clone();
        let s = &mut self.surfs[i];
        if !s.configured || s.gfx.is_none() || s.scene.is_none() {
            return;
        }
        let gpu = self.gpu.as_mut().unwrap();
        let sc = s.scene.as_mut().unwrap();
        let now_ns = monotonic_ns();
        let dt = s.clock.advance(now_ns);
        sc.sync_time((now_ns - self.start_ns) as f32 / 1e9 - dt);
        s.animating = sc.step(dt);
        sc.draw(&mut gpu.fonts, &mut self.list);
        let surface = s.lock_surface.wl_surface().clone();
        surface.frame(&qh, FrameCallbackData(surface.clone()));
        if s.animating {
            let _ = self.presentation.feedback(&surface, &qh);
        }
        let presented = gpu.render(s.gfx.as_mut().unwrap(), &self.list, sc.text_buffers(), self.icons.as_ref().unwrap());
        // Pixels were uploaded (or not: the scene re-queues nothing, so a
        // failed upload simply shows no image); don't keep them in RAM.
        self.list.uploads.clear();
        if presented {
            s.frame_pending = true;
            s.dirty = false;
            s.misses = 0;
            let wake = if s.animating { None } else { sc.wake_at().map(|at| (at, sc.time)) };
            if let Some((at, now)) = wake {
                self.schedule_wake(at, now);
            }
        } else {
            // Nothing was committed, so no frame callback will come: retry.
            s.dirty = true;
            s.misses += 1;
            let ms = (16u64 << s.misses.min(3)).min(100);
            self.schedule_retry(Duration::from_millis(ms));
        }
    }

    /// Software mode: paint the soft screen into a free shm buffer.
    fn draw_soft(&mut self, i: usize) {
        if !self.surfs[i].configured {
            return;
        }
        let Some(shm) = &self.shm else { return };
        let s = &mut self.surfs[i];
        // Logical-size buffer; the viewport (or buffer scale 1) maps it 1:1
        // onto the configured size.
        let (w, h) = s.size;
        let stride = w as i32 * 4;
        if self.pool.is_none() {
            match SlotPool::new((w * h * 4 * 2) as usize, shm) {
                Ok(p) => self.pool = Some(p),
                Err(e) => {
                    log(format_args!("shm pool: {e}"));
                    return;
                }
            }
        }
        let pool = self.pool.as_mut().unwrap();
        s.bufs.retain(|b| b.height() == h as i32 && b.stride() == stride);
        let free = s.bufs.iter().position(|b| b.slot().has_active_buffers() == false);
        let idx = match free {
            Some(k) => k,
            None if s.bufs.len() < 2 => match pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Xrgb8888) {
                Ok((b, _)) => {
                    s.bufs.push(b);
                    s.bufs.len() - 1
                }
                Err(e) => {
                    log(format_args!("shm buffer: {e}"));
                    return;
                }
            },
            None => {
                s.dirty = true;
                self.schedule_retry(Duration::from_millis(16));
                return;
            }
        };
        let primary = self.primary.as_ref() == Some(&s.output);
        let mood = if self.success {
            soft::Mood::Done
        } else if self.busy {
            soft::Mood::Busy
        } else if self.soft_wrong {
            soft::Mood::Wrong
        } else {
            soft::Mood::Idle
        };
        let field = soft::Field { dots: self.secret.chars(), mood, caps: self.mods.caps_lock };
        let buf = &s.bufs[idx];
        if let Some(canvas) = buf.canvas(pool) {
            soft::paint(canvas, w, h, 1.0, primary.then_some(&field), self.accent);
        }
        let surface = s.lock_surface.wl_surface();
        if buf.attach_to(surface).is_err() {
            return;
        }
        surface.damage_buffer(0, 0, w as i32, h as i32);
        surface.commit();
        s.dirty = false;
    }

    /// Redraw dirty surfaces that aren't waiting on a frame callback.
    fn schedule_retry(&mut self, after: Duration) {
        if self.retry_pending {
            return;
        }
        self.retry_pending = true;
        let _ = self.handle.insert_source(Timer::from_duration(after), |_, _, app: &mut LockApp| {
            app.retry_pending = false;
            for i in 0..app.surfs.len() {
                if app.surfs[i].dirty && !app.surfs[i].frame_pending {
                    app.draw(i);
                }
            }
            TimeoutAction::Drop
        });
    }

    /// One-shot timer that redraws every surface at scene time `at`.
    fn schedule_wake(&mut self, at: f32, now: f32) {
        if self.wake_pending.is_some_and(|p| p <= at) {
            return;
        }
        self.wake_pending = Some(at);
        let delay = Duration::from_secs_f32((at - now).max(0.0) + 0.001);
        let _ = self.handle.insert_source(Timer::from_duration(delay), |_, _, app: &mut LockApp| {
            app.wake_pending = None;
            app.redraw_all();
            TimeoutAction::Drop
        });
    }

    // ── input ───────────────────────────────────────────────────────────

    fn on_key(&mut self, mut ev: KeyEvent) {
        // Take the text out of the event and wipe it once used.
        let mut text = ev.utf8.take().map(String::into_bytes).unwrap_or_default();
        self.key(ev.keysym, &text);
        for b in text.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }

    fn key(&mut self, sym: Keysym, text: &[u8]) {
        if self.success {
            return;
        }
        let m = self.mods;
        let edit = match sym {
            Keysym::Return | Keysym::KP_Enter => {
                self.submit();
                return;
            }
            _ if self.busy => return,
            Keysym::Escape => self.clear(),
            Keysym::BackSpace if m.ctrl => self.clear(),
            Keysym::u | Keysym::U if m.ctrl => self.clear(),
            Keysym::BackSpace => self.secret.pop().then_some(Edit::Erased),
            _ if m.ctrl || m.alt || m.logo => None,
            _ => {
                let mut any = false;
                match std::str::from_utf8(text) {
                    Ok(s) if !s.is_empty() => {
                        for c in s.chars().filter(|c| !c.is_control()) {
                            any |= self.secret.push(c);
                        }
                    }
                    _ => {
                        if let Some(c) = sym.key_char().filter(|c| !c.is_control()) {
                            any |= self.secret.push(c);
                        }
                    }
                }
                any.then_some(Edit::Typed)
            }
        };
        if let Some(e) = edit
            && let Some(i) = self.primary_index()
        {
            self.soft_wrong = false;
            let n = self.secret.chars();
            if let Some(sc) = self.surfs[i].scene() {
                sc.edit(e, n);
            }
            self.surfs[i].dirty = true;
            self.request_redraw(i);
        }
    }

    fn clear(&mut self) -> Option<Edit> {
        let had = !self.secret.is_empty();
        self.secret.clear();
        had.then_some(Edit::Cleared)
    }

    fn submit(&mut self) {
        if self.busy || self.success {
            return;
        }
        if self.secret.is_empty() {
            // Nothing to check: a small "no" instead of a PAM round trip.
            if let Some(i) = self.primary_index()
                && let Some(sc) = self.surfs[i].scene()
            {
                sc.nudge();
                self.request_redraw(i);
            }
            return;
        }
        self.busy = true;
        if let Some(i) = self.primary_index() {
            if let Some(sc) = self.surfs[i].scene() {
                sc.verifying();
            }
            self.surfs[i].dirty = true;
            self.request_redraw(i);
        }
        let wait = self.last_attempt.map_or(Duration::ZERO, |t| MIN_GAP.saturating_sub(t.elapsed()));
        if wait.is_zero() {
            self.start_auth();
        } else {
            let _ = self.handle.insert_source(Timer::from_duration(wait), |_, _, app: &mut LockApp| {
                app.start_auth();
                TimeoutAction::Drop
            });
        }
    }

    fn start_auth(&mut self) {
        self.last_attempt = Some(Instant::now());
        self.attempt += 1;
        let id = self.attempt;
        if self.auth.is_none() {
            self.auth = Authenticator::spawn(&self.user.login, self.service, self.auth_tx.clone());
        }
        // Hand the buffer over; the worker wipes it after the attempt.
        let secret = std::mem::take(&mut self.secret);
        if !self.auth.as_ref().is_some_and(|a| a.submit(id, secret)) {
            self.auth = None;
            self.verdict(Verdict { id, ok: false, note: Some("Try again".into()) });
            return;
        }
        // A module that never returns (a hung keyring or network module)
        // must not freeze the field: abandon that worker.
        let _ = self.handle.insert_source(Timer::from_duration(PAM_TIMEOUT), move |_, _, app: &mut LockApp| {
            if app.busy && app.attempt == id {
                log(format_args!("PAM attempt timed out; starting a fresh worker"));
                // The stuck thread keeps (and eventually wipes) its copy;
                // its late verdict carries a stale id and is ignored.
                app.auth = None;
                app.verdict(Verdict { id, ok: false, note: Some("No answer from PAM · try again".into()) });
            }
            TimeoutAction::Drop
        });
    }

    fn verdict(&mut self, v: Verdict) {
        if v.id != self.attempt || !self.busy || self.success {
            return;
        }
        if v.ok {
            log(format_args!("authenticated"));
            self.success = true;
            for i in 0..self.surfs.len() {
                if let Some(sc) = self.surfs[i].scene() {
                    sc.succeeded();
                }
                self.surfs[i].dirty = true;
            }
            self.redraw_all();
            // Unlock on the wall clock, whether or not frames were drawn
            // (an output may be off): animation never delays it further.
            let _ = self.handle.insert_source(Timer::from_duration(Duration::from_secs_f32(SUCCESS_TOTAL)), |_, _, app: &mut LockApp| {
                app.unlock();
                TimeoutAction::Drop
            });
        } else {
            self.busy = false;
            self.soft_wrong = true;
            if let Some(i) = self.primary_index() {
                if let Some(sc) = self.surfs[i].scene() {
                    sc.failed(v.note.as_deref());
                }
                self.surfs[i].dirty = true;
                self.request_redraw(i);
            }
        }
    }

    /// The only place the session is ever unlocked.
    fn unlock(&mut self) {
        if !self.success {
            return;
        }
        if !self.locked {
            // Authenticated before the compositor confirmed the lock:
            // finish once it has.
            self.unlock_wanted = true;
            return;
        }
        UNLOCKED.store(true, Ordering::SeqCst);
        self.lock.unlock();
        let _ = self.conn.flush();
        let _ = self.conn.roundtrip();
        log(format_args!("unlocked"));
        self.exit = Some(0);
    }
}

impl ShmHandler for LockApp {
    fn shm_state(&mut self) -> &mut Shm {
        self.shm.as_mut().expect("wl_shm events need a bound wl_shm")
    }
}

// ── protocol glue ───────────────────────────────────────────────────────

struct LFracMgr;
struct LFracScale;
struct LViewporter;
struct LViewport;
struct LIdleMgr;
struct LIdle;

impl Dispatch2<WpFractionalScaleManagerV1, LockApp> for LFracMgr {
    fn event(&self, _: &mut LockApp, _: &WpFractionalScaleManagerV1, _: <WpFractionalScaleManagerV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<LockApp>) {}
}

impl Dispatch2<wp_fractional_scale_v1::WpFractionalScaleV1, LockApp> for LFracScale {
    fn event(
        &self,
        app: &mut LockApp,
        proxy: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &Connection,
        _: &QueueHandle<LockApp>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && let Some(i) = app.surfs.iter().position(|s| s.fractional.as_ref() == Some(proxy))
            && app.surfs[i].scale120 != scale
        {
            app.surfs[i].scale120 = scale;
            app.reconfigure(i);
            if app.surfs.get(i).is_some() {
                app.request_redraw(i);
            }
        }
    }
}

impl Dispatch2<ExtIdleNotifierV1, LockApp> for LIdleMgr {
    fn event(&self, _: &mut LockApp, _: &ExtIdleNotifierV1, _: <ExtIdleNotifierV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<LockApp>) {}
}

impl Dispatch2<ExtIdleNotificationV1, LockApp> for LIdle {
    fn event(&self, app: &mut LockApp, _: &ExtIdleNotificationV1, ev: <ExtIdleNotificationV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<LockApp>) {
        if app.locked && matches!(ev, ext_idle_notification_v1::Event::Idled) {
            screen_off();
        }
    }
}

impl Dispatch2<WpViewporter, LockApp> for LViewporter {
    fn event(&self, _: &mut LockApp, _: &WpViewporter, _: <WpViewporter as Proxy>::Event, _: &Connection, _: &QueueHandle<LockApp>) {}
}

impl Dispatch2<WpViewport, LockApp> for LViewport {
    fn event(&self, _: &mut LockApp, _: &WpViewport, _: <WpViewport as Proxy>::Event, _: &Connection, _: &QueueHandle<LockApp>) {}
}

impl SessionLockHandler for LockApp {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        self.locked = true;
        log(format_args!("locked"));
        notify_locked();
        if self.unlock_wanted {
            self.unlock();
        }
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        log(format_args!("the compositor {} the lock", if self.locked { "ended" } else { "refused" }));
        // A restarted locker may race the compositor noticing that its
        // predecessor is gone: try again (bounded by MAX_RESPAWNS).
        self.exit = Some(if !self.locked && generation() > 0 { RELOCK } else { 1 });
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: SessionLockSurface, cfg: SessionLockSurfaceConfigure, _: u32) {
        let Some(i) = self.surf_of(surface.wl_surface()) else { return };
        let s = &mut self.surfs[i];
        s.size = (cfg.new_size.0.max(1), cfg.new_size.1.max(1));
        s.configured = true;
        // A new size must be answered with a buffer of that size.
        s.dirty = true;
        self.reconfigure(i);
        self.request_redraw(i);
    }
}

impl CompositorHandler for LockApp {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        if self.fractional_mgr.is_none()
            && let Some(i) = self.surf_of(surface)
        {
            self.surfs[i].scale120 = factor.max(1) as u32 * 120;
            self.reconfigure(i);
            self.request_redraw(i);
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(i) = self.surf_of(surface) {
            let s = &mut self.surfs[i];
            s.frame_pending = false;
            if !s.shown {
                s.shown = true;
                self.want_backdrop(i);
            }
            let s = &mut self.surfs[i];
            if s.gfx.is_some() && (s.animating || s.dirty) {
                self.draw(i);
            }
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl PresentationTimeHandler for LockApp {
    fn presentation_time_state(&mut self) -> &mut PresentationTimeState {
        &mut self.presentation
    }
    fn presented(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        surface: &wl_surface::WlSurface,
        _: Vec<wl_output::WlOutput>,
        time: PresentTime,
        refresh: u32,
        _: u64,
        _: WEnum<wp_presentation_feedback::Kind>,
    ) {
        if time.clk_id as i32 == libc::CLOCK_MONOTONIC
            && let Some(i) = self.surf_of(surface)
        {
            let c = &mut self.surfs[i].clock;
            c.last_present = Some(time.tv_sec * 1_000_000_000 + time.tv_nsec as u64);
            if refresh > 0 {
                c.refresh = refresh as u64;
            }
        }
    }
    fn discarded(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wp_presentation_feedback::WpPresentationFeedback, _: &wl_surface::WlSurface) {}
}

impl OutputHandler for LockApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_output(output);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.surfs.retain(|s| s.output != output);
        if self.primary.as_ref() == Some(&output) {
            self.primary = None;
            if let Some(o) = self.surfs.first().map(|s| s.output.clone()) {
                // Bypass the busy check: the field must live somewhere.
                let n = self.secret.chars();
                self.primary = Some(o);
                if let Some(sc) = self.surfs.first_mut().and_then(|s| s.scene.as_mut()) {
                    sc.set_primary(true, n);
                }
                self.surfs[0].dirty = true;
                self.request_redraw(0);
            }
        }
    }
}

impl SeatHandler for LockApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if self.idle.is_none()
            && let Some(m) = &self.idle_mgr
        {
            self.idle = Some(m.get_idle_notification(SLEEP_AFTER_MS, &seat, qh, LIdle));
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let cb: smithay_client_toolkit::seat::keyboard::repeat::RepeatCallback<LockApp> =
                Box::new(|app: &mut LockApp, _kb, ev| app.on_key(ev));
            self.keyboard = self.seats.get_keyboard_with_repeat(qh, &seat, None, self.handle.clone(), cb).ok();
        }
        if cap == Capability::Pointer
            && self.pointer.is_none()
            && let Ok(p) = self.seats.get_pointer(qh, &seat)
        {
            self.shape_device = self.cursor_shapes.as_ref().map(|m| m.get_shape_device(&p, qh));
            self.pointer = Some(p);
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if cap == Capability::Pointer {
            if let Some(d) = self.shape_device.take() {
                d.destroy();
            }
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for LockApp {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for ev in events {
            if let PointerEventKind::Enter { serial } = ev.kind {
                if let Some(d) = &self.shape_device {
                    d.set_shape(serial, CursorShape::Default);
                }
                if let Some(i) = self.surf_of(&ev.surface) {
                    let o = self.surfs[i].output.clone();
                    self.set_primary(&o);
                }
            }
        }
    }
}

impl KeyboardHandler for LockApp {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        if let Some(i) = self.surf_of(surface) {
            let o = self.surfs[i].output.clone();
            self.set_primary(&o);
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        self.on_key(ev);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        self.on_key(ev);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, mut ev: KeyEvent) {
        if let Some(t) = ev.utf8.take() {
            let mut b = t.into_bytes();
            for x in b.iter_mut() {
                unsafe { std::ptr::write_volatile(x, 0) };
            }
        }
    }
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        let caps = m.caps_lock != self.mods.caps_lock;
        self.mods = m;
        if caps && let Some(i) = self.primary_index() {
            if let Some(sc) = self.surfs[i].scene() {
                sc.set_caps(m.caps_lock);
            }
            self.surfs[i].dirty = true;
            self.request_redraw(i);
        }
    }
}

impl ProvidesRegistryState for LockApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(LockApp);
delegate_dispatch2!(LockApp);
