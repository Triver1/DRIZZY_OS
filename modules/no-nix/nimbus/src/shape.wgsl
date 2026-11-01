// One instanced quad per shape. Each fragment evaluates the signed distance
// to a rounded rectangle and derives everything from it: anti-aliased fill,
// lit glass rim, inner refraction glow, cursor specular, drop shadow and
// coloured bloom, plus the Prism Ink hairline and iridescent fill. All coordinates are physical pixels; output is premultiplied.

struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

struct Shape {
    @location(0) rect: vec4<f32>,      // x, y, w, h
    @location(1) geom: vec4<f32>,      // radius, rim width, shadow sigma, shadow dy
    @location(2) fill_a: vec4<f32>,
    @location(3) fill_b: vec4<f32>,
    @location(4) rim_a: vec4<f32>,     // rim colour at top
    @location(5) rim_b: vec4<f32>,     // rim colour at bottom
    @location(6) shadow: vec4<f32>,
    @location(7) grad: vec4<f32>,      // gradient dir xy, inner glow, bloom sigma
    @location(8) spot: vec4<f32>,      // specular x, y, radius, intensity
    @location(9) bloom: vec4<f32>,     // bloom colour
    @location(10) prism: vec4<f32>,    // prism rim strength, iridescent fill, lighten, _
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) geom: vec4<f32>,
    @location(3) @interpolate(flat) fill_a: vec4<f32>,
    @location(4) @interpolate(flat) fill_b: vec4<f32>,
    @location(5) @interpolate(flat) rim_a: vec4<f32>,
    @location(6) @interpolate(flat) rim_b: vec4<f32>,
    @location(7) @interpolate(flat) shadow: vec4<f32>,
    @location(8) @interpolate(flat) grad: vec4<f32>,
    @location(9) @interpolate(flat) spot: vec4<f32>,
    @location(10) @interpolate(flat) bloom: vec4<f32>,
    @location(11) @interpolate(flat) prism: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, s: Shape) -> VsOut {
    // Grow the quad so shadow and bloom have room to fall off.
    let reach = max(s.geom.z * 3.0 + abs(s.geom.w), s.grad.w * 3.0) + 2.0;
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let lo = s.rect.xy - vec2<f32>(reach);
    let hi = s.rect.xy + s.rect.zw + vec2<f32>(reach);
    let px = mix(lo, hi, corner);
    var o: VsOut;
    o.pos = vec4<f32>(px / g.viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.px = px;
    o.rect = s.rect;
    o.geom = s.geom;
    o.fill_a = s.fill_a;
    o.fill_b = s.fill_b;
    o.rim_a = s.rim_a;
    o.rim_b = s.rim_b;
    o.shadow = s.shadow;
    o.grad = s.grad;
    o.spot = s.spot;
    o.bloom = s.bloom;
    o.prism = s.prism;
    return o;
}

fn sd_round_rect(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Abramowitz–Stegun style erf, good to ~1e-3: plenty for soft shadows.
fn erf(x: f32) -> f32 {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 + (0.278393 + (0.230389 + 0.078108 * a * a) * a) * a;
    let r = t * t;
    return s - s / (r * r);
}

fn over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    return src + dst * (1.0 - src.a);
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

// Prism Ink: light through a prism, slightly desaturated.
// #9fdcff → #c9b2ff (40%) → #ffb3e4 (70%) → #ffd2a8.
fn irid(t: f32) -> vec3<f32> {
    let c0 = vec3<f32>(0.624, 0.863, 1.0);
    let c1 = vec3<f32>(0.788, 0.698, 1.0);
    let c2 = vec3<f32>(1.0, 0.702, 0.894);
    let c3 = vec3<f32>(1.0, 0.824, 0.659);
    var c: vec3<f32>;
    if (t < 0.4) {
        c = mix(c0, c1, t / 0.4);
    } else if (t < 0.7) {
        c = mix(c1, c2, (t - 0.4) / 0.3);
    } else {
        c = mix(c2, c3, (t - 0.7) / 0.3);
    }
    let l = dot(c, vec3<f32>(0.299, 0.587, 0.114));
    return mix(c, vec3<f32>(l), 0.14);
}

// The prism hairline: each edge splits light into its own faint colour,
// blended through the corners by the edge normal. Straight rgb + alpha.
fn prism_rim(p: vec2<f32>, half: vec2<f32>, r: f32) -> vec4<f32> {
    let q = abs(p) - half + vec2<f32>(r);
    var n: vec2<f32>;
    if (q.x > 0.0 && q.y > 0.0) {
        n = normalize(q) * sign(p);
    } else if (q.x > q.y) {
        n = vec2<f32>(sign(p.x), 0.0);
    } else {
        n = vec2<f32>(0.0, sign(p.y));
    }
    let wl = max(-n.x, 0.0);
    let wr = max(n.x, 0.0);
    let wt = max(-n.y, 0.0);
    let wb = max(n.y, 0.0);
    let w = vec4<f32>(wl * wl, wr * wr, wt * wt, wb * wb);
    let al = vec4<f32>(0.18, 0.17, 0.10, 0.10) * w;
    let pre = vec3<f32>(0.471, 0.863, 1.0) * al.x + vec3<f32>(1.0, 0.471, 0.824) * al.y
        + vec3<f32>(0.667, 0.784, 1.0) * al.z + vec3<f32>(1.0, 0.745, 0.588) * al.w;
    let a = al.x + al.y + al.z + al.w;
    return vec4<f32>(pre / max(a, 1e-4), a);
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let half = i.rect.zw * 0.5;
    let center = i.rect.xy + half;
    let p = i.px - center;
    let r = min(i.geom.x, min(half.x, half.y));
    let d = sd_round_rect(p, half, r);
    let outside = clamp(d + 0.5, 0.0, 1.0);

    var col = vec4<f32>(0.0);

    // Drop shadow, clipped to outside the body so glass stays clear.
    let sigma = i.geom.z;
    if (i.shadow.a > 0.0 && sigma > 0.0) {
        let ds = sd_round_rect(p - vec2<f32>(0.0, i.geom.w), half, r);
        let a = i.shadow.a * (0.5 - 0.5 * erf(ds / (sigma * 1.41421))) * outside;
        col = over(col, vec4<f32>(i.shadow.rgb * a, a));
    }

    // Coloured bloom (glow) around luminous shapes.
    let bs = i.grad.w;
    if (i.bloom.a > 0.0 && bs > 0.0) {
        let dd = max(d, 0.0);
        let a = i.bloom.a * exp(-(dd * dd) / (2.0 * bs * bs)) * outside;
        col = over(col, vec4<f32>(i.bloom.rgb * a, a));
    }

    let cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) {
        return col;
    }

    // Body gradient along `grad.xy`.
    let dir = i.grad.xy;
    let extent = max(abs(half.x * dir.x) + abs(half.y * dir.y), 1e-3);
    let t = clamp(dot(p, dir) / extent * 0.5 + 0.5, 0.0, 1.0);
    var fc = mix(i.fill_a, i.fill_b, t);
    if (i.prism.y > 0.0) {
        let ic = mix(irid(t), vec3<f32>(1.0), i.prism.z);
        fc = vec4<f32>(mix(fc.rgb, ic, i.prism.y), fc.a);
    }
    var body = premul(fc);

    // Vertical position for lighting (0 top, 1 bottom).
    let ty = clamp(p.y / max(half.y, 1e-3) * 0.5 + 0.5, 0.0, 1.0);
    var rim = mix(i.rim_a, i.rim_b, ty);
    if (i.prism.x > 0.0) {
        let pr = prism_rim(p, half, r);
        rim = vec4<f32>(pr.rgb, pr.a * i.prism.x);
    }

    // Specular highlight that follows the cursor across the glass.
    var spec = 0.0;
    if (i.spot.w > 0.0) {
        let k = 1.0 - smoothstep(0.0, i.spot.z, length(i.px - i.spot.xy));
        spec = i.spot.w * k * k;
    }

    // Inner glow hugging the edge: fakes light refracting through thickness.
    let inner = i.grad.z;
    if (inner > 0.0) {
        let a = inner * exp(d / max(r * 0.55, 2.0)) * rim.a * (1.0 + spec * 2.0);
        body = over(body, vec4<f32>(rim.rgb * a, a));
    }
    if (spec > 0.0) {
        let a = spec * 0.10;
        body = over(body, vec4<f32>(vec3<f32>(a), a));
    }

    // Rim band of width geom.y, brighter where the cursor is.
    let bw = i.geom.y;
    if (bw > 0.0) {
        // Outer edge AA comes from the final `* cov`.
        let band = 1.0 - clamp(0.5 - (d + bw), 0.0, 1.0);
        let a = clamp(rim.a * (1.0 + spec * 2.5), 0.0, 1.0) * band;
        body = over(body, vec4<f32>(rim.rgb * a, a));
    }

    return over(col, body * cov);
}
