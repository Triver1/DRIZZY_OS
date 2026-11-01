//! "braun": a warm off-white matte faceplate in the spirit of 1960s German
//! hardware (T3 pocket radio, SK4). Every readout is a physical control:
//!
//!   ● nimbus | ○ ○ ◉ ○ ○        WORKSPACE 4 / title        NET ··· BAT ▭  (knob)  SAT 25.10  11:00
//!
//! - workspaces are recessed grille holes; the active one holds an orange
//!   ball that rolls (stretching, squashing) to the next hole
//! - volume is a rotary knob with a printed scale; the indicator swings on
//!   a spring
//! - battery is a recessed slot with a fill, network three LED lenses
//! - large odometer numerals; hovering reveals the date beside them
//!
//! Nothing loops: every value is a spring and `step` goes quiet at rest.

use glyphon::FontSystem;

use crate::{
    anim::{ColorSpring, Spring, SpringCfg, presets},
    bar::{Bar, Font, Hit, Rect},
    gfx::{DrawList, TextDraw},
    paint,
    services::NetInfo,
    theme::Rgba,
};

pub const FONTS: &[&str] = &["Archivo"];

const FAM: &str = "Archivo";

// ── geometry (logical px) ───────────────────────────────────────────────
const PX: f32 = 6.0; // plate side margin
const PY: f32 = 5.0; // plate top
const PH: f32 = 32.0; // plate height
const CY: f32 = PY + PH * 0.5; // centre line of the controls
const PR: f32 = 11.0; // plate corner radius: soft, not a capsule

const PITCH: f32 = 20.0; // grille hole spacing
const HOLE_R: f32 = 6.0;
const BALL_R: f32 = 4.9;

const KNOB_R: f32 = 10.5;
const SCALE_R: f32 = KNOB_R + 4.0;
const SWEEP: f32 = 135.0; // degrees either side of top

// Two printed rows for small controls: a caption above, the control below.
const ROW_LABEL: f32 = PY + 5.0;
const ROW_CTRL: f32 = PY + 21.5;

// ── type ────────────────────────────────────────────────────────────────
const F_MARK: Font = Font::new(13.0, 700).family(FAM).track(-0.01);
const F_LABEL: Font = Font::new(6.6, 700).family(FAM).track(0.16);
const F_TITLE: Font = Font::new(12.5, 500).family(FAM);
const F_CLOCK: Font = Font::new(21.0, 600).family(FAM).track(-0.01);
const F_SMALL: Font = Font::new(9.5, 600).family(FAM).track(0.02);
const F_DATE: Font = Font::new(11.0, 600).family(FAM).track(0.01);

// ── palette ─────────────────────────────────────────────────────────────
const PLATE_TOP: Rgba = Rgba::hex(0xf1ede6, 1.0);
const PLATE_BOT: Rgba = Rgba::hex(0xe0dbd1, 1.0);
const INK: Rgba = Rgba::hex(0x2c2a27, 1.0);
const INK_SOFT: Rgba = Rgba::hex(0x5d5850, 1.0);
const PRINT: Rgba = Rgba::hex(0x948d82, 1.0);
const ORANGE: Rgba = Rgba::hex(0xff5a1f, 1.0);
const ORANGE_HI: Rgba = Rgba::hex(0xff8650, 1.0);
const ORANGE_LO: Rgba = Rgba::hex(0xdb4310, 1.0);
const GREEN: Rgba = Rgba::hex(0x52c93b, 1.0);
const GREEN_HI: Rgba = Rgba::hex(0x9af27d, 1.0);
const HILITE: Rgba = Rgba::hex(0xffffff, 1.0);
const SHADE: Rgba = Rgba::hex(0x3b3225, 1.0);

// ── motion ──────────────────────────────────────────────────────────────
const PLATE_IN: SpringCfg = SpringCfg::new(0.62, 0.66);
const POP: SpringCfg = SpringCfg::new(0.42, 0.52);
const KNOB_SWING: SpringCfg = SpringCfg::new(0.55, 0.42);
const LIFT: SpringCfg = SpringCfg::new(0.26, 0.6);
const LED: SpringCfg = SpringCfg::new(0.22, 1.0);
const FILL: SpringCfg = SpringCfg::new(0.7, 0.78);
const REVEAL: SpringCfg = SpringCfg::new(0.48, 0.74);
/// Entrance choreography runs on its own clock for this long.
const ENTER_T: f32 = 1.0;

struct Hole {
    id: u64,
    gone: bool,
    x: Spring,
    pop: Spring,
    hover: Spring,
    /// 0 = shallow dimple (empty), 1 = deep through-hole (has windows).
    deep: Spring,
}

pub struct State {
    t: f32,
    plate: Spring,
    mark: Spring,
    text_in: Spring,
    ball: Spring,
    lean: Spring,
    knob_pop: Spring,
    knob_ang: Spring,
    knob_lift: Spring,
    knob_press: Spring,
    swept: bool,
    bat_pop: Spring,
    bat_fill: Spring,
    bat_col: ColorSpring,
    leds: [Spring; 3],
    clock_rev: Spring,
    holes: Vec<Hole>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            t: 0.0,
            plate: Spring::new(0.0, PLATE_IN),
            mark: Spring::new(0.0, POP),
            text_in: Spring::new(0.0, presets::SMOOTH),
            ball: Spring::new(0.0, POP),
            lean: Spring::new(0.0, SpringCfg::new(0.34, 0.45)),
            knob_pop: Spring::new(0.0, POP),
            knob_ang: Spring::new(-SWEEP, KNOB_SWING),
            knob_lift: Spring::new(0.0, LIFT),
            knob_press: Spring::new(0.0, presets::SNAPPY),
            swept: false,
            bat_pop: Spring::new(0.0, POP),
            bat_fill: Spring::new(0.0, FILL),
            bat_col: ColorSpring::new(INK_SOFT, presets::SMOOTH),
            leds: [Spring::new(0.0, LED), Spring::new(0.0, LED), Spring::new(0.0, LED)],
            clock_rev: Spring::new(0.0, REVEAL),
            holes: vec![],
        }
    }
}

fn on(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

pub fn step(bar: &Bar, st: &mut State, dt: f32) -> bool {
    let mut m = false;
    if st.t < ENTER_T {
        st.t += dt;
        m = true;
    }
    let t = st.t;
    let after = |d: f32| t >= d;

    st.plate.set(1.0);
    st.mark.set(on(after(0.10)));
    st.text_in.set(on(after(0.22)));

    // Holes: sync with the workspace list, staggered pop on entrance.
    for (i, w) in bar.workspaces.iter().enumerate() {
        let idx = match st.holes.iter().position(|h| h.id == w.id && !h.gone) {
            Some(k) => k,
            None => {
                let x = i as f32 * PITCH;
                st.holes.push(Hole {
                    id: w.id,
                    gone: false,
                    x: Spring::new(x, presets::SMOOTH),
                    pop: Spring::new(0.0, POP),
                    hover: Spring::new(0.0, LIFT),
                    deep: Spring::new(on(w.windows > 0), presets::SMOOTH),
                });
                st.holes.len() - 1
            }
        };
        let h = &mut st.holes[idx];
        h.x.set(i as f32 * PITCH);
        h.pop.set(on(after(0.12 + i as f32 * 0.045)));
        h.hover.set(on(bar.hovered == Hit::Workspace(w.id)));
        h.deep.set(on(w.windows > 0 || w.urgent));
    }
    for h in &mut st.holes {
        if !bar.workspaces.iter().any(|w| w.id == h.id) {
            h.gone = true;
            h.pop.set(0.0);
            h.hover.set(0.0);
        }
    }
    // Keep list order = workspace order (gone ones sort by their x).
    st.holes.sort_by(|a, b| a.x.target.partial_cmp(&b.x.target).unwrap_or(std::cmp::Ordering::Equal));
    for h in &mut st.holes {
        m |= h.x.step(dt) | h.pop.step(dt) | h.hover.step(dt) | h.deep.step(dt);
    }
    st.holes.retain(|h| !(h.gone && h.pop.at_rest()));

    st.ball.set(on(after(0.34) && bar.workspaces.iter().any(|w| w.active)));
    // The ball leans toward a hovered neighbour, curious but staying put.
    let ai = bar.workspaces.iter().position(|w| w.active);
    let hi = bar.workspaces.iter().position(|w| bar.hovered == Hit::Workspace(w.id));
    st.lean.set(match (ai, hi) {
        (Some(a), Some(h)) if a != h => (h as f32 - a as f32).signum(),
        _ => 0.0,
    });

    // Knob: pops in, then sweeps from the end stop to the current level.
    st.knob_pop.set(on(after(0.18)));
    let vol_ang = bar.volume.map_or(-SWEEP, |v| -SWEEP + 2.0 * SWEEP * (v.percent.min(100) as f32 / 100.0));
    if after(0.36) {
        st.swept = true;
    }
    st.knob_ang.set(if st.swept { vol_ang } else { -SWEEP });
    st.knob_lift.set(on(bar.hovered == Hit::Volume));
    st.knob_press.set(on(bar.hovered == Hit::Volume && bar.pressed == Hit::Volume));

    // Battery.
    st.bat_pop.set(on(after(0.26)));
    let (fill, col) = match bar.battery {
        Some(b) => {
            let c = if b.charging {
                GREEN
            } else if b.percent <= 20 {
                ORANGE
            } else {
                INK_SOFT
            };
            (b.percent.min(100) as f32 / 100.0, c)
        }
        None => (0.0, INK_SOFT),
    };
    st.bat_fill.set(if after(0.40) { fill } else { 0.0 });
    st.bat_col.set(col);

    // Network LEDs light one after another.
    let lit = match bar.net {
        NetInfo::Offline => 0,
        NetInfo::Ethernet => 3,
        NetInfo::Wifi(q) => match q {
            0..=33 => 1,
            34..=66 => 2,
            _ => 3,
        },
    };
    for (i, l) in st.leds.iter_mut().enumerate() {
        l.set(on(i < lit && after(0.42 + i as f32 * 0.07)));
    }

    st.clock_rev.set(on(bar.hovered == Hit::Clock));

    m |= st.plate.step(dt) | st.mark.step(dt) | st.text_in.step(dt) | st.ball.step(dt) | st.lean.step(dt);
    m |= st.knob_pop.step(dt) | st.knob_ang.step(dt) | st.knob_lift.step(dt) | st.knob_press.step(dt);
    m |= st.bat_pop.step(dt) | st.bat_fill.step(dt) | st.bat_col.step(dt);
    for l in &mut st.leds {
        m |= l.step(dt);
    }
    m |= st.clock_rev.step(dt);
    m
}

// ── drawing helpers ─────────────────────────────────────────────────────

/// Text printed on the plate: ink plus a faint highlight just below it, as
/// if pad-printed into a very slightly textured surface.
fn print(bar: &Bar, out: &mut DrawList, key: u64, x: f32, y: f32, c: Rgba, clip: Rect, emboss: f32) {
    let s = bar.scale;
    let (px, py) = ((x * s).round(), (y * s).round());
    if emboss > 0.0 {
        out.texts.push(TextDraw { key, x: px, y: py + 1.0, color: HILITE.fade(emboss * c.3), clip: bar.clip(clip.inflate(1.0)) });
    }
    out.texts.push(TextDraw { key, x: px, y: py, color: c, clip: bar.clip(clip) });
}

/// A recessed circular hole: shaded wall at the top, lit lip at the bottom.
fn hole(out: &mut DrawList, s: f32, cx: f32, cy: f32, r: f32, deep: f32, a: f32) {
    let top = Rgba::hex(0xbfb8ad, 1.0).mix(Rgba::hex(0x3f3b35, 1.0), deep);
    let bot = Rgba::hex(0xe4dfd6, 1.0).mix(Rgba::hex(0x7d776d, 1.0), deep);
    // Lit lip just outside the bottom edge.
    out.shapes.push(paint::circle(s, cx, cy + 0.6, r + 0.3).fill(HILITE.fade(0.85 * a)).done());
    out.shapes.push(
        paint::circle(s, cx, cy, r)
            .gradient(top.fade(a), bot.fade(a), 0.0, 1.0)
            .rim(1.0, SHADE.fade((0.45 + 0.3 * deep) * a), HILITE.fade(0.35 * a))
            .inner_glow(0.55)
            .done(),
    );
}

/// An engraved vertical seam.
fn seam(out: &mut DrawList, s: f32, x: f32, y: f32, h: f32, a: f32) {
    out.shapes.push(
        paint::rect(s, Rect::new(x, y, 1.0, h), 0.5)
            .gradient(SHADE.fade(0.0), SHADE.fade(0.22 * a), 0.0, 1.0)
            .done(),
    );
    out.shapes.push(
        paint::rect(s, Rect::new(x + 1.0, y, 1.0, h), 0.5)
            .gradient(HILITE.fade(0.2 * a), HILITE.fade(0.9 * a), 0.0, 1.0)
            .done(),
    );
}

fn polar(cx: f32, cy: f32, r: f32, deg: f32) -> (f32, f32) {
    let a = deg.to_radians();
    (cx + a.sin() * r, cy - a.cos() * r)
}

pub fn draw(bar: &mut Bar, st: &mut State, fs: &mut FontSystem, out: &mut DrawList) {
    let s = bar.scale;
    let w = bar.width;
    let e = st.plate.get();
    let dy = -(1.0 - e) * (PH + PY + 10.0);
    let cy = CY + dy;
    let ta = st.text_in.get().clamp(0.0, 1.0);

    // ── faceplate ───────────────────────────────────────────────────────
    let plate = Rect::new(PX, PY + dy, w - 2.0 * PX, PH);
    bar.glass.push(plate);
    bar.hits.push((plate, Hit::Panel));
    // Long soft cast shadow, then a tight contact shadow.
    out.shapes.push(paint::rect(s, plate, PR).fill(Rgba::TRANSPARENT).shadow(SHADE.fade(0.30), 4.0, 2.6).done());
    out.shapes.push(
        paint::rect(s, plate, PR)
            .gradient(PLATE_TOP, PLATE_BOT, 0.0, 1.0)
            .rim(1.0, HILITE.fade(0.95), SHADE.fade(0.28))
            .shadow(SHADE.fade(0.32), 0.9, 0.8)
            .done(),
    );
    // A second, inner bevel line: the plate's chamfer catching light.
    out.shapes.push(
        paint::rect(s, plate.inflate(-1.0), PR - 1.0)
            .fill(Rgba::TRANSPARENT)
            .rim(1.0, HILITE.fade(0.45), HILITE.fade(0.0))
            .done(),
    );

    // ── left: power lamp, wordmark, grille ──────────────────────────────
    let mk = st.mark.get();
    let mut x = plate.x + 16.0;
    {
        let k = mk.clamp(0.0, 1.3);
        let r = 2.4 * k;
        if r > 0.05 {
            out.shapes.push(
                paint::circle(s, x + 2.4, cy, r + 0.9)
                    .gradient(SHADE.fade(0.30), HILITE.fade(0.9), 0.0, 1.0)
                    .done(),
            );
            out.shapes.push(
                paint::circle(s, x + 2.4, cy, r)
                    .gradient(GREEN_HI, GREEN, 0.0, 1.0)
                    .bloom(GREEN.fade(0.45 * mk.clamp(0.0, 1.0)), 2.6)
                    .done(),
            );
        }
        x += 4.8 + 8.0;
        let (k, mw) = bar.text.get(fs, "nimbus", F_MARK);
        let lh = bar.text.line_h(F_MARK);
        let ma = mk.clamp(0.0, 1.0);
        print(bar, out, k, x, cy - lh * 0.5 - 0.5 + (1.0 - ma) * 4.0, INK.fade(ma), plate, 0.6);
        x += mw + 16.0;
    }
    seam(out, s, x, cy - 10.0, 20.0, e.clamp(0.0, 1.0));
    x += 2.0 + 16.0;

    // Grille.
    let gx = x + HOLE_R;
    let ws_hits: Vec<(u64, f32)> = st.holes.iter().filter(|h| !h.gone).map(|h| (h.id, gx + h.x.get())).collect();
    for h in &st.holes {
        let p = h.pop.get().max(0.0);
        if p < 0.01 {
            continue;
        }
        let hv = h.hover.get();
        let r = HOLE_R * p.min(1.25) + 0.6 * hv;
        let deep = (h.deep.get() + 0.35 * hv).clamp(0.0, 1.0);
        hole(out, s, gx + h.x.get(), cy, r, deep, p.clamp(0.0, 1.0));
    }
    for (id, hx) in &ws_hits {
        bar.hits.push((Rect::new(hx - PITCH * 0.5, plate.y, PITCH, PH), Hit::Workspace(*id)));
    }
    let n_live = ws_hits.len();

    // The ball: head snaps, tail trails; span between them stretches it.
    let ba = st.ball.get();
    if ba > 0.01 && n_live > 0 {
        let (hd, tl) = (bar.ws_head.get(), bar.ws_tail.get());
        let (lo, hi) = (hd.min(tl), hd.max(tl));
        let span = (hi - lo).min(3.0);
        let k = ba.max(0.0);
        // Squash across the travel direction (keeps the area roughly fixed).
        let r = BALL_R * k.min(1.3) / (1.0 + 0.22 * span.min(1.5));
        let ln = st.lean.get() * 1.4;
        let x0 = gx + lo * PITCH + ln;
        let x1 = gx + hi * PITCH + ln;
        // While rolling it rides up out of the hole a touch.
        let ride = 0.9 * (span * 2.0).min(1.0);
        let by = cy - ride;
        let body = Rect::new(x0 - r, by - r, (x1 - x0) + 2.0 * r, 2.0 * r);
        out.shapes.push(
            paint::rect(s, body, r)
                .gradient(ORANGE_HI, ORANGE_LO, 0.0, 1.0)
                .rim(0.9, Rgba::hex(0xffc4a3, 0.75), Rgba::hex(0x8a2400, 0.55))
                .shadow(Rgba::hex(0x3a1500, 0.55), 1.1 + ride * 0.8, 1.1 + ride)
                .done(),
        );
        // Soft specular from the overhead light, leading the travel.
        let hh = r * 0.5;
        let hl = Rect::new(x0 - r * 0.62, by - r * 0.72, (x1 - x0) + r * 0.95, hh);
        out.shapes.push(
            paint::rect(s, hl, hh * 0.5)
                .gradient(HILITE.fade(0.42 * k.min(1.0)), HILITE.fade(0.0), 0.0, 1.0)
                .done(),
        );
    }

    // ── right: clock, date, knob, battery, network ──────────────────────
    let right = plate.x + plate.w - 18.0;

    // Clock numerals.
    let digit_w = ('0'..='9').map(|c| bar.text.get(fs, &c.to_string(), F_CLOCK).1).fold(0.0, f32::max);
    let colon = bar.text.get(fs, ":", F_CLOCK);
    let clock_w = digit_w * 4.0 + colon.1 + 1.0;
    let clock_x = right - clock_w;
    let lh = bar.text.line_h(F_CLOCK);
    let ty = cy - lh * 0.5 - 0.5;
    let clip = Rect::new(clock_x - 2.0, cy - 9.0, clock_w + 4.0, 18.5);
    let roll = 15.0;
    let mut dx = clock_x;
    for i in 0..4 {
        if i == 2 {
            print(bar, out, colon.0, dx + 0.5, ty - 1.0, ORANGE.fade(ta), clip, 0.0);
            dx += colon.1 + 1.0;
        }
        let d = &bar.digits[i];
        let p = d.p.get();
        let (cur, prev) = (d.cur, d.prev);
        let ck = bar.text.get(fs, &cur.to_string(), F_CLOCK);
        let col = INK.fade(ta);
        if p < 1.0 && prev != ' ' {
            let pk = bar.text.get(fs, &prev.to_string(), F_CLOCK);
            let fade = (1.0 - p * 1.4).clamp(0.0, 1.0);
            print(bar, out, pk.0, dx + (digit_w - pk.1) * 0.5, ty - p * roll, col.fade(fade), clip, 0.5);
        }
        let fade = ((p - 0.15) / 0.85).clamp(0.0, 1.0);
        print(bar, out, ck.0, dx + (digit_w - ck.1) * 0.5, ty + (1.0 - p) * roll, col.fade(fade), clip, 0.5);
        dx += digit_w;
    }

    // Date: reveals to the left of the numerals on hover, two printed rows.
    let rv = st.clock_rev.get();
    let (wd, dd) = match bar.clock {
        Some(c) => {
            const WD: [&str; 7] = ["SUNDAY", "MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY"];
            const MO: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
            (WD[c.weekday as usize % 7].to_string(), format!("{} {}", c.day, MO[c.month as usize % 12]))
        }
        None => (String::new(), String::new()),
    };
    let wk = bar.text.get(fs, &wd, F_LABEL);
    let dk = bar.text.get(fs, &dd, F_DATE);
    let date_full = wk.1.max(dk.1) + 16.0;
    let date_w = date_full * rv.max(0.0);
    let date_x = clock_x - date_w;
    if rv > 0.005 {
        let clip = Rect::new(date_x, plate.y, date_w.max(0.0), PH);
        let r1 = ((rv - 0.0) / 0.8).clamp(0.0, 1.0);
        let r2 = ((rv - 0.15) / 0.85).clamp(0.0, 1.0);
        let lx = clock_x - 14.0;
        print(bar, out, wk.0, lx - wk.1, ROW_LABEL + dy + (1.0 - r1) * 6.0, ORANGE.fade(r1 * ta), clip, 0.0);
        let dlh = bar.text.line_h(F_DATE);
        print(bar, out, dk.0, lx - dk.1, cy + 2.0 - dlh * 0.5 + 5.0 + (1.0 - r2) * 8.0 - 3.0, INK_SOFT.fade(r2 * ta), clip, 0.5);
    }
    bar.hits.push((Rect::new(date_x - 6.0, plate.y, right + 12.0 - (date_x - 6.0), PH), Hit::Clock));

    let mut rx = date_x - 18.0;
    seam(out, s, rx - 2.0, cy - 10.0, 20.0, e.clamp(0.0, 1.0));
    rx -= 2.0 + 20.0;

    // Volume knob.
    let kcx = rx - SCALE_R;
    let kp = st.knob_pop.get().max(0.0);
    let kl = st.knob_lift.get() - 1.4 * st.knob_press.get();
    let ang = st.knob_ang.get();
    let muted = bar.volume.is_none_or(|v| v.muted);
    if kp > 0.01 {
        let ka = kp.clamp(0.0, 1.0);
        // Printed scale: 11 dots, inked up to the current level.
        for i in 0..=10 {
            let da = -SWEEP + 2.0 * SWEEP * i as f32 / 10.0;
            let (tx, ty) = polar(kcx, cy, SCALE_R, da);
            let inked = da <= ang + 0.5 && !muted;
            let big = i == 0 || i == 10;
            let c = if inked { INK_SOFT } else { Rgba::hex(0xb7b0a4, 1.0) };
            let r = if big { 1.15 } else { 0.85 };
            let pop = ((kp - i as f32 * 0.03) / 0.7).clamp(0.0, 1.0);
            out.shapes.push(paint::circle(s, tx, ty, r * pop).fill(c.fade(ka)).done());
        }
        let r = KNOB_R * kp.min(1.2) + 0.5 * kl.max(0.0);
        let ky = cy - 0.7 * kl;
        // Contact well the knob sits in.
        out.shapes.push(
            paint::circle(s, kcx, cy, KNOB_R + 1.2)
                .gradient(SHADE.fade(0.22 * ka), HILITE.fade(0.8 * ka), 0.0, 1.0)
                .done(),
        );
        // Knob body: lit from above, soft shadow that grows as it lifts.
        out.shapes.push(
            paint::circle(s, kcx, ky, r)
                .gradient(Rgba::hex(0xfaf8f4, ka), Rgba::hex(0xcac3b8, ka), 0.0, 1.0)
                .rim(1.0, HILITE.fade(ka), SHADE.fade(0.35 * ka))
                .shadow(SHADE.fade((0.42 + 0.12 * kl) * ka), 1.8 + 1.6 * kl.max(0.0), 1.4 + 1.6 * kl.max(0.0))
                .done(),
        );
        // Concave dish.
        let dr = r - 2.6;
        if dr > 0.5 {
            out.shapes.push(
                paint::circle(s, kcx, ky, dr)
                    .gradient(Rgba::hex(0xd3ccc1, ka), Rgba::hex(0xf6f3ee, ka), 0.0, 1.0)
                    .rim(0.8, SHADE.fade(0.22 * ka), HILITE.fade(0.85 * ka))
                    .done(),
            );
        }
        // Indicator: an inset orange dot on the dish edge.
        let (ix, iy) = polar(kcx, ky, (r - 5.4).max(0.0), ang);
        let ic = if muted { Rgba::hex(0xa59e93, 1.0) } else { ORANGE };
        out.shapes.push(
            paint::circle(s, ix, iy, 1.75 * kp.min(1.0))
                .gradient(ic.mix(HILITE, 0.12).fade(ka), ic.mix(SHADE, 0.12).fade(ka), 0.0, 1.0)
                .rim(0.6, SHADE.fade(0.35 * ka), HILITE.fade(0.3 * ka))
                .done(),
        );
    }
    bar.hits.push((Rect::new(kcx - SCALE_R - 3.0, plate.y, 2.0 * SCALE_R + 6.0, PH), Hit::Volume));
    rx = kcx - SCALE_R - 22.0;

    // Battery: caption + recessed slot with fill + figure.
    let bp = st.bat_pop.get().max(0.0);
    let ba = bp.clamp(0.0, 1.0);
    let pct = bar.battery.map(|b| format!("{}", b.percent)).unwrap_or_else(|| "--".into());
    let pk = bar.text.get(fs, &pct, F_SMALL);
    let (tw, th) = (30.0, 8.0);
    // Slot corners: rounded rectangle, not a pill (matches the plate).
    let tr = 2.6;
    let bat_w = tw + 2.5 + 5.0 + pk.1;
    let bx = rx - bat_w;
    {
        let lab = bar.text.get(fs, if bar.battery.is_some_and(|b| b.charging) { "CHARGE" } else { "BATTERY" }, F_LABEL);
        print(bar, out, lab.0, bx, ROW_LABEL + dy, PRINT.fade(ta), plate, 0.5);
        let sy = ROW_CTRL + dy - th * 0.5;
        let tw_p = tw * bp.min(1.0);
        let track = Rect::new(bx, sy, tw_p, th);
        out.shapes.push(paint::rect(s, Rect::new(bx, sy + 0.6, tw_p, th), tr).fill(HILITE.fade(0.85 * ba)).done());
        out.shapes.push(
            paint::rect(s, track, tr)
                .gradient(Rgba::hex(0xb3aca0, ba), Rgba::hex(0xdcd6cc, ba), 0.0, 1.0)
                .rim(1.0, SHADE.fade(0.45 * ba), HILITE.fade(0.3 * ba))
                .inner_glow(0.5)
                .done(),
        );
        // Terminal nub.
        out.shapes.push(
            paint::rect(s, Rect::new(bx + tw_p + 1.2, sy + 2.2, 1.8, th - 4.4), 0.6)
                .gradient(SHADE.fade(0.45 * ba), SHADE.fade(0.25 * ba), 0.0, 1.0)
                .done(),
        );
        let f = st.bat_fill.get().clamp(0.0, 1.0);
        let fw = (tw_p - 3.0) * f;
        if fw > 0.3 {
            let c = st.bat_col.get();
            out.shapes.push(
                paint::rect(s, Rect::new(bx + 1.5, sy + 1.5, fw.max(3.0), th - 3.0), tr - 1.2)
                    .gradient(c.mix(HILITE, 0.18).fade(ba), c.mix(SHADE, 0.1).fade(ba), 0.0, 1.0)
                    .alpha(((fw - 0.3) / 2.0).clamp(0.0, 1.0))
                    .done(),
            );
        }
        let slh = bar.text.line_h(F_SMALL);
        let low = bar.battery.is_some_and(|b| b.percent <= 20 && !b.charging);
        print(bar, out, pk.0, bx + tw + 2.5 + 5.0, ROW_CTRL + dy - slh * 0.5 - 0.3, if low { ORANGE } else { INK_SOFT }.fade(ta), plate, 0.5);
    }
    rx = bx - 22.0;

    // Network: caption + three LED lenses.
    {
        let lab = bar.text.get(fs, "SIGNAL", F_LABEL);
        let lr = 2.3;
        let pitch = 8.5;
        let lw = pitch * 2.0 + 2.0 * lr;
        let col_w = lab.1.max(lw);
        let nx = rx - col_w;
        print(bar, out, lab.0, nx, ROW_LABEL + dy, PRINT.fade(ta), plate, 0.5);
        for i in 0..3 {
            let pop = ((st.t - 0.30 - i as f32 * 0.05) / 0.12).clamp(0.0, 1.0) * e.clamp(0.0, 1.0);
            let lx = nx + lr + i as f32 * pitch;
            let ly = ROW_CTRL + dy;
            let l = st.leds[i].get().clamp(0.0, 1.0);
            // Bezel.
            out.shapes.push(
                paint::circle(s, lx, ly, (lr + 0.9) * pop)
                    .gradient(SHADE.fade(0.32), HILITE.fade(0.9), 0.0, 1.0)
                    .done(),
            );
            let off_t = Rgba::hex(0x8f897f, 1.0);
            let off_b = Rgba::hex(0xb9b2a7, 1.0);
            let mut sh = paint::circle(s, lx, ly, lr * pop)
                .gradient(off_t.mix(GREEN_HI, l), off_b.mix(GREEN, l), 0.0, 1.0)
                .rim(0.7, SHADE.fade(0.4 * (1.0 - l)), HILITE.fade(0.25));
            if l > 0.01 {
                sh = sh.bloom(GREEN.fade(0.42 * l), 1.7);
            }
            out.shapes.push(sh.done());
            // Lens glint.
            out.shapes.push(paint::circle(s, lx - 0.7, ly - 0.8, 0.8 * pop).fill(HILITE.fade(0.35 + 0.35 * l)).done());
        }
    }

    // ── centre: caption + window title ──────────────────────────────────
    let cap = bar
        .workspaces
        .iter()
        .find(|w| w.active)
        .map(|w| {
            let n = w.windows;
            format!("WORKSPACE {} \u{2014} {} WINDOW{}", w.idx, n, if n == 1 { "" } else { "S" })
        })
        .unwrap_or_else(|| "DESKTOP".into());
    let ck = bar.text.get(fs, &cap, F_LABEL);
    let cx = w * 0.5;
    print(bar, out, ck.0, cx - ck.1 * 0.5, ROW_LABEL + dy, PRINT.fade(ta), plate, 0.5);
    let tmax = 520.0;
    let tlh = bar.text.line_h(F_TITLE);
    let tty = ROW_CTRL + dy - tlh * 0.5 - 0.5;
    let xf = bar.title.xf.get().clamp(0.0, 1.0);
    let tclip = Rect::new(cx - tmax * 0.5 - 4.0, plate.y + 12.0, tmax + 8.0, PH - 13.0);
    if let Some(t) = bar.title.prev.clone()
        && xf < 1.0
    {
        let (k, tw) = bar.text.get_fit(fs, &t, F_TITLE, tmax);
        print(bar, out, k, cx - tw * 0.5, tty - xf * 6.0, INK.fade(ta * (1.0 - xf)), tclip, 0.5);
    }
    if let Some(t) = bar.title.cur.clone() {
        let (k, tw) = bar.text.get_fit(fs, &t, F_TITLE, tmax);
        print(bar, out, k, cx - tw * 0.5, tty + (1.0 - xf) * 6.0, INK.fade(ta * xf), tclip, 0.5);
    }
}
