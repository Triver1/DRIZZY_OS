//! Spring physics. Every animated value in the bar is a `Spring`: logic only
//! ever moves *targets*; the render loop integrates positions on its own clock.
//!
//! Integration is the exact closed-form solution of a damped harmonic
//! oscillator, so it is stable for any dt and frame-rate independent.

use crate::theme::Rgba;

#[derive(Clone, Copy, Debug)]
pub struct SpringCfg {
    /// Angular frequency (rad/s). `2π / response_seconds`.
    pub omega: f32,
    /// Damping ratio. <1 overshoots, 1 is critical, >1 is sluggish.
    pub zeta: f32,
}

impl SpringCfg {
    /// SwiftUI-style parametrisation: `response` ≈ duration of one oscillation.
    pub const fn new(response: f32, zeta: f32) -> Self {
        Self { omega: std::f32::consts::TAU / response, zeta }
    }
}

pub mod presets {
    use super::SpringCfg;
    /// Fast with a touch of overshoot. Hover, press.
    pub const SNAPPY: SpringCfg = SpringCfg::new(0.30, 0.72);
    /// Visible, playful overshoot. Workspace indicator head.
    pub const BOUNCY: SpringCfg = SpringCfg::new(0.42, 0.58);
    /// Liquid tail that lags the head.
    pub const TAIL: SpringCfg = SpringCfg::new(0.58, 0.82);
    /// Critically damped. Layout widths, colours.
    pub const SMOOTH: SpringCfg = SpringCfg::new(0.46, 1.0);
    /// Slow ease for ambient things (entrance, opacity).
    pub const GENTLE: SpringCfg = SpringCfg::new(0.75, 1.0);
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub pos: f32,
    pub vel: f32,
    pub target: f32,
    pub cfg: SpringCfg,
}

const REST_POS: f32 = 0.0015;
const REST_VEL: f32 = 0.01;

impl Spring {
    pub fn new(v: f32, cfg: SpringCfg) -> Self {
        Self { pos: v, vel: 0.0, target: v, cfg }
    }

    #[inline]
    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Jump without animating.
    pub fn snap(&mut self, v: f32) {
        self.pos = v;
        self.target = v;
        self.vel = 0.0;
    }

    /// Kick the velocity (impulse), e.g. for press feedback.
    pub fn kick(&mut self, dv: f32) {
        self.vel += dv;
    }

    #[inline]
    pub fn at_rest(&self) -> bool {
        self.vel == 0.0 && self.pos == self.target
    }

    #[inline]
    pub fn get(&self) -> f32 {
        self.pos
    }

    /// Advance by `dt` seconds. Returns true while still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.at_rest() {
            return false;
        }
        let SpringCfg { omega: w, zeta: z } = self.cfg;
        let x0 = self.pos - self.target;
        let v0 = self.vel;
        let (x, v);
        if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * dt).exp();
            let (s, c) = (wd * dt).sin_cos();
            let b = (v0 + z * w * x0) / wd;
            x = e * (x0 * c + b * s);
            v = e * (v0 * c - (x0 * wd + z * w * b) * s);
        } else if z == 1.0 {
            let e = (-w * dt).exp();
            let b = v0 + w * x0;
            x = (x0 + b * dt) * e;
            v = (v0 - w * b * dt) * e;
        } else {
            let sq = (z * z - 1.0).sqrt();
            let r1 = -w * (z - sq);
            let r2 = -w * (z + sq);
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * dt).exp(), (r2 * dt).exp());
            x = c1 * e1 + c2 * e2;
            v = c1 * r1 * e1 + c2 * r2 * e2;
        }
        // Scale rest thresholds to the magnitude being animated so pixel
        // values and 0..1 values both settle sensibly.
        let scale = self.target.abs().max(1.0);
        if x.abs() < REST_POS * scale && v.abs() < REST_VEL * scale {
            self.pos = self.target;
            self.vel = 0.0;
            false
        } else {
            self.pos = self.target + x;
            self.vel = v;
            true
        }
    }
}

/// Four springs animating a colour.
#[derive(Clone, Copy, Debug)]
pub struct ColorSpring([Spring; 4]);

impl ColorSpring {
    pub fn new(c: Rgba, cfg: SpringCfg) -> Self {
        Self([
            Spring::new(c.0, cfg),
            Spring::new(c.1, cfg),
            Spring::new(c.2, cfg),
            Spring::new(c.3, cfg),
        ])
    }
    pub fn set(&mut self, c: Rgba) {
        self.0[0].set(c.0);
        self.0[1].set(c.1);
        self.0[2].set(c.2);
        self.0[3].set(c.3);
    }
    pub fn get(&self) -> Rgba {
        Rgba(
            self.0[0].pos.clamp(0.0, 1.0),
            self.0[1].pos.clamp(0.0, 1.0),
            self.0[2].pos.clamp(0.0, 1.0),
            self.0[3].pos.clamp(0.0, 1.0),
        )
    }
    pub fn step(&mut self, dt: f32) -> bool {
        let mut m = false;
        for s in &mut self.0 {
            m |= s.step(dt);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(cfg: SpringCfg) -> (f32, f32) {
        let mut s = Spring::new(0.0, cfg);
        s.set(100.0);
        let mut peak: f32 = 0.0;
        let mut t = 0.0;
        while s.step(1.0 / 120.0) {
            peak = peak.max(s.pos);
            t += 1.0 / 120.0;
            assert!(t < 5.0, "spring never settled");
        }
        (peak, t)
    }

    #[test]
    fn underdamped_overshoots_and_settles() {
        let (peak, _) = settle(presets::BOUNCY);
        assert!(peak > 100.0);
    }

    #[test]
    fn critical_does_not_overshoot() {
        let (peak, _) = settle(presets::SMOOTH);
        assert!(peak <= 100.0 + 1e-3);
    }

    #[test]
    fn frame_rate_independent() {
        let mut a = Spring::new(0.0, presets::BOUNCY);
        let mut b = a;
        a.set(50.0);
        b.set(50.0);
        for _ in 0..12 {
            a.step(1.0 / 120.0);
        }
        for _ in 0..6 {
            b.step(1.0 / 60.0);
        }
        assert!((a.pos - b.pos).abs() < 1e-3, "{} vs {}", a.pos, b.pos);
    }
}
