// Textured rounded rectangles (thumbnails, wallpapers, app icons). The
// SDF edge matches shape.wgsl; `reveal` masks the image to a soft-edged
// circle (wallpaper transitions). Physical pixels; premultiplied output.

struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Inst {
    @location(0) rect: vec4<f32>,    // x, y, w, h
    @location(1) uv: vec4<f32>,      // u0, v0, u1, v1
    @location(2) params: vec4<f32>,  // radius, alpha, brightness, smooth (0/1)
    @location(3) reveal: vec4<f32>,  // cx, cy, radius (<0: off), softness
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) reveal: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, s: Inst) -> VsOut {
    let c = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let px = s.rect.xy + s.rect.zw * c;
    var o: VsOut;
    o.pos = vec4<f32>(px / g.viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.px = px;
    o.uv = mix(s.uv.xy, s.uv.zw, c);
    o.rect = s.rect;
    o.params = s.params;
    o.reveal = s.reveal;
    return o;
}

fn sd_round_rect(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Cubic B-spline from four bilinear taps: no diamond pattern when a tiny
// blurred texture is stretched across the screen.
fn sample_bspline(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(tex));
    let p = uv * size - 0.5;
    let f = fract(p);
    let base = floor(p) + 0.5;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (base - 1.0 + w1 / g0) / size;
    let h1 = (base + 1.0 + w3 / g1) / size;
    let a = textureSampleLevel(tex, samp, vec2<f32>(h0.x, h0.y), 0.0);
    let b = textureSampleLevel(tex, samp, vec2<f32>(h1.x, h0.y), 0.0);
    let c = textureSampleLevel(tex, samp, vec2<f32>(h0.x, h1.y), 0.0);
    let d = textureSampleLevel(tex, samp, vec2<f32>(h1.x, h1.y), 0.0);
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let half = i.rect.zw * 0.5;
    let r = min(i.params.x, min(half.x, half.y));
    let d = sd_round_rect(i.px - (i.rect.xy + half), half, r);
    var a = clamp(0.5 - d, 0.0, 1.0) * i.params.y;
    if (i.reveal.z >= 0.0) {
        let dist = length(i.px - i.reveal.xy);
        a = a * (1.0 - smoothstep(i.reveal.z - i.reveal.w, i.reveal.z, dist));
    }
    var c = textureSample(tex, samp, i.uv);
    if (i.params.w > 0.5) {
        c = sample_bspline(i.uv);
    }
    var rgb = c.rgb * i.params.z;
    if (i.params.w > 0.5) {
        // Triangular dither, +-1 LSB of the 8-bit target: no banding in
        // the dark gradients the lock screen lives in.
        let n = hash(i.px) + hash(i.px + vec2<f32>(17.3, 41.9)) - 1.0;
        rgb = max(rgb + vec3<f32>(n / 255.0), vec3<f32>(0.0));
    }
    return vec4<f32>(rgb * c.a, c.a) * a;
}
