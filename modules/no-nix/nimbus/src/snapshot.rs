//! Headless rendering for design review: drives the bar with scripted facts
//! and pointer moves at a fixed 120 Hz clock and writes every frame as PAM
//! (RGBA) plus its glass rectangles, so a script can composite the frames
//! over a blurred wallpaper the way niri would.
//!
//!   nimbus --snapshot <dir>

use std::{io::Write, path::Path, rc::Rc};


use crate::{
    bar::{Bar, Hit, SURFACE_H},
    gfx::{DrawList, Gpu, Icons, SurfaceGfx},
    icons::IconSet,
    services::{self, BatteryInfo, ClockInfo, Fact, NetInfo, VolumeInfo},
};

const DT: f32 = 1.0 / 120.0;

struct Rig {
    gpu: Gpu,
    icons: Icons,
    target: SurfaceGfx,
    bar: Bar,
    list: DrawList,
    size: (u32, u32),
    /// `NIMBUS_SNAPSHOT_STILLS=1`: skip the frame-by-frame films.
    stills_only: bool,
}

impl Rig {
    /// Advance like the live loop does: draw (which measures text and
    /// sets layout targets), then integrate.
    fn step(&mut self, secs: f32) {
        for _ in 0..(secs / DT).round() as u32 {
            self.bar.draw(&mut self.gpu.fonts, &mut self.list);
            self.consume_uploads();
            self.bar.step(DT);
        }
    }

    /// Hand a skipped frame's texture uploads / frees to the GPU anyway.
    fn consume_uploads(&mut self) {
        let l = &mut self.list;
        if !l.uploads.is_empty() || !l.frees.is_empty() {
            let mut t = self.gpu.create_offscreen(4, 4);
            let only = DrawList { uploads: std::mem::take(&mut l.uploads), frees: std::mem::take(&mut l.frees), ..Default::default() };
            let empty = std::collections::HashMap::new();
            let _ = self.gpu.render_offscreen(&mut t, &only, &empty, &self.icons);
        }
    }

    fn shoot(&mut self, path: &Path) {
        self.bar.draw(&mut self.gpu.fonts, &mut self.list);
        let px = self.gpu.render_offscreen(&mut self.target, &self.list, &self.bar.text.bufs, &self.icons);
        let (w, h) = self.size;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path.with_extension("pam")).unwrap());
        write!(f, "P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n").unwrap();
        f.write_all(&px).unwrap();
        let s = self.bar.scale;
        let glass: String = self
            .bar
            .glass
            .iter()
            .filter(|_| self.bar.look.blur)
            .map(|r| format!("{} {} {} {}\n", r.x * s, r.y * s, r.w * s, r.h * s))
            .collect();
        std::fs::write(path.with_extension("glass"), glass).unwrap();
    }

    /// Record `secs` of animation, one frame per tick.
    fn film(&mut self, dir: &Path, name: &str, secs: f32) {
        if self.stills_only {
            self.step(secs);
            return;
        }
        let n = (secs / DT).round() as u32;
        for i in 0..n {
            self.shoot(&dir.join(format!("{name}_{i:03}")));
            self.bar.step(DT);
        }
    }

    /// Hover `h`, or the design's body if `h` isn't on screen yet; then
    /// settle briefly and retry so designs that open on hover can reveal it.
    fn hover(&mut self, h: Hit) {
        if let Some((x, y)) = self.bar.hit_center(h).or_else(|| self.bar.hit_center(Hit::Panel)) {
            self.bar.pointer_motion(x, y);
        }
        if self.bar.hovered != h {
            self.step(0.4);
            if let Some((x, y)) = self.bar.hit_center(h) {
                self.bar.pointer_motion(x, y);
            }
        }
    }
}

pub fn run(dir: &Path) {
    // The lock screen renders full-screen frames: only on request.
    if std::env::var("NIMBUS_SNAPSHOT_ONLY").as_deref() == Ok("lock") {
        return crate::lock::demo(dir);
    }
    std::fs::create_dir_all(dir).unwrap();
    let (lw, scale) = (1828.0f32, 1.75f32);
    let size = ((lw * scale).round() as u32, (SURFACE_H as f32 * scale).round() as u32);
    let mut gpu = Gpu::new(None);
    let iconset = Rc::new(IconSet::discover());
    let icons = Icons::load(&iconset.paths);
    let target = gpu.create_offscreen(size.0, size.1);
    let mut bar = Bar::new("eDP-1".into(), &mut gpu.fonts, iconset);
    bar.resize(lw, scale);
    let stills_only = std::env::var_os("NIMBUS_SNAPSHOT_STILLS").is_some();
    let mut r = Rig { gpu, icons, target, bar, list: DrawList::default(), size, stills_only };

    let clock = |hour, minute| Fact::Clock(ClockInfo { hour, minute, weekday: 6, day: 25, month: 9 });
    for f in [
        services::bench_snapshot(0, "eDP-1"),
        clock(10, 59),
        Fact::Battery(Some(BatteryInfo { percent: 43, charging: false })),
        Fact::Net(NetInfo::Wifi(80)),
        Fact::Volume(VolumeInfo { percent: 62, muted: false }),
    ] {
        r.bar.apply(&f);
    }

    let only = std::env::var("NIMBUS_SNAPSHOT_ONLY").ok();
    if only.as_deref() == Some("polkit") {
        crate::polkit::demo(&mut Demo::new(&mut r.gpu, &r.icons, dir, scale));
        eprintln!("nimbus: snapshots written to {}", dir.display());
        return;
    }
    if only.as_deref() == Some("popout") {
        settings(&mut r.gpu, &r.icons, dir, scale);
        eprintln!("nimbus: snapshots written to {}", dir.display());
        return;
    }
    if only.as_deref() == Some("media") {
        media_demo(&mut r, dir);
        eprintln!("nimbus: snapshots written to {}", dir.display());
        return;
    }
    if only.as_deref() == Some("profile") {
        profile_demo(&mut r.gpu, &r.icons, dir, scale);
        eprintln!("nimbus: snapshots written to {}", dir.display());
        return;
    }

    r.film(dir, "enter", 0.6);
    r.step(1.0);
    r.shoot(&dir.join("idle"));

    // Workspace 1 → 4: the liquid indicator.
    r.bar.apply(&services::bench_snapshot(3, "eDP-1"));
    r.film(dir, "ws", 0.6);
    r.step(1.0);

    // Hover the spaces island, then a dot.
    r.hover(Hit::Workspace(1001));
    r.film(dir, "hover_ws", 0.35);
    r.step(1.0);
    r.shoot(&dir.join("hover_ws_rest"));

    // Hover the clock: the date slides out.
    r.hover(Hit::Clock);
    r.film(dir, "hover_clock", 0.5);
    r.step(1.0);
    r.shoot(&dir.join("hover_clock_rest"));

    // Minute rollover 10:59 → 11:00: odometer digits.
    r.bar.apply(&clock(11, 0));
    r.film(dir, "clock", 0.6);
    r.step(1.0);

    // Title change.
    r.bar.pointer_leave();
    r.step(1.0);
    r.bar.apply(&services::bench_snapshot(2, "eDP-1"));
    r.film(dir, "title", 0.5);
    r.step(2.5);
    r.shoot(&dir.join("final"));

    // Same window, new title (a terminal's spinner): should update quietly.
    if let Fact::Niri(mut s) = services::bench_snapshot(2, "eDP-1") {
        s.title = Some("✳ Building nimbus".into());
        r.bar.apply(&Fact::Niri(s));
    }
    r.film(dir, "retitle", 0.5);
    settings(&mut r.gpu, &r.icons, dir, scale);
    prefs(&mut r.gpu, &r.icons, dir, scale);
    // Feature windows: each module scripts its own demo.
    let demos: [(&str, fn(&mut Demo)); 6] = [
        ("launcher", crate::launcher::demo),
        ("monitor", crate::monitor::demo),
        ("wallpaper", crate::wallpaper::demo),
        ("notify", crate::notify::demo),
        ("traymenu", crate::traymenu::demo),
        ("files", crate::files::demo),
    ];
    for (name, f) in demos {
        if only.as_deref().is_none_or(|o| o == name) {
            let mut d = Demo::new(&mut r.gpu, &r.icons, dir, scale);
            f(&mut d);
        }
    }
    eprintln!("nimbus: snapshots written to {}", dir.display());
}

/// Now playing (`NIMBUS_SNAPSHOT_ONLY=media`, best with NIMBUS_DESIGN=deck):
/// fake MPRIS facts with wallpaper crops as covers, stepped through every
/// state the bar has. Stills are `m_*`; films (unless STILLS) `mf_*`.
fn media_demo(r: &mut Rig, dir: &Path) {
    use crate::media::{self, MediaInfo};
    use services::{NiriSnapshot, WorkspaceInfo};
    use std::{sync::Arc, time::Instant};
    let wall = std::env::var("NIMBUS_SNAPSHOT_WALLS").unwrap_or_else(|_| "/home/drizzy/Pictures/Wallpapers/nimbus".into());
    let cover = |name: &str| -> Option<Arc<media::Art>> {
        let bytes = std::fs::read(Path::new(&wall).join(name)).ok()?;
        let (img, tint) = media::decode(&bytes)?;
        Some(Arc::new(media::Art { url: format!("file:///demo/{name}"), img, tint }))
    };
    let niri = |active: usize, title: &str, app: &str| {
        let workspaces = (0..5u8)
            .map(|i| WorkspaceInfo {
                id: 1000 + i as u64,
                idx: i + 1,
                output: "eDP-1".into(),
                active: i as usize == active,
                focused: i as usize == active,
                urgent: false,
                windows: [3, 1, 0, 2, 0][i as usize],
            })
            .collect();
        Fact::Niri(NiriSnapshot {
            workspaces,
            title: Some(title.into()),
            focused_id: Some(2000 + active as u64),
            app_id: Some(app.into()),
            focused_output: Some("eDP-1".into()),
        })
    };
    let track = |title: &str, artist: &str, art: &str, pos: f32, len: f32, playing: bool| {
        Fact::Media(Some(MediaInfo {
            player: "spotify".into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            art_url: Some(format!("file:///demo/{art}")),
            length: len,
            position: pos,
            stamp: Instant::now(),
            rate: 1.0,
            playing,
            can_next: true,
            can_prev: true,
            can_seek: true,
        }))
    };
    let apps = ["firefox", "com.mitchellh.ghostty", "code", "Alacritty"];
    let app = apps.iter().find(|a| crate::appicons::app_icon_now(a, 64).is_some()).copied().unwrap_or("firefox");
    r.bar.apply(&niri(0, "nimbus — Deck design", app));
    r.bar.apply(&Fact::AppIcon(app.into(), crate::appicons::app_icon_now(app, 64)));
    // A few tray items (app icons standing in for StatusNotifierItems).
    let tray: Vec<crate::tray::TrayItem> = ["spotify", "discord", "nm-applet", "steam", "firefox", "code"]
        .iter()
        .filter_map(|n| crate::appicons::app_icon_now(n, crate::tray::ICON_PX).map(|i| (n, i)))
        .take(3)
        .enumerate()
        .map(|(k, (n, i))| crate::tray::TrayItem { id: format!(":1.{k}/{n}"), title: n.to_string(), attention: false, grey: Some(crate::tray::desaturate(&i)), icon: Some(i), rev: k as u64 + 1 })
        .collect();
    eprintln!("demo tray items: {}", tray.len());
    r.bar.apply(&Fact::Tray(std::sync::Arc::new(tray)));

    let still = |r: &mut Rig, name: &str| r.shoot(&dir.join(format!("m_{name}")));
    let film = |r: &mut Rig, name: &str, secs: f32| r.film(dir, &format!("mf_{name}"), secs);

    film(r, "enter", 0.5);
    r.step(1.0);
    still(r, "01_idle");

    // A track starts; its cover arrives a beat later.
    let (art1, art2) = (cover("amber-horizon.jpg"), cover("iris-ribbons.jpg"));
    r.bar.apply(&track("Under The Pressure", "The War On Drugs", "amber-horizon.jpg", 62.0, 532.0, true));
    r.step(0.08);
    still(r, "02_appear_a");
    if let Some(a) = &art1 {
        r.bar.apply(&Fact::MediaArt(a.clone()));
    }
    r.step(0.08);
    still(r, "02_appear_b");
    film(r, "appear", 0.4);
    r.step(1.0);
    still(r, "03_playing");

    // Hover: the tab widens into the transport controls.
    r.hover(Hit::Media);
    r.step(0.12);
    still(r, "04_hover_a");
    r.step(0.6);
    still(r, "04_hover_b");
    r.hover(Hit::MediaCtl(2));
    film(r, "hover_ctl", 0.3);
    r.step(0.5);
    still(r, "05_hover_next");
    r.hover(Hit::MediaCtl(1));
    r.step(0.08);
    still(r, "05_hover_slide");
    r.step(0.5);

    // Next track: the cover reveals from its centre, text wipes in place.
    r.bar.pointer_leave();
    r.step(0.8);
    r.bar.apply(&track("Red Eyes", "The War On Drugs", "iris-ribbons.jpg", 0.0, 299.0, true));
    if let Some(a) = &art2 {
        r.bar.apply(&Fact::MediaArt(a.clone()));
    }
    r.step(0.12);
    still(r, "06_change_a");
    r.step(0.12);
    still(r, "06_change_b");
    film(r, "change", 0.5);
    r.step(1.0);
    still(r, "06_change_c");

    // Long title + pause.
    r.bar.apply(&track("Thinking of a Place (Live from the Electric Ballroom)", "The War On Drugs", "iris-ribbons.jpg", 140.0, 680.0, false));
    r.step(1.2);
    still(r, "07_paused");
    r.hover(Hit::Media);
    r.step(0.8);
    still(r, "07_paused_hover");
    r.bar.pointer_leave();
    r.step(0.8);

    // Workspace switch and a new focused window while it plays.
    r.bar.apply(&track("Red Eyes", "The War On Drugs", "iris-ribbons.jpg", 90.0, 299.0, true));
    r.bar.apply(&niri(3, "Helium — Wayland layer shell protocol", app));
    r.step(0.15);
    still(r, "08_ws_a");
    r.step(1.0);
    still(r, "08_ws_b");
    r.bar.apply(&niri(3, "Keyboard Tester", app));
    r.step(0.15);
    still(r, "09_title_a");
    r.step(1.0);
    still(r, "09_title_b");

    // Hover the clock (date) and see the volume peek.
    r.hover(Hit::Clock);
    r.step(0.8);
    still(r, "10_clock");
    r.bar.pointer_leave();
    r.bar.apply(&Fact::Volume(VolumeInfo { percent: 40, muted: false }));
    r.step(0.6);
    still(r, "11_volume");
    r.step(2.0);

    // Quick settings out: the centre tab hands over to the panel.
    r.bar.popup_cover = 1.0;
    r.step(0.2);
    still(r, "12_settings");
    eprintln!("centre tab for settings: {:?}; tabs: {:?}", r.bar.center_tab, r.bar.tabs);
    r.bar.popup_cover = 0.0;
    r.step(0.3);

    // The player goes: the drawer folds back into the spaces tab.
    r.bar.apply(&Fact::Media(None));
    r.step(0.1);
    still(r, "13_gone_a");
    film(r, "gone", 0.4);
    r.step(1.0);
    still(r, "13_gone_b");
    // Recording: a red dot breathes in the status tab.
    r.bar.apply(&Fact::Toggles(crate::toggles::ToggleState { night: false, caffeine: false, recording: true, recorder: true }));
    r.step(0.6);
    still(r, "14_recording_a");
    r.step(0.5);
    still(r, "14_recording_b");
    r.bar.apply(&Fact::Toggles(crate::toggles::ToggleState::default()));
    r.step(1.0);

    // Rest check: nothing animates, no wake while nothing plays.
    let moving = r.bar.step(DT);
    eprintln!("at rest: animating={moving} wake={:?}", r.bar.wake_at);
    r.bar.apply(&track("Red Eyes", "The War On Drugs", "iris-ribbons.jpg", 90.0, 299.0, true));
    r.step(2.0);
    let moving = r.bar.step(DT);
    eprintln!("playing at rest: animating={moving} wake in {:?} s", r.bar.wake_at.map(|w| w - r.bar.time));
    r.hover(Hit::Media);
    r.step(1.0);
    let moving = r.bar.step(DT);
    eprintln!("playing, hovered: animating={moving} wake in {:?} s", r.bar.wake_at.map(|w| w - r.bar.time));
    // Frame cost.
    let mut times = vec![];
    for i in 0..400 {
        if i % 100 == 0 {
            r.bar.apply(&niri(i / 100, "nimbus — Deck design", app));
        }
        let t0 = Instant::now();
        r.bar.draw(&mut r.gpu.fonts, &mut r.list);
        r.bar.step(DT);
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        r.consume_uploads();
    }
    times.sort_by(f64::total_cmp);
    eprintln!("draw+step cpu: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms", times[200], times[380], times[399]);
}

/// Settings panel: open, the main page, then the Wi-Fi and Bluetooth pages.
fn settings(gpu: &mut Gpu, icons: &Icons, dir: &Path, scale: f32) {
    use crate::{
        bar::Rect,
        services::{BtDev, SystemState, WifiNet},
        settings::{Ctl, SH, SW, Settings},
    };
    let size = ((SW * scale).round() as u32, (SH * scale).round() as u32);
    let mut target = gpu.create_offscreen(size.0, size.1);
    use crate::media::{self, MediaInfo};
    use std::{sync::Arc, time::Instant};
    let wall = std::env::var("NIMBUS_SNAPSHOT_WALLS").unwrap_or_else(|_| "/home/drizzy/Pictures/Wallpapers/nimbus".into());
    let cover = |name: &str| -> Option<Fact> {
        let bytes = std::fs::read(Path::new(&wall).join(name)).ok()?;
        let (img, tint) = media::decode(&bytes)?;
        Some(Fact::MediaArt(Arc::new(media::Art { url: format!("file:///demo/{name}"), img, tint })))
    };
    let track = |title: &str, artist: &str, art: &str, pos: f32, len: f32, playing: bool| {
        Fact::Media(Some(MediaInfo {
            player: "spotify".into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            art_url: Some(format!("file:///demo/{art}")),
            length: len,
            position: pos,
            stamp: Instant::now(),
            rate: 1.0,
            playing,
            can_next: true,
            can_prev: true,
            can_seek: true,
        }))
    };
    let mut facts = vec![
        Fact::Clock(ClockInfo { hour: 11, minute: 56, weekday: 6, day: 25, month: 9 }),
        Fact::Battery(Some(BatteryInfo { percent: 60, charging: true })),
        Fact::Net(NetInfo::Wifi(90)),
        Fact::Volume(VolumeInfo { percent: 62, muted: false }),
        Fact::System(SystemState { brightness: Some(0.6), wifi: Some(true), bluetooth: Some(true) }),
        Fact::WifiList(vec![WifiNet { ssid: "Home Network".into(), signal: 100, secure: true, active: true }]),
        Fact::BtList(vec![BtDev { mac: "8C".into(), name: "Studio Earbuds".into(), kind: Some("audio-headphones".into()), connected: true }]),
        track("Under The Pressure", "The War On Drugs", "amber-horizon.jpg", 62.0, 532.0, true),
    ];
    facts.extend(cover("amber-horizon.jpg"));
    facts.push(Fact::Weather(Some(crate::weather::Weather { temp: 13.4, hi: 16.0, lo: 8.0, code: 2, day: true, place: "Brussels".into() })));
    facts.push(Fact::Toggles(crate::toggles::ToggleState { night: true, caffeine: false, recording: false, recorder: true }));
    let iconset = Rc::new(IconSet::discover());
    // The centre tab, as the tabs design would hand it over.
    let start = Rect::new((SW - 230.0) * 0.5, -15.0, 230.0, 47.0);
    let mut st = Settings::new(&mut gpu.fonts, iconset, scale, start, Some("Keyboard Tester - Helium".into()), &facts);
    let mut list = DrawList::default();
    let mut shoot = |st: &mut Settings, gpu: &mut Gpu, name: &str| {
        st.draw(&mut gpu.fonts, &mut list);
        let px = gpu.render_offscreen(&mut target, &list, st.text_buffers(), icons);
        let path = dir.join(name).with_extension("pam");
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        write!(f, "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n", size.0, size.1).unwrap();
        f.write_all(&px).unwrap();
        std::fs::write(dir.join(name).with_extension("glass"), "").unwrap();
    };
    let tick = |st: &mut Settings, gpu: &mut Gpu, secs: f32| {
        let mut l = DrawList::default();
        for _ in 0..(secs / DT).round() as u32 {
            st.draw(&mut gpu.fonts, &mut l);
            // Keep textures a skipped frame uploaded.
            if !l.uploads.is_empty() || !l.frees.is_empty() {
                let mut t = gpu.create_offscreen(4, 4);
                let only = DrawList { uploads: std::mem::take(&mut l.uploads), frees: std::mem::take(&mut l.frees), ..Default::default() };
                let _ = gpu.render_offscreen(&mut t, &only, &std::collections::HashMap::new(), icons);
            }
            st.step(DT);
        }
    };
    let stills = std::env::var_os("NIMBUS_SNAPSHOT_STILLS").is_some();
    for i in 0..48 {
        if !stills || i % 6 == 0 {
            shoot(&mut st, gpu, &format!("set_open_{i:03}"));
        } else {
            tick(&mut st, gpu, 0.0);
        }
        st.step(DT);
    }
    tick(&mut st, gpu, 1.0);
    shoot(&mut st, gpu, "set_main");
    // Slider drag: press, move in steps, release; the value must follow.
    if let Some((x, y)) = st.hit_center(Ctl::Volume) {
        st.pointer_motion(x - 120.0, y);
        tick(&mut st, gpu, 0.05);
        let mut seen = vec![format!("{:?}", st.press(x - 120.0, y))];
        for dx in [-80.0, -20.0, 40.0, 100.0, 150.0] {
            seen.push(format!("{:?}", st.pointer_motion(x + dx, y + 30.0)));
            tick(&mut st, gpu, 0.03);
        }
        shoot(&mut st, gpu, "set_drag_volume");
        seen.push(format!("{:?}", st.release()));
        eprintln!("popout volume drag: {}", seen.join(" → "));
    }
    let click = |st: &mut Settings, c: Ctl| {
        if let Some((x, y)) = st.hit_center(c) {
            st.pointer_motion(x, y);
            st.press(x, y);
            st.release();
        }
    };
    let hover = |st: &mut Settings, c: Ctl| {
        if let Some((x, y)) = st.hit_center(c) {
            st.pointer_motion(x, y);
        }
    };
    // Now playing: hover the transport, then the scrub bar, drag it.
    hover(&mut st, Ctl::MediaNext);
    tick(&mut st, gpu, 0.5);
    shoot(&mut st, gpu, "set_np_next");
    hover(&mut st, Ctl::Scrub);
    tick(&mut st, gpu, 0.4);
    shoot(&mut st, gpu, "set_np_scrub");
    if let Some((x, y)) = st.hit_center(Ctl::Scrub) {
        st.press(x - 60.0, y);
        st.pointer_motion(x + 40.0, y);
        tick(&mut st, gpu, 0.2);
        shoot(&mut st, gpu, "set_np_drag");
        eprintln!("scrub release -> {:?}", st.release());
    }
    // Pause from the panel.
    click(&mut st, Ctl::MediaPlay);
    tick(&mut st, gpu, 0.6);
    shoot(&mut st, gpu, "set_np_paused");
    // Next track: the cover reveals from its centre, text cross-fades.
    st.pointer_leave();
    st.apply(&track("Red Eyes", "The War On Drugs", "iris-ribbons.jpg", 0.0, 299.0, true));
    if let Some(f) = cover("iris-ribbons.jpg") {
        st.apply(&f);
    }
    tick(&mut st, gpu, 0.15);
    shoot(&mut st, gpu, "set_np_change_a");
    tick(&mut st, gpu, 1.0);
    shoot(&mut st, gpu, "set_np_change_b");
    // Rest check: once a second while playing, nothing when paused.
    let m = st.step(DT);
    eprintln!("popout playing at rest: animating={m} wake in {:?}", st.wake_at.map(|w| w - st.time));
    // The player goes: the module folds to a calm line.
    st.apply(&Fact::Media(None));
    tick(&mut st, gpu, 0.12);
    shoot(&mut st, gpu, "set_np_fold");
    tick(&mut st, gpu, 1.0);
    shoot(&mut st, gpu, "set_idle");
    let m = st.step(DT);
    eprintln!("popout idle at rest: animating={m} wake={:?}", st.wake_at);
    // Weather: hovering the temperature shows the forecast.
    hover(&mut st, Ctl::Weather);
    tick(&mut st, gpu, 0.5);
    shoot(&mut st, gpu, "set_weather");
    // Quick toggles: caffeine on, then a recording in progress.
    click(&mut st, Ctl::Caffeine);
    tick(&mut st, gpu, 0.4);
    st.apply(&Fact::Toggles(crate::toggles::ToggleState { night: true, caffeine: true, recording: true, recorder: true }));
    hover(&mut st, Ctl::Record);
    tick(&mut st, gpu, 0.4);
    shoot(&mut st, gpu, "set_toggles");
    st.apply(&Fact::Toggles(crate::toggles::ToggleState { night: false, caffeine: false, recording: false, recorder: false }));
    st.pointer_leave();
    tick(&mut st, gpu, 0.4);
    shoot(&mut st, gpu, "set_toggles_off");
    // Calendar: the date grows into a month; next month slides in; a
    // swipe goes back.
    click(&mut st, Ctl::Date);
    for i in 0..36 {
        if i % 6 == 0 {
            shoot(&mut st, gpu, &format!("set_cal_open_{i:03}"));
        }
        tick(&mut st, gpu, DT);
    }
    tick(&mut st, gpu, 0.6);
    shoot(&mut st, gpu, "set_cal");
    click(&mut st, Ctl::CalNext);
    tick(&mut st, gpu, 0.08);
    shoot(&mut st, gpu, "set_cal_next_a");
    tick(&mut st, gpu, 0.6);
    shoot(&mut st, gpu, "set_cal_next_b");
    if let Some((x, y)) = st.hit_center(Ctl::CalGrid) {
        st.pointer_motion(x, y);
        st.press(x, y);
        for dx in [20.0, 50.0, 90.0] {
            st.pointer_motion(x + dx, y);
            tick(&mut st, gpu, 0.03);
        }
        shoot(&mut st, gpu, "set_cal_swipe");
        st.release();
        tick(&mut st, gpu, 0.6);
        shoot(&mut st, gpu, "set_cal_back");
    }
    click(&mut st, Ctl::Back);
    tick(&mut st, gpu, 1.0);
    // Power off arms first.
    click(&mut st, Ctl::PowerOff);
    tick(&mut st, gpu, 0.5);
    shoot(&mut st, gpu, "set_arm");
    st.pointer_leave();
    tick(&mut st, gpu, 3.5);
    click(&mut st, Ctl::Wifi);
    for i in 0..48 {
        if !stills || i % 8 == 0 {
            shoot(&mut st, gpu, &format!("set_wifi_{i:03}"));
        }
        st.step(DT);
    }
    st.apply(&Fact::WifiList(vec![
        WifiNet { ssid: "Home Network".into(), signal: 100, secure: true, active: true },
        WifiNet { ssid: "Neighbour 5G".into(), signal: 67, secure: true, active: false },
        WifiNet { ssid: "Neighbour 2G".into(), signal: 30, secure: true, active: false },
        WifiNet { ssid: "Corner Cafe".into(), signal: 30, secure: true, active: false },
        WifiNet { ssid: "Café libre".into(), signal: 22, secure: false, active: false },
    ]));
    tick(&mut st, gpu, 1.0);
    if let Some((x, y)) = st.hit_center(Ctl::Row(1)) {
        st.pointer_motion(x, y);
    }
    tick(&mut st, gpu, 0.5);
    shoot(&mut st, gpu, "set_wifi_page");
    click(&mut st, Ctl::Back);
    tick(&mut st, gpu, 1.0);
    click(&mut st, Ctl::Bluetooth);
    st.apply(&Fact::BtList(vec![
        BtDev { mac: "8C".into(), name: "Studio Earbuds".into(), kind: Some("audio-headphones".into()), connected: true },
        BtDev { mac: "E0".into(), name: "Split Keyboard".into(), kind: Some("input-keyboard".into()), connected: false },
        BtDev { mac: "D0".into(), name: "Trackball".into(), kind: Some("input-mouse".into()), connected: false },
    ]));
    tick(&mut st, gpu, 1.2);
    shoot(&mut st, gpu, "set_bt_page");
}

/// The full settings window: open from the quick panel, then every page.
fn prefs(gpu: &mut Gpu, icons: &Icons, dir: &Path, scale: f32) {
    use crate::{
        bar::Rect,
        prefs::{Ctl, Prefs, SH, SW},
        services::{AboutInfo, AudioDev, AudioState, BtDev, OutputSummary, PowerDetail, SystemState, WifiNet},
    };
    let size = ((SW * scale).round() as u32, (SH * scale).round() as u32);
    let mut target = gpu.create_offscreen(size.0, size.1);
    let facts = vec![
        Fact::Volume(VolumeInfo { percent: 62, muted: false }),
        Fact::System(SystemState { brightness: Some(0.6), wifi: Some(true), bluetooth: Some(true) }),
        Fact::Audio(AudioState {
            sinks: vec![
                AudioDev { id: 52, name: "Ryzen HD Audio Controller Analog Stereo".into(), default: true },
                AudioDev { id: 61, name: "Studio Earbuds".into(), default: false },
            ],
            sources: vec![],
            mic: Some(VolumeInfo { percent: 17, muted: false }),
        }),
        Fact::Outputs(vec![OutputSummary { name: "eDP-1".into(), model: "Samsung Display Corp. 0x4178".into(), mode: "3200×2000 @ 120 Hz".into(), scale: 1.75 }]),
        Fact::Power(PowerDetail { percent: 44, status: "Discharging".into(), health: Some(91), cycles: Some(212), hours: Some(3.4), limit: Some(80), profile: Some("balanced".into()) }),
        Fact::About(AboutInfo { niri: Some("26.04".into()), rss_kb: 41_000, cpu_secs: 3.2, uptime_secs: 5400.0, threads: 7 }),
        Fact::WifiList(vec![
            WifiNet { ssid: "Home Network".into(), signal: 100, secure: true, active: true },
            WifiNet { ssid: "Neighbour 5G".into(), signal: 67, secure: true, active: false },
            WifiNet { ssid: "Corner Cafe".into(), signal: 30, secure: true, active: false },
        ]),
        Fact::BtList(vec![
            BtDev { mac: "8C".into(), name: "Studio Earbuds".into(), kind: Some("audio-headphones".into()), connected: true },
            BtDev { mac: "E0".into(), name: "Split Keyboard".into(), kind: Some("input-keyboard".into()), connected: false },
        ]),
    ];
    let iconset = Rc::new(IconSet::discover());
    // As handed over from the quick panel (its body in this surface's space).
    let start = Rect::new((SW - 440.0) * 0.5, 14.0, 440.0, 300.0);
    let mut st = Prefs::new(&mut gpu.fonts, iconset, scale, start, "AMD Radeon 890M (RADV STRIX1)".into(), &facts);
    let mut list = DrawList::default();
    let mut shoot = |st: &mut Prefs, gpu: &mut Gpu, name: &str| {
        st.draw(&mut gpu.fonts, &mut list);
        let px = gpu.render_offscreen(&mut target, &list, st.text_buffers(), icons);
        let mut f = std::io::BufWriter::new(std::fs::File::create(dir.join(name).with_extension("pam")).unwrap());
        write!(f, "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n", size.0, size.1).unwrap();
        f.write_all(&px).unwrap();
        std::fs::write(dir.join(name).with_extension("glass"), "").unwrap();
    };
    let tick = |st: &mut Prefs, gpu: &mut Gpu, secs: f32| {
        let mut l = DrawList::default();
        for _ in 0..(secs / DT).round() as u32 {
            st.draw(&mut gpu.fonts, &mut l);
            st.step(DT);
        }
    };
    for i in 0..60 {
        shoot(&mut st, gpu, &format!("prefs_open_{i:03}"));
        st.step(DT);
    }
    tick(&mut st, gpu, 1.0);
    for (k, name) in ["profile", "appearance", "sound", "display", "network", "power", "about"].iter().enumerate() {
        if let Some((x, y)) = st.hit_center(Ctl::Nav(k as u8)) {
            st.pointer_motion(x, y);
            st.press(x, y);
            st.release();
        }
        st.pointer_leave();
        tick(&mut st, gpu, 1.0);
        shoot(&mut st, gpu, &format!("prefs_{name}"));
        // Slider drag on the Sound / Display pages.
        if matches!(*name, "sound" | "display")
            && let Some((x, y)) = st.hit_center(Ctl::Slider(if *name == "sound" { 0 } else { 2 }))
        {
            st.pointer_motion(x - 100.0, y);
            tick(&mut st, gpu, 0.05);
            let mut seen = vec![format!("{:?}", st.press(x - 100.0, y))];
            for dx in [-60.0, 0.0, 60.0, 120.0] {
                seen.push(format!("{:?}", st.pointer_motion(x + dx, y + 25.0)));
                tick(&mut st, gpu, 0.03);
            }
            seen.push(format!("{:?}", st.release()));
            eprintln!("prefs {name} drag: {}", seen.join(" → "));
        }
    }
}

/// A made-up picture for the Profile demo: sky, sun, hills, no real photo.
fn fixture_picture(path: &Path, seed: u32, w: u32, h: u32, person: bool) {
    let hue = |t: f32, k: f32| 0.5 + 0.5 * (std::f32::consts::TAU * (t + k)).cos();
    let (k0, k1) = (seed as f32 * 0.137, seed as f32 * 0.291 + 0.2);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
        let sky = [hue(v * 0.4 + k0, 0.0), hue(v * 0.4 + k0, 0.33), hue(v * 0.4 + k0, 0.67)];
        let (dx, dy) = (u - 0.5, (v - 0.42) * h as f32 / w as f32);
        let mut c = sky.map(|c| (c * 0.42 + 0.2 * (1.0 - v)).min(0.8));
        if person {
            // Head and shoulders.
            let head = (dx * dx + dy * dy).sqrt() < 0.16;
            let body = v > 0.62 && dx.abs() < 0.34 - (0.86 - v).max(0.0) * 0.5;
            if head || body {
                c = [0.08 + 0.16 * hue(k1, 0.0), 0.07 + 0.1 * hue(k1, 0.33), 0.12 + 0.12 * hue(k1, 0.67)];
            }
        } else {
            let hill = 0.68 + 0.1 * (u * 7.0 + k1 * 9.0).sin() + 0.05 * (u * 17.0 + k0 * 5.0).sin();
            let sun = (dx * dx + (dy + 0.1) * (dy + 0.1)).sqrt();
            if sun < 0.09 {
                c = [1.0, 0.86, 0.7];
            }
            if v > hill {
                c = [0.05 + 0.1 * hue(k1, 0.1), 0.05 + 0.13 * hue(k1, 0.4), 0.09 + 0.12 * hue(k1, 0.7)];
            }
        }
        image::Rgb(c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8))
    });
    img.save_with_format(path, image::ImageFormat::Jpeg).unwrap();
}

/// Settings' Profile page, start to finish, for a made-up person on a
/// made-up machine (`NIMBUS_SNAPSHOT_ONLY=profile`): the initial, the picker,
/// the crop, a real save into a throwaway home, the name, Remove, and the
/// checks that nothing moves at rest and every texture is handed back.
fn profile_demo(gpu: &mut Gpu, icons: &Icons, dir: &Path, scale: f32) {
    use crate::{
        bar::Rect,
        prefs::{Ctl, Prefs, SH, SW},
        profile::info::Info,
        window::{KeyInput, keys},
    };
    // Name edits go to the config: only ever a throwaway one.
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
    if !config.starts_with(dir.to_string_lossy().as_ref()) {
        eprintln!("nimbus: the profile demo needs XDG_CONFIG_HOME inside {}", dir.display());
        return;
    }
    let home = dir.join("fixture-home");
    let pics = home.join("Pictures");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(pics.join("Trips")).unwrap();
    for i in 0..23 {
        let (w, h) = if i % 3 == 1 { (900, 1300) } else { (1500, 1000) };
        let p = if i % 4 == 3 { pics.join(format!("Trips/hill-{i}.jpg")) } else { pics.join(format!("picture-{i}.jpg")) };
        fixture_picture(&p, i + 3, w, h, i == 1 || i == 7);
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + 60 * (40 - i as u64))).unwrap();
    }

    let mut d = Demo::new(gpu, icons, dir, scale);
    let tx = d.tx.clone();
    let start = Rect::new((SW - 440.0) * 0.5, 14.0, 440.0, 300.0);
    let mut st = Prefs::new(&mut d.gpu.fonts, d.iconset.clone(), scale, start, "Made Up Graphics 9000".into(), &[]);
    st.attach_at(tx, home.clone(), "ada".into(), "Ada Lovelace".into());
    st.set_info(Info {
        login: "ada".into(),
        host: Some("analytical".into()),
        device: Some("Babbage Engine 16 Mk II".into()),
        os: Some("NixOS 26.11 (Fixture)".into()),
        generation: Some(437),
        kernel: Some("6.18.0-fixture".into()),
        cpu: Some("Difference CPU 8000 · 16 threads".into()),
        gpu: Some("Made Up Graphics 9000".into()),
        memory: Some("30.7 GiB".into()),
        disk: Some("412 GiB used · 390 GiB free".into()),
        uptime: Some(3 * 86_400 + 4 * 3600),
        since: Some("10 December 2024 · 691 days".into()),
        days_here: Some(691),
        birthday: None,
        wallpaper: Some("dark lake".into()),
        hour: Some(21),
    });

    fn run(d: &mut Demo, st: &mut Prefs, secs: f32) {
        let mut l = DrawList::default();
        for _ in 0..(secs / DT).round() as u32 {
            let mut q = Vec::new();
            let _ = d.events.dispatch(Some(std::time::Duration::ZERO), &mut q);
            for f in &q {
                st.on_custom(f);
            }
            st.draw(&mut d.gpu.fonts, &mut l);
            d.consume_uploads(&mut l);
            st.step(DT);
        }
    }
    /// Run until `done`, at most `secs` of wall time.
    fn until(d: &mut Demo, st: &mut Prefs, secs: f32, what: &str, done: impl Fn(&Prefs) -> bool) {
        let t = std::time::Instant::now();
        while !done(st) {
            assert!(t.elapsed().as_secs_f32() < secs, "profile demo: timed out waiting for {what}");
            run(d, st, 0.05);
        }
        run(d, st, 0.8);
    }
    fn shoot(d: &mut Demo, st: &mut Prefs, name: &str) {
        let mut l = DrawList::default();
        st.draw(&mut d.gpu.fonts, &mut l);
        let bufs = st.text_buffers().clone();
        d.render(&l, &bufs, (SW, SH), name);
    }
    fn hover(d: &mut Demo, st: &mut Prefs, c: Ctl) -> (f32, f32) {
        run(d, st, 0.02);
        let (x, y) = st.hit_center(c).unwrap_or_else(|| panic!("profile demo: no {c:?} on screen"));
        st.pointer_motion(x, y);
        (x, y)
    }
    fn click(d: &mut Demo, st: &mut Prefs, c: Ctl) {
        let (x, y) = hover(d, st, c);
        run(d, st, 0.03);
        st.press(x, y);
        st.release();
    }
    fn press_key(st: &mut Prefs, sym: u32, text: Option<&str>) {
        st.key(&KeyInput { sym, text: text.map(String::from), ctrl: false, shift: false, alt: false });
    }

    run(&mut d, &mut st, 1.2);
    shoot(&mut d, &mut st, "profile_initial");

    click(&mut d, &mut st, Ctl::Avatar);
    until(&mut d, &mut st, 20.0, "thumbnails", |s| s.profile_thumbs() >= 20);
    let _ = hover(&mut d, &mut st, Ctl::Thumb(6));
    run(&mut d, &mut st, 0.4);
    shoot(&mut d, &mut st, "profile_picker");
    st.scroll(1.5);
    run(&mut d, &mut st, 0.8);
    shoot(&mut d, &mut st, "profile_picker_scrolled");
    st.scroll(-2.0);
    run(&mut d, &mut st, 0.8);
    for t in ["~", "/", "n", "o", "p", "e", ".", "p", "n", "g"] {
        press_key(&mut st, 0x20, Some(t));
    }
    press_key(&mut st, keys::RETURN, None);
    run(&mut d, &mut st, 0.2);
    until(&mut d, &mut st, 5.0, "the path's verdict", |s| s.profile_error().is_some());
    shoot(&mut d, &mut st, "profile_picker_error");
    for _ in 0..10 {
        press_key(&mut st, keys::BACKSPACE, None);
    }

    click(&mut d, &mut st, Ctl::Thumb(1));
    until(&mut d, &mut st, 20.0, "the crop's picture", |s| s.profile_cropping());
    shoot(&mut d, &mut st, "profile_crop");
    let (x, y) = hover(&mut d, &mut st, Ctl::Stage);
    st.scroll(-4.0);
    st.press(x, y);
    for k in 1..=6 {
        st.pointer_motion(x + 6.0 * k as f32, y + 9.0 * k as f32);
    }
    st.release();
    run(&mut d, &mut st, 0.5);
    shoot(&mut d, &mut st, "profile_crop_framed");

    click(&mut d, &mut st, Ctl::CropSave);
    until(&mut d, &mut st, 20.0, "the save", |s| s.profile_has_avatar() && !s.profile_cropping());
    let face = image::load_from_memory(&std::fs::read(home.join(".face")).unwrap()).expect("the saved picture opens");
    eprintln!("profile: saved {}x{} {}", face.width(), face.height(), std::fs::read(home.join(".face")).unwrap().get(1..4).map_or("?", |b| std::str::from_utf8(b).unwrap_or("?")));
    run(&mut d, &mut st, 1.4);
    shoot(&mut d, &mut st, "profile_saved");

    click(&mut d, &mut st, Ctl::NameField);
    run(&mut d, &mut st, 0.1);
    press_key(&mut st, keys::END, None);
    for t in [" ", "K"] {
        press_key(&mut st, 0x20, Some(t));
    }
    run(&mut d, &mut st, 0.4);
    shoot(&mut d, &mut st, "profile_name");
    press_key(&mut st, keys::RETURN, None);
    run(&mut d, &mut st, 0.4);
    eprintln!("profile: config has {:?}", crate::config::get(crate::profile::NAME_KEY));
    st.pointer_leave();
    run(&mut d, &mut st, 2.0);
    let still = (0..240).all(|_| !st.step(DT));
    eprintln!("profile: at rest on the card: {still}");
    shoot(&mut d, &mut st, "profile_rest");

    for name in ["profile_cycle1", "profile_cycle2"] {
        click(&mut d, &mut st, Ctl::Friendly);
        st.pointer_leave();
        run(&mut d, &mut st, 0.6);
        shoot(&mut d, &mut st, name);
    }

    click(&mut d, &mut st, Ctl::RemovePic);
    run(&mut d, &mut st, 0.3);
    shoot(&mut d, &mut st, "profile_remove_armed");
    click(&mut d, &mut st, Ctl::RemovePic);
    until(&mut d, &mut st, 5.0, "the removal", |s| !s.profile_has_avatar());
    st.pointer_leave();
    run(&mut d, &mut st, 0.5);
    shoot(&mut d, &mut st, "profile_removed");
    eprintln!("profile: ~/.face gone: {}", !home.join(".face").exists());

    // The picker's thumbnails go when it's left; what stays is given back.
    click(&mut d, &mut st, Ctl::Avatar);
    until(&mut d, &mut st, 20.0, "thumbnails again", |s| s.profile_thumbs() >= 15);
    let open = st.profile_textures();
    click(&mut d, &mut st, Ctl::Back);
    run(&mut d, &mut st, 1.0);
    eprintln!("profile: textures in the picker {open}, after leaving it {}", st.profile_textures());
    let mut frees = vec![];
    st.close();
    st.release_textures(&mut frees);
    eprintln!("profile: textures after close {} ({} handed back)", st.profile_textures(), frees.len());
}

/// Headless harness for feature-window demos (`launcher::demo` etc.).
/// Renders through the real GPU path into PAM files in the snapshot dir.
pub struct Demo<'a> {
    pub gpu: &'a mut Gpu,
    pub icons: &'a Icons,
    pub dir: &'a Path,
    pub scale: f32,
    pub iconset: Rc<IconSet>,
    /// Facts sent by module workers (via `Ctx::fact_tx`) land here.
    events: calloop::EventLoop<'static, Vec<Fact>>,
    tx: calloop::channel::Sender<Fact>,
    pub facts: Vec<Fact>,
}

impl<'a> Demo<'a> {
    fn new(gpu: &'a mut Gpu, icons: &'a Icons, dir: &'a Path, scale: f32) -> Self {
        let events: calloop::EventLoop<'static, Vec<Fact>> = calloop::EventLoop::try_new().unwrap();
        let (tx, rx) = calloop::channel::channel::<Fact>();
        events
            .handle()
            .insert_source(rx, |e, _, q: &mut Vec<Fact>| {
                if let calloop::channel::Event::Msg(f) = e {
                    q.push(f);
                }
            })
            .unwrap();
        let facts = vec![
            Fact::Clock(ClockInfo { hour: 11, minute: 56, weekday: 6, day: 25, month: 9 }),
            Fact::Battery(Some(BatteryInfo { percent: 60, charging: true })),
            Fact::Net(NetInfo::Wifi(90)),
            Fact::Volume(VolumeInfo { percent: 62, muted: false }),
        ];
        Self { gpu, icons, dir, scale, iconset: Rc::new(IconSet::discover()), events, tx, facts }
    }

    /// A window context like the live bar would give (1828×1142 output).
    pub fn ctx(&mut self) -> crate::window::Ctx<'_> {
        let tabs = [
            crate::bar::Rect::new(9.0, -15.0, 120.0, 47.0),
            crate::bar::Rect::new((1828.0 - 230.0) * 0.5, -15.0, 230.0, 47.0),
            crate::bar::Rect::new(1828.0 - 9.0 - 300.0, -15.0, 300.0, 47.0),
        ];
        crate::window::Ctx {
            fs: &mut self.gpu.fonts,
            icons: self.iconset.clone(),
            scale: self.scale,
            facts: &self.facts,
            out_w: 1828.0,
            out_h: 1142.0,
            tabs,
            fact_tx: self.tx.clone(),
        }
    }

    /// Wait up to `secs` of wall time for worker facts and apply them.
    pub fn pump(&mut self, scene: &mut dyn crate::window::WinScene, secs: f32) {
        let mut q = Vec::new();
        let _ = self.events.dispatch(Some(std::time::Duration::from_secs_f32(secs)), &mut q);
        let _ = self.events.dispatch(Some(std::time::Duration::ZERO), &mut q);
        for f in &q {
            scene.apply(f);
        }
    }

    /// Step `secs` at 120 Hz, drawing every tick like the live loop.
    pub fn tick(&mut self, scene: &mut dyn crate::window::WinScene, secs: f32) {
        let mut l = DrawList::default();
        for _ in 0..(secs / DT).round() as u32 {
            scene.draw(&mut self.gpu.fonts, &mut l);
            self.consume_uploads(&mut l);
            scene.step(DT);
        }
    }

    /// Keep textures that a frame uploaded (a skipped frame would lose them).
    fn consume_uploads(&mut self, l: &mut DrawList) {
        if !l.uploads.is_empty() || !l.frees.is_empty() {
            let mut t = self.gpu.create_offscreen(4, 4);
            let only = DrawList { uploads: std::mem::take(&mut l.uploads), frees: std::mem::take(&mut l.frees), ..Default::default() };
            let empty = std::collections::HashMap::new();
            let _ = self.gpu.render_offscreen(&mut t, &only, &empty, self.icons);
        }
    }

    /// Render the window's current frame to `<name>.pam`.
    pub fn shoot(&mut self, scene: &mut dyn crate::window::WinScene, name: &str) {
        let mut l = DrawList::default();
        scene.draw(&mut self.gpu.fonts, &mut l);
        let (w, h) = scene.size();
        let bufs = scene.text_buffers().clone();
        self.render(&l, &bufs, (w, h), name);
    }

    /// Render an arbitrary draw list of `size` logical px to `<name>.pam`.
    pub fn render(&mut self, l: &DrawList, bufs: &std::collections::HashMap<u64, glyphon::Buffer>, size: (f32, f32), name: &str) {
        let px = ((size.0 * self.scale).round() as u32, (size.1 * self.scale).round() as u32);
        let mut target = self.gpu.create_offscreen(px.0, px.1);
        let data = self.gpu.render_offscreen(&mut target, l, bufs, self.icons);
        let mut f = std::io::BufWriter::new(std::fs::File::create(self.dir.join(name).with_extension("pam")).unwrap());
        write!(f, "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n", px.0, px.1).unwrap();
        f.write_all(&data).unwrap();
        std::fs::write(self.dir.join(name).with_extension("glass"), "").unwrap();
    }
}
