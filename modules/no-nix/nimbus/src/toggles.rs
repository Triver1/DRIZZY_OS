//! Quick toggles that live in nimbus itself:
//!
//! - night light: wlr-gamma-control on every output, ~4000 K, ramped in a
//!   handful of gamma-table updates on a timer (no frames involved);
//!   persisted in the config. Destroying a control (or exiting) restores
//!   the output's normal gamma.
//! - caffeine: an idle inhibitor on a bar surface (the bar is always
//!   visible, so the inhibitor always applies).
//! - screen recording: `wf-recorder` on the focused output, stopped with
//!   SIGINT so the file is finalised; a waiter thread reports its exit.
//! - airplane mode and screenshots shell out (nmcli + rfkill, niri).
//!
//! The UI learns the state through `Fact::Toggles`.

use std::{
    io::{Seek, Write},
    os::fd::{AsFd, FromRawFd},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{Connection, Proxy, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

use smithay_client_toolkit::shell::WaylandSurface;

use crate::{App, services::Fact};

/// Night light colour (≈ 4000 K, relative to 6500 K white).
const WARM: [f32; 3] = [1.0, 0.82, 0.66];
const RAMP_SECS: f32 = 0.6;
const RAMP_STEP: Duration = Duration::from_millis(60);

/// What the popout (and the bar's recording dot) show.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToggleState {
    pub night: bool,
    pub caffeine: bool,
    pub recording: bool,
    /// `wf-recorder` is installed.
    pub recorder: bool,
}

#[derive(Default)]
pub struct Toggles {
    pub gamma_mgr: Option<ZwlrGammaControlManagerV1>,
    pub idle_mgr: Option<ZwpIdleInhibitManagerV1>,
    inhibitor: Option<ZwpIdleInhibitorV1>,
    night: bool,
    /// Current warmth 0..1, and the ramp in progress (from, start).
    level: f32,
    ramp: Option<(f32, Instant)>,
    ramping: bool,
    /// Pid of the running wf-recorder.
    recording: Option<i32>,
    recorder: bool,
}

pub struct GammaMgr;
pub struct Gamma;
pub struct IdleMgr;
pub struct Inhibitor;

/// Payload sent when wf-recorder exits (by itself or after SIGINT).
pub struct RecordEnded(pub i32);

pub fn record_ended(f: &Fact) -> Option<i32> {
    match f {
        Fact::Custom(a) => a.downcast_ref::<RecordEnded>().map(|r| r.0),
        _ => None,
    }
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

impl Toggles {
    pub fn new() -> Self {
        let night = crate::config::get("night_light").is_some_and(|v| v == "on");
        Self { night, level: if night { 1.0 } else { 0.0 }, recorder: on_path("wf-recorder"), ..Default::default() }
    }
}

impl App {
    pub(crate) fn toggle_state(&self) -> ToggleState {
        let t = &self.toggles;
        ToggleState { night: t.night, caffeine: t.inhibitor.is_some(), recording: t.recording.is_some(), recorder: t.recorder }
    }

    /// Tell the bars and windows.
    pub(crate) fn push_toggles(&mut self, qh: &QueueHandle<App>) {
        let s = self.toggle_state();
        self.on_fact(qh, Fact::Toggles(s));
    }

    /// A new output: give it a gamma control (applied once its size is known).
    pub(crate) fn gamma_for_panel(&mut self, qh: &QueueHandle<App>, i: usize) {
        if let Some(m) = &self.toggles.gamma_mgr {
            let c = m.get_gamma_control(&self.panels[i].output, qh, Gamma);
            self.panels[i].gamma = Some((c, 0));
        }
    }

    pub(crate) fn set_night(&mut self, qh: &QueueHandle<App>, on: bool) {
        if on == self.toggles.night {
            return;
        }
        self.toggles.night = on;
        crate::config::set("night_light", if on { "on" } else { "off" });
        self.toggles.ramp = Some((self.toggles.level, Instant::now()));
        if !self.toggles.ramping {
            self.toggles.ramping = true;
            let _ = self.handle.insert_source(Timer::immediate(), |_, _, app: &mut App| {
                let done = app.ramp_step();
                if done {
                    app.toggles.ramping = false;
                    TimeoutAction::Drop
                } else {
                    TimeoutAction::ToDuration(RAMP_STEP)
                }
            });
        }
        self.push_toggles(qh);
    }

    /// One step of the warmth ramp; true when it has arrived.
    fn ramp_step(&mut self) -> bool {
        let target = if self.toggles.night { 1.0 } else { 0.0 };
        let Some((from, t0)) = self.toggles.ramp else { return true };
        let t = (t0.elapsed().as_secs_f32() / RAMP_SECS).clamp(0.0, 1.0);
        let e = t * t * (3.0 - 2.0 * t);
        self.toggles.level = from + (target - from) * e;
        self.apply_gamma();
        if t >= 1.0 {
            self.toggles.ramp = None;
            // Fully off: hand the outputs back their own gamma.
            if target == 0.0 {
                for p in &mut self.panels {
                    if let Some((c, _)) = p.gamma.take() {
                        c.destroy();
                    }
                }
                if let Some(qh) = self.qh.clone() {
                    for i in 0..self.panels.len() {
                        self.gamma_for_panel(&qh, i);
                    }
                }
            }
            return true;
        }
        false
    }

    pub(crate) fn apply_gamma(&mut self) {
        let k = self.toggles.level.clamp(0.0, 1.0);
        for p in &self.panels {
            let Some((c, size)) = &p.gamma else { continue };
            let n = *size as usize;
            if n < 2 || n > 65536 || (k == 0.0 && self.toggles.ramp.is_none()) {
                continue;
            }
            let mut bytes = Vec::with_capacity(n * 6);
            for ch in WARM {
                let m = 1.0 + (ch - 1.0) * k;
                for i in 0..n {
                    let v = (i as f32 / (n - 1) as f32 * m * 65535.0).round().clamp(0.0, 65535.0) as u16;
                    bytes.extend_from_slice(&v.to_ne_bytes());
                }
            }
            // The table goes over a sealed-off memfd.
            let fd = unsafe { libc::memfd_create(c"nimbus-gamma".as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                continue;
            }
            let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
            if f.write_all(&bytes).is_err() || f.rewind().is_err() {
                continue;
            }
            c.set_gamma(f.as_fd());
        }
    }

    pub(crate) fn set_caffeine(&mut self, qh: &QueueHandle<App>, on: bool) {
        if on == self.toggles.inhibitor.is_some() {
            return;
        }
        if let Some(i) = self.toggles.inhibitor.take() {
            i.destroy();
        }
        if on
            && let (Some(m), Some(p)) = (&self.toggles.idle_mgr, self.panels.first())
        {
            self.toggles.inhibitor = Some(m.create_inhibitor(p.layer.wl_surface(), qh, Inhibitor));
        }
        self.push_toggles(qh);
    }

    /// The panel the inhibitor lived on went away: move it.
    pub(crate) fn refresh_caffeine(&mut self, qh: &QueueHandle<App>) {
        if let Some(old) = self.toggles.inhibitor.take() {
            old.destroy();
            if let (Some(m), Some(p)) = (&self.toggles.idle_mgr, self.panels.first()) {
                self.toggles.inhibitor = Some(m.create_inhibitor(p.layer.wl_surface(), qh, Inhibitor));
            }
        }
    }

    pub(crate) fn start_recording(&mut self, qh: &QueueHandle<App>) {
        if self.toggles.recording.is_some() || !self.toggles.recorder {
            return;
        }
        let output = self
            .latest
            .iter()
            .find_map(|f| match f {
                Fact::Niri(n) => n.focused_output.clone(),
                _ => None,
            })
            .or_else(|| self.panels.first().map(|p| p.bar.output.clone()));
        let dir = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Videos/Recordings");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(format!("nimbus-{}.mp4", stamp()));
        let mut cmd = Command::new("wf-recorder");
        if let Some(o) = &output {
            cmd.args(["-o", o]);
        }
        cmd.arg("-f").arg(&file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id() as i32;
                self.toggles.recording = Some(pid);
                let tx = self.fact_tx.clone();
                let _ = thread::Builder::new().name("nimbus-recorder".into()).stack_size(64 * 1024).spawn(move || {
                    let _ = child.wait();
                    let _ = tx.send(Fact::Custom(Arc::new(RecordEnded(pid))));
                });
                eprintln!("nimbus: recording {} to {}", output.as_deref().unwrap_or("?"), file.display());
            }
            Err(e) => eprintln!("nimbus: wf-recorder: {e}"),
        }
        self.push_toggles(qh);
    }

    pub(crate) fn stop_recording(&mut self) {
        if let Some(pid) = self.toggles.recording {
            // SIGINT lets wf-recorder finish the file; the waiter reports.
            unsafe { libc::kill(pid, libc::SIGINT) };
        }
    }

    pub(crate) fn recording_ended(&mut self, qh: &QueueHandle<App>, pid: i32) {
        if self.toggles.recording == Some(pid) {
            self.toggles.recording = None;
            self.push_toggles(qh);
        }
    }
}

/// Airplane mode: every radio off (or back on).
pub fn set_airplane(on: bool) {
    let _ = thread::Builder::new().name("nimbus-airplane".into()).stack_size(64 * 1024).spawn(move || {
        let _ = Command::new("nmcli").args(["radio", "all", if on { "off" } else { "on" }]).status();
        let _ = Command::new("rfkill").args([if on { "block" } else { "unblock" }, "bluetooth"]).status();
    });
}

/// niri's interactive screenshot, once the popout has folded away.
pub fn screenshot() {
    let _ = thread::Builder::new().name("nimbus-screenshot".into()).stack_size(64 * 1024).spawn(|| {
        thread::sleep(Duration::from_millis(350));
        let _ = Command::new("niri").args(["msg", "action", "screenshot"]).stdin(Stdio::null()).status();
    });
}

/// Local time as YYYYmmdd-HHMMSS.
fn stamp() -> String {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

// ── protocol glue ───────────────────────────────────────────────────────

impl Dispatch2<ZwlrGammaControlManagerV1, App> for GammaMgr {
    fn event(&self, _: &mut App, _: &ZwlrGammaControlManagerV1, _: <ZwlrGammaControlManagerV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<ZwlrGammaControlV1, App> for Gamma {
    fn event(&self, app: &mut App, proxy: &ZwlrGammaControlV1, event: zwlr_gamma_control_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        let Some(p) = app.panels.iter_mut().find(|p| p.gamma.as_ref().is_some_and(|(c, _)| c == proxy)) else { return };
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                if let Some(g) = &mut p.gamma {
                    g.1 = size;
                }
                if app.toggles.level > 0.0 {
                    app.apply_gamma();
                }
            }
            zwlr_gamma_control_v1::Event::Failed => {
                // Another client owns this output's gamma.
                if let Some((c, _)) = p.gamma.take() {
                    c.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch2<ZwpIdleInhibitManagerV1, App> for IdleMgr {
    fn event(&self, _: &mut App, _: &ZwpIdleInhibitManagerV1, _: <ZwpIdleInhibitManagerV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<ZwpIdleInhibitorV1, App> for Inhibitor {
    fn event(&self, _: &mut App, _: &ZwpIdleInhibitorV1, _: <ZwpIdleInhibitorV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
}
