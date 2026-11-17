// Turns the supersampled HDR scene into display pixels (Rgba8Unorm, sRGB
// encoded): box filter over ss×ss samples, then per view mode
//   0 physical: exposure, ACES, sRGB
//   1 clay:     ×1.6, ACES, sRGB
//   2 raw:      analysis colours as they are
// Section caps (alpha 2) are display colours and blend in after mapping.

struct Params {
    p: vec4<f32>, // exposure × 2^ev, kind, ss, 0
};

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var<uniform> t: Params;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn srgb(x: vec3<f32>) -> vec3<f32> {
    let c = clamp(x, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let ss = i32(t.p.z + 0.5);
    let base = vec2<i32>(floor(pos.xy)) * ss;
    var acc = vec3<f32>(0.0);
    var n = 0.0;
    var cap = vec3<f32>(0.0);
    var ncap = 0.0;
    for (var y = 0; y < ss; y++) {
        for (var x = 0; x < ss; x++) {
            let s = textureLoad(hdr, base + vec2<i32>(x, y), 0);
            if (s.a > 1.5) {
                cap += s.rgb;
                ncap += 1.0;
            } else {
                acc += s.rgb;
                n += 1.0;
            }
        }
    }
    let c = acc / max(n, 1.0);
    let kind = u32(t.p.y + 0.5);
    var out = c;
    if (kind == 0u) {
        out = srgb(aces(c * t.p.x));
    } else if (kind == 1u) {
        out = srgb(aces(c * 1.6 * t.p.x));
    }
    let f = ncap / f32(ss * ss);
    if (f > 0.0) {
        out = mix(out, cap / ncap, f);
    }
    return vec4<f32>(out, 1.0);
}
