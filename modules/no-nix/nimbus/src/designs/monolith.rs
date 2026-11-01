//! Monolith and Tabs: matte black bodies whose shape follows their content.
//!
//! - `monolith`: one body floating at the top centre.
//! - `tabs`: three bodies hanging from the top edge (flat top, rounded
//!   bottom): workspaces left, the focused app centre (click: settings),
//!   status and clock right.
//!
//! Everything is always visible: workspaces, the focused app, volume,
//! network, battery and the clock. The shape morphs whenever its content
//! does: it reflows around a new title, opens a level bar beside the volume
//! icon when the volume changes or is hovered, and slides the date out on
//! clock hover.
//!
//! Transitions never scroll text or fade it through black. Content keeps
//! full brightness and is revealed by the shape itself: each segment's slot
//! opens like an iris around content drawn at its natural size, and the
//! body follows on a slightly loose spring. Text that changes in place is
//! replaced by a left-to-right wipe behind a thin accent caret.

use glyphon::FontSystem;

use crate::{
    anim::{Spring, SpringCfg, presets},
    bar::{Bar, Font, Hit, ICON, Rect},
    gfx::DrawList,
    paint,
    services::NetInfo,
    theme::{self, Rgba},
};

pub const FONTS: &[&str] = &["Geist"];

const CY: f32 = 21.0;
/// Visible height of a hanging tab; content centres on half of it.
const TAB_H: f32 = 32.0;
const TAB_MARGIN: f32 = 9.0;
const H_REST: f32 = 32.0;
const H_OPEN: f32 = 35.0;
/// Blocky-soft corners rather than a capsule.
const RADIUS: f32 = 11.0;
const PAD: f32 = 13.0;
const GAP: f32 = 13.0;
const SLOT: f32 = 16.0;
const TITLE_MAX: f32 = 380.0;
const VOL_TRACK: f32 = 70.0;

const F_CLOCK: Font = Font::new(14.0, 600).family("Geist");
const F_TITLE: Font = Font::new(13.0, 500).family("Geist");
const F_SMALL: Font = Font::new(12.0, 600).family("Geist");

const BODY_TOP: Rgba = Rgba::hex(0x0e0e11, 0.985);
const BODY_BOT: Rgba = Rgba::hex(0x09090b, 0.985);
const RIM_TOP: Rgba = Rgba::hex(0xffffff, 0.09);
const RIM_BOT: Rgba = Rgba::hex(0xffffff, 0.03);
const SHADOW: Rgba = Rgba::hex(0x000000, 0.5);
const TEXT: Rgba = Rgba::hex(0xf2f2f5, 1.0);
const SOFT: Rgba = Rgba::hex(0xc9cbd3, 1.0);
const MUTED: Rgba = Rgba::hex(0x8a8c97, 1.0);
const DOT_USED: Rgba = Rgba::hex(0xd7d9e2, 0.8);
const DOT_EMPTY: Rgba = Rgba::hex(0xffffff, 0.22);
const TRACK: Rgba = Rgba::hex(0xffffff, 0.10);
const GOOD: Rgba = Rgba::hex(0x7ee2b8, 1.0);
const WARN: Rgba = Rgba::hex(0xffd37a, 1.0);
const BAD: Rgba = Rgba::hex(0xff7b93, 1.0);

/// The body: loose enough to overshoot its content a little, no wobble.
const BODY: SpringCfg = SpringCfg::new(0.6, 0.74);
/// Segment slots: unhurried and critically damped.
const SEG: SpringCfg = SpringCfg::new(0.55, 1.0);

const PEEK_VOL: f32 = 1.8;

pub struct State {
    enter: Spring,
    w: [Spring; 3],
    open: [Spring; 3],
    tabs: bool,
    dots: Spring,
    title: Spring,
    vol: Spring,
    status: Spring,
    date: Spring,
    level: Spring,
    /// Deadline (bar clock) of the earliest live peek; read by designs::step.
    pub wake: Option<f32>,
}

impl Default for State {
    fn default() -> Self {
        let mut enter = Spring::new(0.0, SpringCfg::new(0.7, 0.75));
        enter.set(1.0);
        Self {
            enter,
            w: [Spring::new(H_REST, BODY), Spring::new(H_REST, BODY), Spring::new(H_REST, BODY)],
            open: [Spring::new(0.0, presets::SMOOTH), Spring::new(0.0, presets::SMOOTH), Spring::new(0.0, presets::SMOOTH)],
            tabs: false,
            dots: Spring::new(1.0, SEG),
            title: Spring::new(0.0, SEG),
            vol: Spring::new(0.0, SEG),
            status: Spring::new(1.0, SEG),
            date: Spring::new(0.0, SEG),
            level: Spring::new(0.0, presets::SNAPPY),
            wake: None,
        }
    }
}

fn on(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

pub fn step(bar: &Bar, st: &mut State, dt: f32) -> bool {
    // The volume level peeks open for a moment after a change.
    let now = bar.time;
    st.wake = None;
    let mut live = |at: f32, dur: f32| {
        let l = now >= at && now < at + dur;
        if l {
            st.wake = Some(st.wake.map_or(at + dur, |w: f32| w.min(at + dur)));
        }
        l
    };
    let pv = live(bar.vol_changed, PEEK_VOL);

    st.tabs = bar.design == crate::designs::Design::Tabs;
    let h = bar.hovered;
    if st.tabs {
        st.open[0].set(on(matches!(h, Hit::Spaces | Hit::Workspace(_))));
        st.open[1].set(on(h == Hit::Title));
        st.open[2].set(on(matches!(h, Hit::Volume | Hit::Status | Hit::Clock)));
    } else {
        st.open[0].set(on(h != Hit::None));
    }
    st.dots.set(1.0);
    // The centre tab is also the settings button, so it never disappears.
    st.title.set(on(bar.title.cur.is_some() || st.tabs));
    st.vol.set(on(pv || bar.hovered == Hit::Volume));
    st.status.set(1.0);
    st.date.set(on(bar.hovered == Hit::Clock));
    if let Some(v) = bar.volume {
        st.level.set(if v.muted { 0.0 } else { v.percent.min(100) as f32 / 100.0 });
    }

    let mut m = false;
    for s in st.w.iter_mut().chain(st.open.iter_mut()) {
        m |= s.step(dt);
    }
    for s in [
        &mut st.enter,
        &mut st.dots,
        &mut st.title,
        &mut st.vol,
        &mut st.status,
        &mut st.date,
        &mut st.level,
    ] {
        m |= s.step(dt);
    }
    m
}

/// One segment's on-screen slot. Content is laid out at full size centred
/// on the slot and clipped to it, so a closing slot covers rather than
/// squeezes. `a` only drops in the last sliver so nothing dims mid-way.
#[derive(Clone, Copy)]
struct Slot {
    /// Left edge of the content at its natural width.
    x: f32,
    clip: Rect,
    a: f32,
    /// The body this slot lives in, its content centre line, its hover.
    body: Rect,
    cy: f32,
    open: f32,
}

impl Slot {
    /// How much of the span [x0, x1] is inside the slot (0..1).
    fn cover(&self, x0: f32, x1: f32) -> f32 {
        let l = x0.max(self.clip.x);
        let r = x1.min(self.clip.x + self.clip.w);
        ((r - l) / (x1 - x0).max(0.01)).clamp(0.0, 1.0)
    }
    /// `r` cut to the slot's horizontal extent, if anything is left.
    fn cut(&self, r: Rect) -> Option<Rect> {
        let l = r.x.max(self.clip.x);
        let rr = (r.x + r.w).min(self.clip.x + self.clip.w);
        (rr - l > 0.3).then(|| Rect::new(l, r.y, rr - l, r.h))
    }
}

/// Wipe `prev` → `cur` left to right at progress `p`, with an accent caret
/// on the seam. Both lines are left-aligned at `x`.
#[allow(clippy::too_many_arguments)]
fn wipe(bar: &mut Bar, out: &mut DrawList, prev: Option<u64>, cur: u64, x: f32, y: f32, span: f32, p: f32, clip: Rect, color: Rgba) {
    let Some(prev) = prev.filter(|_| p < 0.999) else {
        bar.text_at(out, cur, x, y, color, clip);
        return;
    };
    let right = clip.x + clip.w;
    let seam = (x + span * p.clamp(0.0, 1.0)).clamp(clip.x, right);
    // Never hand glyphon an empty clip: it rounds outward to a 1 px strip.
    if seam - clip.x > 0.5 {
        bar.text_at(out, cur, x, y, color, Rect::new(clip.x, clip.y, seam - clip.x, clip.h));
    }
    if right - seam > 0.5 {
        bar.text_at(out, prev, x, y, color, Rect::new(seam, clip.y, right - seam, clip.h));
    }
    let glow = (4.0 * p * (1.0 - p)).clamp(0.0, 1.0) * color.3;
    if glow > 0.01 && seam > clip.x && seam < right {
        out.shapes.push(
            paint::rect(bar.scale, Rect::new(seam - 0.75, clip.y + clip.h * 0.5 - 8.0, 1.5, 16.0), 0.75)
                .gradient(theme::accent().0, theme::accent().1, 0.0, 1.0)
                .bloom(theme::accent().0.fade(0.6), 4.0)
                .alpha(glow)
                .done(),
        );
    }
}

pub fn draw(bar: &mut Bar, st: &mut State, fs: &mut FontSystem, out: &mut DrawList) {
    let sc = bar.scale;

    // ── measure every segment at full size ──────────────────────────────
    let n = bar.workspaces.len().max(1) as f32;
    let dots_w = n * SLOT;
    let xf = bar.title.xf.get().clamp(0.0, 1.0);
    let title = bar
        .title
        .cur
        .clone()
        .or_else(|| st.tabs.then(|| "Desktop".to_string()))
        .map(|t| bar.text.get_fit(fs, &t, F_TITLE, TITLE_MAX));
    let prev = bar.title.prev.clone().map(|t| bar.text.get_fit(fs, &t, F_TITLE, TITLE_MAX));
    let cur_w = title.map_or(0.0, |t| t.1);
    let title_w = match prev {
        // Width leads the wipe (ease-out) so the shape has already settled
        // around the new title by the time its text is fully revealed.
        Some((_, pw)) if xf < 1.0 && st.title.get() > 0.95 => pw + (cur_w - pw) * (1.0 - (1.0 - xf).powi(3)),
        _ => cur_w,
    };
    let vol_txt = bar.volume.map(|v| if v.muted { "Muted".to_string() } else { format!("{}%", v.percent) });
    let vol_t = vol_txt.as_deref().map(|t| bar.text.get(fs, t, F_SMALL));
    // The volume segment is always there; its level part opens on demand.
    let level_w = 8.0 + VOL_TRACK + 8.0 + vol_t.map_or(0.0, |t| t.1);
    let reveal = st.vol.get().clamp(0.0, 1.0);
    let vol_w = ICON + level_w * reveal;
    let vol_w_target = ICON + level_w * st.vol.target;
    let bat_t = bar.battery.map(|b| bar.text.get(fs, &format!("{}%", b.percent), F_SMALL));
    let status_w = ICON + 10.0 + ICON + bat_t.map_or(0.0, |t| 4.0 + t.1);
    let date_t = bar.clock.map(|c| {
        const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        bar.text.get(fs, &format!("{} {} {}", WD[c.weekday as usize % 7], c.day, MO[c.month as usize % 12]), F_TITLE)
    });
    let date_w = date_t.map_or(0.0, |t| t.1);
    // Clock: tabular digit slots so digits can cross-fade in place.
    let digit_w = ('0'..='9').map(|c| bar.text.get(fs, &c.to_string(), F_CLOCK).1).fold(0.0, f32::max);
    let colon = bar.text.get(fs, ":", F_CLOCK);
    let clock_w = digit_w * 4.0 + colon.1 + 2.0;

    // Segment order, left to right.
    let widths = [dots_w, title_w, vol_w, status_w, date_w, clock_w];
    let widths_target = [dots_w, title_w, vol_w_target, status_w, date_w, clock_w];
    let g = |sp: &Spring| (sp.get().clamp(0.0, 1.0), sp.target);
    let v = [g(&st.dots), g(&st.title), (1.0, 1.0), g(&st.status), g(&st.date), (1.0, 1.0)];
    let row = |range: std::ops::Range<usize>, use_target: bool| -> f32 {
        let mut w = 0.0;
        let mut first = true;
        for i in range {
            let (cur, tgt) = v[i];
            let k = if use_target { tgt } else { cur };
            if k <= 0.0 {
                continue;
            }
            let sw = if use_target { widths_target[i] } else { widths[i] };
            w += sw * k + if first { 0.0 } else { GAP * k };
            first = false;
        }
        w
    };

    // Groups of segments, each with its own body.
    #[derive(Clone, Copy)]
    enum Anchor {
        Left,
        Center,
        Right,
    }
    let groups: &[(std::ops::Range<usize>, Anchor, Hit)] = if st.tabs {
        &[(0..1, Anchor::Left, Hit::Spaces), (1..2, Anchor::Center, Hit::Title), (2..6, Anchor::Right, Hit::Status)]
    } else {
        &[(0..6, Anchor::Center, Hit::Panel)]
    };

    let enter = st.enter.get();
    let alpha = enter.clamp(0.0, 1.0);
    let mut slots: [Option<Slot>; 6] = [None; 6];
    for (gi, (range, anchor, hit)) in groups.iter().enumerate() {
        let target_w = row(range.clone(), true) + PAD * 2.0;
        let content_w = row(range.clone(), false);
        if st.w[gi].target != target_w {
            st.w[gi].set(target_w);
        }
        let open = st.open[gi].get().clamp(0.0, 1.2);
        let squash = (st.w[gi].vel.abs() * 0.006).min(4.0);
        // Centre tab hides while the settings panel (drawn on its own
        // surface, starting from this exact shape) is out.
        let ga = if st.tabs && gi == 1 { alpha * (1.0 - bar.popup_cover) } else { alpha };
        let (body, cy) = if st.tabs {
            // Hanging tab: the top edge sits above the screen, so only the
            // bottom corners show. Hover drops it a little lower.
            // The tab hugs its content exactly (no separate body spring):
            // an anchored edge never moves, so what's under the pointer
            // stays put while a segment opens beside it.
            let _ = squash;
            let h = TAB_H + 3.0 * open;
            let w = (content_w + PAD * 2.0).max(RADIUS * 2.0);
            let x = match anchor {
                Anchor::Left => TAB_MARGIN,
                Anchor::Center => (bar.width - w) * 0.5,
                Anchor::Right => bar.width - TAB_MARGIN - w,
            };
            let dy = -(1.0 - enter) * (TAB_H + 6.0);
            (Rect::new(x, -RADIUS - 4.0 + dy, w, h + RADIUS + 4.0), TAB_H * 0.5 + 1.5 * open + dy)
        } else {
            let h = (H_REST + (H_OPEN - H_REST) * open - squash).max(16.0) * (0.7 + 0.3 * enter.min(1.0));
            let w = st.w[gi].get().max(h);
            let cy = CY - (1.0 - enter) * 8.0;
            (Rect::new(bar.width * 0.5 - w * 0.5, cy - h * 0.5, w, h), cy)
        };
        if st.tabs {
            bar.tabs[gi] = body;
            if gi == 1 {
                bar.center_tab = body;
            }
        }
        if ga > 0.002 {
            out.shapes.push(
                paint::rect(sc, body, RADIUS.min(body.h * 0.5))
                    .gradient(BODY_TOP, BODY_BOT, 0.0, 1.0)
                    .rim(1.0, RIM_TOP, RIM_BOT)
                    .shadow(SHADOW, 10.0 + 4.0 * open, 4.0 + 2.0 * open)
                    .alpha(ga)
                    .done(),
            );
            bar.glass.push(body);
        }
        bar.hits.push((body.inflate(2.0), *hit));
        let inner = body.inflate(-1.5);

        // Slots, centred as a row and clipped to the body.
        let mut x = body.x + body.w * 0.5 - content_w * 0.5;
        let mut first = true;
        for i in range.clone() {
            let k = v[i].0;
            if k <= 0.001 {
                continue;
            }
            if !first {
                x += GAP * k;
            }
            first = false;
            let sw = widths[i] * k;
            let l = x.max(inner.x);
            let r = (x + sw).min(inner.x + inner.w);
            if r - l < 0.5 {
                x += sw;
                continue;
            }
            slots[i] = Some(Slot {
                x: x + (sw - widths[i]) * 0.5,
                clip: Rect::new(l, inner.y, (r - l).max(0.0), inner.h),
                // Thin slivers fade out instead of lingering as hairlines.
                a: ga * ((r - l) / 10.0).clamp(0.0, 1.0) * (k * 4.0).min(1.0),
                body,
                cy,
                open,
            });
            x += sw;
        }
    }
    let [dots, ttl, vol, status, date, clock] = slots;

    // Workspace row with the liquid indicator.
    if let Some(s) = dots {
        let head = bar.ws_head.get();
        let tail = bar.ws_tail.get();
        let (lo, hi) = (head.min(tail), head.max(tail));
        let ix = |i: f32| s.x + i * SLOT + SLOT * 0.5;
        let (il, ir) = (ix(lo) - 7.0, ix(hi) + 7.0);
        for (i, ws) in bar.workspaces.clone().iter().enumerate() {
            let c = ix(i as f32);
            let dist = (il - c).max(c - ir);
            let free = ((dist + 1.0) / 6.0).clamp(0.0, 1.0);
            let hov = bar.hovered == Hit::Workspace(ws.id);
            let d = (if hov { 8.0 } else { 5.0 }) * (0.4 + 0.6 * free);
            let a = s.a * free * s.cover(c - d * 0.5, c + d * 0.5);
            let col = if ws.urgent { BAD } else if ws.windows > 0 { DOT_USED } else { DOT_EMPTY };
            if a > 0.002 {
                let dot = paint::circle(sc, c, s.cy, d * 0.5);
                let dot = if ws.windows > 0 || ws.urgent { dot.fill(col) } else { dot.rim(1.2, col, col) };
                out.shapes.push(dot.alpha(a).done());
            }
            bar.hits.push((Rect::new(c - SLOT * 0.5, s.body.y, SLOT, s.body.h), Hit::Workspace(ws.id)));
        }
        let stretch = ((ir - il) - 14.0) / 14.0;
        let ih = 6.0 * (1.0 - 0.3 * stretch.clamp(0.0, 1.2));
        if let Some(r) = s.cut(Rect::new(il, s.cy - ih * 0.5, ir - il, ih)) {
            out.shapes.push(
                paint::rect(sc, r, 2.5)
                    .gradient(theme::accent().0, theme::accent().1, 1.0, 0.0)
                    // Matte: a crisp pill with a faint halo, not a glow.
                    .rim(1.0, Rgba::hex(0xffffff, 0.35), Rgba::hex(0xffffff, 0.0))
                    .bloom(theme::accent().0.mix(theme::accent().1, 0.5).fade(0.12), 2.5)
                    .alpha(s.a)
                    .done(),
            );
        }
    }

    // Title: iris on open/close, wipe when the text changes.
    if let Some(s) = ttl
        && let Some((k, _)) = title
    {
        let lh = bar.text.line_h(F_TITLE);
        let c = SOFT.mix(TEXT, s.open.min(1.0)).fade(s.a);
        // Only wipe text that was actually on screen; a slot that is still
        // opening just reveals the new title.
        let prev = prev.filter(|_| st.title.get() > 0.95);
        let span = cur_w.max(prev.map_or(0.0, |p| p.1));
        wipe(bar, out, prev.map(|p| p.0), k, s.x, s.cy - lh * 0.5, span, xf, s.clip, c);
    }

    // Volume: icon always; the level track and percentage open beside it.
    if let Some(s) = vol {
        let ia = s.a * s.cover(s.x, s.x + ICON);
        bar.icon_at(out, bar.vol_icon.cur, s.x, s.cy - ICON * 0.5, ICON, TEXT.fade(ia));
        let tx = s.x + ICON + 8.0;
        let level = st.level.get().clamp(0.0, 1.05);
        let ra = s.a * reveal;
        if ra > 0.002 {
            if let Some(r) = s.cut(Rect::new(tx, s.cy - 2.0, VOL_TRACK, 4.0)) {
                out.shapes.push(paint::rect(sc, r, 1.5).fill(TRACK).alpha(ra).done());
            }
            if let Some(r) = s.cut(Rect::new(tx, s.cy - 2.0, VOL_TRACK * level, 4.0)) {
                out.shapes.push(
                    paint::rect(sc, r, 1.5)
                        .gradient(theme::accent().0, theme::accent().1, 1.0, 0.0)
                        .bloom(theme::accent().0.fade(0.12), 2.5)
                        .alpha(ra)
                        .done(),
                );
            }
            if let Some((k, _)) = vol_t {
                let lh = bar.text.line_h(F_SMALL);
                bar.text_at(out, k, tx + VOL_TRACK + 8.0, s.cy - lh * 0.5, TEXT.fade(ra), s.clip);
            }
        }
        bar.hits.push((Rect::new(s.clip.x - 4.0, s.body.y, s.clip.w + 8.0, s.body.h), Hit::Volume));
    }

    // Network + battery.
    if let Some(s) = status {
        let net_c = if matches!(bar.net, NetInfo::Offline) { MUTED } else { TEXT };
        let na = s.a * s.cover(s.x, s.x + ICON);
        bar.icon_at(out, bar.net_icon.cur, s.x, s.cy - ICON * 0.5, ICON, net_c.fade(na));
        let bx = s.x + ICON + 10.0;
        let bc = match bar.battery {
            Some(b) if b.charging => GOOD,
            Some(b) if b.percent <= 15 => BAD,
            Some(b) if b.percent <= 30 => WARN,
            _ => TEXT,
        };
        let ba = s.a * s.cover(bx, bx + ICON);
        bar.icon_at(out, bar.bat_icon.cur, bx, s.cy - ICON * 0.5, ICON, bc.fade(ba));
        if let Some((k, _)) = bat_t {
            let lh = bar.text.line_h(F_SMALL);
            bar.text_at(out, k, bx + ICON + 4.0, s.cy - lh * 0.5, bc.fade(s.a), s.clip);
        }
        bar.hits.push((s.clip, Hit::Status));
    }

    // Date beside the clock.
    if let Some(s) = date
        && let Some((k, _)) = date_t
    {
        let lh = bar.text.line_h(F_TITLE);
        bar.text_at(out, k, s.x, s.cy - lh * 0.5, MUTED.fade(s.a), s.clip);
    }

    // Clock: changed digits cross-fade in place, both at once, so the
    // figure never dims or moves.
    if let Some(s) = clock {
        let lh = bar.text.line_h(F_CLOCK);
        let ty = s.cy - lh * 0.5;
        let mut x = s.x;
        for i in 0..4 {
            if i == 2 {
                bar.text_at(out, colon.0, x + 1.0, ty - 1.0, SOFT.fade(s.a), s.clip);
                x += colon.1 + 2.0;
            }
            let (cur, prv, p) = (bar.digits[i].cur, bar.digits[i].prev, bar.digits[i].p.get().clamp(0.0, 1.0));
            let ck = bar.text.get(fs, &cur.to_string(), F_CLOCK);
            if p < 1.0 && prv != ' ' && prv != cur {
                let pk = bar.text.get(fs, &prv.to_string(), F_CLOCK);
                bar.text_at(out, pk.0, x + (digit_w - pk.1) * 0.5, ty, TEXT.fade(s.a * (1.0 - p)), s.clip);
                bar.text_at(out, ck.0, x + (digit_w - ck.1) * 0.5, ty, TEXT.fade(s.a * p), s.clip);
            } else {
                bar.text_at(out, ck.0, x + (digit_w - ck.1) * 0.5, ty, TEXT.fade(s.a), s.clip);
            }
            x += digit_w;
        }
        bar.hits.push((Rect::new(s.x - 4.0, s.body.y, clock_w + PAD, s.body.h), Hit::Clock));
        if let Some(d) = date {
            bar.hits.push((Rect::new(d.clip.x, s.body.y, s.x - d.clip.x + clock_w + PAD, s.body.h), Hit::Clock));
        }
    }
}
