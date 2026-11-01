//! "swiss": an International Typographic Style grid strip.
//!
//!   ┃ 1 █2█ 3 4 5 ┃ WINDOW                          ┃ VOL ┃ WIFI ┃ BAT ┃▓ TIME  ▓┃
//!   ┃             ┃ NIMBUS — ~/NEWFLAKE/…            ┃ 62  ┃ 80   ┃ 43% ┃▓ 10:59 ▓┃
//!
//! A matte near-black strip cut into cells by hairline rules, built only from
//! type: tiny tracked labels over values that share one baseline, like a
//! departure board. One signal colour (Swiss red) marks the active space.
//!
//! Motion: cell widths are springs, so the rules physically slide (with a
//! little overshoot) when content changes; the red block stretches between
//! numerals and the numerals it covers flip to ink; type never scrolls or
//! fades: a new window's title is wiped in by a red hairline, clock digits
//! change behind a hairline dropping through them like a split flap, and
//! hover wipes a paper fill through a cell, inverting the type it covers.
//! Everything settles to zero frames at rest.

use glyphon::FontSystem;

use crate::{
    anim::{Spring, SpringCfg},
    bar::{Bar, Font, Hit, Rect},
    gfx::DrawList,
    paint,
    services::NetInfo,
    theme::Rgba,
};

pub const FONTS: &[&str] = &["Archivo"];
const FAM: &str = "Archivo";

// ── grid ────────────────────────────────────────────────────────────────

/// Strip height (the exclusive zone).
const H: f32 = 40.0;
/// Cell padding, left and right.
const PAD: f32 = 12.0;
/// Workspace slots: inset from the cell edge, and fixed pitch.
const WS_IN: f32 = 5.0;
const PITCH: f32 = 25.0;
/// Status cells share one module width unless their content needs more.
const MODULE: f32 = 66.0;
/// Label baseline and the shared value baseline.
const LB: f32 = 13.5;
const VB: f32 = 30.0;
/// Window tally squares over each workspace numeral.
const TALLY: f32 = 2.9;
const TALLY_GAP: f32 = 2.3;

const F_LABEL: Font = Font::new(7.0, 600).family(FAM).track(0.16);
const F_WS: Font = Font::new(15.0, 700).family(FAM);
const F_WS_EMPTY: Font = Font::new(15.0, 400).family(FAM);
const F_VAL: Font = Font::new(15.0, 600).family(FAM);
const F_UNIT: Font = Font::new(9.0, 600).family(FAM);
const F_TITLE: Font = Font::new(11.0, 600).family(FAM).track(0.1);
const F_CLOCK: Font = Font::new(18.0, 700).family(FAM);
const F_DATE: Font = Font::new(11.0, 700).family(FAM).track(0.1);
const F_WORD: Font = F_DATE;

// ── colour ──────────────────────────────────────────────────────────────

const INK: Rgba = Rgba::hex(0x0f0f10, 1.0);
const PAPER: Rgba = Rgba::hex(0xf1f0eb, 1.0);
const RED: Rgba = Rgba::hex(0xff3b1f, 1.0);
const WHITE: Rgba = Rgba::hex(0xf4f3ef, 1.0);
const GREY: Rgba = Rgba::hex(0x86868b, 1.0);
const DIM: Rgba = Rgba::hex(0x4f4f54, 1.0);
const RULE: Rgba = Rgba::hex(0x333336, 1.0);
const INK_LABEL: Rgba = Rgba::hex(0x0f0f10, 0.5);

// ── motion ──────────────────────────────────────────────────────────────

/// Cell widths: a visible overshoot so the rules land with a bounce.
const CELL: SpringCfg = SpringCfg::new(0.52, 0.62);
const WIPE_IN: SpringCfg = SpringCfg::new(0.34, 0.8);
const WIPE_OUT: SpringCfg = SpringCfg::new(0.42, 0.92);
const DROP: SpringCfg = SpringCfg::new(0.5, 0.8);
const ENTER: SpringCfg = SpringCfg::new(0.55, 0.7);
/// The ink sheet and base rule wiping across on entrance (the overshoot is
/// clamped, so the sheet hits the far edge decisively).
const SHEET: SpringCfg = SpringCfg::new(0.55, 0.72);
/// Entrance delays: rules drop once the sheet has passed them; then type.
const RULE_AT: [f32; 5] = [0.08, 0.19, 0.21, 0.23, 0.25];
const CONTENT_AT: [f32; N] = [0.16, 0.22, 0.28, 0.31, 0.34, 0.40];
/// The title wipe: quick, with a little settle at the end.
const ROLL: SpringCfg = SpringCfg::new(0.5, 0.8);
const REVEAL: SpringCfg = SpringCfg::new(0.5, 0.66);

const WS: usize = 0;
const TITLE: usize = 1;
const VOL: usize = 2;
const NET: usize = 3;
const BAT: usize = 4;
const CLOCK: usize = 5;
const N: usize = 6;

/// A fill that wipes in from the left and, on leave, wipes out to the right.
#[derive(Clone, Copy)]
struct Wipe {
    lead: Spring,
    trail: Spring,
    on: bool,
}

impl Wipe {
    fn new() -> Self {
        Self { lead: Spring::new(0.0, WIPE_IN), trail: Spring::new(0.0, WIPE_OUT), on: false }
    }
    fn set(&mut self, on: bool) {
        if on == self.on {
            return;
        }
        self.on = on;
        if on {
            self.lead.set(1.0);
            self.trail.set(0.0);
        } else {
            self.trail.set(1.0);
        }
    }
    fn step(&mut self, dt: f32) -> bool {
        let m = self.lead.step(dt) | self.trail.step(dt);
        if !self.on && !m && self.trail.pos >= 1.0 {
            self.lead.snap(0.0);
            self.trail.snap(0.0);
        }
        m
    }
    /// Covered fraction of the cell, (from, to).
    fn span(&self) -> Option<(f32, f32)> {
        let (a, b) = (self.trail.pos.clamp(0.0, 1.0), self.lead.pos.clamp(0.0, 1.0));
        (b - a > 0.002).then_some((a, b))
    }
}

/// A spring that starts towards 1 after `delay` seconds of the entrance.
#[derive(Clone, Copy)]
struct Stage {
    s: Spring,
    delay: f32,
}

impl Stage {
    fn new(delay: f32, cfg: SpringCfg) -> Self {
        Self { s: Spring::new(0.0, cfg), delay }
    }
    fn get(&self) -> f32 {
        self.s.get()
    }
}

#[derive(Default)]
struct Metrics {
    ok: bool,
    /// Content widths of the status cells (label/value, whichever is wider).
    content: [f32; N],
    /// Time column width and the date column width.
    time_w: f32,
    date_w: f32,
}

pub struct State {
    t: f32,
    entered: bool,
    bg: Stage,
    base: Stage,
    rules: [Stage; 5],
    block: Stage,
    paper: Stage,
    content: [Stage; N],

    w: [Spring; N],
    snapped: bool,
    date: Spring,
    wipes: [Wipe; N],
    ws_wipes: Vec<(u64, Wipe)>,
    title: Spring,
    seen_title: f32,

    m: Metrics,
    rects: [Rect; N],
}

impl Default for State {
    fn default() -> Self {
        Self {
            t: 0.0,
            entered: false,
            bg: Stage::new(0.03, SHEET),
            base: Stage::new(0.0, SHEET),
            rules: std::array::from_fn(|i| Stage::new(RULE_AT[i], DROP)),
            block: Stage::new(0.18, DROP),
            paper: Stage::new(0.28, DROP),
            content: std::array::from_fn(|i| Stage::new(CONTENT_AT[i], ENTER)),
            w: [Spring::new(0.0, CELL); N],
            snapped: false,
            date: Spring::new(0.0, REVEAL),
            wipes: [Wipe::new(); N],
            ws_wipes: vec![],
            title: Spring::new(1.0, ROLL),
            seen_title: -1e9,
            m: Metrics::default(),
            rects: [Rect::default(); N],
        }
    }
}

// ── step ────────────────────────────────────────────────────────────────

pub fn step(bar: &Bar, st: &mut State, dt: f32) -> bool {
    let mut m = false;

    // Entrance timeline.
    if !st.entered {
        st.t += dt;
        let t = st.t;
        let mut pending = false;
        let all = [&mut st.bg, &mut st.base, &mut st.block, &mut st.paper]
            .into_iter()
            .chain(st.rules.iter_mut())
            .chain(st.content.iter_mut());
        for sg in all {
            if t >= sg.delay {
                sg.s.set(1.0);
            } else {
                pending = true;
            }
        }
        st.entered = !pending;
        m = true;
    }
    for sg in [&mut st.bg, &mut st.base, &mut st.block, &mut st.paper]
        .into_iter()
        .chain(st.rules.iter_mut())
        .chain(st.content.iter_mut())
    {
        m |= sg.s.step(dt);
    }

    // Layout targets (from the last draw's measurements).
    if st.m.ok {
        let n = bar.workspaces.len() as f32;
        let status = |i: usize, present: bool| {
            if present { (st.m.content[i] + PAD * 2.0).max(MODULE) } else { 0.0 }
        };
        let targets = [
            WS_IN * 2.0 + n * PITCH,
            0.0,
            status(VOL, bar.volume.is_some()),
            status(NET, true),
            status(BAT, bar.battery.is_some()),
            PAD * 2.0 + st.m.time_w,
        ];
        for (sp, t) in st.w.iter_mut().zip(targets) {
            if st.snapped { sp.set(t) } else { sp.snap(t) }
        }
        st.snapped = true;
    }
    for sp in &mut st.w {
        m |= sp.step(dt);
    }

    // Hover.
    let inside = |i: usize| bar.pointer.is_some_and(|(x, y)| st.rects[i].contains(x, y));
    let h = bar.hovered;
    let hov = [
        false,
        h == Hit::Panel && inside(TITLE),
        h == Hit::Volume || bar.time < bar.vol_hide_at,
        h == Hit::Status && inside(NET),
        h == Hit::Status && inside(BAT),
        false,
    ];
    for (w, on) in st.wipes.iter_mut().zip(hov) {
        w.set(on);
        m |= w.step(dt);
    }
    st.date.set(if h == Hit::Clock { 1.0 } else { 0.0 });
    m |= st.date.step(dt);

    for ws in &bar.workspaces {
        if !st.ws_wipes.iter().any(|(id, _)| *id == ws.id) {
            st.ws_wipes.push((ws.id, Wipe::new()));
        }
    }
    st.ws_wipes.retain(|(id, w)| bar.workspaces.iter().any(|ws| ws.id == *id) || w.span().is_some());
    for (id, w) in &mut st.ws_wipes {
        w.set(h == Hit::Workspace(*id) && bar.workspaces.iter().any(|ws| ws.id == *id && !ws.active));
        m |= w.step(dt);
    }

    // Departure-board title roll.
    see_title(bar, st);
    m |= st.title.step(dt);
    m
}

/// A new window rolls the title board; the same window retitling itself
/// (a terminal spinner, a browser tab's progress) just swaps quietly.
fn see_title(bar: &Bar, st: &mut State) {
    if bar.title_changed == st.seen_title {
        return;
    }
    st.seen_title = bar.title_changed;
    // During the entrance the cell's own reveal brings the title in.
    if st.entered && (bar.focus_changed >= bar.title_changed || bar.title.prev.is_none()) {
        st.title.snap(0.0);
        st.title.set(1.0);
    } else {
        st.title.snap(1.0);
    }
}

// ── draw ────────────────────────────────────────────────────────────────

/// Distance from the top of a laid-out line to its baseline.
fn asc(bar: &Bar, key: u64) -> f32 {
    bar.text.bufs.get(&key).and_then(|b| b.layout_runs().next().map(|r| r.line_y)).unwrap_or(0.0) / bar.scale
}

/// Draw `key` with its baseline at `base`: `fg` normally, `inv` inside the
/// (sorted, disjoint) x `spans`, all within `clip`.
#[allow(clippy::too_many_arguments)]
fn ink(bar: &mut Bar, out: &mut DrawList, key: u64, x: f32, base: f32, fg: Rgba, inv: Rgba, spans: &[(f32, f32)], clip: Rect) {
    if (fg.3 <= 0.002 && inv.3 <= 0.002) || clip.w <= 0.0 || clip.h <= 0.0 {
        return;
    }
    let top = base - asc(bar, key);
    let (x0, x1) = (clip.x, clip.x + clip.w);
    let mut cur = x0;
    for &(a, b) in spans {
        let (a, b) = (a.max(x0), b.min(x1));
        if b <= a {
            continue;
        }
        if a > cur {
            bar.text_at(out, key, x, top, fg, Rect::new(cur, clip.y, a - cur, clip.h));
        }
        bar.text_at(out, key, x, top, inv, Rect::new(a, clip.y, b - a, clip.h));
        cur = cur.max(b);
    }
    if x1 > cur {
        bar.text_at(out, key, x, top, fg, Rect::new(cur, clip.y, x1 - cur, clip.h));
    }
}

fn merge(v: &mut Vec<(f32, f32)>) {
    v.retain(|(a, b)| b > a);
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(v.len());
    for &(a, b) in v.iter() {
        match out.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    *v = out;
}

/// A value set in tabular figures: digits sit on a fixed pitch, units are
/// set smaller; words ("OFF", "LAN") are set as tracked caps like the date.
/// Returns glyphs as (key, x offset) + width.
fn figures(bar: &mut Bar, fs: &mut FontSystem, s: &str, f: Font, unit: Font) -> (Vec<(u64, f32)>, f32) {
    if s.chars().any(|c| c.is_alphabetic()) {
        let (k, w) = bar.text.get(fs, s, F_WORD);
        return (vec![(k, 0.0)], w);
    }
    let pitch = ('0'..='9').map(|c| bar.text.get(fs, c.encode_utf8(&mut [0; 4]), f).1).fold(0.0, f32::max);
    let mut x = 0.0;
    let mut g = Vec::with_capacity(s.len());
    let mut buf = [0u8; 4];
    for c in s.chars() {
        if c.is_ascii_digit() {
            let (k, w) = bar.text.get(fs, c.encode_utf8(&mut buf), f);
            g.push((k, x + (pitch - w) * 0.5));
            x += pitch;
        } else {
            let (k, w) = bar.text.get(fs, c.encode_utf8(&mut buf), unit);
            g.push((k, x + 1.0));
            x += w + 1.0;
        }
    }
    (g, x)
}

struct Cell {
    label: &'static str,
    value: String,
    tone: Rgba,
}

fn status_cells(bar: &Bar) -> [Cell; 3] {
    let vol = match bar.volume {
        Some(v) if v.muted => Cell { label: "VOL", value: "OFF".into(), tone: GREY },
        Some(v) => Cell { label: "VOL", value: v.percent.to_string(), tone: WHITE },
        None => Cell { label: "VOL", value: String::new(), tone: GREY },
    };
    let net = match bar.net {
        NetInfo::Wifi(q) => Cell { label: "WIFI", value: q.to_string(), tone: WHITE },
        NetInfo::Ethernet => Cell { label: "NET", value: "LAN".into(), tone: WHITE },
        NetInfo::Offline => Cell { label: "NET", value: "OFF".into(), tone: GREY },
    };
    let bat = match bar.battery {
        Some(b) => Cell {
            label: if b.charging { "CHG" } else { "BAT" },
            value: format!("{}%", b.percent),
            tone: if b.percent <= 20 && !b.charging { RED } else { WHITE },
        },
        None => Cell { label: "BAT", value: String::new(), tone: GREY },
    };
    [vol, net, bat]
}

const WD: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const MO: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];

pub fn draw(bar: &mut Bar, st: &mut State, fs: &mut FontSystem, out: &mut DrawList) {
    let s = bar.scale;
    let width = bar.width;
    let px = |x: f32| (x * s).round() / s;
    let th = s.round().max(1.0) / s;
    let body_h = H - th;

    // ── measure ─────────────────────────────────────────────────────────
    // A title change not yet stepped: start the roll on this frame.
    see_title(bar, st);
    let cells = status_cells(bar);
    let mut glyphs: Vec<(Vec<(u64, f32)>, f32, u64, f32)> = Vec::with_capacity(3);
    for (i, c) in cells.iter().enumerate() {
        let (g, vw) = figures(bar, fs, &c.value, F_VAL, F_UNIT);
        let (lk, lw) = bar.text.get(fs, c.label, F_LABEL);
        st.m.content[VOL + i] = vw.max(lw);
        glyphs.push((g, vw, lk, lw));
    }
    let time = bar.clock.map(|c| format!("{:02}{:02}", c.hour, c.minute));
    let clock_pitch = ('0'..='9').map(|c| bar.text.get(fs, &c.to_string(), F_CLOCK).1).fold(0.0, f32::max);
    let colon = bar.text.get(fs, ":", F_CLOCK);
    let colon_w = colon.1 + 2.0;
    st.m.time_w = clock_pitch * 4.0 + colon_w;
    let date = bar.clock.map(|c| {
        let t = format!("{} {} {}", WD[c.weekday as usize % 7], c.day, MO[c.month as usize % 12]);
        bar.text.get(fs, &t, F_DATE)
    });
    let date_lab = bar.text.get(fs, "DATE", F_LABEL);
    st.m.date_w = date.map_or(0.0, |d| d.1.max(date_lab.1));
    st.m.ok = true;

    // ── layout ──────────────────────────────────────────────────────────
    let w = |i: usize| st.w[i].get().max(0.0);
    let date_x = st.date.get().max(0.0) * (st.m.date_w + PAD * 2.0);
    let clock_x = width - w(CLOCK) - date_x;
    let bat_x = clock_x - w(BAT);
    let net_x = bat_x - w(NET);
    let vol_x = net_x - w(VOL);
    let ws_w = w(WS);
    st.rects = [
        Rect::new(0.0, 0.0, ws_w, H),
        Rect::new(ws_w, 0.0, (vol_x - ws_w).max(0.0), H),
        Rect::new(vol_x, 0.0, w(VOL), H),
        Rect::new(net_x, 0.0, w(NET), H),
        Rect::new(bat_x, 0.0, w(BAT), H),
        Rect::new(clock_x, 0.0, width - clock_x, H),
    ];
    let rects = st.rects;

    let strip = Rect::new(0.0, 0.0, width, H);
    bar.glass.push(strip);
    bar.hits.push((strip, Hit::Panel));

    // ── ground, fills, rules ────────────────────────────────────────────
    let bg = st.bg.get().clamp(0.0, 1.0);
    if bg > 0.0 {
        out.shapes.push(paint::rect(s, Rect::new(0.0, 0.0, px(width * bg), body_h), 0.0).fill(INK).done());
    }

    // Clock: an inverted paper cell, dropped in from the top.
    let pd = st.paper.get().clamp(0.0, 1.0);
    let cl = px(clock_x);
    out.shapes.push(paint::rect(s, Rect::new(cl, 0.0, width - cl, body_h * pd), 0.0).fill(PAPER).done());

    // Titles fit the cell's resting width (so shaping is cached), capped.
    let rest: f32 = st.w.iter().map(|w| w.target).sum();
    let title_max = (width - rest - PAD * 2.0).min(width * 0.5).max(40.0).floor();

    // Hover wipes.
    let title_ext = {
        let cur = bar.title.cur.as_ref().map(|t| t.to_uppercase());
        let wcur = cur.map_or(0.0, |t| bar.text.get_fit(fs, &t, F_TITLE, title_max).1);
        let lw = bar.text.get(fs, "WINDOW", F_LABEL).1;
        (PAD * 2.0 + wcur.max(lw)).min(rects[TITLE].w)
    };
    let mut wipe_spans = [None; N];
    for i in [TITLE, VOL, NET, BAT] {
        let r = rects[i];
        let ext = if i == TITLE { title_ext } else { r.w };
        if let Some((a, b)) = st.wipes[i].span() {
            let (x0, x1) = (px(r.x + a * ext), px(r.x + b * ext));
            if x1 > x0 {
                out.shapes.push(paint::rect(s, Rect::new(x0, 0.0, x1 - x0, body_h), 0.0).fill(PAPER).done());
                wipe_spans[i] = Some((x0, x1));
            }
        }
    }

    // Workspace slot wipes and the red block.
    let slot_x = |i: f32| WS_IN + i * PITCH;
    let mut ws_spans: Vec<(f32, f32)> = vec![];
    for (i, ws) in bar.workspaces.iter().enumerate() {
        if let Some((_, wp)) = st.ws_wipes.iter().find(|(id, _)| *id == ws.id)
            && let Some((a, b)) = wp.span()
        {
            let x = slot_x(i as f32);
            let (x0, x1) = (px(x + a * PITCH).min(ws_w), px(x + b * PITCH).min(ws_w));
            if x1 > x0 {
                out.shapes.push(paint::rect(s, Rect::new(x0, 0.0, x1 - x0, body_h), 0.0).fill(PAPER).done());
                ws_spans.push((x0, x1));
            }
        }
    }
    let have_active = bar.workspaces.iter().any(|w| w.active);
    let bd = st.block.get().clamp(0.0, 1.0);
    if have_active && bd > 0.002 {
        let (hd, tl) = (bar.ws_head.get(), bar.ws_tail.get());
        let (l, r) = (px(slot_x(hd.min(tl))).max(0.0), px(slot_x(hd.max(tl) + 1.0)).min(ws_w));
        if r > l {
            out.shapes.push(paint::rect(s, Rect::new(l, 0.0, r - l, body_h * bd), 0.0).fill(RED).done());
            ws_spans.push((l, r));
        }
    }
    merge(&mut ws_spans);

    // Rules drop in from the top, staggered; the base rule draws across.
    let base = st.base.get().clamp(0.0, 1.0);
    out.shapes.push(paint::rect(s, Rect::new(0.0, H - th, width * base, th), 0.0).fill(RULE).done());
    for (k, x) in [ws_w, vol_x, net_x, bat_x, clock_x].into_iter().enumerate() {
        let d = st.rules[k].get().clamp(0.0, 1.0);
        if d > 0.002 {
            out.shapes.push(paint::rect(s, Rect::new(px(x - th * 0.5), 0.0, th, body_h * d), 0.0).fill(RULE).done());
        }
    }

    // ── type ────────────────────────────────────────────────────────────
    // Entrance: type is uncovered left to right by its cell, never moved.
    let shown = |i: usize, clip: Rect| {
        let e = st.content[i].get().clamp(0.0, 1.0);
        let r = rects[i];
        isect(clip, Rect::new(r.x, 0.0, r.w * e, H))
    };

    // Workspaces.
    let ws_clip = shown(WS, Rect::new(0.0, 0.0, ws_w, body_h));
    let wss = bar.workspaces.clone();
    for (i, ws) in wss.iter().enumerate() {
        let f = if ws.windows > 0 { F_WS } else { F_WS_EMPTY };
        let (k, tw) = bar.text.get(fs, &ws.idx.to_string(), f);
        let fg = if ws.urgent { RED } else if ws.windows > 0 { WHITE } else { DIM };
        let x = slot_x(i as f32) + (PITCH - tw) * 0.5;
        ink(bar, out, k, x, VB, fg, INK, &ws_spans, ws_clip);
        // Label tier: a tally of the windows on that space.
        let n = ws.windows.min(5) as f32;
        let cx = slot_x(i as f32) + PITCH * 0.5;
        for j in 0..n as usize {
            let x = px(cx - (n * TALLY + (n - 1.0) * TALLY_GAP) * 0.5 + j as f32 * (TALLY + TALLY_GAP));
            let mid = x + TALLY * 0.5;
            let covered = ws_spans.iter().any(|(a, b)| mid >= *a && mid < *b);
            let c = if covered { INK.fade(0.6) } else { GREY };
            if x + TALLY <= ws_clip.x + ws_clip.w {
                let r = Rect::new(x, px(LB - TALLY), px(TALLY), px(TALLY));
                out.shapes.push(paint::rect(s, r, 0.0).fill(c).done());
            }
        }
        bar.hits.push((Rect::new(slot_x(i as f32), 0.0, PITCH, H), Hit::Workspace(ws.id)));
    }

    // Title: label over a tracked uppercase line. A new window is wiped in
    // place by a red hairline sweeping across: new type behind it, old ahead.
    let r = rects[TITLE];
    let tclip = shown(TITLE, Rect::new(r.x, 0.0, r.w, body_h));
    let spans: Vec<(f32, f32)> = wipe_spans[TITLE].into_iter().collect();
    let lab = bar.text.get(fs, "WINDOW", F_LABEL).0;
    let tx = r.x + PAD;
    ink(bar, out, lab, tx, LB, GREY, INK_LABEL, &spans, tclip);
    let p = st.title.get().clamp(0.0, 1.0);
    let cur = bar.title.cur.clone().map(|t| bar.text.get_fit(fs, &t.to_uppercase(), F_TITLE, title_max));
    let prev = bar.title.prev.clone().filter(|_| p < 1.0).map(|t| bar.text.get_fit(fs, &t.to_uppercase(), F_TITLE, title_max));
    let ext = cur.map_or(0.0, |c| c.1).max(prev.map_or(0.0, |c| c.1)) + 8.0;
    let front = if p < 1.0 { px(tx - 4.0 + p * ext) } else { r.x + r.w };
    if let Some((k, _)) = prev {
        let c = isect(tclip, Rect::new(front, 0.0, r.x + r.w - front, body_h));
        ink(bar, out, k, tx, VB, WHITE, INK, &spans, c);
    }
    if let Some((k, _)) = cur {
        let c = isect(tclip, Rect::new(r.x, 0.0, front - r.x, body_h));
        ink(bar, out, k, tx, VB, WHITE, INK, &spans, c);
    }
    let bw = (th * 2.0 * ((1.0 - p) * 6.0).min(1.0) * s).round() / s;
    if p < 1.0 && bw > 0.0 {
        out.shapes.push(paint::rect(s, Rect::new(front - bw, 0.0, bw, body_h), 0.0).fill(RED).done());
    }

    // Status: label over value, flush left.
    for (i, (c, (g, _vw, lk, _lw))) in cells.iter().zip(glyphs).enumerate() {
        let ci = VOL + i;
        let r = rects[ci];
        if r.w < 1.0 {
            continue;
        }
        let clip = shown(ci, Rect::new(r.x, 0.0, r.w, body_h));
        let spans: Vec<(f32, f32)> = wipe_spans[ci].into_iter().collect();
        let x = r.x + PAD;
        ink(bar, out, lk, x, LB, GREY, INK_LABEL, &spans, clip);
        let inv = if c.tone == RED { RED } else { INK };
        for (k, dx) in g {
            ink(bar, out, k, x + dx, VB, c.tone, inv, &spans, clip);
        }
        let hit = if ci == VOL { Hit::Volume } else { Hit::Status };
        bar.hits.push((r, hit));
    }

    // Clock: the inverted cell. The date column is uncovered by the rule
    // sliding left on hover; digits change behind a red hairline that drops
    // through each figure like a split flap (the type itself never moves).
    let r = rects[CLOCK];
    bar.hits.push((r, Hit::Clock));
    let cell = shown(CLOCK, Rect::new(px(r.x), 0.0, width - px(r.x), body_h * pd));
    let time_x = width - PAD - st.m.time_w;
    let tl = bar.text.get(fs, "TIME", F_LABEL).0;
    ink(bar, out, tl, time_x, LB, INK_LABEL, INK_LABEL, &[], cell);

    let dr = st.date.get();
    if let Some((dk, _)) = date
        && dr > 0.002
    {
        let dx = time_x - PAD * 2.0 - st.m.date_w;
        ink(bar, out, date_lab.0, dx, LB, INK_LABEL, INK, &[], cell);
        ink(bar, out, dk, dx, VB, INK, INK, &[], cell);
        // Inner rule between date and time.
        let rh = body_h * dr.clamp(0.0, 1.0);
        let rx = px(time_x - PAD - th * 0.5);
        out.shapes.push(paint::rect(s, Rect::new(rx, 0.0, th, rh), 0.0).fill(INK.fade(0.16)).done());
    }

    if time.is_some() {
        let (top, bot) = (VB - 15.0, VB + 3.0);
        let mut x = time_x;
        for i in 0..4 {
            if i == 2 {
                ink(bar, out, colon.0, x + 1.0, VB - 1.0, RED, RED, &[], cell);
                x += colon_w;
            }
            let (cur, prev, p) = (bar.digits[i].cur, bar.digits[i].prev, bar.digits[i].p.get().clamp(0.0, 1.0));
            let edge = if p < 1.0 && prev != ' ' { px(top + p * (bot - top)) } else { body_h };
            if edge < body_h {
                let (k, dw) = bar.text.get(fs, &prev.to_string(), F_CLOCK);
                let c = isect(cell, Rect::new(x, edge, clock_pitch, body_h - edge));
                ink(bar, out, k, x + (clock_pitch - dw) * 0.5, VB, INK, INK, &[], c);
                // The flap's edge, shown only mid-flight.
                let fl = isect(cell, Rect::new(x, edge, clock_pitch, th));
                if fl.w > 0.0 && fl.h > 0.0 && p > 0.03 && p < 0.95 {
                    out.shapes.push(paint::rect(s, fl, 0.0).fill(RED).done());
                }
            }
            let (k, dw) = bar.text.get(fs, &cur.to_string(), F_CLOCK);
            let c = isect(cell, Rect::new(x, 0.0, clock_pitch, edge));
            ink(bar, out, k, x + (clock_pitch - dw) * 0.5, VB, INK, INK, &[], c);
            x += clock_pitch;
        }
    }
}

fn isect(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}
