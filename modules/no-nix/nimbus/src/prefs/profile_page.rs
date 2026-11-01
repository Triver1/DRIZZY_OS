//! Settings' Profile page: who you are, what you're sitting at.
//!
//! Three views cross-fade in place: the card (picture, name, a friendly
//! line, then the machine in two calm columns), the picker (recent pictures
//! under ~/Pictures, or a typed path) and the crop (a circle over the
//! picture; drag, scroll, save). Pixels and file work live in
//! `crate::profile`; this is what the page draws and does with its input.
//!
//! The picture gets the same 1.6 px ring as the lock and the login screen,
//! so it follows the accent; the ring's light leans toward the pointer and
//! turns once when a new picture lands. Nothing moves at rest. Textures
//! exist while they're shown: thumbnails go when the picker is left, the
//! crop's picture when the crop is, and all of it when the window closes.

use std::path::PathBuf;

use calloop::channel::Sender;
use glyphon::FontSystem;

use super::{DANGER_FG, F_BODY, F_LABEL, F_PAGE, F_SECTION, F_SMALL, F_SUB, HOVER, MUTED, Page, Prefs, SOFT, TEXT, TILE, TRACK};
use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect},
    gfx::{DrawList, ImageData, ImageDraw},
    paint,
    profile::{
        self, Body, Msg, Workers, avatar,
        crop::Crop,
        field::{Edit, Line},
        info::{self, Info},
    },
    services::Fact,
    theme::{self, Rgba},
    wallpaper::loader::{self, Job, Pool, Thumb},
    window::{Cursor, KeyInput, keys},
};

use super::Ctl;

const F_NAME: Font = Font::new(26.0, 650).family("Geist");
const F_INITIAL: Font = Font::new(48.0, 600).family("Geist");

/// Card: the picture's radius, and the ring's width (the lock's).
const AV_R: f32 = 54.0;
const RING_W: f32 = 1.6;
/// Where the ring's light sits at rest (the lock's angle).
const RING_REST: f32 = -0.52;
/// Picker: columns and gap.
const COLS: usize = 5;
const GAP: f32 = 10.0;
/// Crop: the stage's side and the circle's diameter inside it.
const STAGE: f32 = 380.0;
const CIRCLE: f32 = 332.0;
/// The "Remove picture" confirmation (shares `Prefs::armed` with the
/// power buttons, whose ids are 2 and 3).
const ARM_REMOVE: u8 = 9;

const RING_TURN: SpringCfg = SpringCfg::new(1.1, 1.0);
const VIEW: SpringCfg = SpringCfg::new(0.34, 1.0);
const LINE: SpringCfg = SpringCfg::new(0.3, 1.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Card,
    Picker,
    Crop,
}

struct Item {
    path: PathBuf,
    queued: bool,
    ready: bool,
    broken: bool,
    shown: Spring,
}

struct Picker {
    owner: u64,
    pool: Pool,
    /// None until the scan is back.
    items: Option<Vec<Item>>,
    scroll: Spring,
    /// Largest useful scroll, from the last draw.
    max_scroll: f32,
    thumb_px: u32,
}

struct CropView {
    path: PathBuf,
    /// The thumbnail it came from, shown until the picture itself arrives.
    thumb: Option<u64>,
    /// None while the picture decodes.
    crop: Option<Crop>,
    saving: bool,
}

pub struct State {
    workers: Option<Workers>,
    freed: bool,
    base: u64,
    info: Option<Box<Info>>,
    /// The name set in Settings, else GECOS or the login.
    name: String,
    fallback: String,
    login: String,
    /// The friendly lines, the one showing, the one leaving.
    lines: Vec<String>,
    line: usize,
    prev_line: Option<usize>,
    line_t: Spring,
    has_avatar: bool,
    /// There is a ~/.face or ~/.face.icon.
    ours: bool,
    uploads: Vec<(u64, ImageData)>,
    frees: Vec<u64>,

    view: View,
    prev_view: View,
    view_t: Spring,
    name_edit: Option<Line>,
    path: Line,
    error: Option<String>,
    picker: Option<Picker>,
    crop: Option<CropView>,
    has_preview: bool,

    ptr: (f32, f32),
    last_drag: (f32, f32),
    /// Picture centre on the card, for the ring's lean.
    centre: (f32, f32),
    ring_x: Spring,
    ring_y: Spring,
    ring_turn: Spring,
    zoom_rail: Rect,
    stage_centre: (f32, f32),
}

impl State {
    pub fn new() -> Self {
        let id = loader::next_id();
        Self {
            workers: None,
            freed: false,
            base: 0x5052_0000_0000_0000 | (id << 24),
            info: None,
            name: String::new(),
            fallback: String::new(),
            login: String::new(),
            lines: vec![],
            line: 0,
            prev_line: None,
            line_t: Spring::new(1.0, LINE),
            has_avatar: false,
            ours: false,
            uploads: vec![],
            frees: vec![],
            view: View::Card,
            prev_view: View::Card,
            view_t: Spring::new(1.0, VIEW),
            name_edit: None,
            path: Line::new("", 4096),
            error: None,
            picker: None,
            crop: None,
            has_preview: false,
            ptr: (0.0, 0.0),
            last_drag: (0.0, 0.0),
            centre: (0.0, 0.0),
            ring_x: Spring::new(RING_REST.cos(), presets::SMOOTH),
            ring_y: Spring::new(RING_REST.sin(), presets::SMOOTH),
            ring_turn: Spring::new(0.0, RING_TURN),
            zoom_rail: Rect::default(),
            stage_centre: (0.0, 0.0),
        }
    }

    fn key_avatar(&self) -> u64 {
        self.base
    }

    fn key_preview(&self) -> u64 {
        self.base | 1
    }

    fn key_thumb(&self, i: usize) -> u64 {
        self.base | (i as u64 + 2)
    }

    fn set_view(&mut self, v: View) {
        if v != self.view {
            self.prev_view = self.view;
            self.view = v;
            self.view_t.snap(0.0);
            self.view_t.set(1.0);
        }
    }

    fn drop_preview(&mut self) {
        if self.has_preview {
            let k = self.key_preview();
            self.uploads.retain(|(u, _)| *u != k);
            self.frees.push(k);
            self.has_preview = false;
        }
    }

    fn drop_picker(&mut self) {
        let Some(p) = self.picker.take() else { return };
        let keys: Vec<u64> = p.items.iter().flatten().enumerate().filter(|(_, it)| it.ready).map(|(i, _)| self.key_thumb(i)).collect();
        self.uploads.retain(|(u, _)| !keys.contains(u));
        self.frees.extend(keys);
    }

    fn set_avatar(&mut self, img: Option<ImageData>, ours: bool) {
        let k = self.key_avatar();
        self.ours = ours;
        self.uploads.retain(|(u, _)| *u != k);
        match img {
            Some(i) => {
                self.uploads.push((k, i));
                self.has_avatar = true;
            }
            None => {
                if self.has_avatar {
                    self.frees.push(k);
                }
                self.has_avatar = false;
            }
        }
    }

    /// Stop for good: every texture key still held.
    fn take_textures(&mut self) -> Vec<u64> {
        self.freed = true;
        self.drop_preview();
        self.drop_picker();
        if self.has_avatar {
            self.frees.push(self.key_avatar());
            self.has_avatar = false;
        }
        self.uploads.clear();
        std::mem::take(&mut self.frees)
    }

    fn refresh_lines(&mut self) {
        let Some(i) = &self.info else { return };
        let keep = self.line;
        self.lines = i.lines(&self.name);
        self.line = keep.min(self.lines.len().saturating_sub(1));
    }

    fn initial(&self) -> String {
        self.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
    }

    /// Everything the machine list shows, in reading order.
    fn rows(&self, accent: usize) -> Vec<(&'static str, String)> {
        let version = match option_env!("NIMBUS_COMMIT") {
            Some(c) => format!("{} · {c}", env!("CARGO_PKG_VERSION")),
            None => env!("CARGO_PKG_VERSION").to_string(),
        };
        let accent = theme::ACCENTS.get(accent).map_or("", |a| a.0).to_string();
        let Some(i) = &self.info else { return vec![] };
        let i = i.as_ref();
        [
            ("Device", i.device.clone()),
            ("System", i.os.clone()),
            ("Kernel", i.kernel.clone()),
            ("Processor", i.cpu.clone()),
            ("Graphics", i.gpu.clone()),
            ("Memory", i.memory.clone()),
            ("Disk", i.disk.clone()),
            ("Uptime", i.uptime.map(info::fmt_uptime)),
            ("nimbus", Some(version)),
            ("On this install since", i.since.clone()),
            ("Wallpaper", i.wallpaper.clone()),
            ("Accent", Some(accent)),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some((k, v?)))
        .collect()
    }
}

/// A window that goes without saying goodbye still leaves nothing behind.
impl Drop for State {
    fn drop(&mut self) {
        crate::gfx::free_later(self.take_textures());
    }
}

impl Prefs {
    /// Start the Profile page: who's here, their picture, the machine.
    pub fn attach(&mut self, tx: Sender<Fact>) {
        let u = crate::lock::who();
        self.attach_at(tx, PathBuf::from(u.home), u.login, u.name);
        if let Some(w) = &self.prof.workers {
            w.gather(self.gpu_name.clone());
        }
    }

    /// `attach` for someone in particular (the demo's made-up home).
    pub fn attach_at(&mut self, tx: Sender<Fact>, home: PathBuf, login: String, fallback: String) {
        let w = Workers { tx, owner: self.prof.base, home, login: login.clone() };
        w.read_avatar();
        let p = &mut self.prof;
        p.workers = Some(w);
        p.name = profile::display_name().unwrap_or_else(|| fallback.clone());
        p.fallback = fallback;
        p.login = login;
    }

    /// A worker's or a thumbnail's result. True when it changed something.
    pub fn on_custom(&mut self, f: &Fact) -> bool {
        let Fact::Custom(any) = f else { return false };
        if self.prof.freed {
            return false;
        }
        if let Some(t) = any.downcast_ref::<Thumb>() {
            return self.on_thumb(t);
        }
        let Some(m) = any.downcast_ref::<Msg>() else { return false };
        if m.owner != self.prof.base {
            return false;
        }
        let Some(body) = m.body.lock().ok().and_then(|mut b| b.take()) else { return false };
        let p = &mut self.prof;
        match body {
            Body::Info(i) => {
                p.info = Some(i);
                p.refresh_lines();
            }
            Body::Avatar { img, ours } => p.set_avatar(img, ours),
            Body::Scan(paths) => {
                if let Some(pk) = &mut p.picker {
                    let item = |path| Item { path, queued: false, ready: false, broken: false, shown: Spring::new(0.0, presets::SMOOTH) };
                    pk.items = Some(paths.into_iter().map(item).collect());
                }
            }
            Body::Picked { path, result } => {
                // Too late: the crop it was for has gone.
                if p.crop.as_ref().is_none_or(|c| c.path != path) {
                    return false;
                }
                match result {
                    Ok(pv) => {
                        let k = p.key_preview();
                        if let Some(c) = &mut p.crop {
                            c.crop = Some(Crop::new(pv.size.0, pv.size.1));
                        }
                        p.uploads.retain(|(u, _)| *u != k);
                        p.uploads.push((k, pv.img));
                        p.has_preview = true;
                    }
                    Err(e) => {
                        p.crop = None;
                        p.drop_preview();
                        self.open_picker();
                        self.prof.error = Some(e);
                    }
                }
            }
            Body::Saved(Ok((img, ours))) => {
                p.set_avatar(img, ours);
                p.error = None;
                p.ring_turn.set(std::f32::consts::TAU);
                p.path.clear();
                p.set_view(View::Card);
                if !p.lines.is_empty() {
                    let first = p.name.split_whitespace().next().unwrap_or("").to_string();
                    p.lines.insert(0, format!("Looking good, {first}."));
                    p.line = 0;
                }
            }
            Body::Saved(Err(e)) => {
                if let Some(c) = &mut p.crop {
                    c.saving = false;
                }
                p.error = Some(e);
            }
            Body::Removed(r) => match r {
                Ok((img, ours)) => {
                    p.set_avatar(img, ours);
                    p.error = None;
                }
                Err(e) => p.error = Some(e),
            },
            Body::Pasted(s) => {
                let home = p.workers.as_ref().map(|w| w.home.clone()).unwrap_or_default();
                if let Some(l) = &mut p.name_edit {
                    l.insert(&s);
                } else if p.view == View::Picker {
                    // A path that came whole from the clipboard needs no
                    // editing, but the field shows what was understood.
                    if let Some(path) = avatar::parse_path(&s, &home) {
                        p.path.insert(&path.to_string_lossy());
                    }
                }
            }
        }
        true
    }

    fn on_thumb(&mut self, t: &Thumb) -> bool {
        let p = &mut self.prof;
        let base = p.base;
        let Some(pk) = &mut p.picker else { return false };
        if t.owner != pk.owner {
            return false;
        }
        let img = t.img.lock().ok().and_then(|mut g| g.take());
        let Some(it) = pk.items.as_mut().and_then(|v| v.get_mut(t.idx)) else { return false };
        it.queued = false;
        match img {
            Some(i) => {
                p.uploads.push((base | (t.idx as u64 + 2), i));
                it.ready = true;
                it.shown.snap(if t.cached { 1.0 } else { 0.0 });
                it.shown.set(1.0);
            }
            None => it.broken = true,
        }
        true
    }

    /// Texture work for this frame. Runs before anything can skip the draw.
    pub(super) fn profile_sync(&mut self, out: &mut DrawList) {
        out.frees.append(&mut self.prof.frees);
        out.uploads.append(&mut self.prof.uploads);
    }

    /// The surface is going: hand over every texture, stop every queue.
    pub fn release_textures(&mut self, frees: &mut Vec<u64>) {
        frees.append(&mut self.prof.take_textures());
    }

    /// The machine, without reading it (the demo's made-up one).
    pub fn set_info(&mut self, i: Info) {
        self.prof.info = Some(Box::new(i));
        self.prof.refresh_lines();
    }

    pub fn profile_thumbs(&self) -> usize {
        self.prof.picker.as_ref().and_then(|k| k.items.as_ref()).map_or(0, |v| v.iter().filter(|i| i.ready).count())
    }

    pub fn profile_error(&self) -> Option<&str> {
        self.prof.error.as_deref()
    }

    /// The crop view has its picture.
    pub fn profile_cropping(&self) -> bool {
        self.prof.view == View::Crop && self.prof.has_preview
    }

    pub fn profile_has_avatar(&self) -> bool {
        self.prof.has_avatar
    }

    /// Textures the page holds right now (demo and tests).
    pub fn profile_textures(&self) -> usize {
        self.prof.has_avatar as usize + self.prof.has_preview as usize + self.profile_thumbs()
    }

    pub(super) fn profile_step(&mut self, dt: f32) -> bool {
        let lean = (self.hovered == Ctl::Avatar).then_some(());
        let p = &mut self.prof;
        // The ring's light leans toward the pointer over the picture.
        let (dx, dy) = (p.ptr.0 - p.centre.0, p.ptr.1 - p.centre.1);
        let len = (dx * dx + dy * dy).sqrt();
        let (tx, ty) = if lean.is_some() && len > 1.0 { (dx / len, dy / len) } else { (RING_REST.cos(), RING_REST.sin()) };
        p.ring_x.set(tx);
        p.ring_y.set(ty);
        let mut m = false;
        for s in [&mut p.view_t, &mut p.line_t, &mut p.ring_x, &mut p.ring_y, &mut p.ring_turn] {
            m |= s.step(dt);
        }
        if p.ring_turn.at_rest() && p.ring_turn.target != 0.0 {
            p.ring_turn.snap(0.0);
        }
        if let Some(pk) = &mut p.picker {
            m |= pk.scroll.step(dt);
            for it in pk.items.iter_mut().flatten() {
                m |= it.shown.step(dt);
            }
        }
        // Leave no pixels behind once a view has faded out.
        if p.view_t.at_rest() {
            if p.view != View::Crop && p.crop.take().is_some() {
                p.drop_preview();
            }
            if p.view == View::Card {
                p.drop_picker();
            }
        }
        if p.line_t.at_rest() {
            p.prev_line = None;
        }
        m
    }

    // ── actions ─────────────────────────────────────────────────────────

    fn open_picker(&mut self) {
        let thumb_px = ((self.prof_tile()) * self.scale).round() as u32;
        let p = &mut self.prof;
        if p.picker.is_none()
            && let Some(w) = &p.workers
        {
            let owner = loader::next_id();
            p.picker = Some(Picker {
                owner,
                pool: Pool::new(w.tx.clone(), owner),
                items: None,
                scroll: Spring::new(0.0, presets::SMOOTH),
                max_scroll: 0.0,
                thumb_px,
            });
            w.scan();
        }
        p.set_view(View::Picker);
    }

    fn prof_tile(&self) -> f32 {
        ((super::FW - super::SIDEBAR - 8.0 - 56.0) - GAP * (COLS as f32 - 1.0)) / COLS as f32
    }

    /// Open `path` for cropping.
    fn pick(&mut self, path: PathBuf, thumb: Option<u64>) {
        let p = &mut self.prof;
        let Some(w) = &p.workers else { return };
        w.pick(path.clone());
        p.drop_preview();
        p.crop = Some(CropView { path, thumb, crop: None, saving: false });
        p.error = None;
        p.set_view(View::Crop);
    }

    fn try_typed_path(&mut self) {
        let home = self.prof.workers.as_ref().map(|w| w.home.clone()).unwrap_or_default();
        match avatar::parse_path(&self.prof.path.text, &home) {
            Some(path) => self.pick(path, None),
            None => self.prof.error = Some("Type or paste a path first.".into()),
        }
    }

    fn save_crop(&mut self) {
        let p = &mut self.prof;
        let Some(c) = &mut p.crop else { return };
        let Some(crop) = c.crop else { return };
        if c.saving {
            return;
        }
        if let Some(w) = &p.workers {
            c.saving = true;
            p.error = None;
            w.save(c.path.clone(), crop);
        }
    }

    fn back(&mut self) {
        // The file is being written: let it land.
        if self.prof.crop.as_ref().is_some_and(|c| c.saving) {
            return;
        }
        match self.prof.view {
            View::Crop if self.prof.picker.is_some() => self.prof.set_view(View::Picker),
            _ => self.prof.set_view(View::Card),
        }
        self.prof.error = None;
    }

    fn start_name_edit(&mut self) {
        // A click inside the field while typing keeps what's typed.
        if self.prof.name_edit.is_some() {
            return;
        }
        let shown = self.prof.name.clone();
        self.prof.name_edit = Some(Line::new(&shown, profile::NAME_MAX));
        self.prof.error = None;
    }

    /// Keep what was typed. Clearing it goes back to the account's name.
    pub(super) fn commit_name(&mut self) {
        let p = &mut self.prof;
        let Some(l) = p.name_edit.take() else { return };
        let n = profile::clean_name(&l.text);
        if n == p.name {
            return;
        }
        profile::save_display_name(&n);
        p.name = if n.is_empty() { p.fallback.clone() } else { n };
        p.refresh_lines();
    }

    /// Leaving the page puts it back on the card.
    pub(super) fn profile_left(&mut self) {
        self.commit_name();
        self.prof.error = None;
        self.prof.set_view(View::Card);
        self.prof.view_t.snap(1.0);
    }

    pub(super) fn profile_click(&mut self, c: Ctl) {
        match c {
            Ctl::Avatar | Ctl::ChangePic => self.open_picker(),
            Ctl::NameField => self.start_name_edit(),
            Ctl::RemovePic => {
                if self.armed.is_some_and(|(a, _)| a == ARM_REMOVE) {
                    self.armed = None;
                    if let Some(w) = &self.prof.workers {
                        w.remove();
                    }
                } else {
                    self.armed = Some((ARM_REMOVE, self.time));
                }
            }
            Ctl::Back | Ctl::CropCancel => self.back(),
            Ctl::CropSave => self.save_crop(),
            Ctl::Thumb(i) => {
                let i = i as usize;
                let key = self.prof.key_thumb(i);
                let found = self.prof.picker.as_ref().and_then(|p| p.items.as_ref()).and_then(|v| v.get(i)).map(|it| (it.path.clone(), it.ready, it.broken));
                match found {
                    Some((_, _, true)) => self.prof.error = Some("That one doesn't open as a picture.".into()),
                    Some((path, ready, _)) => self.pick(path, ready.then_some(key)),
                    None => {}
                }
            }
            Ctl::Friendly => {
                let p = &mut self.prof;
                if p.lines.len() > 1 {
                    p.prev_line = Some(p.line);
                    p.line = (p.line + 1) % p.lines.len();
                    p.line_t.snap(0.0);
                    p.line_t.set(1.0);
                }
            }
            _ => {}
        }
    }

    pub(super) fn profile_press(&mut self, x: f32, y: f32) -> bool {
        if self.prof.name_edit.is_some() && self.hovered != Ctl::NameField {
            self.commit_name();
        }
        match self.hovered {
            Ctl::Stage => {
                self.dragging = Ctl::Stage;
                self.prof.last_drag = (x, y);
                true
            }
            Ctl::ZoomBar => {
                self.dragging = Ctl::ZoomBar;
                self.zoom_to(x);
                true
            }
            _ => false,
        }
    }

    fn zoom_to(&mut self, x: f32) {
        let r = self.prof.zoom_rail;
        let t = ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0);
        if let Some(c) = self.prof.crop.as_mut().and_then(|c| c.crop.as_mut()) {
            c.set_zoom_t(t);
        }
    }

    /// A drag of ours in progress: true when it took the motion.
    pub(super) fn profile_motion(&mut self, x: f32, y: f32) -> bool {
        self.prof.ptr = (x, y);
        match self.dragging {
            Ctl::Stage => {
                let (lx, ly) = std::mem::replace(&mut self.prof.last_drag, (x, y));
                if let Some(c) = self.prof.crop.as_mut().and_then(|c| c.crop.as_mut()) {
                    c.pan(x - lx, y - ly, CIRCLE);
                }
                true
            }
            Ctl::ZoomBar => {
                self.zoom_to(x);
                true
            }
            _ => false,
        }
    }

    /// The wheel on this page: true when it was used.
    pub(super) fn profile_scroll(&mut self, dy: f32) -> bool {
        if self.page != Page::Profile {
            return false;
        }
        let p = &mut self.prof;
        match p.view {
            View::Picker => {
                if let Some(k) = &mut p.picker {
                    let t = (k.scroll.target + dy * 64.0).clamp(0.0, k.max_scroll);
                    k.scroll.set(t);
                }
                true
            }
            View::Crop => {
                let (ax, ay) = (p.ptr.0 - p.stage_centre.0, p.ptr.1 - p.stage_centre.1);
                if let Some(c) = p.crop.as_mut().and_then(|c| c.crop.as_mut()) {
                    c.zoom_at(c.zoom * 1.12f32.powf(-dy), ax, ay, CIRCLE);
                }
                true
            }
            View::Card => false,
        }
    }

    /// A key on this page: true when it was used.
    pub(super) fn profile_key(&mut self, k: &KeyInput) -> bool {
        if self.page != Page::Profile {
            return false;
        }
        if let Some(l) = &mut self.prof.name_edit {
            match l.key(k) {
                Edit::Submit => self.commit_name(),
                Edit::Cancel => self.prof.name_edit = None,
                Edit::Paste => {
                    if let Some(w) = &self.prof.workers {
                        w.paste();
                    }
                }
                Edit::Changed | Edit::Ignored => {}
            }
            return true;
        }
        match self.prof.view {
            View::Card => false,
            View::Picker => {
                match self.prof.path.key(k) {
                    Edit::Submit => self.try_typed_path(),
                    Edit::Cancel => self.back(),
                    Edit::Paste => {
                        if let Some(w) = &self.prof.workers {
                            w.paste();
                        }
                    }
                    Edit::Changed => self.prof.error = None,
                    Edit::Ignored => {}
                }
                true
            }
            View::Crop => {
                match k.sym {
                    keys::ESCAPE => self.back(),
                    keys::RETURN | keys::KP_ENTER => self.save_crop(),
                    _ => {}
                }
                true
            }
        }
    }

    /// Tiles scroll under a pointer that isn't moving: what's under it is
    /// what a click must get. After the frame's hits are in.
    pub(super) fn profile_rehover(&mut self) {
        let scrolling = self.prof.picker.as_ref().is_some_and(|k| !k.scroll.at_rest());
        if scrolling && self.prof.view == View::Picker && self.dragging == Ctl::None {
            self.hovered = self.hit(self.prof.ptr.0, self.prof.ptr.1);
        }
    }

    /// Cursor shapes the generic rules don't cover.
    pub fn cursor_hint(&self) -> Option<Cursor> {
        match (self.dragging, self.hovered) {
            (Ctl::Stage, _) => Some(Cursor::Grabbing),
            (Ctl::None, Ctl::Stage) if self.prof.crop.as_ref().is_some_and(|c| c.crop.is_some()) => Some(Cursor::Grab),
            (Ctl::None, Ctl::NameField | Ctl::PathField) => Some(Cursor::Text),
            _ => None,
        }
    }

    // ── drawing ─────────────────────────────────────────────────────────

    /// The ring under a picture, as on the lock screen: a disk of the "on"
    /// fill, the picture drawn over all but its outer `RING_W`.
    fn ring(&self, out: &mut DrawList, cx: f32, cy: f32, r: f32, a: f32) {
        let p = &self.prof;
        let ang = p.ring_y.get().atan2(p.ring_x.get()) + p.ring_turn.get();
        out.shapes.push(paint::circle(self.scale, cx, cy, r + RING_W).on_fill(ang.cos(), ang.sin(), 0.08).shadow(Rgba::hex(0x000000, 0.45), 18.0, 8.0).alpha(a).done());
        // The dark gap between ring and picture.
        out.shapes.push(paint::circle(self.scale, cx, cy, r).fill(Rgba::hex(0x09090b, 1.0)).alpha(a).done());
    }

    fn round_image(&self, out: &mut DrawList, key: u64, cx: f32, cy: f32, r: f32, a: f32, bright: f32, uv: [f32; 4]) {
        let s = self.scale;
        let rr = r - 0.5;
        let mut img = ImageDraw::new(key, [(cx - rr) * s, (cy - rr) * s, rr * 2.0 * s, rr * 2.0 * s]);
        img.radius = rr * s;
        img.alpha = a;
        img.bright = bright;
        img.uv = uv;
        out.images.push(img);
    }

    fn centred(&mut self, fs: &mut FontSystem, out: &mut DrawList, s: &str, f: Font, cx: f32, cy: f32, c: Rgba, clip: Rect) {
        let (k, w) = self.text.get(fs, s, f);
        let lh = self.text.line_h(f);
        self.text(out, k, cx - w * 0.5, cy - lh * 0.5, c, clip);
    }

    /// The page: its views cross-fade in place, like the pages themselves.
    pub(super) fn page_profile(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Rect, a: f32, clip: Rect) {
        let t = self.prof.view_t.get().clamp(0.0, 1.0);
        let (view, prev) = (self.prof.view, self.prof.prev_view);
        let hits = self.hits.len();
        if t < 1.0 && prev != view {
            let pa = a * (1.0 - t * 2.2).clamp(0.0, 1.0);
            if pa > 0.0 {
                self.draw_view(fs, out, prev, c, pa, clip);
                self.hits.truncate(hits);
            }
        }
        let na = a * ((t - 0.3) / 0.7).clamp(0.0, 1.0);
        if na > 0.0 {
            self.draw_view(fs, out, view, c, na, clip);
        }
    }

    fn draw_view(&mut self, fs: &mut FontSystem, out: &mut DrawList, v: View, c: Rect, a: f32, clip: Rect) {
        match v {
            View::Card => self.draw_card(fs, out, c, a, clip),
            View::Picker => self.draw_picker(fs, out, c, a, clip),
            View::Crop => self.draw_crop(fs, out, c, a, clip),
        }
    }

    fn draw_card(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Rect, a: f32, clip: Rect) {
        let top = c.y + 2.0;
        let (cx, cy) = (c.x + 2.0 + RING_W + AV_R, top + RING_W + AV_R);
        self.prof.centre = (cx, cy);
        let hit = Rect::new(cx - AV_R - 3.0, cy - AV_R - 3.0, (AV_R + 3.0) * 2.0, (AV_R + 3.0) * 2.0);
        self.hits.push((hit, Ctl::Avatar));
        let h = self.hover_of(Ctl::Avatar);
        self.ring(out, cx, cy, AV_R, a);
        if self.prof.has_avatar {
            self.round_image(out, self.prof.key_avatar(), cx, cy, AV_R, a, 1.0 - 0.45 * h, [0.0, 0.0, 1.0, 1.0]);
        } else {
            out.shapes.push(paint::circle(self.scale, cx, cy, AV_R - 0.5).fill(Rgba::hex(0x111116, 1.0)).alpha(a).done());
            let (ca, cb) = theme::accent();
            let initial = self.prof.initial();
            self.centred(fs, out, &initial, F_INITIAL, cx, cy + 1.0, ca.mix(cb, 0.45).with_a(1.0).fade(0.95 * a * (1.0 - 0.8 * h)), clip);
        }
        if h > 0.01 {
            self.icon(out, "camera-photo-symbolic", cx - 11.0, cy - 11.0, 22.0, TEXT.fade(a * h));
        }

        let nx = cx + AV_R + 26.0;
        let right = c.x + c.w - 44.0;
        self.draw_name(fs, out, nx, top + 26.0, right - nx, a, clip);
        let who = match self.prof.info.as_ref().and_then(|i| i.host.as_deref()) {
            Some(h) => format!("{}@{h}", self.prof.login),
            None => self.prof.login.clone(),
        };
        self.label(fs, out, &who, F_SUB, nx, top + 56.0, SOFT.fade(a), clip);
        self.draw_friendly(fs, out, nx, top + 79.0, right - nx, a, clip);

        // Quiet links under the line; they're text until the pointer is on them.
        let ly = top + 106.0;
        let mut x = nx;
        x += self.link(fs, out, Ctl::ChangePic, "Change picture", x, ly, SOFT, a, clip) + 18.0;
        if self.prof.ours {
            let armed = self.armed.is_some_and(|(k, _)| k == ARM_REMOVE);
            let (t, col) = if armed { ("Really remove?", DANGER_FG) } else { ("Remove picture", MUTED) };
            x += self.link(fs, out, Ctl::RemovePic, t, x, ly, col, a, clip) + 18.0;
        }
        if let Some(e) = self.prof.error.clone() {
            let (k, _) = self.text.get_fit(fs, &e, F_SMALL, (right - x).max(40.0));
            let lh = self.text.line_h(F_SMALL);
            self.text(out, k, x, ly - lh * 0.5, super::BAD.fade(a), clip);
        }

        let gy = top + 130.0;
        self.divider(out, c.x, gy, c.w, a);
        self.section(fs, out, "This machine", c.x + 2.0, gy + 24.0, a, clip);
        self.draw_rows(fs, out, Rect::new(c.x, gy + 38.0, c.w, c.y + c.h - gy - 38.0), a, clip);
    }

    fn draw_name(&mut self, fs: &mut FontSystem, out: &mut DrawList, x: f32, cy: f32, w: f32, a: f32, clip: Rect) {
        let sc = self.scale;
        let lh = self.text.line_h(F_NAME);
        if let Some(l) = self.prof.name_edit.clone() {
            self.hits.push((Rect::new(x - 6.0, cy - 21.0, w + 12.0, 42.0), Ctl::NameField));
            let cw = if l.head().is_empty() { 0.0 } else { self.text.get(fs, l.head(), F_NAME).1 };
            // A long name keeps the caret in view.
            let shift = (cw + 4.0 - w).max(0.0);
            let field = Rect::new(x, cy - lh, w, lh * 2.0);
            if l.text.is_empty() {
                self.label(fs, out, &self.prof.fallback.clone(), F_NAME, x, cy, MUTED.fade(a * 0.5), field);
            } else {
                let (k, _) = self.text.get(fs, &l.text, F_NAME);
                self.text(out, k, x - shift, cy - lh * 0.5, TEXT.fade(a), field);
            }
            out.shapes.push(paint::rect(sc, Rect::new(x + cw - shift, cy - 13.0, 2.0, 26.0), 1.0).on_fill(0.0, 1.0, 0.0).alpha(a).done());
            out.shapes.push(paint::rect(sc, Rect::new(x, cy + 18.0, w, 1.0), 0.5).on_fill(1.0, 0.0, 0.0).alpha(a * 0.7).done());
            return;
        }
        let name = self.prof.name.clone();
        let (k, tw) = self.text.get_fit(fs, &name, F_NAME, w - 26.0);
        self.text(out, k, x, cy - lh * 0.5, TEXT.fade(a), clip);
        let hit = Rect::new(x - 6.0, cy - 19.0, tw + 36.0, 38.0);
        self.hits.push((hit, Ctl::NameField));
        let h = self.hover_of(Ctl::NameField);
        if h > 0.01 {
            self.icon(out, "document-edit-symbolic", x + tw + 10.0, cy - 7.0, 14.0, MUTED.fade(a * h));
        }
    }

    fn draw_friendly(&mut self, fs: &mut FontSystem, out: &mut DrawList, x: f32, cy: f32, w: f32, a: f32, clip: Rect) {
        let p = &self.prof;
        let Some(line) = p.lines.get(p.line).cloned() else { return };
        let t = p.line_t.get().clamp(0.0, 1.0);
        let prev = p.prev_line.and_then(|i| p.lines.get(i)).cloned();
        let (ca, cb) = theme::accent();
        let col = MUTED.mix(ca.mix(cb, 0.5).with_a(1.0), 0.55);
        if let Some(pl) = prev {
            let (k, _) = self.text.get_fit(fs, &pl, F_BODY, w);
            let lh = self.text.line_h(F_BODY);
            self.text(out, k, x, cy - lh * 0.5, col.fade(a * (1.0 - t)), clip);
        }
        let (k, tw) = self.text.get_fit(fs, &line, F_BODY, w);
        let lh = self.text.line_h(F_BODY);
        self.text(out, k, x, cy - lh * 0.5, col.fade(a * t), clip);
        self.hits.push((Rect::new(x - 4.0, cy - 10.0, tw + 8.0, 20.0), Ctl::Friendly));
    }

    /// A text button: nothing at rest, a faint plate under the pointer.
    fn link(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Ctl, s: &str, x: f32, cy: f32, col: Rgba, a: f32, clip: Rect) -> f32 {
        let (k, w) = self.text.get(fs, s, F_SMALL);
        let lh = self.text.line_h(F_SMALL);
        let r = Rect::new(x - 8.0, cy - 12.0, w + 16.0, 24.0);
        self.hits.push((r, c));
        let h = self.hover_of(c);
        self.hover_bg(out, r, 9.0, a, h);
        self.text(out, k, x, cy - lh * 0.5, col.mix(TEXT, h).fade(a), clip);
        w
    }

    /// Two calm columns of label over value.
    fn draw_rows(&mut self, fs: &mut FontSystem, out: &mut DrawList, b: Rect, a: f32, clip: Rect) {
        let rows = self.prof.rows(self.accent);
        if rows.is_empty() {
            self.label(fs, out, "Reading this machine…", F_SMALL, b.x + 2.0, b.y + 16.0, MUTED.fade(a), clip);
            return;
        }
        let per = rows.len().div_ceil(2);
        let col_w = (b.w - 32.0) * 0.5;
        let row_h = (b.h / per as f32).min(50.0);
        let lh_l = self.text.line_h(F_SMALL);
        for (i, (k, v)) in rows.iter().enumerate() {
            let (col, row) = ((i / per) as f32, (i % per) as f32);
            let x = b.x + 2.0 + col * (col_w + 32.0);
            let y = b.y + row * row_h + 2.0;
            let (kl, _) = self.text.get(fs, k, F_SMALL);
            self.text(out, kl, x, y, MUTED.fade(a), clip);
            let (kv, _) = self.text.get_fit(fs, v, F_LABEL, col_w);
            self.text(out, kv, x, y + lh_l + 1.0, TEXT.fade(a), clip);
        }
    }

    fn round_button(&mut self, out: &mut DrawList, c: Ctl, icon: &str, x: f32, cy: f32, a: f32) {
        let r = Rect::new(x, cy - 15.0, 30.0, 30.0);
        self.hits.push((r, c));
        let h = self.hover_of(c);
        self.hover_bg(out, r, 15.0, a, h);
        self.icon(out, icon, r.x + 7.0, r.y + 7.0, 16.0, SOFT.mix(TEXT, h).fade(a));
    }

    fn draw_picker(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        self.round_button(out, Ctl::Back, "go-previous-symbolic", c.x - 6.0, c.y + 18.0, a);
        self.label(fs, out, "Choose a picture", F_PAGE, c.x + 32.0, c.y + 18.0, TEXT.fade(a), clip);
        self.label(fs, out, "Recent ones from ~/Pictures, or any file by its path", F_SUB, c.x + 32.0, c.y + 46.0, MUTED.fade(a), clip);

        // The path: no box, a line that lights up when there's something in it.
        let fy = c.y + 86.0;
        let field = Rect::new(c.x + 2.0, fy - 13.0, c.w - 4.0, 26.0);
        self.hits.push((field, Ctl::PathField));
        let lh = self.text.line_h(F_BODY);
        let text = self.prof.path.text.clone();
        let head = self.prof.path.head().to_string();
        let cw = if head.is_empty() { 0.0 } else { self.text.get(fs, &head, F_BODY).1 };
        let shift = (cw + 4.0 - field.w).max(0.0);
        if text.is_empty() {
            self.label(fs, out, "Paste or type a path, then press Enter", F_BODY, field.x + 5.0, fy, MUTED.fade(a * 0.8), field);
        } else {
            let (k, _) = self.text.get(fs, &text, F_BODY);
            self.text(out, k, field.x + 2.0 - shift, fy - lh * 0.5, TEXT.fade(a), field);
        }
        let lit = if text.is_empty() { 0.35 } else { 0.8 };
        out.shapes.push(paint::rect(sc, Rect::new(field.x + 2.0 + cw - shift, fy - 9.0, 1.5, 18.0), 0.75).on_fill(0.0, 1.0, 0.0).alpha(a).done());
        out.shapes.push(paint::rect(sc, Rect::new(c.x, fy + 15.0, c.w, 1.0), 0.5).on_fill(1.0, 0.0, 0.0).alpha(a * lit).done());

        let vp = Rect::new(c.x, fy + 30.0, c.w, c.y + c.h - fy - 54.0);
        let tile = self.prof_tile();
        let foot = vp.y + vp.h + 14.0;
        if let Some(e) = self.prof.error.clone() {
            let (k, _) = self.text.get_fit(fs, &e, F_SMALL, c.w);
            let flh = self.text.line_h(F_SMALL);
            self.text(out, k, c.x + 2.0, foot - flh * 0.5, super::BAD.fade(a), clip);
        }
        let Some(pk) = &self.prof.picker else { return };
        let Some(items) = pk.items.as_ref() else {
            self.label(fs, out, "Looking through ~/Pictures…", F_SMALL, c.x + 2.0, vp.y + 20.0, MUTED.fade(a), clip);
            return;
        };
        if items.is_empty() {
            self.label(fs, out, "Nothing in ~/Pictures yet. A path works just as well.", F_SMALL, c.x + 2.0, vp.y + 20.0, MUTED.fade(a), clip);
            return;
        }
        let n = items.len();
        let rows = n.div_ceil(COLS);
        let content = rows as f32 * (tile + GAP) - GAP;
        let scroll = pk.scroll.get();
        let max_scroll = (content - vp.h).max(0.0);
        let mut want: Vec<(usize, PathBuf)> = vec![];
        let mut hovered = None;
        for i in 0..n {
            let (cx, row) = (i % COLS, i / COLS);
            let x = c.x + cx as f32 * (tile + GAP);
            let top = vp.y + row as f32 * (tile + GAP) - scroll;
            let bot = top + tile;
            // One row ahead each way, so scrolling finds them there.
            let near = bot > vp.y - tile && top < vp.y + vp.h + tile;
            let it = &self.prof.picker.as_ref().unwrap().items.as_ref().unwrap()[i];
            if near && !it.ready && !it.queued && !it.broken {
                want.push((i, it.path.clone()));
            }
            if bot <= vp.y || top >= vp.y + vp.h {
                continue;
            }
            let (ready, broken, shown) = (it.ready, it.broken, it.shown.get().clamp(0.0, 1.0));
            let name = it.path.file_name().map(|n| n.to_string_lossy().into_owned());
            // Clipped by hand: shapes and images have no clip of their own.
            let (y0, y1) = (top.max(vp.y), bot.min(vp.y + vp.h));
            let r = Rect::new(x, y0, tile, y1 - y0);
            let ctl = Ctl::Thumb(i as u16);
            self.hits.push((r, ctl));
            let h = self.hover_of(ctl);
            if self.hovered == ctl {
                hovered = name;
            }
            let rad = 12.0f32.min(r.h * 0.5);
            if h > 0.01 {
                out.shapes.push(paint::rect(sc, r.inflate(2.0), rad + 2.0).prism(1.6, 3.6).alpha(a * h).done());
            }
            // A row fades as it leaves, so no slivers line the edges.
            let a = a * ((y1 - y0) / 56.0).clamp(0.0, 1.0).powi(2);
            if ready {
                let mut img = ImageDraw::new(self.prof.key_thumb(i), [r.x * sc, r.y * sc, r.w * sc, r.h * sc]);
                img.radius = rad * sc;
                img.alpha = a * shown;
                img.uv = [0.0, (y0 - top) / tile, 1.0, (y1 - top) / tile];
                out.images.push(img);
            }
            if !ready || shown < 1.0 {
                out.shapes.push(paint::rect(sc, r, rad).fill(TILE).alpha(a * (1.0 - shown)).done());
            }
            if broken {
                self.icon(out, "image-x-generic-symbolic", x + tile * 0.5 - 10.0, (y0 + y1) * 0.5 - 10.0, 20.0, MUTED.fade(a * 0.6));
            }
        }
        let name = hovered;
        let px = self.prof.picker.as_ref().map_or(0, |p| p.thumb_px);
        if let Some(pk) = &mut self.prof.picker {
            pk.max_scroll = max_scroll;
            for (i, path) in want {
                pk.pool.push(Job { idx: i, hi: false, path, w: px, h: px }, false);
                if let Some(it) = pk.items.as_mut().and_then(|v| v.get_mut(i)) {
                    it.queued = true;
                }
            }
        }
        if self.prof.error.is_none()
            && let Some(n) = name
        {
            let (k, _) = self.text.get_fit(fs, &n, F_SMALL, c.w);
            let flh = self.text.line_h(F_SMALL);
            self.text(out, k, c.x + 2.0, foot - flh * 0.5, MUTED.fade(a), clip);
        }
    }

    fn draw_crop(&mut self, fs: &mut FontSystem, out: &mut DrawList, c: Rect, a: f32, clip: Rect) {
        let sc = self.scale;
        self.round_button(out, Ctl::Back, "go-previous-symbolic", c.x - 6.0, c.y + 18.0, a);
        self.label(fs, out, "Frame your picture", F_PAGE, c.x + 32.0, c.y + 18.0, TEXT.fade(a), clip);
        self.label(fs, out, "Drag to move it, scroll to zoom", F_SUB, c.x + 32.0, c.y + 46.0, MUTED.fade(a), clip);
        let st = Rect::new(c.x, c.y + 76.0, STAGE, STAGE);
        let (ox, oy) = (st.x + st.w * 0.5, st.y + st.h * 0.5);
        self.prof.stage_centre = (ox, oy);
        self.hits.push((st, Ctl::Stage));
        let (crop, thumb, saving) = match &self.prof.crop {
            Some(v) => (v.crop, v.thumb, v.saving),
            None => (None, None, false),
        };
        let key = self.prof.key_preview();
        let r = CIRCLE * 0.5;
        out.shapes.push(paint::rect(sc, st, 18.0).fill(TILE).alpha(a).done());
        match crop {
            Some(cr) if self.prof.has_preview => {
                // Under the ink, so the picture outside the circle sinks
                // into the glass; the circle sits on top of it, full bright.
                if let Some((rect, uv)) = cr.visible(ox, oy, CIRCLE, [st.x, st.y, st.w, st.h]) {
                    let mut bg = ImageDraw::new(key, [rect[0] * sc, rect[1] * sc, rect[2] * sc, rect[3] * sc]);
                    bg.uv = uv;
                    bg.radius = 18.0 * sc;
                    bg.alpha = a;
                    let mut fg = bg;
                    bg.bright = 0.55;
                    out.backdrop.push(bg);
                    out.shapes.push(paint::circle(sc, ox, oy, r + RING_W).on_fill(RING_REST.cos(), RING_REST.sin(), 0.08).alpha(a).done());
                    fg.reveal = [ox * sc, oy * sc, r * sc, 1.0];
                    out.images.push(fg);
                }
            }
            _ => {
                self.ring(out, ox, oy, r, a);
                if let Some(t) = thumb {
                    self.round_image(out, t, ox, oy, r, a, 1.0, [0.0, 0.0, 1.0, 1.0]);
                } else {
                    self.centred(fs, out, "Opening…", F_BODY, ox, oy, MUTED.fade(a), clip);
                }
            }
        }

        // The side column: how it will look, the zoom, the verdict.
        let x = st.x + st.w + 28.0;
        let w = c.x + c.w - x;
        let cx = x + w * 0.5;
        self.label(fs, out, "AT THE LOCK SCREEN", F_SECTION, x, st.y + 8.0, MUTED.fade(a), clip);
        let (py, pr) = (st.y + 76.0, 44.0);
        self.ring(out, cx, py, pr, a);
        if let Some(cr) = crop.filter(|_| self.prof.has_preview) {
            self.round_image(out, key, cx, py, pr, a, 1.0, cr.uv());
        } else if let Some(t) = thumb {
            self.round_image(out, t, cx, py, pr, a, 1.0, [0.0, 0.0, 1.0, 1.0]);
        }
        let name = self.prof.name.clone();
        let (k, tw) = self.text.get_fit(fs, &name, F_LABEL, w);
        let lh = self.text.line_h(F_LABEL);
        self.text(out, k, cx - tw * 0.5, py + pr + 22.0 - lh * 0.5, TEXT.fade(a), clip);

        // Zoom: a thin rail with the clear glass knob.
        let zy = st.y + 232.0;
        let rail = Rect::new(x + 2.0, zy, w - 4.0, 1.0);
        self.prof.zoom_rail = rail;
        self.hits.push((Rect::new(x - 8.0, zy - 14.0, w + 16.0, 28.0), Ctl::ZoomBar));
        self.label(fs, out, "ZOOM", F_SECTION, x, zy - 24.0, MUTED.fade(a), clip);
        out.shapes.push(paint::rect(sc, Rect::new(rail.x, zy - 2.0, rail.w, 4.0), 2.0).fill(TRACK).alpha(a).done());
        let t = crop.map_or(0.0, |c| c.zoom_t());
        if rail.w * t > 0.5 {
            out.shapes.push(paint::rect(sc, Rect::new(rail.x, zy - 2.0, (rail.w * t).max(4.0), 4.0), 2.0).on_fill(1.0, 0.0, 0.0).alpha(a).done());
        }
        let kh = self.hover_of(Ctl::ZoomBar).max(if self.dragging == Ctl::ZoomBar { 1.0 } else { 0.0 });
        out.shapes.push(paint::knob(sc, rail.x + rail.w * t, zy, 11.0 + 0.5 * kh).alpha(a).done());

        // Save is the one lit thing on the page.
        let by = st.y + st.h - 40.0;
        let bw = (w - 10.0) * 0.5;
        let cancel = Rect::new(x, by, bw, 40.0);
        let save = Rect::new(x + bw + 10.0, by, bw, 40.0);
        if !saving {
            self.hits.push((cancel, Ctl::CropCancel));
        }
        let ch = self.hover_of(Ctl::CropCancel);
        out.shapes.push(paint::rect(sc, cancel, 13.0).fill(TILE.mix(HOVER, ch)).alpha(a).done());
        self.centred(fs, out, "Cancel", F_LABEL, cancel.x + cancel.w * 0.5, cancel.y + 20.0, SOFT.mix(TEXT, ch).fade(a), clip);
        let ready = crop.is_some() && !saving;
        if ready {
            self.hits.push((save, Ctl::CropSave));
        }
        let sh = self.hover_of(Ctl::CropSave);
        out.shapes.push(paint::rect(sc, save, 13.0).on_fill(paint::IRID.0, paint::IRID.1, 0.1 * sh).alpha(a * if ready { 1.0 } else { 0.5 }).done());
        let label = if saving { "Saving…" } else { "Save" };
        self.centred(fs, out, label, F_LABEL, save.x + save.w * 0.5, save.y + 20.0, theme::on_ink().fade(a), clip);
        if let Some(e) = self.prof.error.clone() {
            let (k, _) = self.text.get_fit(fs, &e, F_SMALL, c.w);
            let flh = self.text.line_h(F_SMALL);
            self.text(out, k, st.x + 2.0, st.y + st.h + 22.0 - flh * 0.5, super::BAD.fade(a), clip);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        rc::Rc,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use calloop::channel::{Channel, channel};
    use glyphon::fontdb;

    use super::*;
    use crate::{icons::IconSet, profile::crop::Crop};

    const DT: f32 = 1.0 / 120.0;

    struct Rig {
        st: Prefs,
        fs: FontSystem,
        rx: Channel<Fact>,
        list: DrawList,
        home: PathBuf,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    fn rig(name: &str) -> Rig {
        let home = std::env::temp_dir().join(format!("nimbus-profile-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Pictures")).unwrap();
        // The shell's own short list: all system fonts take seconds.
        let mut db = fontdb::Database::new();
        crate::fonts::load(&mut db, &["Geist"]);
        let mut fs = FontSystem::new_with_locale_and_db("en".into(), db);
        let start = Rect::new(100.0, 40.0, 440.0, 300.0);
        let mut st = Prefs::new(&mut fs, Rc::new(IconSet::discover()), 1.0, start, "Test GPU".into(), &[]);
        let (tx, rx) = channel();
        st.attach_at(tx, home.clone(), "ada".into(), "Ada Lovelace".into());
        let mut r = Rig { st, fs, rx, list: DrawList::default(), home };
        r.wait_msg(); // the first look at ~/.face
        // Not whatever a test running beside this one just saved.
        r.st.prof.name = "Ada Lovelace".into();
        r.settle();
        r
    }

    impl Rig {
        fn frame(&mut self) {
            self.st.draw(&mut self.fs, &mut self.list);
            self.st.step(DT);
        }

        fn settle(&mut self) {
            for _ in 0..240 {
                self.frame();
            }
        }

        fn hover(&mut self, c: Ctl) -> (f32, f32) {
            self.frame();
            let (x, y) = self.st.hit_center(c).unwrap_or_else(|| panic!("{c:?} isn't on screen"));
            self.st.pointer_motion(x, y);
            (x, y)
        }

        fn click(&mut self, c: Ctl) {
            let (x, y) = self.hover(c);
            self.st.press(x, y);
            self.st.release();
        }

        /// The next message from the workers, applied.
        fn wait_msg(&mut self) {
            let t = Instant::now();
            loop {
                if let Ok(f) = self.rx.try_recv() {
                    self.st.on_custom(&f);
                    return;
                }
                assert!(t.elapsed() < Duration::from_secs(20), "no message from the workers");
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        fn msg(&self, body: Body) -> Fact {
            Fact::Custom(Arc::new(Msg { owner: self.st.prof.base, body: Mutex::new(Some(body)) }))
        }

        fn key(&mut self, sym: u32) {
            self.st.key(&KeyInput { sym, text: None, ctrl: false, shift: false, alt: false });
        }

        fn typed(&mut self, t: &str) {
            self.st.key(&KeyInput { sym: 0x20, text: Some(t.into()), ctrl: false, shift: false, alt: false });
        }

        /// Open the picker on `n` made-up, empty entries.
        fn picker(&mut self, n: usize) {
            self.click(Ctl::Avatar);
            self.wait_msg(); // the (empty) scan
            let f = self.msg(Body::Scan((0..n).map(|i| PathBuf::from(format!("/no/such/{i}.png"))).collect()));
            assert!(self.st.on_custom(&f));
            self.settle();
        }

        fn thumb(&mut self, idx: usize, ok: bool) {
            let owner = self.st.prof.picker.as_ref().unwrap().owner;
            let img = ok.then(|| ImageData { w: 2, h: 2, rgba: Arc::new(vec![0; 16]) });
            let t = Thumb { owner, idx, hi: false, cached: true, img: Mutex::new(img), src: (2, 2), avg: [0.0; 3] };
            assert!(self.st.on_custom(&Fact::Custom(Arc::new(t))));
        }
    }

    fn png(p: &Path, w: u32, h: u32) {
        image::RgbImage::from_pixel(w, h, image::Rgb([200, 80, 40])).save_with_format(p, image::ImageFormat::Png).unwrap();
    }

    #[test]
    fn it_opens_on_the_profile_with_the_initial() {
        let mut r = rig("open");
        assert_eq!(r.st.page, Page::Profile);
        assert_eq!(r.st.prof.initial(), "A");
        assert!(!r.st.profile_has_avatar());
        assert!(!r.st.prof.ours);
        // The machine list waits for facts.
        assert!(r.st.prof.rows(0).is_empty());
    }

    #[test]
    fn nothing_moves_or_uploads_at_rest() {
        let mut r = rig("rest");
        r.st.set_info(Info { hour: Some(9), ..Default::default() });
        r.settle();
        for _ in 0..60 {
            r.st.draw(&mut r.fs, &mut r.list);
            assert!(r.list.uploads.is_empty() && r.list.frees.is_empty());
            assert!(!r.st.step(DT), "something still animates at rest");
            assert_eq!(r.st.wake_at, None);
        }
    }

    #[test]
    fn a_saved_picture_turns_the_ring_once_then_stops() {
        let mut r = rig("ring");
        let img = ImageData { w: 2, h: 2, rgba: Arc::new(vec![0; 16]) };
        let f = r.msg(Body::Saved(Ok((Some(img), true))));
        r.st.on_custom(&f);
        assert!(r.st.step(DT));
        for _ in 0..600 {
            r.st.step(DT);
        }
        assert!(!r.st.step(DT));
        assert_eq!(r.st.prof.ring_turn.get(), 0.0);
    }

    #[test]
    fn thumbnails_upload_once_and_are_freed_on_leaving() {
        let mut r = rig("thumbs");
        r.picker(12);
        for i in 0..12 {
            r.thumb(i, i != 3);
        }
        r.st.draw(&mut r.fs, &mut r.list);
        assert_eq!(r.list.uploads.len(), 11);
        assert_eq!(r.st.profile_thumbs(), 11);
        r.st.draw(&mut r.fs, &mut r.list);
        assert!(r.list.uploads.is_empty(), "uploaded once");
        // A failed one is a placeholder that explains itself.
        r.click(Ctl::Thumb(3));
        assert_eq!(r.st.prof.view, View::Picker);
        assert!(r.st.profile_error().is_some());
        r.click(Ctl::Back);
        r.settle();
        assert_eq!(r.st.prof.view, View::Card);
        assert!(r.st.prof.picker.is_none());
        assert_eq!(r.st.profile_textures(), 0);
    }

    #[test]
    fn a_click_after_scrolling_gets_the_tile_under_the_pointer() {
        let mut r = rig("scroll");
        r.picker(40);
        let (x, y) = r.hover(Ctl::Thumb(5));
        r.st.scroll(2.0);
        r.settle();
        assert_ne!(r.st.hovered, Ctl::Thumb(5), "the tile scrolled away");
        assert!(matches!(r.st.hovered, Ctl::Thumb(i) if i > 5));
        let Ctl::Thumb(i) = r.st.hovered else { unreachable!() };
        r.st.press(x, y);
        r.st.release();
        assert_eq!(r.st.prof.crop.as_ref().unwrap().path, PathBuf::from(format!("/no/such/{i}.png")));
    }

    #[test]
    fn scrolling_stops_at_both_ends() {
        let mut r = rig("ends");
        r.picker(40);
        r.st.scroll(-5.0);
        r.settle();
        assert_eq!(r.st.prof.picker.as_ref().unwrap().scroll.get(), 0.0);
        for _ in 0..50 {
            r.st.scroll(5.0);
        }
        r.settle();
        let k = r.st.prof.picker.as_ref().unwrap();
        assert!(k.max_scroll > 0.0);
        assert_eq!(k.scroll.get(), k.max_scroll);
    }

    #[test]
    fn frees_reach_the_gpu_in_a_later_frame() {
        let mut r = rig("frees");
        r.picker(4);
        r.thumb(0, true);
        r.st.draw(&mut r.fs, &mut r.list);
        r.click(Ctl::Back);
        let mut freed = vec![];
        for _ in 0..240 {
            r.st.draw(&mut r.fs, &mut r.list);
            freed.extend_from_slice(&r.list.frees);
            r.st.step(DT);
        }
        assert_eq!(freed, [r.st.prof.key_thumb(0)]);
    }

    #[test]
    fn closing_hands_back_every_texture_and_stops_listening() {
        let mut r = rig("close");
        let img = || ImageData { w: 2, h: 2, rgba: Arc::new(vec![0; 16]) };
        let f = r.msg(Body::Avatar { img: Some(img()), ours: true });
        r.st.on_custom(&f);
        r.picker(4);
        r.thumb(0, true);
        r.thumb(1, true);
        let path = PathBuf::from("/no/such/0.png");
        r.st.pick(path.clone(), None);
        let f = r.msg(Body::Picked { path, result: Ok(profile::Preview { img: img(), size: (2, 2) }) });
        r.st.on_custom(&f);
        r.st.draw(&mut r.fs, &mut r.list);
        assert_eq!(r.st.profile_textures(), 4);
        let keys = (r.st.prof.key_avatar(), r.st.prof.key_preview(), r.st.prof.key_thumb(0), r.st.prof.key_thumb(1));
        let mut frees = vec![];
        r.st.release_textures(&mut frees);
        frees.sort();
        let mut want = vec![keys.0, keys.1, keys.2, keys.3];
        want.sort();
        assert_eq!(frees, want);
        assert_eq!(r.st.profile_textures(), 0);
        // Late results find nobody home, and don't resurrect a texture.
        let f = r.msg(Body::Avatar { img: Some(img()), ours: true });
        assert!(!r.st.on_custom(&f));
        assert_eq!(r.st.profile_textures(), 0);
    }

    #[test]
    fn other_windows_messages_are_ignored() {
        let mut r = rig("owner");
        r.picker(2);
        let f = Fact::Custom(Arc::new(Msg { owner: 1, body: Mutex::new(Some(Body::Pasted("x".into()))) }));
        assert!(!r.st.on_custom(&f));
        let t = Thumb { owner: 1, idx: 0, hi: false, cached: true, img: Mutex::new(None), src: (0, 0), avg: [0.0; 3] };
        assert!(!r.st.on_custom(&Fact::Custom(Arc::new(t))));
        assert!(!r.st.on_custom(&Fact::Custom(Arc::new(0u32))));
    }

    #[test]
    fn escape_steps_back_one_view_at_a_time() {
        let mut r = rig("escape");
        r.picker(2);
        let path = PathBuf::from("/no/such/0.png");
        r.st.pick(path, None);
        assert_eq!(r.st.prof.view, View::Crop);
        r.key(keys::ESCAPE);
        assert_eq!(r.st.prof.view, View::Picker);
        r.key(keys::ESCAPE);
        assert_eq!(r.st.prof.view, View::Card);
        assert!(!r.st.closing());
        r.key(keys::ESCAPE);
        assert!(r.st.closing());
    }

    #[test]
    fn a_late_result_for_a_crop_that_is_gone_changes_nothing() {
        let mut r = rig("late");
        r.picker(2);
        r.st.pick(PathBuf::from("/a.png"), None);
        r.key(keys::ESCAPE);
        r.settle();
        let f = r.msg(Body::Picked { path: PathBuf::from("/a.png"), result: Err("nope".into()) });
        assert!(!r.st.on_custom(&f));
        assert_eq!(r.st.profile_error(), None);
        r.st.pick(PathBuf::from("/b.png"), None);
        let f = r.msg(Body::Picked { path: PathBuf::from("/a.png"), result: Err("nope".into()) });
        assert!(!r.st.on_custom(&f));
        assert_eq!(r.st.prof.view, View::Crop);
    }

    #[test]
    fn leaving_the_page_goes_back_to_the_card_and_keeps_no_pixels() {
        let mut r = rig("leave");
        r.picker(2);
        r.thumb(0, true);
        r.frame();
        r.click(Ctl::Nav(1));
        r.settle();
        assert_eq!(r.st.page, Page::Appearance);
        assert_eq!(r.st.prof.view, View::Card);
        assert_eq!(r.st.profile_textures(), 0);
    }

    #[test]
    fn a_typed_path_that_does_not_exist_comes_back_as_an_error() {
        let mut r = rig("typed");
        r.click(Ctl::Avatar);
        r.wait_msg();
        for c in ["n", "o", "p", "e", ".", "p", "n", "g"] {
            r.typed(c);
        }
        r.key(keys::RETURN);
        assert_eq!(r.st.prof.view, View::Crop);
        r.wait_msg();
        assert_eq!(r.st.prof.view, View::Picker);
        assert!(r.st.profile_error().is_some());
        assert!(!r.st.profile_cropping());
    }

    #[test]
    fn return_with_an_empty_field_says_what_it_wants() {
        let mut r = rig("empty");
        r.click(Ctl::Avatar);
        r.key(keys::RETURN);
        assert_eq!(r.st.prof.view, View::Picker);
        assert!(r.st.profile_error().is_some());
    }

    #[test]
    fn pick_crop_save_remove_through_the_real_workers() {
        let mut r = rig("flow");
        let src = r.home.join("Pictures/me.png");
        png(&src, 600, 300);
        r.click(Ctl::Avatar);
        r.wait_msg(); // the scan finds it
        r.settle();
        assert_eq!(r.st.prof.picker.as_ref().unwrap().items.as_ref().unwrap().len(), 1);
        r.wait_msg(); // its thumbnail
        assert_eq!(r.st.profile_thumbs(), 1);
        r.settle();
        r.click(Ctl::Thumb(0));
        r.wait_msg(); // the decoded picture
        assert!(r.st.profile_cropping());
        r.settle();

        // Drag and scroll land in the crop.
        let (x, y) = r.hover(Ctl::Stage);
        let before = r.st.prof.crop.as_ref().unwrap().crop.unwrap();
        r.st.scroll(-3.0);
        r.st.press(x, y);
        r.st.pointer_motion(x + 40.0, y);
        r.st.release();
        let after = r.st.prof.crop.as_ref().unwrap().crop.unwrap();
        assert!(after.zoom > before.zoom);
        assert!(after.cx < before.cx, "dragging right shows more of the left");
        assert_eq!(r.st.cursor_hint(), Some(Cursor::Grab));

        r.click(Ctl::CropSave);
        assert!(r.st.prof.crop.as_ref().unwrap().saving);
        r.click(Ctl::Stage); // busy: nothing else happens
        r.key(keys::ESCAPE);
        assert_eq!(r.st.prof.view, View::Crop, "a save in progress isn't abandoned");
        r.wait_msg();
        assert_eq!(r.st.prof.view, View::Card);
        assert!(r.st.profile_has_avatar());
        assert!(r.st.prof.ours);
        let b = std::fs::read(r.home.join(".face")).unwrap();
        let img = image::load_from_memory(&b).unwrap();
        assert_eq!((img.width(), img.height()), (avatar::FACE_PX, avatar::FACE_PX));
        r.settle();
        assert_eq!(r.st.profile_textures(), 1, "only the avatar is left");

        // Remove asks once more, then goes back to the initial.
        r.click(Ctl::RemovePic);
        assert!(r.home.join(".face").exists());
        r.click(Ctl::RemovePic);
        r.wait_msg();
        assert!(!r.home.join(".face").exists());
        assert!(!r.st.profile_has_avatar());
        assert!(!r.st.prof.ours);
        assert_eq!(r.st.profile_textures(), 0);
    }

    #[test]
    fn a_link_at_face_shows_up_as_an_error_in_the_crop() {
        let mut r = rig("link");
        let src = r.home.join("Pictures/me.png");
        png(&src, 64, 64);
        std::os::unix::fs::symlink("/no/such/target", r.home.join(".face")).unwrap();
        r.st.prof.crop = Some(CropView { path: src, thumb: None, crop: Some(Crop::new(64, 64)), saving: false });
        r.st.prof.set_view(View::Crop);
        r.st.save_crop();
        r.wait_msg();
        assert_eq!(r.st.prof.view, View::Crop);
        assert!(r.st.profile_error().is_some_and(|e| e.contains(".face")));
        assert!(!r.st.prof.crop.as_ref().unwrap().saving);
    }

    #[test]
    fn the_name_is_edited_in_place_and_kept() {
        let mut r = rig("name");
        r.st.set_info(Info { hour: Some(9), ..Default::default() });
        r.settle();
        assert_eq!(r.st.prof.lines[0], "Good morning, Ada.");

        // Escape abandons it.
        r.click(Ctl::NameField);
        assert_eq!(r.st.cursor_hint(), Some(Cursor::Text));
        r.typed("!");
        r.click(Ctl::NameField);
        assert_eq!(r.st.prof.name_edit.as_ref().unwrap().text, "Ada Lovelace!", "a second click keeps the typing");
        r.key(keys::ESCAPE);
        assert_eq!(r.st.prof.name, "Ada Lovelace");
        assert!(!r.st.closing(), "Escape left the field, not the window");

        // Return keeps it, and it reaches the config and the greeting.
        r.click(Ctl::NameField);
        r.key(keys::HOME);
        for _ in 0.."Ada Lovelace".len() {
            r.key(keys::DELETE);
        }
        for c in ["G", "r", "a", "c", "e"] {
            r.typed(c);
        }
        r.key(keys::RETURN);
        assert_eq!(r.st.prof.name, "Grace");
        assert_eq!(r.st.prof.lines[0], "Good morning, Grace.");
        assert_eq!(profile::display_name().as_deref(), Some("Grace"));
        assert_eq!(r.st.prof.initial(), "G");

        // Cleared: back to the account's name, nothing kept.
        r.click(Ctl::NameField);
        r.key(keys::END);
        for _ in 0..5 {
            r.key(keys::BACKSPACE);
        }
        // A click elsewhere commits too.
        let (x, y) = r.hover(Ctl::ChangePic);
        r.st.press(x, y);
        r.st.release();
        assert_eq!(r.st.prof.name, "Ada Lovelace");
        assert_eq!(profile::display_name(), None);
    }

    #[test]
    fn the_machine_list_leaves_out_what_is_unknown() {
        let mut r = rig("rows");
        r.st.set_info(Info { kernel: Some("6.1".into()), ..Default::default() });
        let rows = r.st.prof.rows(0);
        let labels: Vec<&str> = rows.iter().map(|x| x.0).collect();
        assert_eq!(labels, ["Kernel", "nimbus", "Accent"]);
        assert_eq!(rows[2].1, theme::ACCENTS[0].0);
        // All twelve fit the page.
        let full = |s: &str| Some(s.to_string());
        r.st.set_info(Info {
            device: full("a"),
            os: full("b"),
            kernel: full("c"),
            cpu: full("d"),
            gpu: full("e"),
            memory: full("f"),
            disk: full("g"),
            uptime: Some(60),
            since: full("h"),
            wallpaper: full("i"),
            ..Default::default()
        });
        assert_eq!(r.st.prof.rows(0).len(), 12);
        r.frame();
    }

    #[test]
    fn the_friendly_line_cycles_through_what_is_known() {
        let mut r = rig("cycle");
        r.st.set_info(Info { hour: Some(9), uptime: Some(7200), generation: Some(7), ..Default::default() });
        r.settle();
        let first = r.st.prof.lines[r.st.prof.line].clone();
        let n = r.st.prof.lines.len();
        assert_eq!(n, 3);
        for _ in 0..n {
            r.click(Ctl::Friendly);
            r.settle();
        }
        assert_eq!(r.st.prof.lines[r.st.prof.line], first);
        assert!(!r.st.step(DT));
    }
}
