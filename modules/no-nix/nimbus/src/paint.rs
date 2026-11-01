//! Small builder over `gfx::Shape` in *logical* pixels, for designs.
//!
//!   out.shapes.push(paint::rect(bar.scale, r, 6.0).fill(c).rim(1.0, a, b).shadow(s, 8.0, 3.0).done());
//!
//! Every shape is a rounded rectangle evaluated as an SDF on the GPU, so
//! circles are `rect` with radius = half size, and everything is
//! anti-aliased at any scale.

use crate::{
    bar::Rect,
    gfx::Shape,
    theme::{self, Rgba},
};

/// Prism Ink's iridescent gradient direction (CSS 120°: right, a little down).
pub const IRID: (f32, f32) = (0.866, 0.5);
/// The prism hairline width, logical px.
pub const HAIR: f32 = 1.25;

/// The iridescent ramp at `t` (0..1): cyan, lilac, pink, peach, a little
/// desaturated, for things drawn as text rather than as shapes.
pub fn irid_at(t: f32) -> Rgba {
    const STOPS: [(f32, [f32; 3]); 4] = [(0.0, [0.624, 0.863, 1.0]), (0.4, [0.788, 0.698, 1.0]), (0.7, [1.0, 0.702, 0.894]), (1.0, [1.0, 0.824, 0.659])];
    let t = t.clamp(0.0, 1.0);
    let j = STOPS.iter().rposition(|s| s.0 <= t).unwrap_or(0).min(STOPS.len() - 2);
    let (a, b) = (STOPS[j], STOPS[j + 1]);
    let k = (t - a.0) / (b.0 - a.0);
    let c: [f32; 3] = std::array::from_fn(|q| a.1[q] + (b.1[q] - a.1[q]) * k);
    let l = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
    Rgba(c[0] + (l - c[0]) * 0.14, c[1] + (l - c[1]) * 0.14, c[2] + (l - c[2]) * 0.14, 1.0)
}

pub struct ShapeB {
    s: f32,
    sh: Shape,
}

/// A rounded rect. `radius` is clamped to half the short side.
pub fn rect(scale: f32, r: Rect, radius: f32) -> ShapeB {
    ShapeB {
        s: scale,
        sh: Shape {
            rect: [r.x * scale, r.y * scale, r.w * scale, r.h * scale],
            geom: [radius * scale, 0.0, 0.0, 0.0],
            grad: [0.0, 1.0, 0.0, 0.0],
            ..Default::default()
        },
    }
}

/// A circle centred on (cx, cy).
pub fn circle(scale: f32, cx: f32, cy: f32, radius: f32) -> ShapeB {
    rect(scale, Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0), radius)
}

impl ShapeB {
    /// Flat fill.
    pub fn fill(mut self, c: Rgba) -> Self {
        self.sh.fill_a = c.arr();
        self.sh.fill_b = c.arr();
        self
    }
    /// Linear gradient from `a` to `b` along (dx, dy); (0, 1) is top→bottom.
    pub fn gradient(mut self, a: Rgba, b: Rgba, dx: f32, dy: f32) -> Self {
        self.sh.fill_a = a.arr();
        self.sh.fill_b = b.arr();
        self.sh.grad[0] = dx;
        self.sh.grad[1] = dy;
        self
    }
    /// Inner edge band of `width`, coloured `top` at the top edge fading to
    /// `bottom`. Use dark-top/light-bottom for a recessed look.
    pub fn rim(mut self, width: f32, top: Rgba, bottom: Rgba) -> Self {
        self.sh.geom[1] = width * self.s;
        self.sh.rim_a = top.arr();
        self.sh.rim_b = bottom.arr();
        self
    }
    /// Soft glow hugging the inside edge, tinted by the rim colour.
    pub fn inner_glow(mut self, k: f32) -> Self {
        self.sh.grad[2] = k;
        self
    }
    /// Gaussian drop shadow outside the shape.
    pub fn shadow(mut self, c: Rgba, sigma: f32, dy: f32) -> Self {
        self.sh.shadow = c.arr();
        self.sh.geom[2] = sigma * self.s;
        self.sh.geom[3] = dy * self.s;
        self
    }
    /// Coloured glow outside the shape.
    pub fn bloom(mut self, c: Rgba, sigma: f32) -> Self {
        self.sh.bloom = c.arr();
        self.sh.grad[3] = sigma * self.s;
        self
    }
    /// Highlight following a point (e.g. the cursor), logical coords.
    pub fn spot(mut self, x: f32, y: f32, radius: f32, intensity: f32) -> Self {
        self.sh.spot = [x * self.s, y * self.s, radius * self.s, intensity];
        self
    }
    /// Prism Ink hairline: a `width` edge band whose colour follows the
    /// edge normal (cyan left, magenta right, cool top, amber bottom).
    /// `k` scales the house strength.
    pub fn prism(mut self, width: f32, k: f32) -> Self {
        self.sh.geom[1] = width * self.s;
        self.sh.prism[0] = k;
        self
    }
    /// Iridescent fill along (dx, dy), opaque.
    pub fn irid(mut self, dx: f32, dy: f32) -> Self {
        self.sh.fill_a = [1.0; 4];
        self.sh.fill_b = [1.0; 4];
        self.sh.grad[0] = dx;
        self.sh.grad[1] = dy;
        self.sh.prism[1] = 1.0;
        self
    }
    /// Lift the iridescent fill toward white (pastel dots, hover).
    pub fn lighten(mut self, k: f32) -> Self {
        self.sh.prism[2] = k.clamp(0.0, 1.0);
        self
    }
    /// The "on" fill: iridescent under the Prism accent, otherwise the
    /// accent gradient. `lift` brightens it (hover).
    pub fn on_fill(self, dx: f32, dy: f32, lift: f32) -> Self {
        if theme::prism() {
            self.irid(dx, dy).lighten(lift)
        } else {
            let (a, b) = theme::accent();
            let w = Rgba::hex(0xffffff, 1.0);
            self.gradient(a.mix(w, lift), b.mix(w, lift), dx, dy)
        }
    }
    /// `off` → the "on" fill by `k` (toggles easing on).
    pub fn on_mix(mut self, off: Rgba, k: f32, dx: f32, dy: f32, lift: f32) -> Self {
        let k = k.clamp(0.0, 1.0);
        if theme::prism() {
            let c = off.with_a(off.3 + (1.0 - off.3) * k);
            self = self.gradient(c, c, dx, dy);
            self.sh.prism[1] = k;
            self.lighten(lift)
        } else {
            let (a, b) = theme::accent();
            let w = Rgba::hex(0xffffff, 1.0);
            self.gradient(off.mix(a.mix(w, lift), k), off.mix(b.mix(w, lift), k), dx, dy)
        }
    }
    /// Multiply every colour's alpha by `a` (for fades).
    pub fn alpha(mut self, a: f32) -> Self {
        for c in [
            &mut self.sh.fill_a,
            &mut self.sh.fill_b,
            &mut self.sh.rim_a,
            &mut self.sh.rim_b,
            &mut self.sh.shadow,
            &mut self.sh.bloom,
        ] {
            c[3] *= a;
        }
        self.sh.spot[3] *= a;
        self.sh.prism[0] *= a;
        self
    }
    pub fn done(self) -> Shape {
        self.sh
    }
}

/// Prism Ink's matte panel fill.
pub const INK: Rgba = Rgba::hex(0x09090d, 0.52);
/// Faint white for "off" dots and quiet hover circles.
pub const FAINT: Rgba = Rgba::hex(0xffffff, 0.08);

/// A Prism Ink surface: flat matte ink, the prism hairline. Add a shadow
/// (and the cursor spot) at the call site.
pub fn ink(scale: f32, r: Rect, radius: f32) -> ShapeB {
    rect(scale, r, radius).fill(INK).prism(HAIR, 1.0)
}

/// A clear glass knob: faint fill, hairline ring, small shadow.
pub fn knob(scale: f32, cx: f32, cy: f32, radius: f32) -> ShapeB {
    circle(scale, cx, cy, radius)
        .fill(Rgba::hex(0xe8ecf6, 0.20))
        .rim(1.0, Rgba::hex(0xffffff, 0.62), Rgba::hex(0xffffff, 0.48))
        .shadow(Rgba::hex(0x000000, 0.32), 3.0, 1.5)
}

/// A horizontal hairline that fades in from both ends.
pub fn divider(out: &mut Vec<Shape>, scale: f32, x: f32, y: f32, w: f32, a: f32) {
    if a <= 0.002 || w <= 2.0 {
        return;
    }
    let c = Rgba::hex(0xffffff, 0.12);
    let h = w * 0.5;
    out.push(rect(scale, Rect::new(x, y, h, 1.0), 0.0).gradient(c.fade(0.0), c, 1.0, 0.0).alpha(a).done());
    out.push(rect(scale, Rect::new(x + h, y, w - h, 1.0), 0.0).gradient(c, c.fade(0.0), 1.0, 0.0).alpha(a).done());
}
