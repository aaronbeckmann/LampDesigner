//! Light + heat simulation.
//!
//! * Direct light: analytic irradiance from the bulb (point source with a
//!   dark cone towards the socket) on every shade vertex, with shadow rays.
//! * Indirect light: Monte-Carlo photon tracing. Each wall hit is resolved
//!   with Fresnel reflection, unscattered (ballistic) transmission, diffuse
//!   transmission, diffuse reflection or absorption (Kubelka–Munk slab with
//!   the local wall thickness). Escaping photons build the far-field
//!   distribution and illuminance maps on the table and the wall behind.
//! * Heat: steady-state estimate of wall temperature from bulb IR radiation,
//!   absorbed light and the convective plume above the bulb.

use crate::bulb::Bulb;
use crate::material::{lum, Optics};
use crate::mesh::Tag;
use crate::scene::Scene;
use glam::Vec3;
use rayon::prelude::*;
use std::f32::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, Ordering};

pub const POLAR_BINS: usize = 36;
pub const AMBIENT_C: f32 = 25.0;

#[derive(Clone)]
pub struct Map2 {
    pub nx: usize,
    pub ny: usize,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub data: Vec<Vec3>,
}

impl Map2 {
    pub fn new(nx: usize, ny: usize, x0: f32, x1: f32, y0: f32, y1: f32) -> Map2 {
        Map2 { nx, ny, x0, x1, y0, y1, data: vec![Vec3::ZERO; nx * ny] }
    }

    pub fn cell(&self, x: f32, y: f32) -> Option<usize> {
        let fx = (x - self.x0) / (self.x1 - self.x0);
        let fy = (y - self.y0) / (self.y1 - self.y0);
        if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
            return None;
        }
        Some((fy * self.ny as f32) as usize * self.nx + (fx * self.nx as f32) as usize)
    }

    pub fn sample(&self, x: f32, y: f32) -> Vec3 {
        let fx = (x - self.x0) / (self.x1 - self.x0) * self.nx as f32 - 0.5;
        let fy = (y - self.y0) / (self.y1 - self.y0) * self.ny as f32 - 0.5;
        if fx < -0.5 || fy < -0.5 || fx > self.nx as f32 - 0.5 || fy > self.ny as f32 - 0.5 {
            return Vec3::ZERO;
        }
        let ix = (fx.floor() as i32).clamp(0, self.nx as i32 - 1) as usize;
        let iy = (fy.floor() as i32).clamp(0, self.ny as i32 - 1) as usize;
        let jx = (ix + 1).min(self.nx - 1);
        let jy = (iy + 1).min(self.ny - 1);
        let tx = (fx - ix as f32).clamp(0.0, 1.0);
        let ty = (fy - iy as f32).clamp(0.0, 1.0);
        let g = |x: usize, y: usize| self.data[y * self.nx + x];
        let a = g(ix, iy).lerp(g(jx, iy), tx);
        let b = g(ix, jy).lerp(g(jx, jy), tx);
        a.lerp(b, ty)
    }

    pub fn cell_area_m2(&self) -> f32 {
        (self.x1 - self.x0) / self.nx as f32 * (self.y1 - self.y0) / self.ny as f32 * 1e-6
    }

    pub fn blur(&mut self, passes: usize) {
        let (nx, ny) = (self.nx, self.ny);
        for _ in 0..passes {
            let src = self.data.clone();
            for y in 0..ny {
                for x in 0..nx {
                    let xl = x.saturating_sub(1);
                    let xr = (x + 1).min(nx - 1);
                    self.data[y * nx + x] = (src[y * nx + xl] + src[y * nx + x] * 2.0 + src[y * nx + xr]) * 0.25;
                }
            }
            let src = self.data.clone();
            for y in 0..ny {
                let yd = y.saturating_sub(1);
                let yu = (y + 1).min(ny - 1);
                for x in 0..nx {
                    self.data[y * nx + x] = (src[yd * nx + x] + src[y * nx + x] * 2.0 + src[yu * nx + x]) * 0.25;
                }
            }
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct Stats {
    /// Fraction of bulb lumens leaving the lamp.
    pub efficiency: f32,
    pub via_top: f32,
    pub via_bottom: f32,
    pub via_wall: f32,
    pub absorbed: f32,
    /// Area-weighted mean outer luminance (cd/m²).
    pub glow_mean: f32,
    pub glow_max: f32,
    pub glow_min: f32,
    /// Luminance of the bulb seen through the thinnest wall (cd/m²).
    pub bulb_through: f32,
    pub table_lux_25: f32,
    pub max_temp: f32,
    pub mount_temp: f32,
}

pub struct SimResult {
    /// Luminance each shell vertex emits on its own side due to the lamp (cd/m²).
    pub l_emit: Vec<Vec3>,
    pub temp: Vec<f32>,
    pub table: Map2,
    pub wall: Map2,
    /// Candela per polar bin (0° = straight up).
    pub polar: Vec<f32>,
    pub stats: Stats,
}

impl SimResult {
    /// Placeholder when the light simulation is switched off.
    pub fn empty(sc: &Scene) -> SimResult {
        let nv = sc.mesh.pos.len();
        SimResult {
            l_emit: vec![Vec3::ZERO; nv],
            temp: vec![AMBIENT_C; nv],
            table: Map2::new(2, 2, -700.0, 700.0, -700.0, sc.wall_y),
            wall: Map2::new(2, 2, -700.0, 700.0, sc.z_table, sc.z_table + 900.0),
            polar: vec![0.0; POLAR_BINS],
            stats: Stats { max_temp: AMBIENT_C, mount_temp: AMBIENT_C, ..Default::default() },
        }
    }
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
    }
}

fn cosine_dir(n: Vec3, rng: &mut Rng) -> Vec3 {
    let r1 = rng.f();
    let r2 = rng.f();
    let phi = TAU * r1;
    let r = r2.sqrt();
    let (t, b) = n.any_orthonormal_pair();
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - r2).max(0.0).sqrt()).normalize()
}

fn mean(v: Vec3) -> f32 {
    (v.x + v.y + v.z) / 3.0
}

struct Acc {
    vflux: Vec<Vec3>,
    table: Vec<Vec3>,
    wall: Vec<Vec3>,
    polar: Vec<f64>,
    top: f64,
    bottom: f64,
    through: f64,
    absorbed: f64,
    total: f64,
}

impl Acc {
    fn new(nv: usize, nt: usize, nw: usize) -> Acc {
        Acc {
            vflux: vec![Vec3::ZERO; nv],
            table: vec![Vec3::ZERO; nt],
            wall: vec![Vec3::ZERO; nw],
            polar: vec![0.0; POLAR_BINS],
            top: 0.0,
            bottom: 0.0,
            through: 0.0,
            absorbed: 0.0,
            total: 0.0,
        }
    }

    fn merge(mut self, o: Acc) -> Acc {
        for (a, b) in self.vflux.iter_mut().zip(&o.vflux) {
            *a += *b;
        }
        for (a, b) in self.table.iter_mut().zip(&o.table) {
            *a += *b;
        }
        for (a, b) in self.wall.iter_mut().zip(&o.wall) {
            *a += *b;
        }
        for (a, b) in self.polar.iter_mut().zip(&o.polar) {
            *a += *b;
        }
        self.top += o.top;
        self.bottom += o.bottom;
        self.through += o.through;
        self.absorbed += o.absorbed;
        self.total += o.total;
        self
    }
}

struct Tracer<'a> {
    sc: &'a Scene,
    op: &'a Optics,
    color: Vec3,
    cos_dark: f32,
    table: &'a Map2,
    wall: &'a Map2,
    mount_albedo: Vec3,
}

const EPS: f32 = 0.002;

impl Tracer<'_> {
    fn escape(&self, acc: &mut Acc, o: Vec3, d: Vec3, w: Vec3, through: bool) {
        let l = lum(w) as f64;
        if through {
            acc.through += l;
        } else if d.z > 0.0 {
            acc.top += l;
        } else {
            acc.bottom += l;
        }
        let th = d.z.clamp(-1.0, 1.0).acos();
        let bin = ((th / PI * POLAR_BINS as f32) as usize).min(POLAR_BINS - 1);
        acc.polar[bin] += l;
        let sc = self.sc;
        let mut best = f32::INFINITY;
        let mut target: Option<(bool, usize)> = None;
        if d.z < -1e-6 && o.z > sc.z_table {
            let t = (sc.z_table - o.z) / d.z;
            let q = o + d * t;
            if q.y < sc.wall_y {
                if let Some(i) = self.table.cell(q.x, q.y) {
                    best = t;
                    target = Some((true, i));
                }
            }
        }
        if d.y > 1e-6 && o.y < sc.wall_y {
            let t = (sc.wall_y - o.y) / d.y;
            if t < best {
                let q = o + d * t;
                if q.z > sc.z_table {
                    if let Some(i) = self.wall.cell(q.x, q.z) {
                        target = Some((false, i));
                    }
                }
            }
        }
        match target {
            Some((true, i)) => acc.table[i] += w,
            Some((false, i)) => acc.wall[i] += w,
            None => {}
        }
    }

    fn photon(&self, rng: &mut Rng, acc: &mut Acc) {
        let sc = self.sc;
        let z = 1.0 - rng.f() * (1.0 + self.cos_dark);
        let phi = TAU * rng.f();
        let rxy = (1.0 - z * z).max(0.0).sqrt();
        let mut d = Vec3::new(rxy * phi.cos(), rxy * phi.sin(), z);
        let mut o = sc.bulb_c;
        let mut w = self.color;
        let mut bounce = 0u32;
        let mut through = false;
        acc.total += 1.0;
        for _ in 0..64 {
            let Some(h) = sc.bvh.intersect(o, d, 1e-4, 1e7, false) else {
                self.escape(acc, o, d, w, through);
                return;
            };
            let tri = h.tri as usize;
            let tag = sc.mesh.tags[tri];
            let n = sc.fnormal[tri];
            let p = o + d * h.t;
            if d.dot(n) >= 0.0 {
                // started inside a solid (overlapping parts): pass through
                o = p + d * EPS;
                continue;
            }
            if bounce > 0 && tag != Tag::Rim {
                let t = sc.mesh.tris[tri];
                let (wa, wb, wc) = (1.0 - h.u - h.v, h.u, h.v);
                acc.vflux[t[0] as usize] += w * wa;
                acc.vflux[t[1] as usize] += w * wb;
                acc.vflux[t[2] as usize] += w * wc;
            }
            match tag {
                Tag::Proxy | Tag::Mount | Tag::Rim => {
                    let alb = if tag == Tag::Proxy { Vec3::splat(0.7) } else { self.mount_albedo };
                    let pr = mean(alb).clamp(0.01, 0.99);
                    if rng.f() < pr {
                        w *= alb / pr;
                        d = cosine_dir(n, rng);
                        o = p + n * EPS;
                    } else {
                        acc.absorbed += lum(w) as f64;
                        return;
                    }
                }
                Tag::Outer | Tag::Inner => {
                    let cos_i = (-d.dot(n)).clamp(0.0, 1.0);
                    if rng.f() < self.op.fresnel(cos_i) {
                        d -= n * (2.0 * d.dot(n));
                        o = p + n * EPS;
                        bounce += 1;
                        continue;
                    }
                    let t_n = sc.interp(tri, h.u, h.v, &sc.thickness).max(0.05);
                    let sl = self.op.slab(t_n / self.op.cos_t(cos_i), t_n);
                    let pb = mean(sl.ballistic);
                    let pt = mean(sl.t_diff);
                    let prr = mean(sl.r_diff);
                    let u = rng.f();
                    let inside = p - n * EPS;
                    if u < pb {
                        match sc.bvh.intersect(inside, d, 1e-4, 20.0 * t_n + 5.0, false) {
                            Some(h2) => {
                                let n2 = sc.fnormal[h2.tri as usize];
                                w *= sl.ballistic / pb;
                                o = inside + d * h2.t + n2 * EPS;
                                through = true;
                            }
                            None => {
                                acc.absorbed += lum(w) as f64;
                                return;
                            }
                        }
                    } else if u < pb + pt {
                        match sc.bvh.intersect(inside, -n, 1e-4, 20.0 * t_n + 5.0, false) {
                            Some(h2) => {
                                let n2 = sc.fnormal[h2.tri as usize];
                                w *= sl.t_diff / pt;
                                o = inside - n * h2.t + n2 * EPS;
                                d = cosine_dir(n2, rng);
                                through = true;
                            }
                            None => {
                                acc.absorbed += lum(w) as f64;
                                return;
                            }
                        }
                    } else if u < pb + pt + prr {
                        w *= sl.r_diff / prr;
                        d = cosine_dir(n, rng);
                        o = p + n * EPS;
                    } else {
                        acc.absorbed += lum(w) as f64;
                        return;
                    }
                }
            }
            bounce += 1;
            let l = lum(w);
            if l < 0.1 {
                if rng.f() < 0.5 {
                    acc.absorbed += l as f64;
                    return;
                }
                w *= 2.0;
            }
        }
        acc.absorbed += lum(w) as f64;
    }
}

fn blur_grid(vals: &mut [Vec3], base: usize, nt: usize, nz: usize, passes: usize) {
    for _ in 0..passes {
        let src: Vec<Vec3> = vals[base..base + nt * nz].to_vec();
        for iz in 0..nz {
            for it in 0..nt {
                let g = |z: usize, t: usize| src[z * nt + (t % nt)];
                vals[base + iz * nt + it] = (g(iz, it + nt - 1) + g(iz, it) * 2.0 + g(iz, it + 1)) * 0.25;
            }
        }
        let src: Vec<Vec3> = vals[base..base + nt * nz].to_vec();
        for iz in 0..nz {
            let dn = iz.saturating_sub(1);
            let up = (iz + 1).min(nz - 1);
            for it in 0..nt {
                vals[base + iz * nt + it] =
                    (src[dn * nt + it] + src[iz * nt + it] * 2.0 + src[up * nt + it]) * 0.25;
            }
        }
    }
}

/// Runs the simulation. `photons == 0` gives a fast direct-light-only result.
/// Returns `None` when cancelled.
/// `map_res`: cells per side of the table/wall light maps.
pub fn run(
    sc: &Scene,
    op: &Optics,
    bulb: &Bulb,
    photons: usize,
    map_res: usize,
    cancel: &AtomicBool,
) -> Option<SimResult> {
    let nv = sc.mesh.pos.len();
    let g = sc.grid;
    let map_res = map_res.clamp(32, 1024);
    let mut table = Map2::new(map_res, map_res, -700.0, 700.0, -700.0, sc.wall_y);
    let mut wall = Map2::new(map_res, map_res * 9 / 14, -700.0, 700.0, sc.z_table, sc.z_table + 900.0);
    let tracer = Tracer {
        sc,
        op,
        color: bulb.color(),
        cos_dark: bulb.dark_cone_deg.to_radians().cos(),
        table: &table,
        wall: &wall,
        mount_albedo: op.albedo(),
    };

    const CHUNK: usize = 2048;
    let nchunks = photons.div_ceil(CHUNK);
    let (nt_len, nw_len) = (table.data.len(), wall.data.len());
    let acc = (0..nchunks)
        .into_par_iter()
        .fold(
            || Acc::new(nv, nt_len, nw_len),
            |mut acc, ci| {
                if cancel.load(Ordering::Relaxed) {
                    return acc;
                }
                let mut rng = Rng::new(ci as u64 * 7919 + 17);
                let n = CHUNK.min(photons - ci * CHUNK);
                for _ in 0..n {
                    tracer.photon(&mut rng, &mut acc);
                }
                acc
            },
        )
        .reduce(|| Acc::new(nv, nt_len, nw_len), Acc::merge);
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    let per = if photons > 0 { bulb.lumens / photons as f32 } else { 0.0 };

    // --- irradiance: direct (analytic) + indirect (photons) ---
    let color = bulb.color();
    let nshell = g.shell_verts();
    let direct: Vec<(Vec3, f32)> = (0..nshell)
        .into_par_iter()
        .map(|v| {
            let p = sc.mesh.pos[v];
            let n = sc.vnormal[v];
            let to = sc.bulb_c - p;
            let dist = to.length();
            let l = to / dist;
            let cos = n.dot(l);
            if cos <= 0.0 || sc.bvh.occluded(p + n * 0.01, l, dist - 0.01) {
                return (Vec3::ZERO, 0.0);
            }
            let dm2 = (dist * 1e-3).powi(2);
            let i = bulb.intensity(-l.z);
            (color * (i * cos / dm2), cos / (4.0 * PI * dm2))
        })
        .collect();
    let mut e_in = vec![Vec3::ZERO; nv];
    for ((e, &flux), &area) in e_in.iter_mut().zip(&acc.vflux).zip(&sc.varea) {
        if flux != Vec3::ZERO && area > 0.0 {
            *e = flux * (per / (area * 1e-6));
        }
    }
    // Indirect light is smooth; keep the same physical smoothing on finer
    // meshes (fewer photons per vertex), blur radius ~ sqrt(passes).
    let density = ((g.nt * g.nz) as f32 / (224.0 * 100.0)).max(1.0);
    let passes = (3.0 * density).round() as usize;
    blur_grid(&mut e_in, g.outer0, g.nt, g.nz, passes);
    blur_grid(&mut e_in, g.inner0, g.nt, g.nz, passes);
    for v in 0..nshell {
        e_in[v] += direct[v].0;
    }

    // --- emitted luminance of the shell ---
    let mut l_emit = vec![Vec3::ZERO; nv];
    let fr = 1.0 - op.f0 * 1.2;
    for iz in 0..g.nz {
        for it in 0..g.nt {
            let (o, i) = (g.o(iz, it), g.i(iz, it));
            let t = sc.thickness[o].max(0.05);
            let sl = op.slab(t * 1.3, t);
            l_emit[o] = (e_in[i] * sl.t_diff + e_in[o] * sl.r_diff) * (fr / PI);
            l_emit[i] = (e_in[i] * sl.r_diff + e_in[o] * sl.t_diff) * (fr / PI);
        }
    }

    // --- temperature ---
    let (p_ir, p_conv) = bulb.heat();
    let qc_kw = p_conv / 1000.0;
    let rb_m = bulb.rx.max(bulb.rz) * 1e-3;
    let (h_in, h_out) = (8.0f32, 10.0f32);
    let mut temp = vec![AMBIENT_C; nv];
    let mut max_temp = AMBIENT_C;
    for iz in 0..g.nz {
        for it in 0..g.nt {
            let (o, i) = (g.o(iz, it), g.i(iz, it));
            let p = sc.mesh.pos[i];
            let t = sc.thickness[o].max(0.05);
            let sl = op.slab(t, t);
            let a_frac = (1.0 - mean(sl.ballistic + sl.t_diff + sl.r_diff)).max(0.0);
            let q_ir = p_ir * direct[i].1 * 0.9;
            let q_vis = lum(e_in[i]) / 300.0 * a_frac;
            let zb = (p.z - sc.bulb_c.z) * 1e-3;
            let z_eff = zb + 2.0 * rb_m;
            let dta = if z_eff > 0.005 {
                let dtc = (25.0 * qc_kw.powf(2.0 / 3.0) * z_eff.powf(-5.0 / 3.0)).min(150.0);
                let bw = 0.15 * z_eff;
                let r = p.truncate().length() * 1e-3;
                dtc * (-(r / bw).powi(2)).exp()
            } else {
                0.0
            };
            let tw = AMBIENT_C + (q_ir + q_vis + h_in * dta) / (h_in + h_out);
            temp[o] = tw;
            temp[i] = tw;
            max_temp = max_temp.max(tw);
        }
    }
    let clamps_cap = sc.mount_hole_r < 17.0;
    let mount_temp = if clamps_cap { bulb.cap_temp() } else { AMBIENT_C + 0.5 * (bulb.cap_temp() - AMBIENT_C) };
    for (t, p) in temp.iter_mut().zip(&sc.mesh.pos).skip(nshell) {
        if p.z >= sc.shade_z0 - 0.01 && p.z <= sc.shade_z0 + 60.0 {
            *t = mount_temp;
        }
    }

    // --- maps & far field ---
    let ta = table.cell_area_m2();
    for (dst, src) in table.data.iter_mut().zip(&acc.table) {
        *dst = *src * (per / ta);
    }
    let wa = wall.cell_area_m2();
    for (dst, src) in wall.data.iter_mut().zip(&acc.wall) {
        *dst = *src * (per / wa);
    }
    // finer maps need more passes for the same physical smoothing
    let rel = (map_res as f32 / 140.0).powf(1.5);
    table.blur((3.0 * rel).round() as usize);
    wall.blur((5.0 * rel).round() as usize);
    let polar: Vec<f32> = (0..POLAR_BINS)
        .map(|b| {
            let t0 = b as f32 / POLAR_BINS as f32 * PI;
            let t1 = (b + 1) as f32 / POLAR_BINS as f32 * PI;
            let omega = TAU * (t0.cos() - t1.cos());
            acc.polar[b] as f32 * per / omega
        })
        .collect();

    // --- stats ---
    let mut wsum = 0.0f32;
    let mut gsum = 0.0f32;
    let mut gmax = 0.0f32;
    let mut gmin = f32::INFINITY;
    for iz in 2..g.nz.saturating_sub(2) {
        for it in 0..g.nt {
            let o = g.o(iz, it);
            let l = lum(l_emit[o]);
            gsum += l * sc.varea[o];
            wsum += sc.varea[o];
            gmax = gmax.max(l);
            gmin = gmin.min(l);
        }
    }
    let tot = acc.total.max(1.0);
    let table_lux_25 = (0..64)
        .map(|k| {
            let a = TAU * k as f32 / 64.0;
            lum(table.sample(250.0 * a.cos(), 250.0 * a.sin()))
        })
        .sum::<f32>()
        / 64.0;
    let fr2 = (1.0 - op.f0).powi(2);
    let stats = Stats {
        efficiency: ((acc.top + acc.bottom + acc.through) / tot) as f32,
        via_top: (acc.top / tot) as f32,
        via_bottom: (acc.bottom / tot) as f32,
        via_wall: (acc.through / tot) as f32,
        absorbed: (acc.absorbed / tot) as f32,
        glow_mean: if wsum > 0.0 { gsum / wsum } else { 0.0 },
        glow_max: gmax,
        glow_min: if gmin.is_finite() { gmin } else { 0.0 },
        bulb_through: lum(op.ballistic(sc.min_thickness)) * fr2 * bulb.luminance(),
        table_lux_25,
        max_temp,
        mount_temp,
    };

    Some(SimResult { l_emit, temp, table, wall, polar, stats })
}
