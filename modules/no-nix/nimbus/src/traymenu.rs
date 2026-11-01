//! A tray item's menu (com.canonical.dbusmenu), drawn by nimbus: a small
//! Prism Ink card (flat matte ink, the prism hairline) that grows out of
//! the tray icon just under the bar. The hovered row lights up faint white
//! with a small iridescent pill at its edge; submenus slide in place of
//! their parent (with a back row); a click sends the item its "clicked"
//! event and folds the card back into the icon. It closes when the
//! pointer leaves it, on Escape, or when the icon is clicked again.

use std::{collections::HashMap, rc::Rc, sync::Arc};

use glyphon::FontSystem;

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Font, Rect, TextCache},
    gfx::{DrawList, IconDraw, TextDraw},
    icons::IconSet,
    paint,
    services::Fact,
    settings::Action,
    theme::{self, Rgba},
    tray::{Menu, MenuEntry},
    window::{Chrome, Ctx, Placement, WinScene},
};

const W: f32 = 260.0;
const ROW: f32 = 32.0;
const SEP: f32 = 11.0;
const HEAD: f32 = 30.0;
const PAD: f32 = 6.0;
/// Card top, just below the bar.
const TOP: f32 = 44.0;
const MARGIN: f32 = 20.0;
const MAX_H: f32 = 560.0;

const F_ROW: Font = Font::new(13.0, 500).family("Geist");
const F_HEAD: Font = Font::new(10.5, 700).family("Geist").track(0.06);

const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
/// The hovered row: a faint white, nothing at rest.
const HOVER: Rgba = Rgba::hex(0xffffff, 0.08);

/// Is this the payload that opens a menu?
pub fn wants_window(f: &Fact) -> bool {
    matches!(f, Fact::Custom(a) if a.downcast_ref::<Menu>().is_some())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Ctl {
    None,
    Body,
    Back,
    Row(u16),
}

pub struct TrayMenu {
    chrome: Chrome,
    text: TextCache,
    icons: Rc<IconSet>,
    scale: f32,
    size: (f32, f32),
    margin_right: f32,
    menu: Arc<Menu>,
    /// Submenu path (indices into `entries`).
    path: Vec<usize>,
    /// Slide when entering / leaving a submenu.
    slide: Spring,
    hovered: Ctl,
    pressed: Ctl,
    hover_y: Spring,
    hover_a: Spring,
    entered: bool,
    hits: Vec<(Rect, Ctl)>,
}

fn rows_h(v: &[MenuEntry]) -> f32 {
    v.iter().map(|e| if e.separator { SEP } else { ROW }).sum()
}

fn max_rows_h(v: &[MenuEntry]) -> f32 {
    let own = rows_h(v) + ROW;
    v.iter().filter(|e| !e.children.is_empty()).map(|e| max_rows_h(&e.children)).fold(own, f32::max)
}

/// Open the menu the tray worker just fetched (None if it was consumed).
pub fn open(ctx: &mut Ctx) -> Option<Box<dyn WinScene>> {
    let menu = crate::tray::take_menu()?;
    Some(Box::new(TrayMenu::new(ctx, menu)))
}

impl TrayMenu {
    fn new(ctx: &mut Ctx, menu: Arc<Menu>) -> Self {
        let mut text = TextCache::new(ctx.fs, theme::current());
        text.set_scale(ctx.scale);
        let h = (HEAD + max_rows_h(&menu.entries) + PAD * 2.0).min(MAX_H);
        let size = (W + MARGIN * 2.0, TOP + h + MARGIN * 2.0);
        // Right edge a little right of the icon, kept on the output.
        let surf_left = (menu.x + 36.0 - size.0 + MARGIN).clamp(0.0, (ctx.out_w - size.0).max(0.0));
        let margin_right = ctx.out_w - size.0 - surf_left;
        let home = Rect::new(MARGIN, TOP, W, HEAD + rows_h(&menu.entries) + PAD * 2.0);
        let ix = menu.x - surf_left;
        let start = Rect::new(ix - 12.0, 6.0, 24.0, 24.0);
        let chrome = Chrome::new(start, home, SpringCfg::new(0.36, 0.86), SpringCfg::new(0.42, 0.8));
        Self {
            chrome,
            text,
            icons: ctx.icons.clone(),
            scale: ctx.scale,
            size,
            margin_right,
            menu,
            path: vec![],
            slide: Spring::new(1.0, presets::SNAPPY),
            hovered: Ctl::None,
            pressed: Ctl::None,
            hover_y: Spring::new(0.0, SpringCfg::new(0.22, 0.9)),
            hover_a: Spring::new(0.0, presets::SNAPPY),
            entered: false,
            hits: vec![],
        }
    }

    fn level(&self) -> &[MenuEntry] {
        let mut v: &[MenuEntry] = &self.menu.entries;
        for &i in &self.path {
            match v.get(i) {
                Some(e) => v = &e.children,
                None => break,
            }
        }
        v
    }

    fn hit(&self, x: f32, y: f32) -> Ctl {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map_or(Ctl::None, |(_, c)| *c)
    }

    fn text_at(&self, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect) {
        if c.3 <= 0.002 || clip.w <= 0.5 {
            return;
        }
        let s = self.scale;
        let px = [(clip.x * s).floor() as i32, (clip.y * s).floor() as i32, ((clip.x + clip.w) * s).ceil() as i32, ((clip.y + clip.h) * s).ceil() as i32];
        out.texts.push(TextDraw { key, x: (x * s).round(), y: (y * s).round(), color: c, clip: px });
    }
}

impl WinScene for TrayMenu {
    fn size(&self) -> (f32, f32) {
        self.size
    }
    fn placement(&self) -> Placement {
        Placement::TopRight(self.margin_right)
    }
    fn chrome(&self) -> &Chrome {
        &self.chrome
    }
    fn chrome_mut(&mut self) -> &mut Chrome {
        &mut self.chrome
    }
    fn set_scale(&mut self, s: f32) {
        self.scale = s;
        self.text.set_scale(s);
    }
    fn on_fact(&mut self, _f: &Fact) -> bool {
        false
    }
    fn step(&mut self, dt: f32) -> bool {
        self.chrome.wake_at = None;
        let row = match self.hovered {
            Ctl::Row(i) => Some(i),
            _ => None,
        };
        match row {
            Some(i) => {
                let y = self.hits.iter().find(|(_, c)| *c == Ctl::Row(i)).map_or(0.0, |(r, _)| r.y);
                if self.hover_a.get() < 0.05 {
                    self.hover_y.snap(y);
                }
                self.hover_y.set(y);
                self.hover_a.set(1.0);
            }
            None => self.hover_a.set(0.0),
        }
        let mut m = self.chrome.step(dt);
        m |= self.slide.step(dt) | self.hover_y.step(dt) | self.hover_a.step(dt);
        m
    }
    fn draw(&mut self, fs: &mut FontSystem, out: &mut DrawList) {
        out.clear();
        self.hits.clear();
        let sc = self.scale;
        // The card follows its current level's height.
        let lv_h = HEAD + rows_h(self.level()) + if self.path.is_empty() { 0.0 } else { ROW } + PAD * 2.0;
        self.chrome.home.h = lv_h.min(MAX_H - 0.0);
        let r = self.chrome.morph();
        self.chrome.body = r;
        let open = self.chrome.openness();
        out.shapes.push(paint::ink(sc, r, 16.0f32.min(r.h * 0.5)).shadow(Rgba::hex(0x000000, 0.55), 16.0, 6.0).done());
        out.glass(r, 16.0f32.min(r.h * 0.5), 1.0);
        self.hits.push((r, Ctl::Body));
        let a = ((open - 0.55) / 0.45).clamp(0.0, 1.0);
        let clip = r.inflate(-1.5);
        if a > 0.0 {
            let title = self.menu.title.to_uppercase();
            let (k, _) = self.text.get_fit(fs, &title, F_HEAD, W - 28.0);
            let lh = self.text.line_h(F_HEAD);
            self.text_at(out, k, r.x + 14.0, r.y + PAD + HEAD * 0.5 - lh * 0.5, MUTED.fade(a), clip);

            // Rows (the level slides in from the side it came from).
            let slide = self.slide.get();
            let dx = (1.0 - slide) * 24.0;
            let ra = a * slide.clamp(0.0, 1.0);
            let mut y = r.y + PAD + HEAD;
            let hov = self.hover_a.get().clamp(0.0, 1.0);
            if hov > 0.01 {
                let hr = Rect::new(r.x + PAD, self.hover_y.get(), r.w - PAD * 2.0, ROW);
                let pressed = matches!(self.pressed, Ctl::Row(_));
                out.shapes.push(paint::rect(sc, hr, 10.0).fill(HOVER.fade(if pressed { 1.6 } else { 1.0 })).alpha(a * hov).done());
                // The iridescent accent: a short pill on the row's edge.
                let pill = Rect::new(hr.x + 3.0, hr.y + ROW * 0.5 - 7.0, 3.0, 14.0);
                out.shapes.push(paint::rect(sc, pill, 1.5).on_fill(0.0, 1.0, 0.0).alpha(a * hov).done());
            }
            if !self.path.is_empty() {
                let row = Rect::new(r.x + PAD, y, r.w - PAD * 2.0, ROW);
                self.hits.push((row, Ctl::Back));
                let (k, _) = self.text.get(fs, "‹  Back", F_ROW);
                let lh = self.text.line_h(F_ROW);
                self.text_at(out, k, row.x + 10.0 + dx, y + ROW * 0.5 - lh * 0.5, MUTED.fade(ra), clip);
                y += ROW;
            }
            let level: Vec<MenuEntry> = self.level().to_vec();
            let toggles = level.iter().any(|e| e.toggle.is_some());
            for (i, e) in level.iter().enumerate() {
                if e.separator {
                    paint::divider(&mut out.shapes, sc, r.x + 10.0, y + SEP * 0.5 - 0.5, r.w - 20.0, ra);
                    y += SEP;
                    continue;
                }
                let row = Rect::new(r.x + PAD, y, r.w - PAD * 2.0, ROW);
                if y + ROW > r.y + r.h {
                    break;
                }
                if e.enabled {
                    self.hits.push((row, Ctl::Row(i as u16)));
                }
                let col = if !e.enabled { MUTED.fade(0.6) } else { TEXT };
                let mut tx = row.x + 10.0 + dx;
                if toggles {
                    if e.toggle == Some(true) {
                        let id = self.icons.id(&["object-select-symbolic"]);
                        out.icons.push(IconDraw { id, x: tx * sc, y: (y + ROW * 0.5 - 7.0) * sc, size: (14.0 * sc).round(), color: col.fade(ra) });
                    }
                    tx += 22.0;
                }
                let (k, _) = self.text.get_fit(fs, &e.label, F_ROW, row.x + row.w - 28.0 - tx);
                let lh = self.text.line_h(F_ROW);
                self.text_at(out, k, tx, y + ROW * 0.5 - lh * 0.5, col.fade(ra), clip);
                if !e.children.is_empty() {
                    let (k, w) = self.text.get(fs, "›", F_ROW);
                    self.text_at(out, k, row.x + row.w - 12.0 - w + dx, y + ROW * 0.5 - lh * 0.5, col.fade(ra * 0.8), clip);
                }
                y += ROW;
            }
        }
        self.text.end_frame();
        let (dx, dy) = self.chrome.translate(out, self.scale);
        for (r, _) in &mut self.hits {
            r.x += dx;
            r.y += dy;
        }
    }
    fn text_buffers(&self) -> &HashMap<u64, glyphon::Buffer> {
        &self.text.bufs
    }
    fn pointer_motion(&mut self, x: f32, y: f32) -> Option<Action> {
        self.hovered = self.hit(x, y);
        // Only a deliberate visit counts: while the card is still growing
        // out of the icon it sweeps under a pointer that never moved into it.
        if matches!(self.hovered, Ctl::Row(_) | Ctl::Back) && self.chrome.openness() > 0.9 {
            self.entered = true;
        }
        None
    }
    fn pointer_leave(&mut self) {
        self.hovered = Ctl::None;
        // A menu you've moved into and out of again is done with.
        if self.entered {
            self.chrome.close();
        }
    }
    fn press(&mut self, _x: f32, _y: f32) -> Option<Action> {
        self.pressed = self.hovered;
        None
    }
    fn release(&mut self) -> (Option<Action>, bool) {
        let c = std::mem::replace(&mut self.pressed, Ctl::None);
        if c != self.hovered || self.chrome.closing {
            return (None, false);
        }
        match c {
            Ctl::Back => {
                self.path.pop();
                self.slide.snap(0.0);
                self.slide.set(1.0);
                (None, false)
            }
            Ctl::Row(i) => {
                let Some(e) = self.level().get(i as usize).cloned() else { return (None, false) };
                if !e.children.is_empty() {
                    self.path.push(i as usize);
                    self.slide.snap(0.0);
                    self.slide.set(1.0);
                    self.hover_a.snap(0.0);
                    return (None, false);
                }
                crate::tray::menu_event(&self.menu.item, e.id);
                (None, true)
            }
            _ => (None, false),
        }
    }
}

/// Snapshot demo: a fake item's menu, open and with a row hovered.
pub fn demo(d: &mut crate::snapshot::Demo) {
    use crate::tray::MenuEntry as E;
    let e = |id: i32, label: &str| E { id, label: label.into(), enabled: true, separator: false, toggle: None, children: vec![] };
    let sep = E { id: 0, label: String::new(), enabled: true, separator: true, toggle: None, children: vec![] };
    let menu = Menu {
        item: "demo".into(),
        title: "Spotify".into(),
        x: 1828.0 - 260.0,
        entries: vec![
            e(1, "Show Spotify"),
            sep.clone(),
            e(2, "Play"),
            e(3, "Next"),
            e(4, "Previous"),
            E { id: 5, label: "Shuffle".into(), enabled: true, separator: false, toggle: Some(true), children: vec![] },
            E { id: 6, label: "Devices".into(), enabled: true, separator: false, toggle: None, children: vec![e(7, "This computer"), e(8, "Living room")] },
            sep,
            E { id: 9, label: "Log out".into(), enabled: false, separator: false, toggle: None, children: vec![] },
            e(10, "Quit"),
        ],
    };
    let mut ctx = d.ctx();
    let mut m = TrayMenu::new(&mut ctx, Arc::new(menu));
    for i in 0..30 {
        if i % 5 == 0 {
            d.shoot(&mut m, &format!("traymenu_open_{i:03}"));
        }
        d.tick(&mut m, 1.0 / 120.0);
    }
    d.tick(&mut m, 0.6);
    let row = m.hits.iter().find(|(_, c)| *c == Ctl::Row(3)).map(|(r, _)| (r.x + 20.0, r.y + 10.0));
    if let Some((x, y)) = row {
        m.pointer_motion(x, y);
    }
    d.tick(&mut m, 0.4);
    d.shoot(&mut m, "traymenu");
}
