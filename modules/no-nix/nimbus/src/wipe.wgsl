// An image revealed through an order map (wallpaper changes). The map is
// laid over the image's UVs: R + G/256 = when each spot appears (0..1),
// B = when the shape under it starts to fill, A = contour strength. The image shows
// where R < progress, with a soft front that refracts the picture a little
// like the edge of a glass sheet, a faint bright rim on its inner side and
// a faint shade ahead of it. Physical pixels; premultiplied output.

struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var order: texture_2d<f32>;
@group(2) @binding(1) var osamp: sampler;

struct Inst {
    @location(0) rect: vec4<f32>,    // x, y, w, h
    @location(1) uv: vec4<f32>,      // u0, v0, u1, v1
    @location(2) params: vec4<f32>,  // radius, alpha, -, -
    @location(3) reveal: vec4<f32>,  // unused here
    @location(4) wipe: vec4<f32>,    // progress, soft edge, refraction px, rim
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) wipe: vec4<f32>,
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
    o.wipe = s.wipe;
    return o;
}

/// Order from the map's two channels.
fn at(m: vec4<f32>) -> f32 {
    return m.r + m.g / 256.0;
}

fn sd_round_rect(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let half = i.rect.zw * 0.5;
    let r = min(i.params.x, min(half.x, half.y));
    let sd = sd_round_rect(i.px - (i.rect.xy + half), half, r);
    let shape = clamp(0.5 - sd, 0.0, 1.0) * i.params.y;

    let t = i.wipe.x;
    let w = max(i.wipe.y, 1.0);
    // Order and its slope (per pixel), from the map's neighbours.
    let ts = vec2<f32>(textureDimensions(order));
    let du = vec2<f32>(3.0, 0.0) / ts;
    let dv = vec2<f32>(0.0, 3.0) / ts;
    let m = textureSample(order, osamp, i.uv);
    let o = at(m);
    let gx = at(textureSample(order, osamp, i.uv + du)) - at(textureSample(order, osamp, i.uv - du));
    let gy = at(textureSample(order, osamp, i.uv + dv)) - at(textureSample(order, osamp, i.uv - dv));
    let slope = vec2<f32>(gx, gy) * ts / (6.0 * i.rect.zw);
    let steep = length(slope);
    // Points away from the revealed side.
    let n = slope / max(steep, 1e-7);

    // Distance to the front in pixels (> 0: revealed), like an SDF: the
    // front is `w` px wide whether it crawls over a soft gradient or snaps
    // along a contour. Flat spots cap it at a few widths.
    let d = (t - o) / max(steep, 1.0 / (w * 400.0));
    let a = smoothstep(-0.5 * w, 0.5 * w, d);
    // Centre of the glass lip, just inside the front.
    let x = (d - 0.35 * w) / w;
    let band = exp(-x * x * 2.5);

    // Glass lip: the picture next to the front is seen through a rounded
    // edge (pulled outwards, then let go), split a hair by colour.
    let lip = x * exp(-x * x * 1.6) * 1.9;
    // (Only where the slope gives a clear direction.)
    let sure = smoothstep(0.2, 1.0, steep * w * 400.0);
    let pull = n * (i.wipe.z * lip * sure) / i.rect.zw;
    let cr = textureSample(tex, samp, i.uv + pull * 1.05).r;
    let cg = textureSample(tex, samp, i.uv + pull).g;
    let cb = textureSample(tex, samp, i.uv + pull * 0.95).b;
    let c = vec3<f32>(cr, cg, cb);

    // Each shape glints as the flood enters it, then settles.
    let since = t - m.b;
    let glint = select(0.0, exp(-since / 0.07), since >= 0.0) * a;
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    var rgb = c * (1.0 + glint * 0.08);

    // Rim on the lip: the picture's own light caught in the edge, brighter
    // along contours. Dark parts of the picture get next to none (no grey
    // outlines on black); colour stays colour (glassy, not neon).
    let rim = i.wipe.w * band * (0.6 + 0.4 * m.a);
    let tint = c * 1.4 + vec3<f32>(0.15 * sqrt(lum));
    rgb = rgb * a + tint * rim;

    // A soft shade ahead of the front, on the old picture.
    let ahead = max(-d / w - 0.2, 0.0);
    // (Near the front in time too: a steep contour far ahead is no front.)
    let soon = smoothstep(0.06, 0.0, o - t);
    let shade = (1.0 - a) * (i.wipe.w / 0.12) * 0.13 * exp(-ahead * 0.45) * soon;
    let alpha = a + (1.0 - a) * clamp(shade, 0.0, 0.3);
    return vec4<f32>(rgb, alpha) * shape;
}
