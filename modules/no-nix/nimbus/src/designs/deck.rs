//! Deck: the hanging tabs, dealt out richer.
//!
//!   [ spaces ][ ▣ now playing ━━━ ]     [ ◧ app · title ]     [ vol  net  ▭ 43% │ date 11:00 ]
//!
//! Four Prism Ink tabs hang from the top edge (flat top, blocky-round
//! bottom): flat matte ink, no sheen or bevel, edged by the prism
//! hairline (cyan left, magenta right, amber along the bottom):
//!
//! - spaces: dots, occupied ones stretch with their window count; the
//!   active one is a liquid iridescent pill.
//! - now playing (MPRIS): pulls out from behind the spaces tab like a
//!   drawer when something plays and tucks back in when the player goes.
//!   Rounded album art, title and artist, a hairline progress lip along
//!   the bottom edge (iridescent; the cover's colours under another accent).
//!   Hover widens it to the right into prev / play-pause / next and the
//!   elapsed time; scroll seeks. A new track reveals its cover from the
//!   centre and wipes the text in place.
//! - centre: the focused app's colour icon and its title (click: quick
//!   settings, which grows out of this tab).
//! - status: volume (level peeks on change / hover), network, a drawn
//!   battery cell, the clock; the date slides out on hover.
//!
//! At rest nothing animates: progress advances on wake timers (about once
//! a second, slower for long tracks), and only while something plays.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use glyphon::FontSystem;

use crate::{
    anim::{ColorSpring, Spring, SpringCfg, presets},
    bar::{Bar, Font, Hit, ICON, Rect},
    gfx::{self, DrawList, ImageData, ImageDraw},
    media::MediaInfo,
    paint,
    services::NetInfo,
    theme::{self, Rgba},
};

pub const FONTS: &[&str] = &["Geist"];

const TAB_H: f32 = 32.0;
const TAB_MARGIN: f32 = 9.0;
const TAB_GAP: f32 = 8.0;
const DROP: f32 = 3.0;
const RADIUS: f32 = 13.0;
const PAD: f32 = 13.0;
const GAP: f32 = 12.0;
const SLOT: f32 = 16.0;
const TITLE_MAX: f32 = 360.0;
const VOL_TRACK: f32 = 64.0;
const APP_ICON: f32 = 16.0;

// now playing
const ART: f32 = 22.0;
const ART_R: f32 = 6.0;
/// Art inset from the tab's left edge (matches its inset from the bottom).
const M_PAD_L: f32 = 5.0;
const M_TITLE_MAX: f32 = 220.0;
const M_ARTIST_MAX: f32 = 160.0;
const BTN: f32 = 26.0;
const LINE_INSET: f32 = 10.0;

const F_CLOCK: Font = Font::new(14.0, 600).family("Geist");
const F_TITLE: Font = Font::new(13.0, 500).family("Geist");
const F_ARTIST: Font = Font::new(12.5, 500).family("Geist");
const F_SMALL: Font = Font::new(12.0, 600).family("Geist");
const F_TIME: Font = Font::new(11.5, 600).family("Geist");

const SHADOW: Rgba = Rgba::hex(0x000000, 0.5);
/// Cursor spotlight strength on a hovered tab.
const SPOT: f32 = 0.85;
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
const DOT_USED: Rgba = Rgba::hex(0xd7d9e2, 0.8);
const DOT_EMPTY: Rgba = Rgba::hex(0xffffff, 0.22);
const TRACK: Rgba = Rgba::hex(0xffffff, 0.08);
const GOOD: Rgba = Rgba::hex(0x7ee2b8, 1.0);
const WARN: Rgba = Rgba::hex(0xffd37a, 1.0);
const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);
/// Recording: a soft red, never iridescent.
const REC_RED: Rgba = Rgba::hex(0xff6b7f, 1.0);

/// Tabs entering and the drawer: quick with a hint of overshoot.
const ENTER: SpringCfg = SpringCfg::new(0.5, 0.78);
const DRAWER: SpringCfg = SpringCfg::new(0.42, 0.8);
/// Hover reactions.
const UI: SpringCfg = SpringCfg::new(0.28, 0.86);
const SEG: SpringCfg = SpringCfg::new(0.34, 1.0);
const WIPE: SpringCfg = SpringCfg::new(0.5, 1.0);

/// Recording dot size.
const REC: f32 = 9.0;
/// Tray icon size and spacing.
const TRAY: f32 = 16.0;
const TRAY_GAP: f32 = 9.0;
const PEEK_VOL: f32 = 1.8;

static NEXT_KEY: AtomicU64 = AtomicU64::new(0xDEC0_0000_0000_0001);

/// A texture that cross-fades to its successor. Uploads and frees ride on
/// the next draw list; whatever is left is freed when the design goes.
struct Swap {
    cur: Option<(u64, String)>,
    prev: Option<(u64, String)>,
    pending: Option<(u64, ImageData)>,
    t: Spring,
    frees: Vec<u64>,
}

impl Default for Swap {
    fn default() -> Self {
        Self { cur: None, prev: None, pending: None, t: Spring::new(1.0, WIPE), frees: vec![] }
    }
}

impl Swap {
    fn want(&mut self, id: &str, img: &ImageData) {
        if self.cur.as_ref().is_some_and(|c| c.1 == id) {
            return;
        }
        if let Some((k, _)) = self.prev.take() {
            self.frees.push(k);
        }
        self.prev = self.cur.take();
        let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
        self.cur = Some((key, id.to_string()));
        self.pending = Some((key, img.clone()));
        if self.prev.is_some() {
            self.t.snap(0.0);
            self.t.set(1.0);
        } else {
            self.t.snap(1.0);
        }
    }
    fn clear(&mut self) {
        self.frees.extend(self.cur.take().map(|c| c.0));
        self.frees.extend(self.prev.take().map(|c| c.0));
        self.pending = None;
        self.t.snap(1.0);
    }
    fn flush(&mut self, out: &mut DrawList) {
        if self.t.at_rest()
            && let Some((k, _)) = self.prev.take()
        {
            self.frees.push(k);
        }
        out.uploads.extend(self.pending.take());
        out.frees.append(&mut self.frees);
    }
    fn keys(&mut self) -> Vec<u64> {
        let mut v = std::mem::take(&mut self.frees);
        v.extend(self.cur.take().map(|c| c.0));
        v.extend(self.prev.take().map(|c| c.0));
        v
    }
}

pub struct State {
    t: f32,
    enter: [Spring; 4],
    /// Hover drop per tab: spaces, media, centre, status.
    drop: [Spring; 4],
    ws_w: Spring,

    media_k: Spring,
    /// What the media tab shows (kept while it folds away).
    shown: Option<MediaInfo>,
    track: Option<(String, String)>,
    prev_track: Option<(String, String)>,
    track_xf: Spring,
    ctl: Spring,
    btn_x: Spring,
    btn_a: Spring,
    playing: Spring,
    tint_a: ColorSpring,
    tint_b: ColorSpring,
    art: Swap,
    /// Progress lip width last frame (sets the wake cadence).
    line_w: f32,

    icon: Swap,
    icon_k: Spring,

    vol: Spring,
    date: Spring,
    level: Spring,
    bat: Spring,
    /// Earliest deadline this design wants a frame at; read by designs::step.
    pub wake: Option<f32>,
    /// Tray textures by item id: (colour key, grey key, icon rev).
    tray_tex: HashMap<String, (u64, u64, u64)>,
    tray_free: Vec<u64>,
    /// Per-item hover (grey → colour).
    tray_hover: HashMap<String, Spring>,
    /// Recording dot: presence and its slow pulse.
    rec: Spring,
    rec_pulse: Spring,
}

impl Default for State {
    fn default() -> Self {
        let (a, b) = theme::accent();
        Self {
            t: 0.0,
            enter: [Spring::new(0.0, ENTER); 4],
            drop: [Spring::new(0.0, UI); 4],
            ws_w: Spring::new(0.0, SEG),
            media_k: Spring::new(0.0, DRAWER),
            shown: None,
            track: None,
            prev_track: None,
            track_xf: Spring::new(1.0, WIPE),
            ctl: Spring::new(0.0, UI),
            btn_x: Spring::new(1.0, presets::SNAPPY),
            btn_a: Spring::new(0.0, UI),
            playing: Spring::new(1.0, SEG),
            tint_a: ColorSpring::new(a, presets::GENTLE),
            tint_b: ColorSpring::new(b, presets::GENTLE),
            art: Swap::default(),
            line_w: 200.0,
            icon: Swap::default(),
            icon_k: Spring::new(0.0, SEG),
            vol: Spring::new(0.0, SEG),
            date: Spring::new(0.0, SEG),
            level: Spring::new(0.0, presets::SNAPPY),
            bat: Spring::new(-1.0, SEG),
            wake: None,
            tray_tex: HashMap::new(),
            tray_free: vec![],
            tray_hover: HashMap::new(),
            rec: Spring::new(0.0, SEG),
            rec_pulse: Spring::new(1.0, SpringCfg::new(0.4, 1.0)),
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let mut k = self.art.keys();
        k.extend(self.icon.keys());
        k.append(&mut self.tray_free);
        k.extend(self.tray_tex.drain().flat_map(|(_, (a, b, _))| [a, b]));
        if !k.is_empty() {
            gfx::free_later(k);
        }
    }
}

fn on(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

fn mtime(s: f32) -> String {
    let s = s.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn step(bar: &Bar, st: &mut State, dt: f32) -> bool {
    let now = bar.time;
    st.wake = None;
    let mut m = false;

    // Tabs drop in one after another.
    st.t += dt;
    for (i, e) in st.enter.iter_mut().enumerate() {
        if e.target == 0.0 {
            if st.t >= i as f32 * 0.05 {
                e.set(1.0);
            }
            m = true;
        }
    }

    let h = bar.hovered;
    let over_media = matches!(h, Hit::Media | Hit::MediaCtl(_));
    st.drop[0].set(on(matches!(h, Hit::Spaces | Hit::Workspace(_))));
    st.drop[1].set(on(over_media));
    st.drop[2].set(on(h == Hit::Title));
    st.drop[3].set(on(matches!(h, Hit::Volume | Hit::Status | Hit::Clock)));

    // ── now playing ─────────────────────────────────────────────────────
    match &bar.media {
        Some(mi) => {
            let key = (mi.title.clone(), mi.artist.clone());
            if st.track.as_ref() != Some(&key) {
                // Only wipe text that is actually on screen.
                if st.track.is_some() && st.media_k.get() > 0.6 {
                    st.prev_track = st.track.take();
                    st.track_xf.snap(0.0);
                    st.track_xf.set(1.0);
                } else {
                    st.prev_track = None;
                    st.track_xf.snap(1.0);
                }
                st.track = Some(key);
            }
            let stale = st.shown.as_ref().is_none_or(|s| {
                s.position != mi.position || s.playing != mi.playing || s.length != mi.length || s.art_url != mi.art_url || s.title != mi.title
                    || s.artist != mi.artist || s.can_next != mi.can_next || s.can_prev != mi.can_prev
            });
            if stale {
                st.shown = Some(mi.clone());
            }
            st.media_k.set(1.0);
            st.playing.set(on(mi.playing));
        }
        None => st.media_k.set(0.0),
    }
    st.ctl.set(on(bar.media.is_some() && over_media));
    if let Hit::MediaCtl(i) = h {
        if st.btn_a.get() < 0.05 {
            st.btn_x.snap(i as f32);
        }
        st.btn_x.set(i as f32);
        st.btn_a.set(1.0);
    } else {
        st.btn_a.set(0.0);
    }
    // The tab takes the cover's colours (the accent while there are none).
    let art_tint = st
        .shown
        .as_ref()
        .and_then(|s| s.art_url.as_ref())
        .and_then(|u| bar.media_art.as_ref().filter(|a| &a.url == u))
        .and_then(|a| a.tint);
    let (ta, tb) = art_tint.unwrap_or_else(theme::accent);
    st.tint_a.set(ta);
    st.tint_b.set(tb);
    // Progress moves on timers, never by animating.
    if let Some(mi) = &bar.media
        && mi.playing
        && st.media_k.target > 0.0
    {
        let pos = bar.media_pos();
        let rate = mi.rate.max(0.1);
        let next = if st.ctl.target > 0.0 || mi.length <= 0.0 {
            // The elapsed time is showing: tick with its seconds.
            (pos.floor() + 1.0 - pos) / rate
        } else {
            let px = (st.line_w * bar.scale).max(1.0);
            (mi.length / px / rate).clamp(1.0, 5.0)
        };
        if mi.length <= 0.0 || pos < mi.length {
            st.wake = Some(now + next.max(0.05));
        }
    }

    // ── status ──────────────────────────────────────────────────────────
    let pv = now >= bar.vol_changed && now < bar.vol_changed + PEEK_VOL;
    if pv {
        let end = bar.vol_changed + PEEK_VOL;
        st.wake = Some(st.wake.map_or(end, |w| w.min(end)));
    }
    st.vol.set(on(pv || h == Hit::Volume));
    st.date.set(on(h == Hit::Clock));
    if let Some(v) = bar.volume {
        st.level.set(if v.muted { 0.0 } else { v.percent.min(100) as f32 / 100.0 });
    }
    if let Some(b) = bar.battery {
        let l = b.percent.min(100) as f32 / 100.0;
        if st.bat.target < 0.0 {
            st.bat.snap(l);
        }
        st.bat.set(l);
    }

    // Recording: a red dot that breathes twice a second (timers, not a
    // continuous animation).
    st.rec.set(on(bar.toggles.recording));
    if bar.toggles.recording {
        let half = (now * 2.0).floor();
        st.rec_pulse.set(if half as i64 % 2 == 0 { 1.0 } else { 0.35 });
        let next = (half + 1.0) * 0.5;
        st.wake = Some(st.wake.map_or(next, |w: f32| w.min(next)));
    } else {
        st.rec_pulse.set(1.0);
    }
    m |= st.rec.step(dt) | st.rec_pulse.step(dt);

    // Tray icons warm up from grey to colour under the pointer.
    st.tray_hover.retain(|id, _| bar.tray.iter().any(|t| &t.id == id));
    for (i, t) in bar.tray.iter().enumerate() {
        let sp = st.tray_hover.entry(t.id.clone()).or_insert(Spring::new(0.0, SEG));
        sp.set(on(h == Hit::Tray(i as u8)));
        m |= sp.step(dt);
    }
    for s in st.enter.iter_mut().chain(st.drop.iter_mut()) {
        m |= s.step(dt);
    }
    for s in [
        &mut st.ws_w,
        &mut st.media_k,
        &mut st.track_xf,
        &mut st.ctl,
        &mut st.btn_x,
        &mut st.btn_a,
        &mut st.playing,
        &mut st.icon_k,
        &mut st.vol,
        &mut st.date,
        &mut st.level,
        &mut st.bat,
        &mut st.art.t,
        &mut st.icon.t,
    ] {
        m |= s.step(dt);
    }
    m |= st.tint_a.step(dt) | st.tint_b.step(dt);

    // Folded away: forget the track and free its cover.
    if bar.media.is_none() && st.media_k.at_rest() && st.shown.is_some() {
        st.shown = None;
        st.track = None;
        st.prev_track = None;
        st.art.clear();
    }
    m
}

/// A tab body: Prism Ink (flat matte ink, the prism hairline). `lift`
/// (hover) deepens the shadow and turns on the cursor spotlight; the tab
/// itself never changes size.
fn body(bar: &mut Bar, out: &mut DrawList, r: Rect, a: f32, lift: f32) {
    let sc = bar.scale;
    let l = lift.max(0.0);
    out.shapes.push(
        paint::ink(sc, r, RADIUS)
            .shadow(SHADOW.fade(1.0 + 0.5 * l), 10.0 + 10.0 * l, 4.0 + 4.0 * l)
            // The islands' cursor spotlight: a soft light that follows the
            // pointer across the hovered tab.
            .spot(bar.spot_x.get(), bar.spot_y.get(), 70.0, SPOT * l.min(1.0))
            .alpha(a)
            .done(),
    );
    bar.glass.push(r);
    out.glass(r, RADIUS, a);
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    (x1 - x0 > 0.3 && y1 - y0 > 0.3).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// A texture in `r` cropped to `clip` (uv follows), optionally masked by
/// a circle growing from the centre (`reveal` 0..1, ≥1 = off).
#[allow(clippy::too_many_arguments)]
fn image(bar: &Bar, out: &mut DrawList, key: u64, r: Rect, radius: f32, clip: Rect, alpha: f32, reveal: f32) {
    if alpha <= 0.003 {
        return;
    }
    let s = bar.scale;
    let Some(c) = intersect(r, clip) else { return };
    let mut d = ImageDraw::new(key, [c.x * s, c.y * s, c.w * s, c.h * s]);
    d.uv = [(c.x - r.x) / r.w, (c.y - r.y) / r.h, (c.x + c.w - r.x) / r.w, (c.y + c.h - r.y) / r.h];
    // Only round the corners that are still the image's own.
    d.radius = if (c.w - r.w).abs() < 0.5 { radius * s } else { 1.0 * s };
    d.alpha = alpha.min(1.0);
    if reveal < 1.0 {
        let diag = (r.w * r.w + r.h * r.h).sqrt() * 0.5;
        d.reveal = [(r.x + r.w * 0.5) * s, (r.y + r.h * 0.5) * s, (diag + 3.0) * reveal.max(0.0) * s, 3.0 * s];
    }
    out.images.push(d);
}

/// Wipe `prev` → `cur` left to right at progress `p` behind a thin caret.
#[allow(clippy::too_many_arguments)]
fn wipe(bar: &mut Bar, out: &mut DrawList, prev: Option<u64>, cur: u64, x: f32, y: f32, span: f32, p: f32, clip: Rect, color: Rgba, caret: (Rgba, Rgba)) {
    let Some(prev) = prev.filter(|_| p < 0.999) else {
        bar.text_at(out, cur, x, y, color, clip);
        return;
    };
    let right = clip.x + clip.w;
    let seam = (x + span * p.clamp(0.0, 1.0)).clamp(clip.x, right);
    if seam - clip.x > 0.5 {
        bar.text_at(out, cur, x, y, color, Rect::new(clip.x, clip.y, seam - clip.x, clip.h));
    }
    if right - seam > 0.5 {
        bar.text_at(out, prev, x, y, color, Rect::new(seam, clip.y, right - seam, clip.h));
    }
    let k = (4.0 * p * (1.0 - p)).clamp(0.0, 1.0) * color.3;
    if k > 0.01 && seam > clip.x && seam < right {
        out.shapes.push(
            paint::rect(bar.scale, Rect::new(seam - 0.75, y + 1.0, 1.5, 15.0), 0.75)
                .gradient(caret.0, caret.1, 0.0, 1.0)
                .alpha(k)
                .done(),
        );
    }
}

pub fn draw(bar: &mut Bar, st: &mut State, fs: &mut FontSystem, out: &mut DrawList) {
    let sc = bar.scale;
    let (acc_a, acc_b) = theme::accent();
    let ent: [f32; 4] = std::array::from_fn(|i| st.enter[i].get());
    let tab = |x: f32, w: f32, i: usize, drop: f32| -> (Rect, f32) {
        let dy = -(1.0 - ent[i]) * (TAB_H + 8.0);
        let h = TAB_H + DROP * drop;
        (Rect::new(x, -RADIUS - 4.0 + dy, w, h + RADIUS + 4.0), TAB_H * 0.5 + DROP * 0.5 * drop + dy)
    };
    let alpha = |i: usize| ent[i].clamp(0.0, 1.0);
    let dr: [f32; 4] = std::array::from_fn(|i| st.drop[i].get().clamp(0.0, 1.3));

    // ── centre: measure first (the media tab must stay clear of it) ─────
    let xf = bar.title.xf.get().clamp(0.0, 1.0);
    let title = bar.title.cur.clone().unwrap_or_else(|| "Desktop".to_string());
    let (tk, cur_w) = bar.text.get_fit(fs, &title, F_TITLE, TITLE_MAX);
    let prev = bar.title.prev.clone().map(|t| bar.text.get_fit(fs, &t, F_TITLE, TITLE_MAX));
    let title_w = match prev {
        Some((_, pw)) if xf < 1.0 => pw + (cur_w - pw) * (1.0 - (1.0 - xf).powi(3)),
        _ => cur_w,
    };
    // App icon: wait for the right one rather than flicker through none.
    match (&bar.app_id, &bar.app_icon) {
        (None, _) => st.icon_k.set(0.0),
        (Some(id), Some((iid, img))) if id == iid => match img {
            Some(img) => {
                let img = img.clone();
                st.icon.want(id, &img);
                st.icon_k.set(1.0);
            }
            None => st.icon_k.set(0.0),
        },
        _ => {}
    }
    if st.icon_k.target == 0.0 && st.icon_k.at_rest() && st.icon.cur.is_some() {
        st.icon.clear();
    }
    st.icon.flush(out);
    let ik = st.icon_k.get().clamp(0.0, 1.0);
    let c_content = (APP_ICON + 8.0) * ik + title_w;
    let c_w = c_content + PAD * 2.0;
    let (c_body, c_cy) = tab((bar.width - c_w) * 0.5, c_w, 2, 0.0);

    // ── spaces ──────────────────────────────────────────────────────────
    let n = bar.workspaces.len().max(1) as f32;
    let ws_target = n * SLOT + PAD * 2.0 - 6.0;
    if st.ws_w.target != ws_target {
        if st.ws_w.get() == 0.0 {
            st.ws_w.snap(ws_target);
        }
        st.ws_w.set(ws_target);
    }
    let (l_body, l_cy) = tab(TAB_MARGIN, st.ws_w.get(), 0, 0.0);

    // ── now playing: lay out ────────────────────────────────────────────
    let mk = st.media_k.get().max(0.0);
    let m_x = l_body.x + l_body.w + TAB_GAP * mk.min(1.0);
    let mut m_layout = None;
    if let Some(mi) = st.shown.clone()
        && mk > 0.002
    {
        // Room up to the centre tab, in coarse steps so text isn't
        // re-shaped every frame while the centre tab breathes.
        let ctl = st.ctl.get().clamp(0.0, 1.2);
        let time_proto = if mi.length > 0.0 {
            let l = mtime(mi.length);
            format!("{} / {}", l.replace(|c: char| c.is_ascii_digit(), "0"), l.replace(|c: char| c.is_ascii_digit(), "0"))
        } else {
            "00:00".to_string()
        };
        let (_, time_w) = bar.text.get(fs, &time_proto, F_TIME);
        let ctl_full = 6.0 + BTN * 3.0 + 6.0 + time_w + 4.0;
        let fixed = M_PAD_L + ART + 9.0 + PAD + ctl_full;
        let avail = ((c_body.x - 14.0 - m_x - fixed) / 20.0).floor() * 20.0;
        let t_max = M_TITLE_MAX.min((avail * 0.6).max(60.0));
        let a_max = M_ARTIST_MAX.min((avail - t_max).max(0.0));
        let (title, artist) = st.track.clone().unwrap_or_default();
        let cur_t = bar.text.get_fit(fs, &title, F_TITLE, t_max);
        let cur_a = (!artist.is_empty() && a_max > 30.0).then(|| bar.text.get_fit(fs, &artist, F_ARTIST, a_max));
        let pxf = st.track_xf.get().clamp(0.0, 1.0);
        let prev = st.prev_track.clone().filter(|_| pxf < 1.0);
        let prev_t = prev.as_ref().map(|p| bar.text.get_fit(fs, &p.0, F_TITLE, t_max));
        let prev_a = prev.as_ref().filter(|p| !p.1.is_empty() && a_max > 30.0).map(|p| bar.text.get_fit(fs, &p.1, F_ARTIST, a_max));
        let ease = 1.0 - (1.0 - pxf).powi(3);
        let lerp_w = |p: Option<(u64, f32)>, c: Option<(u64, f32)>| -> f32 {
            let cw = c.map_or(0.0, |c| c.1);
            match p {
                Some(p) if prev.is_some() => p.1 + (cw - p.1) * ease,
                _ if prev.is_some() && c.is_some() => cw * ease,
                _ => cw,
            }
        };
        let tw = lerp_w(prev_t, Some(cur_t));
        let aw_cur = cur_a.map_or(0.0, |a| a.1);
        let aw_prev = prev_a.map_or(0.0, |a| a.1);
        let aw = if prev.is_some() { aw_prev + (aw_cur - aw_prev) * ease } else { aw_cur };
        let text_w = tw + if aw > 0.5 { 8.0 + aw } else { 0.0 };
        let full = M_PAD_L + ART + 9.0 + text_w + PAD + ctl_full * ctl;
        let w = full * mk;
        m_layout = Some((mi, w, cur_t, cur_a, prev_t, prev_a, tw, text_w, time_w, pxf));
    }

    // ── draw: media first, so it slides out from behind the spaces tab ──
    if let Some((mi, w, cur_t, cur_a, prev_t, prev_a, tw, text_w, time_w, pxf)) = m_layout
        && w > 1.0
    {
        let a = alpha(1).min(alpha(0)) * (w / 12.0).clamp(0.0, 1.0);
        let (mb, cy) = tab(m_x, w, 1, 0.0);
        let ctl = st.ctl.get().clamp(0.0, 1.2);
        let ta = st.tint_a.get();
        let tb = st.tint_b.get();
        let pl = st.playing.get().clamp(0.0, 1.0);
        body(bar, out, mb, a, dr[1]);
        bar.hits.push((mb.inflate(2.0), Hit::Media));
        let clip = Rect::new(mb.x + 1.0, mb.y, (mb.w - 2.0).max(0.0), mb.h);

        // Album art (or a stand-in tile until it arrives).
        let ar = Rect::new(mb.x + M_PAD_L, cy - ART * 0.5, ART, ART);
        let want = mi.art_url.as_ref().and_then(|u| bar.media_art.as_ref().filter(|x| &x.url == u)).cloned();
        if let Some(art) = want {
            st.art.want(&art.url, &art.img);
        } else if mi.art_url.is_none() {
            st.art.clear();
        }
        st.art.flush(out);
        let art_a = a * (0.62 + 0.38 * pl);
        let rv = st.art.t.get().clamp(0.0, 1.0);
        if st.art.cur.is_none()
            && let Some(r) = intersect(ar, clip)
        {
            out.shapes.push(
                paint::rect(sc, r, if (r.w - ar.w).abs() < 0.5 { ART_R } else { 1.0 })
                    .fill(paint::FAINT)
                    .alpha(a)
                    .done(),
            );
            let id = bar.icons.id(&["audio-x-generic-symbolic"]);
            bar.icon_at(out, id, ar.x + 4.0, ar.y + 4.0, 14.0, TEXT.fade(a * 0.85 * ((clip.x + clip.w - ar.x) / ART).clamp(0.0, 1.0)));
        } else if let Some(r) = intersect(ar.inflate(1.0), clip) {
            // Recessed frame behind the cover keeps its edge crisp on black.
            out.shapes.push(paint::rect(sc, r, ART_R + 1.0).fill(Rgba::hex(0x000000, 0.5)).alpha(a).done());
        }
        if let Some((k, _)) = &st.art.prev {
            image(bar, out, *k, ar, ART_R, clip, art_a, 1.0);
        }
        if let Some((k, _)) = &st.art.cur {
            image(bar, out, *k, ar, ART_R, clip, art_a, if st.art.prev.is_some() { rv } else { 1.0 });
        }
        // Paused: the cover dims and shows the pause glyph.
        if pl < 0.99 {
            let id = bar.icons.id(&["media-playback-pause-symbolic"]);
            let cover = ((clip.x + clip.w - ar.x) / ART).clamp(0.0, 1.0);
            bar.icon_at(out, id, ar.x + 4.0, ar.y + 4.0, 14.0, TEXT.fade(a * (1.0 - pl) * cover));
        }

        // Title · artist, wiped in place on a new track.
        let tx = ar.x + ART + 9.0;
        let tclip = Rect::new(tx - 1.0, clip.y, (clip.x + clip.w - tx + 1.0).max(0.0), clip.h);
        let lh = bar.text.line_h(F_TITLE);
        // Text as calm as the other tabs: soft at rest, bright on hover.
        let tcol = SOFT.mix(TEXT, dr[1].min(1.0)).mix(MUTED, 0.35 * (1.0 - pl)).fade(a);
        let caret = (ta, tb);
        let span_t = cur_t.1.max(prev_t.map_or(0.0, |p| p.1));
        // Clip the title to its own (interpolated) width so it never runs
        // under the artist mid-change.
        let t_clip = Rect::new(tclip.x, tclip.y, (tw + 1.5).min(tclip.w), tclip.h);
        wipe(bar, out, prev_t.map(|p| p.0), cur_t.0, tx, cy - lh * 0.5, span_t, pxf, t_clip, tcol, caret);
        if let Some(ca) = cur_a.or(prev_a) {
            let ax = tx + tw + 8.0;
            let alh = bar.text.line_h(F_ARTIST);
            let acol = MUTED.fade(a);
            let axf = ((pxf - 0.18) / 0.82).clamp(0.0, 1.0);
            let span_a = cur_a.map_or(0.0, |c| c.1).max(prev_a.map_or(0.0, |p| p.1));
            let a_clip = Rect::new(ax - 1.0, tclip.y, (tclip.x + tclip.w - ax + 1.0).max(0.0), tclip.h);
            match cur_a {
                Some(c) => wipe(bar, out, prev_a.map(|p| p.0), c.0, ax, cy - alh * 0.5 + 0.5, span_a, axf, a_clip, acol, caret),
                None if axf < 1.0 => {
                    let r = Rect::new(ax + span_a * axf, a_clip.y, (a_clip.w - span_a * axf).max(0.0), a_clip.h);
                    bar.text_at(out, ca.0, ax, cy - alh * 0.5 + 0.5, acol, r);
                }
                None => {}
            }
        }

        // Transport, revealed by the tab widening to the right.
        let bx = tx + text_w + 6.0;
        if ctl > 0.01 {
            let ca = a * (ctl * 1.6 - 0.25).clamp(0.0, 1.0);
            let hov = st.btn_a.get().clamp(0.0, 1.0);
            if hov > 0.01 {
                let hx = bx + st.btn_x.get() * BTN;
                let pressed = matches!(bar.pressed, Hit::MediaCtl(_));
                // A faint circle slides between the bare buttons.
                let tile = Rect::new(hx + 1.0, cy - 12.0, BTN - 2.0, 24.0);
                if let Some(t) = intersect(tile, clip) {
                    out.shapes.push(
                        paint::rect(sc, t, 12.0)
                            .fill(Rgba::hex(0xffffff, if pressed { 0.14 } else { 0.09 }))
                            .alpha(ca * hov)
                            .done(),
                    );
                }
            }
            let ids = [
                bar.icons.id(&["media-skip-backward-symbolic"]),
                bar.icons.id(&["media-playback-pause-symbolic"]),
                bar.icons.id(&["media-skip-forward-symbolic"]),
            ];
            let play = bar.icons.id(&["media-playback-start-symbolic"]);
            let enabled = [mi.can_prev, true, mi.can_next];
            for i in 0..3 {
                let ix = bx + i as f32 * BTN + (BTN - ICON) * 0.5;
                let over = bar.hovered == Hit::MediaCtl(i as u8);
                let base = if enabled[i] { if over { TEXT } else { SOFT } } else { MUTED.fade(0.5) };
                let cover = ((clip.x + clip.w - ix) / ICON).clamp(0.0, 1.0);
                let ia = ca * cover;
                if i == 1 {
                    bar.icon_at(out, ids[1], ix, cy - ICON * 0.5, ICON, base.fade(ia * pl));
                    bar.icon_at(out, play, ix, cy - ICON * 0.5, ICON, base.fade(ia * (1.0 - pl)));
                } else {
                    bar.icon_at(out, ids[i], ix, cy - ICON * 0.5, ICON, base.fade(ia));
                }
                if ctl > 0.5 && enabled[i] {
                    bar.hits.push((Rect::new(bx + i as f32 * BTN, mb.y, BTN, mb.h), Hit::MediaCtl(i as u8)));
                }
            }
            let pos = bar.media_pos();
            let label = if mi.length > 0.0 { format!("{} / {}", mtime(pos), mtime(mi.length)) } else { mtime(pos) };
            let (k, _) = bar.text.get(fs, &label, F_TIME);
            let lx = bx + BTN * 3.0 + 6.0;
            let llh = bar.text.line_h(F_TIME);
            if lx < clip.x + clip.w {
                let _ = time_w;
                bar.text_at(out, k, lx, cy - llh * 0.5, MUTED.fade(ca), clip);
            }
        }

        // Progress lip along the bottom edge.
        let ly = mb.y + mb.h - 3.5 - 0.5 * ctl;
        // It underlines the text and controls, not the cover.
        let lx = tx;
        let lw = (mb.x + mb.w - LINE_INSET - lx).max(0.0);
        st.line_w = lw.max(1.0);
        if mi.length > 0.0 && lw > 2.0 {
            let th = 1.5 + 0.75 * ctl.min(1.0);
            let frac = (bar.media_pos() / mi.length).clamp(0.0, 1.0);
            out.shapes.push(paint::rect(sc, Rect::new(lx, ly - th * 0.5, lw, th), th * 0.5).fill(TRACK).alpha(a).done());
            if frac * lw > 0.5 {
                let lip = paint::rect(sc, Rect::new(lx, ly - th * 0.5, (frac * lw).max(th), th), th * 0.5);
                // Iridescent while playing, greying out when paused.
                let lip = if theme::prism() {
                    lip.on_mix(MUTED.fade(0.7), 0.4 + 0.6 * pl, 1.0, 0.0, 0.0)
                } else {
                    let (fa, fb) = (ta.mix(MUTED, 0.55 * (1.0 - pl)), tb.mix(MUTED, 0.55 * (1.0 - pl)));
                    lip.gradient(fa.fade(0.85), fb, 1.0, 0.0)
                };
                out.shapes.push(lip.alpha(a).done());
            }
        }
    }

    // ── spaces ──────────────────────────────────────────────────────────
    let la = alpha(0);
    body(bar, out, l_body, la, dr[0]);
    bar.hits.push((l_body.inflate(2.0), Hit::Spaces));
    bar.tabs[0] = l_body;
    {
        let row_w = n * SLOT;
        let x0 = l_body.x + (l_body.w - row_w) * 0.5;
        let cy = l_cy;
        let clip = l_body.inflate(-1.5);
        let head = bar.ws_head.get();
        let tail = bar.ws_tail.get();
        let (lo, hi) = (head.min(tail), head.max(tail));
        let ix = |i: f32| x0 + i * SLOT + SLOT * 0.5;
        let (il, ir) = (ix(lo) - 8.0, ix(hi) + 8.0);
        for (i, ws) in bar.workspaces.clone().iter().enumerate() {
            let c = ix(i as f32);
            let dist = (il - c).max(c - ir);
            let free = ((dist + 1.0) / 6.0).clamp(0.0, 1.0);
            let hov = bar.hovered == Hit::Workspace(ws.id);
            // Occupied workspaces stretch a little per window.
            let extra = (ws.windows.saturating_sub(1).min(3)) as f32 * 2.0;
            let d = (if hov { 7.5 } else { 5.0 }) * (0.4 + 0.6 * free);
            let wd = d + if ws.windows > 0 { extra * free } else { 0.0 };
            let a = la * free;
            let col = if ws.urgent { BAD } else if ws.windows > 0 { DOT_USED.mix(TEXT, if hov { 0.6 } else { 0.0 }) } else { DOT_EMPTY };
            if a > 0.002 {
                let r = Rect::new(c - wd * 0.5, cy - d * 0.5, wd, d);
                if let Some(r) = intersect(r, clip) {
                    let dot = paint::rect(sc, r, d * 0.5);
                    let dot = if ws.windows > 0 || ws.urgent { dot.fill(col) } else { dot.rim(1.2, col, col) };
                    out.shapes.push(dot.alpha(a).done());
                }
            }
            bar.hits.push((Rect::new(c - SLOT * 0.5, l_body.y, SLOT, l_body.h), Hit::Workspace(ws.id)));
        }
        let stretch = ((ir - il) - 16.0) / 16.0;
        let ih = 7.0 * (1.0 - 0.3 * stretch.clamp(0.0, 1.2));
        if !bar.workspaces.is_empty()
            && let Some(r) = intersect(Rect::new(il, cy - ih * 0.5, ir - il, ih), clip)
        {
            out.shapes.push(paint::rect(sc, r, ih * 0.5).on_fill(1.0, 0.0, 0.0).alpha(la).done());
        }
    }

    // ── centre: app icon + title (click: quick settings) ────────────────
    let ca = alpha(2) * (1.0 - bar.popup_cover);
    bar.tabs[1] = c_body;
    bar.center_tab = c_body;
    if ca > 0.002 {
        body(bar, out, c_body, ca, dr[2]);
    }
    bar.hits.push((c_body.inflate(2.0), Hit::Title));
    {
        let clip = c_body.inflate(-1.5);
        let x0 = c_body.x + PAD;
        let ir = Rect::new(x0, c_cy - APP_ICON * 0.5, APP_ICON, APP_ICON);
        let t = st.icon.t.get().clamp(0.0, 1.0);
        if let Some((k, _)) = &st.icon.prev {
            image(bar, out, *k, ir, 3.0, clip, ca * ik * (1.0 - t), 1.0);
        }
        if let Some((k, _)) = &st.icon.cur {
            image(bar, out, *k, ir, 3.0, clip, ca * ik * if st.icon.prev.is_some() { t } else { 1.0 }, 1.0);
        }
        let tx = x0 + (APP_ICON + 8.0) * ik;
        let lh = bar.text.line_h(F_TITLE);
        let c = SOFT.mix(TEXT, dr[2].min(1.0)).fade(ca);
        let span = cur_w.max(prev.map_or(0.0, |p| p.1));
        let tclip = Rect::new(tx - 1.0, clip.y, (clip.x + clip.w - tx + 1.0).max(0.0), clip.h);
        wipe(bar, out, prev.map(|p| p.0).filter(|_| xf < 1.0), tk, tx, c_cy - lh * 0.5, span, xf, tclip, c, (acc_a, acc_b));
    }

    // ── status ──────────────────────────────────────────────────────────
    draw_status(bar, st, fs, out, &tab, dr[3], alpha(3));
}

#[allow(clippy::too_many_arguments)]
fn draw_status(bar: &mut Bar, st: &mut State, fs: &mut FontSystem, out: &mut DrawList, tab: &dyn Fn(f32, f32, usize, f32) -> (Rect, f32), drop: f32, a: f32) {
    let sc = bar.scale;
    let (acc_a, _) = theme::accent();
    let vol_txt = bar.volume.map(|v| if v.muted { "Muted".to_string() } else { format!("{}%", v.percent) });
    let vol_t = vol_txt.as_deref().map(|t| bar.text.get(fs, t, F_SMALL));
    let level_w = 8.0 + VOL_TRACK + 8.0 + vol_t.map_or(0.0, |t| t.1);
    let reveal = st.vol.get().clamp(0.0, 1.0);
    let vol_w = ICON + level_w * reveal;
    let bat_t = bar.battery.map(|b| bar.text.get(fs, &format!("{}%", b.percent), F_SMALL));
    let bat_w = bat_t.map_or(0.0, |t| GAP + 22.0 + 5.0 + t.1);
    let date_t = bar.clock.map(|c| {
        const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        bar.text.get(fs, &format!("{} {} {}", WD[c.weekday as usize % 7], c.day, MO[c.month as usize % 12]), F_TITLE)
    });
    let dk = st.date.get().clamp(0.0, 1.0);
    let date_w = date_t.map_or(0.0, |t| (t.1 + 10.0) * dk);
    let digit_w = ('0'..='9').map(|c| bar.text.get(fs, &c.to_string(), F_CLOCK).1).fold(0.0, f32::max);
    let colon = bar.text.get(fs, ":", F_CLOCK);
    let clock_w = digit_w * 4.0 + colon.1 + 2.0;
    let sep = GAP + 1.0 + GAP;
    // Tray: textures follow the item list (uploaded once per icon change).
    let tray = bar.tray.clone();
    for t in tray.iter() {
        let (Some(c), Some(g)) = (&t.icon, &t.grey) else { continue };
        if st.tray_tex.get(&t.id).is_some_and(|e| e.2 == t.rev) {
            continue;
        }
        if let Some((a, b, _)) = st.tray_tex.remove(&t.id) {
            st.tray_free.extend([a, b]);
        }
        let (kc, kg) = (NEXT_KEY.fetch_add(1, Ordering::Relaxed), NEXT_KEY.fetch_add(1, Ordering::Relaxed));
        out.uploads.push((kc, c.clone()));
        out.uploads.push((kg, g.clone()));
        st.tray_tex.insert(t.id.clone(), (kc, kg, t.rev));
    }
    let gone: Vec<String> = st.tray_tex.keys().filter(|k| !tray.iter().any(|t| &t.id == *k)).cloned().collect();
    for k in gone {
        if let Some((a, b, _)) = st.tray_tex.remove(&k) {
            st.tray_free.extend([a, b]);
        }
    }
    out.frees.append(&mut st.tray_free);
    let shown: Vec<(usize, u64, u64, bool)> =
        tray.iter().enumerate().filter_map(|(i, t)| st.tray_tex.get(&t.id).map(|e| (i, e.0, e.1, t.attention))).collect();
    let tray_w = if shown.is_empty() { 0.0 } else { shown.len() as f32 * TRAY + (shown.len() as f32 - 1.0) * TRAY_GAP + GAP };
    let rk = st.rec.get().clamp(0.0, 1.0);
    let rec_w = (REC + GAP) * rk;
    let content = rec_w + tray_w + vol_w + GAP + ICON + bat_w + sep + date_w + clock_w;
    let w = content + PAD * 2.0;
    let (rb, cy) = tab(bar.width - TAB_MARGIN - w, w, 3, 0.0);
    body(bar, out, rb, a, drop);
    bar.tabs[2] = rb;
    bar.hits.push((rb.inflate(2.0), Hit::Status));
    let clip = rb.inflate(-1.5);
    let mut x = rb.x + PAD;

    // Recording dot (click: stop).
    if rk > 0.002 {
        let pulse = st.rec_pulse.get().clamp(0.0, 1.0);
        let over = bar.hovered == Hit::Record;
        let r = REC * 0.5 * (0.6 + 0.4 * rk) * if over { 1.15 } else { 1.0 };
        let c = x + REC * 0.5;
        out.shapes.push(
            paint::circle(sc, c, cy, r)
                .fill(REC_RED)
                .alpha(a * rk * (0.45 + 0.55 * pulse))
                .done(),
        );
        if rk > 0.5 {
            bar.hits.push((Rect::new(x - 4.0, rb.y, REC + 8.0, rb.h), Hit::Record));
        }
        x += rec_w;
    }

    // Tray icons: calm grey at rest, their own colours under the pointer.
    for (j, (i, kc, kg, attention)) in shown.iter().enumerate() {
        let ir = Rect::new(x, cy - TRAY * 0.5, TRAY, TRAY);
        let id = &tray[*i].id;
        let hv = st.tray_hover.get(id).map_or(0.0, |s| s.get().clamp(0.0, 1.0));
        let warm = if *attention { 1.0 } else { hv };
        image(bar, out, *kg, ir, 3.0, clip, a * (1.0 - warm), 1.0);
        image(bar, out, *kc, ir, 3.0, clip, a * warm, 1.0);
        bar.hits.push((Rect::new(x - TRAY_GAP * 0.5, rb.y, TRAY + TRAY_GAP, rb.h), Hit::Tray(*i as u8)));
        x += TRAY + if j + 1 < shown.len() { TRAY_GAP } else { GAP };
    }

    // Volume + peeking level.
    let vx = x;
    bar.icon_at(out, bar.vol_icon.cur, x, cy - ICON * 0.5, ICON, TEXT.fade(a));
    if reveal > 0.002 {
        let tx = x + ICON + 8.0;
        let level = st.level.get().clamp(0.0, 1.05);
        let vclip = Rect::new(x + ICON, clip.y, level_w * reveal, clip.h);
        let ra = a * reveal;
        if let Some(r) = intersect(Rect::new(tx, cy - 2.0, VOL_TRACK, 4.0), vclip) {
            out.shapes.push(paint::rect(sc, r, 2.0).fill(Rgba::hex(0xffffff, 0.1)).alpha(ra).done());
        }
        if let Some(r) = intersect(Rect::new(tx, cy - 2.0, VOL_TRACK * level, 4.0), vclip) {
            out.shapes.push(paint::rect(sc, r, 2.0).on_fill(1.0, 0.0, 0.0).alpha(ra).done());
        }
        if let Some((k, _)) = vol_t {
            let lh = bar.text.line_h(F_SMALL);
            bar.text_at(out, k, tx + VOL_TRACK + 8.0, cy - lh * 0.5, TEXT.fade(ra), vclip);
        }
    }
    bar.hits.push((Rect::new(vx - 4.0, rb.y, vol_w + 8.0, rb.h), Hit::Volume));
    x += vol_w + GAP;

    // Network.
    let net_c = if matches!(bar.net, NetInfo::Offline) { MUTED } else { TEXT };
    bar.icon_at(out, bar.net_icon.cur, x, cy - ICON * 0.5, ICON, net_c.fade(a));
    x += ICON;

    // Battery: a drawn cell that fills, mint while charging.
    if let (Some(b), Some((k, _))) = (bar.battery, bat_t) {
        x += GAP;
        let lv = st.bat.get().clamp(0.0, 1.0);
        let col = if b.charging {
            GOOD
        } else if b.percent <= 15 {
            BAD
        } else if b.percent <= 30 {
            WARN
        } else {
            TEXT
        };
        let cell = Rect::new(x, cy - 5.5, 19.0, 11.0);
        out.shapes.push(paint::rect(sc, cell, 3.2).rim(1.2, Rgba::hex(0xffffff, 0.5), Rgba::hex(0xffffff, 0.5)).alpha(a).done());
        out.shapes.push(paint::rect(sc, Rect::new(x + 19.8, cy - 2.0, 1.8, 4.0), 0.9).fill(Rgba::hex(0xffffff, 0.45)).alpha(a).done());
        let inner = Rect::new(x + 2.4, cy - 3.1, (19.0 - 4.8) * lv.max(0.06), 6.2);
        let fc = if b.charging { GOOD } else { col.fade(0.9) };
        out.shapes.push(paint::rect(sc, inner, 1.6).fill(fc).alpha(a).done());
        let lh = bar.text.line_h(F_SMALL);
        let tc = if b.charging || b.percent <= 30 { col } else { SOFT };
        bar.text_at(out, k, x + 22.0 + 5.0, cy - lh * 0.5, tc.fade(a), clip);
        x += 22.0 + 5.0 + bat_t.map_or(0.0, |t| t.1);
    }
    bar.hits.push((Rect::new(vx + vol_w + 4.0, rb.y, x - vx - vol_w, rb.h), Hit::Status));

    // Hairline.
    x += GAP;
    out.shapes.push(
        paint::rect(sc, Rect::new(x, cy - 7.0, 1.0, 14.0), 0.5)
            .fill(Rgba::hex(0xffffff, 0.12))
            .alpha(a)
            .done(),
    );
    x += 1.0 + GAP;

    // Date slides out of the clock on hover.
    let clock_hit = x;
    if let Some((k, tw)) = date_t
        && dk > 0.002
    {
        let lh = bar.text.line_h(F_TITLE);
        let dclip = Rect::new(x, clip.y, date_w, clip.h);
        bar.text_at(out, k, x + date_w - tw - 10.0, cy - lh * 0.5, MUTED.fade(a * dk.min(1.0)), dclip);
    }
    x += date_w;

    // Clock: changed digits cross-fade in place.
    let lh = bar.text.line_h(F_CLOCK);
    let ty = cy - lh * 0.5;
    for i in 0..4 {
        if i == 2 {
            bar.text_at(out, colon.0, x + 1.0, ty - 1.0, acc_a.mix(SOFT, 0.35).fade(a), clip);
            x += colon.1 + 2.0;
        }
        let (cur, prv, p) = (bar.digits[i].cur, bar.digits[i].prev, bar.digits[i].p.get().clamp(0.0, 1.0));
        let ck = bar.text.get(fs, &cur.to_string(), F_CLOCK);
        if p < 1.0 && prv != ' ' && prv != cur {
            let pk = bar.text.get(fs, &prv.to_string(), F_CLOCK);
            bar.text_at(out, pk.0, x + (digit_w - pk.1) * 0.5, ty, TEXT.fade(a * (1.0 - p)), clip);
            bar.text_at(out, ck.0, x + (digit_w - ck.1) * 0.5, ty, TEXT.fade(a * p), clip);
        } else {
            bar.text_at(out, ck.0, x + (digit_w - ck.1) * 0.5, ty, TEXT.fade(a), clip);
        }
        x += digit_w;
    }
    bar.hits.push((Rect::new(clock_hit - 6.0, rb.y, x - clock_hit + PAD + 6.0, rb.h), Hit::Clock));
}
