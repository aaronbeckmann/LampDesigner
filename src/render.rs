//! CPU ray tracer for the preview: physically based luminance (cd/m²),
//! tone mapped like a camera, plus false-colour analysis views.

use crate::bulb::Bulb;
use crate::material::{lum, Optics};
use crate::mesh::Tag;
use crate::scene::Scene;
use crate::sim::SimResult;
use glam::Vec3;
use rayon::prelude::*;
use std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    /// Fast clay shading of the geometry (no light simulation needed).
    Shape,
    /// Lamp on in a dark room.
    Night,
    /// Lamp off in a lit room.
    Day,
    Thickness,
    Overhang,
    Temperature,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: Vec3,
    pub fov_deg: f32,
}

impl Camera {
    pub fn for_scene(sc: &Scene) -> Camera {
        let h = sc.shade_z1 - sc.z_table;
        Camera {
            yaw: -90f32.to_radians(),
            pitch: 12f32.to_radians(),
            dist: (h * 1.9).max(sc.max_r * 4.0),
            target: Vec3::new(0.0, 0.0, (sc.shade_z1 + sc.z_table) * 0.5 + 10.0),
            fov_deg: 40.0,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.target
            + self.dist
                * Vec3::new(self.pitch.cos() * self.yaw.cos(), self.pitch.cos() * self.yaw.sin(), self.pitch.sin())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderSettings {
    pub mode: ViewMode,
    /// Ambient room illuminance (lux).
    pub room_lux: f32,
    /// Exposure compensation (EV).
    pub ev: f32,
    pub show_room: bool,
    /// Anti-aliasing samples per pixel axis (1 = off, 4 = 16 rays/pixel).
    pub samples: u32,
    /// Linear RGB of the filament, used by the Shape view.
    pub clay: Vec3,
    /// Optional section plane: geometry on the removed side is cut away.
    pub clip: Option<Clip>,
}

/// Section plane perpendicular to `axis` (0 = X, 1 = Y, 2 = Z) at `pos` mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub axis: usize,
    pub pos: f32,
    /// Keep the part with coordinate >= pos (else <= pos).
    pub keep_above: bool,
}

impl Clip {
    pub fn keeps(&self, p: Vec3) -> bool {
        let x = p[self.axis];
        if self.keep_above {
            x >= self.pos
        } else {
            x <= self.pos
        }
    }
}

enum Prim {
    Miss,
    Hit(crate::bvh::Hit),
    /// The ray crosses the section plane inside a solid with this tag.
    Cap(f32, Tag),
}

/// Pixel kind for section caps (colour is already display sRGB).
const CAP: Kind = 4;

pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    pub exposure: f32,
}

struct Ctx<'a> {
    sc: &'a Scene,
    sim: &'a SimResult,
    op: &'a Optics,
    bulb: &'a Bulb,
    rs: &'a RenderSettings,
    lamp_on: bool,
    l_bulb: Vec3,
    bulb_col: Vec3,
    key_dir: Vec3,
    e_key: f32,
    shell_albedo_cache: Vec3,
    temp_max: f32,
}

const TABLE_RHO: Vec3 = Vec3::new(0.46, 0.37, 0.28);
const WALL_RHO: Vec3 = Vec3::new(0.80, 0.78, 0.74);
const PROXY_RHO: Vec3 = Vec3::new(0.78, 0.78, 0.76);

/// 0 = background, 1 = shade, 2 = bulb, 3 = other, 4 = section cap
type Kind = u8;

const PRIMARY_DEPTH: u32 = 4;

impl Ctx<'_> {
    fn ambient(&self, n: Vec3) -> f32 {
        let mut e = self.rs.room_lux;
        if self.e_key > 0.0 {
            e += self.e_key * n.dot(self.key_dir).max(0.0);
        }
        e
    }

    fn direct_bulb(&self, p: Vec3, n: Vec3) -> Vec3 {
        if !self.lamp_on {
            return Vec3::ZERO;
        }
        let to = self.sc.bulb_c - p;
        let dist = to.length();
        let l = to / dist;
        let cos = n.dot(l);
        if cos <= 0.0 || self.sc.bvh.occluded(p + n * 0.05, l, dist) {
            return Vec3::ZERO;
        }
        self.bulb_col * (self.bulb.intensity(-l.z) * cos / (dist * 1e-3).powi(2))
    }

    /// First surface along the ray, honouring the section plane on primary rays.
    fn first_hit(&self, o: Vec3, d: Vec3, primary: bool) -> Prim {
        let sc = self.sc;
        let clip = match (primary, self.rs.clip) {
            (true, Some(c)) => c,
            _ => {
                return match sc.bvh.intersect(o, d, 1e-4, 1e7, false) {
                    Some(h) => Prim::Hit(h),
                    None => Prim::Miss,
                }
            }
        };
        let dc = d[clip.axis];
        let t_plane = if dc.abs() > 1e-9 { (clip.pos - o[clip.axis]) / dc } else { -1.0 };
        let mut tmin = 1e-4;
        for _ in 0..256 {
            let Some(h) = sc.bvh.intersect(o, d, tmin, 1e7, false) else {
                return Prim::Miss;
            };
            let p = o + d * h.t;
            if !clip.keeps(p) {
                tmin = h.t + 1e-3;
                continue;
            }
            let tri = h.tri as usize;
            if d.dot(sc.fnormal[tri]) > 0.0 && t_plane > 0.0 && t_plane < h.t {
                return Prim::Cap(t_plane, sc.mesh.tags[tri]);
            }
            return Prim::Hit(h);
        }
        Prim::Miss
    }

    fn bulb_t(&self, o: Vec3, d: Vec3, primary: bool) -> Option<f32> {
        match (primary, self.rs.clip) {
            (true, Some(c)) => {
                let (t0, t1) = self.sc.hit_bulb_both(o, d)?;
                [t0, t1].into_iter().find(|&t| t > 1e-3 && c.keeps(o + d * t))
            }
            _ => self.sc.hit_bulb(o, d),
        }
    }

    /// Hatched CAD-style colour of a cut face (display sRGB 0..1).
    fn cap_color(&self, p: Vec3, tag: Tag) -> Vec3 {
        let axis = self.rs.clip.map_or(2, |c| c.axis);
        let (u, v) = match axis {
            0 => (p.y, p.z),
            1 => (p.x, p.z),
            _ => (p.x, p.y),
        };
        let base = match tag {
            Tag::Outer | Tag::Inner | Tag::Rim => Vec3::new(0.96, 0.52, 0.16),
            Tag::Mount => Vec3::new(0.30, 0.56, 0.96),
            Tag::Proxy => Vec3::splat(0.55),
        };
        let stripe = ((u + v) / 2.0).rem_euclid(1.0) < 0.22;
        if stripe {
            base * 0.62
        } else {
            base
        }
    }

    fn background(&self) -> Vec3 {
        Vec3::new(0.9, 0.95, 1.0) * (self.rs.room_lux * 0.3 / PI)
    }

    fn radiance(&self, o: Vec3, d: Vec3, depth: u32) -> (Vec3, Kind) {
        let sc = self.sc;
        let primary = depth == PRIMARY_DEPTH;
        let prim = self.first_hit(o, d, primary);
        let t_prim = match prim {
            Prim::Hit(h) => h.t,
            Prim::Cap(t, _) => t,
            Prim::Miss => f32::INFINITY,
        };
        let tb = self.bulb_t(o, d, primary).filter(|&t| t < t_prim);
        let hit = match prim {
            Prim::Cap(t, tag) if tb.is_none() => return (self.cap_color(o + d * t, tag), CAP),
            Prim::Hit(h) => Some(h),
            _ => None,
        };
        let mut best = hit.map_or(f32::INFINITY, |h| h.t);
        if tb.is_some() {
            return if self.lamp_on {
                (self.l_bulb, 2)
            } else {
                (Vec3::splat(0.85) * self.ambient(-d) / PI, 2)
            };
        }
        // room planes
        let mut plane: Option<(f32, bool)> = None;
        if self.rs.show_room {
            if d.z < -1e-6 && o.z > sc.z_table {
                let t = (sc.z_table - o.z) / d.z;
                if t < best {
                    best = t;
                    plane = Some((t, true));
                }
            }
            if d.y > 1e-6 && o.y < sc.wall_y {
                let t = (sc.wall_y - o.y) / d.y;
                if t < best && (o + d * t).z > sc.z_table {
                    plane = Some((t, false));
                }
            }
        }
        if let Some((t, is_table)) = plane {
            let q = o + d * t;
            let (e_map, n, rho) = if is_table {
                (if self.lamp_on { self.sim.table.sample(q.x, q.y) } else { Vec3::ZERO }, Vec3::Z, TABLE_RHO)
            } else {
                (if self.lamp_on { self.sim.wall.sample(q.x, q.z) } else { Vec3::ZERO }, -Vec3::Y, WALL_RHO)
            };
            let e = e_map + Vec3::splat(self.ambient(n));
            return (e * rho / PI, 3);
        }
        let Some(h) = hit else {
            return (self.background(), 0);
        };
        let tri = h.tri as usize;
        let tag = sc.mesh.tags[tri];
        let ng = sc.fnormal[tri];
        let p = o + d * h.t;
        if d.dot(ng) > 0.0 {
            // inside a solid: continue
            if depth == 0 {
                return (Vec3::ZERO, 3);
            }
            return self.radiance(p + d * 0.002, d, depth - 1);
        }
        match tag {
            Tag::Proxy | Tag::Mount => {
                let rho = if tag == Tag::Proxy { PROXY_RHO } else { self.shell_albedo_cache };
                let mut e = self.direct_bulb(p, ng) + Vec3::splat(self.ambient(ng));
                if self.lamp_on && tag == Tag::Proxy {
                    // light arriving at table level (photon map) also reaches the lamp foot
                    e += self.sim.table.sample(p.x, p.y) * (0.3 + 0.7 * ng.z.max(0.0));
                }
                (e * rho / PI, 3)
            }
            Tag::Outer | Tag::Inner | Tag::Rim => {
                let mut ns = if tag == Tag::Rim { ng } else { sc.interp(tri, h.u, h.v, &sc.vnormal).normalize_or_zero() };
                if ns.dot(ng) < 0.2 {
                    ns = ng;
                }
                let cos = (-d.dot(ns)).clamp(0.0, 1.0);
                let t_n = sc.interp(tri, h.u, h.v, &sc.thickness).max(0.05);
                let sl = self.op.slab(t_n * 1.3, t_n);
                let fr = self.op.fresnel(cos);
                let mut col = Vec3::ZERO;
                if self.lamp_on {
                    if tag == Tag::Rim {
                        col += self.direct_bulb(p, ng) * sl.r_diff / PI;
                    } else {
                        col += sc.interp(tri, h.u, h.v, &self.sim.l_emit);
                    }
                }
                col += sl.r_diff * (self.ambient(ns) * (1.0 - fr) / PI);
                col += Vec3::splat(fr * self.rs.room_lux * 0.5 / PI);
                if tag != Tag::Rim && depth > 0 {
                    let cos_t = self.op.cos_t(cos);
                    let b = self.op.ballistic(t_n / cos_t) * (1.0 - fr) * (1.0 - fr);
                    if b.max_element() > 0.002 {
                        let inside = p - ng * 0.002;
                        if let Some(h2) = sc.bvh.intersect(inside, d, 1e-4, 30.0 * t_n + 5.0, false) {
                            let q = inside + d * h2.t + d * 0.002;
                            let (behind, _) = self.radiance(q, d, depth - 1);
                            col += b * behind;
                        }
                    }
                }
                (col, 1)
            }
        }
    }

    /// Clay view: studio lighting on the bare geometry (linear RGB).
    /// Primary hit for the LDR views: Ok(hit) or Err((colour, kind)) for
    /// bulb / section cap / nothing.
    fn ldr_primary(&self, o: Vec3, d: Vec3) -> Result<Option<crate::bvh::Hit>, (Vec3, Kind)> {
        let prim = self.first_hit(o, d, true);
        let t_prim = match prim {
            Prim::Hit(h) => h.t,
            Prim::Cap(t, _) => t,
            Prim::Miss => f32::INFINITY,
        };
        if self.bulb_t(o, d, true).is_some_and(|t| t < t_prim) {
            return Err((Vec3::splat(0.75), 2));
        }
        match prim {
            Prim::Cap(t, tag) => Err((self.cap_color(o + d * t, tag), CAP)),
            Prim::Hit(h) => Ok(Some(h)),
            Prim::Miss => Ok(None),
        }
    }

    fn clay(&self, o: Vec3, d: Vec3) -> (Vec3, Kind) {
        let sc = self.sc;
        let key = Vec3::new(-0.45, -0.55, 0.7).normalize();
        let hit = match self.ldr_primary(o, d) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let t_hit = hit.map_or(f32::INFINITY, |h| h.t);
        if self.rs.show_room && d.z < -1e-6 && o.z > sc.z_table {
            let t = (sc.z_table - o.z) / d.z;
            if t < t_hit {
                let q = o + d * t;
                let shadow = sc.bvh.occluded(q + Vec3::Z * 0.1, key, 1e5);
                let fade = (-(q.truncate().length() / 900.0).powi(2)).exp();
                return (Vec3::splat(0.05 + fade * if shadow { 0.06 } else { 0.16 }), 3);
            }
        }
        let Some(h) = hit else {
            let g = 0.5 + 0.5 * d.z;
            return (Vec3::new(0.045, 0.05, 0.06) * (0.6 + g), 0);
        };
        let tri = h.tri as usize;
        let tag = sc.mesh.tags[tri];
        let ng = sc.fnormal[tri];
        let mut n = match tag {
            Tag::Outer | Tag::Inner => {
                let ns = sc.interp(tri, h.u, h.v, &sc.vnormal).normalize_or_zero();
                if ns.dot(ng) > 0.2 {
                    ns
                } else {
                    ng
                }
            }
            _ => ng,
        };
        if n.dot(d) > 0.0 {
            n = -n;
        }
        let base = match tag {
            Tag::Proxy => Vec3::splat(0.55),
            _ => self.rs.clay,
        };
        let p = o + d * h.t;
        let shadowed = sc.bvh.occluded(p + n * 0.05, key, 1e5);
        let lit = n.dot(key).max(0.0) * if shadowed { 0.35 } else { 1.0 };
        let fill = (-n.dot(d)).max(0.0);
        let sky = 0.5 + 0.5 * n.z;
        let r = d - n * (2.0 * d.dot(n));
        let spec = r.dot(key).max(0.0).powi(40) * 0.35 * (!shadowed) as u8 as f32;
        (base * (0.10 * sky + 0.70 * lit + 0.25 * fill) + Vec3::splat(spec), 1)
    }

    /// Analysis views: LDR colour directly.
    fn false_color(&self, o: Vec3, d: Vec3) -> (Vec3, Kind) {
        if self.rs.mode == ViewMode::Shape {
            return self.clay(o, d);
        }
        let sc = self.sc;
        let h = match self.ldr_primary(o, d) {
            Ok(Some(h)) => h,
            Ok(None) => return (Vec3::splat(0.12), 0),
            Err((c, CAP)) => return (c, CAP),
            Err(_) => return (Vec3::splat(0.85), 2),
        };
        let tri = h.tri as usize;
        let ng = sc.fnormal[tri];
        let shade = 0.35 + 0.65 * (-d.dot(ng)).abs();
        let tag = sc.mesh.tags[tri];
        let v = match (tag, self.rs.mode) {
            (Tag::Proxy, _) => return (Vec3::splat(0.5 * shade), 3),
            (Tag::Mount, ViewMode::Temperature) => {
                let t = sc.interp(tri, h.u, h.v, &self.sim.temp);
                (t - 25.0) / (self.temp_max - 25.0)
            }
            (Tag::Mount, _) => return (Vec3::splat(0.6 * shade), 3),
            (_, ViewMode::Thickness) => {
                let t = sc.interp(tri, h.u, h.v, &sc.thickness);
                (t / sc.max_thickness.max(0.5)).min(1.0)
            }
            (_, ViewMode::Overhang) => {
                let a = sc.interp(tri, h.u, h.v, &sc.overhang);
                a / 90.0
            }
            (_, _) => {
                let t = sc.interp(tri, h.u, h.v, &self.sim.temp);
                (t - 25.0) / (self.temp_max - 25.0)
            }
        };
        (turbo(v.clamp(0.0, 1.0)) * shade, 1)
    }
}

pub fn turbo(x: f32) -> Vec3 {
    let r = 0.135_721_38 + x * (4.615_392_6 + x * (-42.660_32 + x * (132.131_08 + x * (-152.942_39 + x * 59.286_38))));
    let g = 0.091_402_61 + x * (2.194_188_4 + x * (4.842_966_6 + x * (-14.185_033 + x * (4.277_298_6 + x * 2.829_566))));
    let b = 0.106_673_3 + x * (12.641_946 + x * (-60.582_05 + x * (110.362_77 + x * (-89.903_11 + x * 27.348_25))));
    Vec3::new(r, g, b).clamp(Vec3::ZERO, Vec3::ONE)
}

fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

fn srgb(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let v = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0 + 0.5) as u8
}

/// Upper end of the temperature colour scale.
pub fn temp_scale_max(hdt: f32) -> f32 {
    hdt + 10.0
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    sc: &Scene,
    sim: &SimResult,
    op: &Optics,
    bulb: &Bulb,
    hdt: f32,
    cam: &Camera,
    rs: &RenderSettings,
    w: usize,
    h: usize,
    fixed_exposure: Option<f32>,
) -> Frame {
    let lamp_on = rs.mode != ViewMode::Day;
    let ctx = Ctx {
        sc,
        sim,
        op,
        bulb,
        rs,
        lamp_on,
        l_bulb: bulb.color() * bulb.luminance(),
        bulb_col: bulb.color(),
        key_dir: Vec3::new(-0.4, -0.6, 0.7).normalize(),
        e_key: if rs.mode == ViewMode::Day { rs.room_lux * 2.5 } else { 0.0 },
        shell_albedo_cache: op.albedo(),
        temp_max: temp_scale_max(hdt),
    };
    let eye = cam.eye();
    let fwd = (cam.target - eye).normalize();
    let right = fwd.cross(Vec3::Z).normalize_or(Vec3::X);
    let up = right.cross(fwd);
    let tan = (cam.fov_deg.to_radians() * 0.5).tan();
    let aspect = w as f32 / h as f32;
    let ss = rs.samples.clamp(1, 8) as usize;
    let analysis =
        matches!(rs.mode, ViewMode::Shape | ViewMode::Thickness | ViewMode::Overhang | ViewMode::Temperature);

    let mut hdr = vec![Vec3::ZERO; w * h];
    let mut kind = vec![0u8; w * h];
    // section caps: display colour + coverage fraction, blended after tone mapping
    let mut caps = vec![(Vec3::ZERO, 0f32); w * h];
    hdr.par_chunks_mut(w)
        .zip(kind.par_chunks_mut(w))
        .zip(caps.par_chunks_mut(w))
        .enumerate()
        .for_each(|(j, ((row, krow), crow))| {
            for i in 0..w {
                let mut acc = Vec3::ZERO;
                let mut n = 0u32;
                let mut cap = Vec3::ZERO;
                let mut ncap = 0u32;
                let mut k0 = 0u8;
                for sy in 0..ss {
                    for sx in 0..ss {
                        let fx = (i as f32 + (sx as f32 + 0.5) / ss as f32) / w as f32;
                        let fy = (j as f32 + (sy as f32 + 0.5) / ss as f32) / h as f32;
                        let x = (2.0 * fx - 1.0) * aspect * tan;
                        let y = (1.0 - 2.0 * fy) * tan;
                        let d = (fwd + right * x + up * y).normalize();
                        let (c, k) =
                            if analysis { ctx.false_color(eye, d) } else { ctx.radiance(eye, d, PRIMARY_DEPTH) };
                        if k == CAP {
                            cap += c;
                            ncap += 1;
                        } else {
                            acc += c;
                            n += 1;
                            if k0 == 0 || k == 1 {
                                k0 = k;
                            }
                        }
                    }
                }
                row[i] = if n > 0 { acc / n as f32 } else { Vec3::ZERO };
                krow[i] = k0;
                if ncap > 0 {
                    crow[i] = (cap / ncap as f32, ncap as f32 / (ss * ss) as f32);
                }
            }
        });

    let mut rgba = vec![255u8; w * h * 4];
    let blend = |px: &mut [u8], cap: &(Vec3, f32)| {
        if cap.1 > 0.0 {
            let mix = |a: u8, b: f32| ((a as f32 / 255.0) * (1.0 - cap.1) + b.clamp(0.0, 1.0) * cap.1) * 255.0;
            px[0] = mix(px[0], cap.0.x) as u8;
            px[1] = mix(px[1], cap.0.y) as u8;
            px[2] = mix(px[2], cap.0.z) as u8;
        }
    };
    if analysis {
        let shape = rs.mode == ViewMode::Shape;
        for ((px, c), cap) in rgba.chunks_mut(4).zip(&hdr).zip(&caps) {
            if shape {
                let c = *c * 2f32.powf(rs.ev);
                px[0] = srgb(aces(c.x * 1.6));
                px[1] = srgb(aces(c.y * 1.6));
                px[2] = srgb(aces(c.z * 1.6));
            } else {
                px[0] = (c.x * 255.0) as u8;
                px[1] = (c.y * 255.0) as u8;
                px[2] = (c.z * 255.0) as u8;
            }
            blend(px, cap);
        }
        return Frame { w, h, rgba, exposure: fixed_exposure.unwrap_or(1.0) };
    }

    let exposure = fixed_exposure.unwrap_or_else(|| {
        let mut ls: Vec<f32> = hdr.iter().zip(&kind).filter(|(_, &k)| k == 1).map(|(c, _)| lum(*c)).collect();
        let key = if lamp_on { 0.35 } else { 0.25 };
        if ls.len() < 16 {
            return key / (rs.room_lux / PI).max(1.0);
        }
        let mid = ls.len() / 2;
        ls.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        key / ls[mid].max(1e-3)
    });
    let k = exposure * 2f32.powf(rs.ev);
    for ((px, c), cap) in rgba.chunks_mut(4).zip(&hdr).zip(&caps) {
        let c = *c * k;
        px[0] = srgb(aces(c.x));
        px[1] = srgb(aces(c.y));
        px[2] = srgb(aces(c.z));
        blend(px, cap);
    }
    Frame { w, h, rgba, exposure }
}
