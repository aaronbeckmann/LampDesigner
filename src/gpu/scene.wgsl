// Lampshade scene shading. Ports the CPU renderer (src/render.rs) to the GPU:
// luminance in cd/m² for the lamp-on / daylight views, clay and false colour
// for the analysis views. Output goes to an Rgba16Float target that
// tonemap.wgsl turns into display pixels.
//
// alpha channel of the output:
//   translucent passes: monochrome unscattered transmittance of the wall
//   opaque pass:        1 = shade normally, 2 = section cap (rgb is display sRGB)

const PI: f32 = 3.14159265;

// mesh tags (src/mesh.rs Tag order, then extra scene parts)
const OUTER: u32 = 0u;
const INNER: u32 = 1u;
const RIM: u32 = 2u;
const MOUNT: u32 = 3u;
const PROXY: u32 = 4u;
const BULB: u32 = 5u;
const TABLE: u32 = 6u;
const WALL: u32 = 7u;

// view modes (src/render.rs ViewMode order)
const SHAPE: u32 = 0u;
const NIGHT: u32 = 1u;
const DAY: u32 = 2u;
const THICKNESS: u32 = 3u;
const OVERHANG: u32 = 4u;
const TEMPERATURE: u32 = 5u;

struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,        // xyz, mode
    key: vec4<f32>,        // key light direction xyz, key illuminance (day)
    room: vec4<f32>,       // room lux, lamp on, show room, temperature scale max
    bulb_c: vec4<f32>,     // bulb centre xyz, cos(dark cone)
    bulb_col: vec4<f32>,   // bulb colour rgb (lum 1), intensity (cd)
    l_bulb: vec4<f32>,     // bulb glass luminance rgb
    km: vec4<f32>,         // wall absorption k rgb (1/mm), scattering s
    km2: vec4<f32>,        // haze, ior, f0, largest wall thickness
    clay: vec4<f32>,       // filament colour (linear rgb)
    clip: vec4<f32>,       // section plane normal xyz (points to the kept side), offset
    clip_on: vec4<f32>,    // enabled, axis, 0, 0
    albedo: vec4<f32>,     // mount albedo rgb
    table_rect: vec4<f32>, // light map x0, x1, y0, y1
    wall_rect: vec4<f32>,  // light map x0, x1, z0, z1
    planes: vec4<f32>,     // z of the table, y of the wall
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var table_map: texture_2d<f32>;
@group(0) @binding(2) var wall_map: texture_2d<f32>;
@group(1) @binding(0) var shell_depth: texture_depth_2d;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) tag: u32,
    @location(3) thick: f32,
    @location(4) over: f32,
    @location(5) emit: vec3<f32>,
    @location(6) temp: f32,
};

struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) @interpolate(flat) tag: u32,
    @location(3) thick: f32,
    @location(4) over: f32,
    @location(5) emit: vec3<f32>,
    @location(6) temp: f32,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip_pos = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.nrm = v.nrm;
    o.tag = v.tag;
    o.thick = v.thick;
    o.over = v.over;
    o.emit = v.emit;
    o.temp = v.temp;
    return o;
}

fn mode() -> u32 {
    return u32(u.eye.w + 0.5);
}

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn lamp_on() -> bool {
    return u.room.y > 0.5;
}

fn ambient(n: vec3<f32>) -> f32 {
    var e = u.room.x;
    if (u.key.w > 0.0) {
        e += u.key.w * max(dot(n, u.key.xyz), 0.0);
    }
    return e;
}

// Direct light of the bulb (point source with a dark cone towards the socket).
fn direct_bulb(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if (!lamp_on()) {
        return vec3<f32>(0.0);
    }
    let to = u.bulb_c.xyz - p;
    let d = length(to);
    let l = to / d;
    let c = dot(n, l);
    if (c <= 0.0 || -l.z < -u.bulb_c.w) {
        return vec3<f32>(0.0);
    }
    let dm = d * 1e-3;
    return u.bulb_col.rgb * (u.bulb_col.w * c / (dm * dm));
}

// Kubelka–Munk reflectance and transmittance of a slab (black background).
fn km(k: f32, s: f32, t: f32) -> vec2<f32> {
    let ss = max(s, 1e-6);
    let kk = max(k, 1e-5);
    let a = 1.0 + kk / ss;
    let b = max(sqrt(a * a - 1.0), 1e-9);
    let x = min(b * ss * t, 40.0);
    let sh = sinh(x);
    let ch = cosh(x);
    let den = a * sh + b * ch;
    return vec2<f32>(sh / den, b / den);
}

struct Slab {
    ballistic: vec3<f32>,
    t_diff: vec3<f32>,
    r_diff: vec3<f32>,
};

fn slab(path: f32, t: f32) -> Slab {
    var o: Slab;
    let s = u.km.w;
    let haze = u.km2.x;
    for (var i = 0; i < 3; i++) {
        let k = u.km[i];
        let e = exp(-(k + s) * path);
        let rt = km(k, s, t);
        let rem = 1.0 - e;
        o.ballistic[i] = (1.0 - haze) * e;
        o.t_diff[i] = haze * e + rem * rt.y;
        o.r_diff[i] = rem * rt.x;
    }
    return o;
}

fn fresnel(c: f32) -> f32 {
    let f0 = u.km2.z;
    return f0 + (1.0 - f0) * pow(1.0 - clamp(c, 0.0, 1.0), 5.0);
}

fn cos_t(ci: f32) -> f32 {
    let ior = u.km2.y;
    return sqrt(max(1.0 - (1.0 - ci * ci) / (ior * ior), 0.05));
}

// Bilinear sample of a light map (texel centres), zero outside it.
fn map_sample(tex: texture_2d<f32>, rect: vec4<f32>, x: f32, y: f32) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(tex));
    let f = vec2<f32>((x - rect.x) / (rect.y - rect.x), (y - rect.z) / (rect.w - rect.z)) * dims - 0.5;
    if (f.x < -0.5 || f.y < -0.5 || f.x > dims.x - 0.5 || f.y > dims.y - 0.5) {
        return vec3<f32>(0.0);
    }
    let i0 = clamp(vec2<i32>(floor(f)), vec2<i32>(0), vec2<i32>(dims) - 1);
    let i1 = min(i0 + 1, vec2<i32>(dims) - 1);
    let t = clamp(f - vec2<f32>(i0), vec2<f32>(0.0), vec2<f32>(1.0));
    let a = mix(textureLoad(tex, i0, 0).rgb, textureLoad(tex, vec2<i32>(i1.x, i0.y), 0).rgb, t.x);
    let b = mix(textureLoad(tex, vec2<i32>(i0.x, i1.y), 0).rgb, textureLoad(tex, i1, 0).rgb, t.x);
    return mix(a, b, t.y);
}

fn turbo(x: f32) -> vec3<f32> {
    let r = 0.13572138 + x * (4.6153926 + x * (-42.66032 + x * (132.13108 + x * (-152.94239 + x * 59.28638))));
    let g = 0.09140261 + x * (2.1941884 + x * (4.8429666 + x * (-14.185033 + x * (4.2772986 + x * 2.829566))));
    let b = 0.1066733 + x * (12.641946 + x * (-60.58205 + x * (110.36277 + x * (-89.90311 + x * 27.34825))));
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Normal of the flat triangle under the pixel, facing the viewer.
fn flat_normal(world: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    var n = normalize(cross(dpdx(world), dpdy(world)));
    if (dot(n, d) > 0.0) {
        n = -n;
    }
    return n;
}

fn is_solid(tag: u32) -> bool {
    return tag <= PROXY;
}

// Hatched CAD-style colour of a cut face (display sRGB).
fn cap_color(p: vec3<f32>, tag: u32) -> vec3<f32> {
    let axis = u32(u.clip_on.y + 0.5);
    var uv = vec2<f32>(p.x, p.y);
    if (axis == 0u) {
        uv = vec2<f32>(p.y, p.z);
    } else if (axis == 1u) {
        uv = vec2<f32>(p.x, p.z);
    }
    var base = vec3<f32>(0.55);
    if (tag <= RIM) {
        base = vec3<f32>(0.96, 0.52, 0.16);
    } else if (tag == MOUNT) {
        base = vec3<f32>(0.30, 0.56, 0.96);
    }
    let stripe = fract((uv.x + uv.y) / 2.0) < 0.22;
    return select(base, base * 0.62, stripe);
}

const TABLE_RHO: vec3<f32> = vec3<f32>(0.46, 0.37, 0.28);
const WALL_RHO: vec3<f32> = vec3<f32>(0.80, 0.78, 0.74);
const PROXY_RHO: vec3<f32> = vec3<f32>(0.78, 0.78, 0.76);

// Clay view: studio lighting on the bare geometry (linear rgb).
fn clay(in: VOut, d: vec3<f32>) -> vec3<f32> {
    let key = normalize(vec3<f32>(-0.45, -0.55, 0.7));
    if (in.tag == BULB) {
        return vec3<f32>(0.75);
    }
    if (in.tag == TABLE) {
        let fade = exp(-pow(length(in.world.xy) / 900.0, 2.0));
        return vec3<f32>(0.05 + fade * 0.16);
    }
    var n = flat_normal(in.world, d);
    if (in.tag <= INNER) {
        n = normalize(in.nrm);
        if (dot(n, d) > 0.0) {
            n = -n;
        }
    }
    var base = u.clay.rgb;
    if (in.tag == PROXY) {
        base = vec3<f32>(0.55);
    }
    let lit = max(dot(n, key), 0.0);
    let fill = max(-dot(n, d), 0.0);
    let sky = 0.5 + 0.5 * n.z;
    let r = reflect(d, n);
    let spec = pow(max(dot(r, key), 0.0), 40.0) * 0.35;
    return base * (0.10 * sky + 0.70 * lit + 0.25 * fill) + vec3<f32>(spec);
}

// Analysis views: display colour directly.
fn false_color(in: VOut, d: vec3<f32>) -> vec3<f32> {
    if (in.tag == BULB) {
        return vec3<f32>(0.85);
    }
    let n = flat_normal(in.world, d);
    let shade = 0.35 + 0.65 * abs(dot(d, n));
    let m = mode();
    let temp_v = (in.temp - 25.0) / (u.room.w - 25.0);
    if (in.tag == PROXY) {
        return vec3<f32>(0.5 * shade);
    }
    if (in.tag == MOUNT) {
        if (m == TEMPERATURE) {
            return turbo(clamp(temp_v, 0.0, 1.0)) * shade;
        }
        return vec3<f32>(0.6 * shade);
    }
    var v = temp_v;
    if (m == THICKNESS) {
        v = min(in.thick / max(u.km2.w, 0.5), 1.0);
    } else if (m == OVERHANG) {
        v = in.over / 90.0;
    }
    return turbo(clamp(v, 0.0, 1.0)) * shade;
}

// Physical luminance (cd/m²) for the lamp-on and daylight views.
// Returns rgb and the wall's unscattered transmittance (shell only).
fn physical(in: VOut, d: vec3<f32>) -> vec4<f32> {
    let p = in.world;
    if (in.tag == BULB) {
        if (lamp_on()) {
            return vec4<f32>(u.l_bulb.rgb, 1.0);
        }
        return vec4<f32>(vec3<f32>(0.85) * ambient(-d) / PI, 1.0);
    }
    if (in.tag == TABLE) {
        var e = vec3<f32>(ambient(vec3<f32>(0.0, 0.0, 1.0)));
        if (lamp_on()) {
            e += map_sample(table_map, u.table_rect, p.x, p.y);
        }
        return vec4<f32>(e * TABLE_RHO / PI, 1.0);
    }
    if (in.tag == WALL) {
        var e = vec3<f32>(ambient(vec3<f32>(0.0, -1.0, 0.0)));
        if (lamp_on()) {
            e += map_sample(wall_map, u.wall_rect, p.x, p.z);
        }
        return vec4<f32>(e * WALL_RHO / PI, 1.0);
    }
    let nf = flat_normal(p, d);
    if (in.tag == MOUNT || in.tag == PROXY) {
        var rho = u.albedo.rgb;
        var e = direct_bulb(p, nf) + vec3<f32>(ambient(nf));
        if (in.tag == PROXY) {
            rho = PROXY_RHO;
            if (lamp_on()) {
                e += map_sample(table_map, u.table_rect, p.x, p.y) * (0.3 + 0.7 * max(nf.z, 0.0));
            }
        }
        return vec4<f32>(e * rho / PI, 1.0);
    }
    // shade wall
    var ns = nf;
    if (in.tag != RIM) {
        ns = normalize(in.nrm);
        if (dot(ns, nf) < 0.2) {
            ns = nf;
        }
    }
    let c = clamp(-dot(d, ns), 0.0, 1.0);
    let t_n = max(in.thick, 0.05);
    let sl = slab(t_n * 1.3, t_n);
    let fr = fresnel(c);
    var col = vec3<f32>(0.0);
    if (lamp_on()) {
        if (in.tag == RIM) {
            col += direct_bulb(p, nf) * sl.r_diff / PI;
        } else {
            col += in.emit;
        }
    }
    col += sl.r_diff * (ambient(ns) * (1.0 - fr) / PI);
    col += vec3<f32>(fr * u.room.x * 0.5 / PI);
    var transmit = 0.0;
    if (in.tag != RIM) {
        let b = slab(t_n / cos_t(c), t_n).ballistic;
        transmit = lum(b) * (1.0 - fr) * (1.0 - fr);
    }
    return vec4<f32>(col, transmit);
}

fn view_dir(p: vec3<f32>) -> vec3<f32> {
    return normalize(p - u.eye.xyz);
}

// The section plane cuts the lamp and the bulb, never the room planes.
fn clipped(p: vec3<f32>, tag: u32) -> bool {
    return u.clip_on.x > 0.5 && tag <= BULB && dot(u.clip.xyz, p) < u.clip.w;
}

// Opaque pass: everything in the analysis / clay views, everything but the
// translucent shade wall in the lamp-on and daylight views, and section caps.
@fragment
fn fs_opaque(in: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (clipped(in.world, in.tag)) {
        discard;
    }
    let d = view_dir(in.world);
    // a back face nearest to the eye means the eye ray is inside a solid: the cut face
    if (!front && u.clip_on.x > 0.5 && is_solid(in.tag)) {
        // where the eye ray crosses the plane
        let denom = dot(u.clip.xyz, d);
        var q = in.world;
        if (abs(denom) > 1e-6) {
            q = u.eye.xyz + d * ((u.clip.w - dot(u.clip.xyz, u.eye.xyz)) / denom);
        }
        return vec4<f32>(cap_color(q, in.tag), 2.0);
    }
    let m = mode();
    if (m == SHAPE) {
        return vec4<f32>(clay(in, d), 1.0);
    }
    if (m >= THICKNESS) {
        return vec4<f32>(false_color(in, d), 1.0);
    }
    return vec4<f32>(physical(in, d).rgb, 1.0);
}

// Translucent shade wall, drawn in two layers over the opaque scene:
// first everything behind the nearest wall surface, then the nearest one.
// Blend: dst = src.rgb + dst.rgb * src.a (src.a = transmittance).
@fragment
fn fs_far(in: VOut) -> @location(0) vec4<f32> {
    let nearest = textureLoad(shell_depth, vec2<i32>(in.clip_pos.xy), 0);
    if (in.clip_pos.z <= nearest + 1e-7) {
        discard;
    }
    return physical(in, view_dir(in.world));
}

@fragment
fn fs_near(in: VOut) -> @location(0) vec4<f32> {
    let nearest = textureLoad(shell_depth, vec2<i32>(in.clip_pos.xy), 0);
    if (in.clip_pos.z > nearest + 1e-7) {
        discard;
    }
    return physical(in, view_dir(in.world));
}
