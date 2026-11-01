//! `nimbus lock`: a session locker (ext-session-lock-v1 + PAM) drawn with
//! the bar's renderer.
//!
//! - `scene`: the lock screen itself (state, springs, drawing), no Wayland
//! - `session`: the Wayland plumbing and the unlock decision
//! - `auth`: zeroizing password buffer and the PAM worker
//! - `assets`: wallpaper blur, avatar, checkmark, prepared off-thread
//!
//! `nimbus lock` blocks until unlocked; `nimbus lock --daemonize` (or `-f`)
//! returns as soon as the session is locked (for `swayidle -w`).
//!
//! `nimbus lock --software` skips the GPU (the same screen the locker falls
//! back to by itself when Vulkan is missing or the device is lost).
//!
//! PAM: the first of /etc/pam.d/{nimbus,swaylock,hyprlock,login} that
//! exists. A dedicated, minimal stack is best (NixOS:
//! `security.pam.services.nimbus = {};`, i.e. pam_unix only): no
//! pam_faillock (three wrong guesses at a locked laptop would lock the
//! account everywhere) and no keyring modules that can block. Empty
//! passwords are never accepted (PAM_DISALLOW_NULL_AUTHTOK); an attempt
//! that hangs is abandoned after 30 s.
//!
//! Recovery, set up before the first real use: if a locker dies the
//! compositor keeps the session locked and accepts a new lock client
//! (this one restarts itself in software mode on a panic or a stalled main
//! thread). Keep a bind that only spawns a locker, e.g. in niri
//! `Mod+Alt+L allow-when-locked=true { spawn "hyprlock"; }` (niri refuses
//! it while a live lock client exists), and from a TTY:
//! `WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000 hyprlock`.
//! niri has no logind Unlock handler, so `loginctl unlock-session` won't help.
//!
//! `NIMBUS_SNAPSHOT_ONLY=lock nimbus --snapshot <dir>` renders the scene's
//! states headless (never locks anything).

mod assets;
mod auth;
mod scene;
mod session;
mod soft;

use std::{io::Write, path::Path, rc::Rc};

pub(crate) use auth::Secret;
pub use session::run;

use crate::{
    gfx::{DrawList, Gpu, Icons, SurfaceGfx},
    icons::IconSet,
    services::{BatteryInfo, ClockInfo},
};
use scene::{Edit, Scene};

const DT: f32 = 1.0 / 120.0;

/// Log a line to stderr, ignoring failures: `eprintln!` panics when stderr
/// is a closed pipe, which must never take the locker down.
pub(crate) fn log(args: std::fmt::Arguments) {
    let _ = writeln!(std::io::stderr().lock(), "nimbus lock: {args}");
}

struct Rig {
    gpu: Gpu,
    icons: Icons,
    target: SurfaceGfx,
    scratch: SurfaceGfx,
    list: DrawList,
    dir: std::path::PathBuf,
    px: (u32, u32),
}

impl Rig {
    /// Draw (which may queue texture uploads) and hand uploads to the GPU
    /// even for frames that aren't saved.
    fn draw(&mut self, sc: &mut Scene) {
        sc.draw(&mut self.gpu.fonts, &mut self.list);
    }

    fn tick(&mut self, sc: &mut Scene, secs: f32) {
        for _ in 0..(secs / DT).round() as u32 {
            sc.step(DT);
            self.draw(sc);
            if !self.list.uploads.is_empty() || !self.list.frees.is_empty() {
                let only = DrawList { uploads: std::mem::take(&mut self.list.uploads), frees: std::mem::take(&mut self.list.frees), ..Default::default() };
                let _ = self.gpu.render_offscreen(&mut self.scratch, &only, sc.text_buffers(), &self.icons);
            }
        }
    }

    fn shoot(&mut self, sc: &mut Scene, name: &str) {
        self.draw(sc);
        let data = self.gpu.render_offscreen(&mut self.target, &self.list, sc.text_buffers(), &self.icons);
        let (w, h) = self.px;
        let path = self.dir.join(name).with_extension("pam");
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        write!(f, "P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n").unwrap();
        f.write_all(&data).unwrap();
        std::fs::write(self.dir.join(name).with_extension("glass"), "").unwrap();
    }

    /// Step `secs`, saving a frame every `every` s (`name_NN`).
    fn film(&mut self, sc: &mut Scene, name: &str, secs: f32, every: f32) {
        let n = (secs / every).round() as u32;
        for i in 0..n {
            self.shoot(sc, &format!("{name}_{i:02}"));
            self.tick(sc, every);
        }
    }
}

/// Headless demo for review: every state of the lock screen, rendered
/// through the real GPU path into `<dir>/lock_*.pam`.
pub fn demo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let only = std::env::var("NIMBUS_LOCK_PART").unwrap_or_default();
    let part = |p: &str| only.is_empty() || only.split(',').any(|o| o == p);
    let scale = 1.75f32;
    let px = (3200u32, 2000u32);
    let (w, h) = (px.0 as f32 / scale, px.1 as f32 / scale);
    if part("soft") {
        soft_demo(dir, px);
    }
    let mut gpu = match Gpu::try_new(None) {
        Ok(g) => g,
        Err(e) => {
            log(format_args!("demo: {e} (only the software screen was rendered)"));
            return;
        }
    };
    crate::fonts::load(gpu.fonts.db_mut(), &["Geist"]);
    let iconset = Rc::new(IconSet::discover());
    let icons = Icons::load(&iconset.paths);
    let target = gpu.create_offscreen(px.0, px.1);
    let scratch = gpu.create_offscreen(8, 8);
    let mut r = Rig { gpu, icons, target, scratch, list: DrawList::default(), dir: dir.to_path_buf(), px };
    let mut user = auth::User::current();
    if let Ok(n) = std::env::var("NIMBUS_LOCK_NAME") {
        user.name = n;
    }
    let wall = std::env::var_os("NIMBUS_LOCK_WALLPAPER").map(std::path::PathBuf::from).or_else(|| assets::wallpaper_path(&user.home));
    let t0 = std::time::Instant::now();
    let bd = assets::backdrop(wall.as_deref(), px.0, px.1, crate::theme::accent());
    eprintln!("nimbus lock demo: wallpaper {:?} prepared in {:.0} ms", wall, t0.elapsed().as_secs_f32() * 1e3);
    let avatar = std::env::var_os("NIMBUS_LOCK_AVATAR").and_then(|p| {
        image::open(std::path::PathBuf::from(p)).ok().map(|i| {
            let i = i.into_rgba8();
            let s = i.width().min(i.height());
            let sq = image::imageops::crop_imm(&i, (i.width() - s) / 2, (i.height() - s) / 2, s, s).to_image();
            let o = image::imageops::resize(&sq, 192, 192, image::imageops::FilterType::Triangle);
            crate::gfx::ImageData { w: 192, h: 192, rgba: std::sync::Arc::new(o.into_raw()) }
        })
    });

    let clock = |hour, minute| ClockInfo { hour, minute, weekday: 0, day: 25, month: 9 };
    let mut sc = Scene::new(&mut r.gpu.fonts, iconset.clone(), &user, true, 1);
    sc.resize(w, h, scale);
    sc.set_clock(clock(11, 59));
    sc.set_battery(Some(BatteryInfo { percent: 62, charging: false }));
    if let Some(a) = &avatar {
        sc.set_avatar(a.clone());
    }
    sc.sync_time(0.001);
    r.tick(&mut sc, 0.05);
    if part("intro") {
        r.shoot(&mut sc, "lock_00_dark");
    }
    sc.set_backdrop(bd.clone());
    if part("intro") {
        r.film(&mut sc, "lock_intro", 0.9, 0.075);
    } else {
        r.tick(&mut sc, 0.9);
    }
    r.tick(&mut sc, 1.5);
    r.shoot(&mut sc, "lock_empty");

    // Typing six characters; the sixth caught mid-pop.
    let mut count = 0;
    for k in 0..6 {
        count += 1;
        sc.edit(Edit::Typed, count);
        if k < 5 {
            r.tick(&mut sc, 0.11);
        }
    }
    if part("typing") {
        r.film(&mut sc, "lock_typing", 0.2, 0.025);
    }
    r.tick(&mut sc, 1.0);
    r.shoot(&mut sc, "lock_typed");

    // Verifying: the dots ripple in the accent.
    sc.verifying();
    if part("verify") {
        r.film(&mut sc, "lock_verify", 0.6, 0.075);
    } else {
        r.tick(&mut sc, 0.6);
    }
    // Wrong password: shake, red tint, message.
    sc.failed(None);
    if part("wrong") {
        r.film(&mut sc, "lock_wrong", 0.6, 0.025);
    }
    r.tick(&mut sc, 0.9);
    r.shoot(&mut sc, "lock_wrong_rest");

    // Caps Lock, then typing again (the message cross-fades away).
    sc.set_caps(true);
    count = 0;
    for _ in 0..3 {
        count += 1;
        sc.edit(Edit::Typed, count);
        r.tick(&mut sc, 0.09);
    }
    r.tick(&mut sc, 1.0);
    r.shoot(&mut sc, "lock_caps");
    sc.set_caps(false);
    sc.edit(Edit::Cleared, 0);
    r.tick(&mut sc, 0.6);
    for _ in 0..9 {
        count += 1;
        sc.edit(Edit::Typed, count);
        r.tick(&mut sc, 0.08);
    }
    r.tick(&mut sc, 0.6);
    sc.verifying();
    r.tick(&mut sc, 0.35);
    // Success: morph to a check, iris open.
    sc.succeeded();
    if part("success") {
        r.film(&mut sc, "lock_success", scene::SUCCESS_TOTAL + 0.05, 0.025);
    }

    // A second output: clock only, and a minute change.
    let mut sc2 = Scene::new(&mut r.gpu.fonts, iconset, &user, false, 2);
    sc2.resize(w, h, scale);
    sc2.set_clock(clock(11, 59));
    sc2.set_battery(Some(BatteryInfo { percent: 62, charging: true }));
    sc2.sync_time(0.001);
    sc2.set_backdrop(bd);
    r.tick(&mut sc2, 2.0);
    r.shoot(&mut sc2, "lock_secondary");
    sc2.set_clock(ClockInfo { hour: 12, minute: 0, weekday: 0, day: 25, month: 9 });
    if part("clock") {
        r.film(&mut sc2, "lock_clock", 0.5, 0.05);
    }
    eprintln!("nimbus lock demo: frames written to {}", dir.display());
}

/// The no-GPU screen: idle, typed, wrong, verifying.
fn soft_demo(dir: &Path, px: (u32, u32)) {
    let (w, h) = px;
    let acc = crate::theme::accent();
    for (name, dots, mood) in [
        ("soft_idle", 0, soft::Mood::Idle),
        ("soft_typed", 7, soft::Mood::Idle),
        ("soft_busy", 7, soft::Mood::Busy),
        ("soft_wrong", 0, soft::Mood::Wrong),
    ] {
        let mut canvas = vec![0u8; (w * h * 4) as usize];
        let f = soft::Field { dots, mood, caps: name == "soft_typed" };
        soft::paint(&mut canvas, w, h, 1.75, Some(&f), acc);
        let mut rgba = Vec::with_capacity(canvas.len());
        for p in canvas.chunks_exact(4) {
            rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
        let path = dir.join(name).with_extension("pam");
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        write!(f, "P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n").unwrap();
        f.write_all(&rgba).unwrap();
    }
}

/// Who's logged in (Settings' Profile page).
pub fn who() -> auth::User {
    auth::User::current()
}

/// The picture the lock shows for this home, square, `px` wide.
pub fn avatar(home: &str, login: &str, px: u32) -> Option<crate::gfx::ImageData> {
    assets::avatar(home, login, px)
}

/// The wallpaper the lock would use.
pub fn wallpaper_path(home: &str) -> Option<std::path::PathBuf> {
    assets::wallpaper_path(home)
}
