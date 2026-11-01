//! The circular crop: which square of the picture ends up in the avatar.
//! Plain numbers in picture pixels (upright), so the page draws from it and
//! the saver cuts with it, whatever size each of them holds the picture at.

/// Deepest zoom, and the fewest source pixels across a crop may have.
const MAX_ZOOM: f32 = 6.0;
const MIN_SIDE: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub w: f32,
    pub h: f32,
    /// Centre of the crop.
    pub cx: f32,
    pub cy: f32,
    /// 1: the largest square that fits. 2: half as wide.
    pub zoom: f32,
}

impl Crop {
    pub fn new(w: u32, h: u32) -> Self {
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        // Portraits: a face sits above the middle.
        let cy = if h > w * 1.1 { h * 0.42 } else { h * 0.5 };
        let mut c = Self { w, h, cx: w * 0.5, cy, zoom: 1.0 };
        c.clamp();
        c
    }

    fn base(&self) -> f32 {
        self.w.min(self.h)
    }

    /// Width of the crop.
    pub fn side(&self) -> f32 {
        self.base() / self.zoom
    }

    pub fn max_zoom(&self) -> f32 {
        (self.base() / MIN_SIDE).clamp(1.0, MAX_ZOOM)
    }

    /// The crop stays whole inside the picture.
    fn clamp(&mut self) {
        self.zoom = self.zoom.clamp(1.0, self.max_zoom());
        let half = self.side() * 0.5;
        self.cx = self.cx.clamp(half, self.w - half);
        self.cy = self.cy.clamp(half, self.h - half);
    }

    /// The picture was dragged by (dx, dy) screen px under a circle `d`
    /// screen px wide.
    pub fn pan(&mut self, dx: f32, dy: f32, d: f32) {
        let k = self.side() / d.max(1.0);
        self.cx -= dx * k;
        self.cy -= dy * k;
        self.clamp();
    }

    /// Zoom to `z`, keeping the point (ax, ay) screen px from the circle's
    /// centre where it is.
    pub fn zoom_at(&mut self, z: f32, ax: f32, ay: f32, d: f32) {
        let k0 = self.side() / d.max(1.0);
        let (px, py) = (self.cx + ax * k0, self.cy + ay * k0);
        self.zoom = z;
        self.clamp();
        let k1 = self.side() / d.max(1.0);
        self.cx = px - ax * k1;
        self.cy = py - ay * k1;
        self.clamp();
    }

    /// Slider position (0..1): even steps feel even.
    pub fn zoom_t(&self) -> f32 {
        let m = self.max_zoom();
        if m <= 1.0 { 0.0 } else { self.zoom.ln() / m.ln() }
    }

    pub fn set_zoom_t(&mut self, t: f32) {
        let z = self.max_zoom().powf(t.clamp(0.0, 1.0));
        self.zoom_at(z, 0.0, 0.0, 1.0);
    }

    /// The whole picture on screen, as [x, y, w, h], for a circle `d` wide
    /// centred on (ox, oy).
    pub fn placed(&self, ox: f32, oy: f32, d: f32) -> [f32; 4] {
        let k = d / self.side();
        [ox - self.cx * k, oy - self.cy * k, self.w * k, self.h * k]
    }

    /// The part of `placed` inside `clip` ([x, y, w, h]) and its uv.
    pub fn visible(&self, ox: f32, oy: f32, d: f32, clip: [f32; 4]) -> Option<([f32; 4], [f32; 4])> {
        let p = self.placed(ox, oy, d);
        let (x0, y0) = (p[0].max(clip[0]), p[1].max(clip[1]));
        let (x1, y1) = ((p[0] + p[2]).min(clip[0] + clip[2]), (p[1] + p[3]).min(clip[1] + clip[3]));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let uv = [(x0 - p[0]) / p[2], (y0 - p[1]) / p[3], (x1 - p[0]) / p[2], (y1 - p[1]) / p[3]];
        Some(([x0, y0, x1 - x0, y1 - y0], uv))
    }

    /// The crop as uv of the picture.
    pub fn uv(&self) -> [f32; 4] {
        let h = self.side() * 0.5;
        [(self.cx - h) / self.w, (self.cy - h) / self.h, (self.cx + h) / self.w, (self.cy + h) / self.h]
    }

    /// The crop in a decoded copy `iw`×`ih` of the picture: (x, y, side) in
    /// whole pixels, always inside it.
    pub fn square_in(&self, iw: u32, ih: u32) -> (u32, u32, u32) {
        let s = (iw as f32 / self.w).min(ih as f32 / self.h);
        let side = ((self.side() * s).round() as u32).clamp(1, iw.min(ih).max(1));
        let half = side as f32 * 0.5;
        let x = ((self.cx * iw as f32 / self.w - half).round().max(0.0) as u32).min(iw.saturating_sub(side));
        let y = ((self.cy * ih as f32 / self.h - half).round().max(0.0) as u32).min(ih.saturating_sub(side));
        (x, y, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }

    #[test]
    fn starts_as_the_largest_square_that_fits() {
        let c = Crop::new(4000, 3000);
        assert_eq!(c.side(), 3000.0);
        assert_eq!((c.cx, c.cy), (2000.0, 1500.0));
        assert_eq!(c.square_in(4000, 3000), (500, 0, 3000));
    }

    #[test]
    fn portraits_start_above_the_middle() {
        let c = Crop::new(3000, 4000);
        assert!(c.cy < 2000.0);
        assert!(c.cy >= 1500.0, "the crop stays inside");
    }

    #[test]
    fn panning_never_leaves_the_picture() {
        let mut c = Crop::new(1000, 500);
        c.pan(10_000.0, 10_000.0, 300.0);
        assert_eq!(c.square_in(1000, 500), (0, 0, 500));
        c.pan(-10_000.0, -10_000.0, 300.0);
        assert_eq!(c.square_in(1000, 500), (500, 0, 500));
    }

    #[test]
    fn panning_follows_the_pointer_one_to_one() {
        let mut c = Crop::new(1000, 1000);
        c.zoom_at(2.0, 0.0, 0.0, 300.0);
        let before = c.placed(0.0, 0.0, 300.0);
        c.pan(30.0, -20.0, 300.0);
        let after = c.placed(0.0, 0.0, 300.0);
        near(after[0] - before[0], 30.0);
        near(after[1] - before[1], -20.0);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut c = Crop::new(2000, 2000);
        c.zoom_at(2.0, 0.0, 0.0, 300.0);
        let (ax, ay) = (60.0, -40.0);
        let on_image = |c: &Crop| (c.cx + ax * c.side() / 300.0, c.cy + ay * c.side() / 300.0);
        let before = on_image(&c);
        c.zoom_at(3.0, ax, ay, 300.0);
        let after = on_image(&c);
        near(after.0, before.0);
        near(after.1, before.1);
    }

    #[test]
    fn zoom_is_bounded_and_the_crop_stays_inside_at_the_edges() {
        let mut c = Crop::new(1000, 1000);
        c.zoom_at(100.0, 0.0, 0.0, 300.0);
        assert_eq!(c.zoom, MAX_ZOOM);
        c.pan(10_000.0, 0.0, 300.0);
        // Back out at the edge: it slides inward rather than poking out.
        c.zoom_at(1.0, 0.0, 0.0, 300.0);
        assert_eq!(c.square_in(1000, 1000), (0, 0, 1000));
        c.zoom_at(0.1, 0.0, 0.0, 300.0);
        assert_eq!(c.zoom, 1.0);
    }

    #[test]
    fn tiny_pictures_do_not_zoom_into_nothing() {
        let c = Crop::new(20, 20);
        assert_eq!(c.max_zoom(), 1.0);
        assert_eq!(c.zoom_t(), 0.0);
        let mut c = Crop::new(1, 1);
        c.set_zoom_t(1.0);
        assert_eq!(c.square_in(1, 1), (0, 0, 1));
    }

    #[test]
    fn the_slider_round_trips() {
        let mut c = Crop::new(3000, 2000);
        c.set_zoom_t(0.5);
        near(c.zoom_t(), 0.5);
        near(c.zoom, MAX_ZOOM.sqrt());
        c.set_zoom_t(0.0);
        assert_eq!(c.zoom, 1.0);
    }

    #[test]
    fn the_same_crop_lands_on_the_same_spot_at_any_decode_size() {
        let mut c = Crop::new(4000, 3000);
        c.zoom_at(2.0, 0.0, 0.0, 300.0);
        c.pan(-200.0, 100.0, 300.0);
        let (x, y, s) = c.square_in(4000, 3000);
        let (x2, y2, s2) = c.square_in(1000, 750);
        assert!((x as f32 / 4.0 - x2 as f32).abs() <= 1.0);
        assert!((y as f32 / 4.0 - y2 as f32).abs() <= 1.0);
        assert!((s as f32 / 4.0 - s2 as f32).abs() <= 1.0);
    }

    #[test]
    fn square_in_is_inside_for_any_pose() {
        for (w, h) in [(1, 1), (2, 3), (31, 977), (640, 480), (4096, 2160)] {
            let mut c = Crop::new(w, h);
            for k in 0..40 {
                let f = k as f32;
                c.zoom_at(1.0 + f * 0.2, f * 7.0 - 100.0, 60.0 - f * 3.0, 300.0);
                c.pan(f * 11.0 - 200.0, 150.0 - f * 13.0, 300.0);
                let (x, y, s) = c.square_in(w, h);
                assert!(s >= 1 && x + s <= w && y + s <= h, "{w}x{h}: {x},{y},{s}");
            }
        }
    }

    #[test]
    fn visible_clips_and_maps_uv() {
        let c = Crop::new(1000, 500);
        // A 500-side crop in a 100 px circle: 1 screen px is 5 picture px.
        let p = c.placed(50.0, 50.0, 100.0);
        assert_eq!(p, [-50.0, 0.0, 200.0, 100.0]);
        let (r, uv) = c.visible(50.0, 50.0, 100.0, [0.0, 0.0, 100.0, 100.0]).unwrap();
        assert_eq!(r, [0.0, 0.0, 100.0, 100.0]);
        near(uv[0], 0.25);
        near(uv[2], 0.75);
        near(uv[1], 0.0);
        near(uv[3], 1.0);
        assert!(c.visible(50.0, 50.0, 100.0, [500.0, 500.0, 10.0, 10.0]).is_none());
    }
}
