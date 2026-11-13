//! Planar cross-section of the scene mesh plus wall analysis.
//!
//! Section plane: axis 0 = X, 1 = Y, 2 = Z at `pos` (mm). 2D coordinates:
//! Z cut → (x, y), X cut → (y, z), Y cut → (x, z).

use crate::mesh::Tag;
use crate::scene::Scene;
use glam::{Vec2, Vec3};
use rayon::prelude::*;

pub const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

#[derive(Clone, Copy)]
pub struct Seg {
    pub a: Vec2,
    pub b: Vec2,
    pub tag: Tag,
}

/// One wall-thickness sample: outer point, nearest inner point, distance.
#[derive(Clone, Copy)]
pub struct Sample {
    pub outer: Vec2,
    pub inner: Vec2,
    pub t: f32,
    /// Angle (deg, Z cuts) or height (mm, X/Y cuts) for the thickness graph.
    pub param: f32,
}

pub struct Section {
    pub axis: usize,
    pub pos: f32,
    pub segs: Vec<Seg>,
    pub lo: Vec2,
    pub hi: Vec2,
    pub samples: Vec<Sample>,
    pub t_min: f32,
    pub t_mean: f32,
    pub t_max: f32,
    pub min_sample: Option<Sample>,
    /// Bulb glass outline in this plane.
    pub bulb: Vec<Vec2>,
    /// (gap mm, bulb point, wall point)
    pub clearance: Option<(f32, Vec2, Vec2)>,
    /// Z cuts: outer radius range (mm).
    pub outer_r: Option<(f32, f32)>,
    /// Z cuts: inner radius range (mm).
    pub inner_r: Option<(f32, f32)>,
}

pub fn to2d(axis: usize, p: Vec3) -> Vec2 {
    match axis {
        0 => Vec2::new(p.y, p.z),
        1 => Vec2::new(p.x, p.z),
        _ => Vec2::new(p.x, p.y),
    }
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> (f32, Vec2) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    let q = a + ab * t;
    (p.distance(q), q)
}

pub fn compute(sc: &Scene, axis: usize, pos: f32) -> Section {
    // nudge the plane off exact vertex coordinates
    let pos_eps = pos + 1.37e-4;
    let m = &sc.mesh;
    let segs: Vec<Seg> = m
        .tris
        .par_iter()
        .zip(m.tags.par_iter())
        .filter_map(|(t, &tag)| {
            let p = [m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]];
            let s = [p[0][axis] - pos_eps, p[1][axis] - pos_eps, p[2][axis] - pos_eps];
            let mut pts = [Vec2::ZERO; 2];
            let mut n = 0;
            for k in 0..3 {
                let (i, j) = (k, (k + 1) % 3);
                if (s[i] < 0.0) != (s[j] < 0.0) {
                    let f = s[i] / (s[i] - s[j]);
                    if n < 2 {
                        pts[n] = to2d(axis, p[i].lerp(p[j], f));
                    }
                    n += 1;
                }
            }
            (n == 2).then_some(Seg { a: pts[0], b: pts[1], tag })
        })
        .collect();

    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    for s in &segs {
        lo = lo.min(s.a).min(s.b);
        hi = hi.max(s.a).max(s.b);
    }

    // wall thickness: each outer-surface point → nearest inner-surface point
    let outer: Vec<&Seg> = segs.iter().filter(|s| s.tag == Tag::Outer).collect();
    let inner: Vec<&Seg> = segs.iter().filter(|s| s.tag == Tag::Inner).collect();
    let mut samples: Vec<Sample> = outer
        .par_iter()
        .filter_map(|s| {
            let p = (s.a + s.b) * 0.5;
            let mut best = (f32::INFINITY, Vec2::ZERO);
            for q in &inner {
                let r = seg_dist(p, q.a, q.b);
                if r.0 < best.0 {
                    best = r;
                }
            }
            if !best.0.is_finite() || best.0 > 30.0 {
                return None;
            }
            let param = if axis == 2 { p.y.atan2(p.x).to_degrees().rem_euclid(360.0) } else { p.y };
            Some(Sample { outer: p, inner: best.1, t: best.0, param })
        })
        .collect();
    samples.sort_by(|a, b| a.param.partial_cmp(&b.param).unwrap_or(std::cmp::Ordering::Equal));
    let (mut t_min, mut t_max, mut t_sum) = (f32::INFINITY, 0.0f32, 0.0f32);
    let mut min_sample = None;
    for s in &samples {
        if s.t < t_min {
            t_min = s.t;
            min_sample = Some(*s);
        }
        t_max = t_max.max(s.t);
        t_sum += s.t;
    }
    let t_mean = if samples.is_empty() { 0.0 } else { t_sum / samples.len() as f32 };

    // bulb outline in this plane
    let c = sc.bulb_c;
    let r = sc.bulb_r;
    let off = pos - c[axis];
    let k2 = 1.0 - (off / r[axis]).powi(2);
    let mut bulb = Vec::new();
    if k2 > 0.0 {
        let k = k2.sqrt();
        let (ca, ra, rb) = match axis {
            0 => (Vec2::new(c.y, c.z), r.y * k, r.z * k),
            1 => (Vec2::new(c.x, c.z), r.x * k, r.z * k),
            _ => (Vec2::new(c.x, c.y), r.x * k, r.y * k),
        };
        for i in 0..=128 {
            let a = std::f32::consts::TAU * i as f32 / 128.0;
            bulb.push(ca + Vec2::new(ra * a.cos(), rb * a.sin()));
        }
    }
    let clearance = if bulb.is_empty() || inner.is_empty() {
        None
    } else {
        bulb.par_iter()
            .map(|&bp| {
                let mut best = (f32::INFINITY, Vec2::ZERO);
                for q in &inner {
                    let r = seg_dist(bp, q.a, q.b);
                    if r.0 < best.0 {
                        best = r;
                    }
                }
                (best.0, bp, best.1)
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    };

    let radius_range = |v: &[&Seg]| -> Option<(f32, f32)> {
        if axis != 2 || v.is_empty() {
            return None;
        }
        let mut a = f32::INFINITY;
        let mut b = 0.0f32;
        for s in v {
            for p in [s.a, s.b] {
                a = a.min(p.length());
                b = b.max(p.length());
            }
        }
        Some((a, b))
    };

    Section {
        axis,
        pos,
        lo,
        hi,
        t_min: if t_min.is_finite() { t_min } else { 0.0 },
        t_mean,
        t_max,
        min_sample,
        bulb,
        clearance,
        outer_r: radius_range(&outer),
        inner_r: radius_range(&inner),
        samples,
        segs,
    }
}

impl Section {
    pub fn summary(&self) -> Vec<String> {
        let mut v = vec![format!("{} = {:.1} mm: {} cut edges", AXIS_NAMES[self.axis], self.pos, self.segs.len())];
        if self.samples.is_empty() {
            v.push("no shade wall in this plane".into());
        } else {
            v.push(format!("wall {:.2} / {:.2} / {:.2} mm (min / mean / max)", self.t_min, self.t_mean, self.t_max));
        }
        if let (Some(o), Some(i)) = (self.outer_r, self.inner_r) {
            v.push(format!("outer Ø {:.1}–{:.1} mm, inner Ø {:.1}–{:.1} mm", 2.0 * o.0, 2.0 * o.1, 2.0 * i.0, 2.0 * i.1));
        }
        if self.axis != 2 && self.lo.is_finite() {
            v.push(format!("extent {:.1} × {:.1} mm", self.hi.x - self.lo.x, self.hi.y - self.lo.y));
        }
        if let Some((g, _, _)) = self.clearance {
            v.push(format!("bulb gap {:.1} mm", g));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Design;

    #[test]
    fn plain_cylinder_section_measures_the_wall() {
        let d = Design {
            r_bottom: 60.0,
            r_top: 60.0,
            bulge: 0.0,
            ribs: 0,
            wall: 1.5,
            ..Design::default()
        };
        let bulb = crate::bulb::Bulb::default();
        let sc = crate::scene::build(&d, &bulb, &crate::geometry::LampBase::default(), 256, 60);
        for axis in 0..3 {
            let pos = if axis == 2 { 80.0 } else { 0.0 };
            let s = compute(&sc, axis, pos);
            assert!(!s.samples.is_empty(), "axis {axis}");
            assert!((s.t_min - 1.5).abs() < 0.05, "axis {axis}: min {}", s.t_min);
            assert!((s.t_max - 1.5).abs() < 0.1, "axis {axis}: max {}", s.t_max);
        }
        let z = compute(&sc, 2, 80.0);
        let (o0, o1) = z.outer_r.unwrap();
        assert!((o0 - 60.0).abs() < 0.1 && (o1 - 60.0).abs() < 0.1);
        let (g, _, _) = z.clearance.unwrap();
        assert!(g > 20.0);
    }
}
