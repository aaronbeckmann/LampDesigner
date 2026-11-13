//! Turns a `Design` into meshes: shade shell, mount spider, lamp-base proxy.

use crate::design::{Design, MountKind};
use crate::mesh::{tube, Mesh, Row, Tag};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// The lamp the shade sits on. Defaults approximate an IKEA GENNAKER
/// (Ø120 mm foot, 100 mm tall). Measure your socket and adjust.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct LampBase {
    /// Table to top of socket.
    pub height: f32,
    pub base_d: f32,
    pub socket_d: f32,
    pub socket_h: f32,
}

impl Default for LampBase {
    fn default() -> Self {
        // socket Ø 42.4 mm: the default 42.75 mm sleeve leaves 0.35 mm clearance
        LampBase { height: 100.0, base_d: 120.0, socket_d: 42.4, socket_h: 40.0 }
    }
}

pub fn angles(n: usize) -> Vec<f32> {
    (0..n).map(|i| TAU * i as f32 / n as f32).collect()
}

pub fn shell_rows(d: &Design, nt: usize, nz: usize) -> (Vec<f32>, Vec<Row>) {
    let a = angles(nt);
    let z0 = d.z0();
    let rows = (0..nz)
        .map(|iz| {
            let s = iz as f32 / (nz - 1) as f32;
            let mut ri = Vec::with_capacity(nt);
            let mut ro = Vec::with_capacity(nt);
            for &th in &a {
                let (i, o) = d.radii(th, s);
                ri.push(i);
                ro.push(o);
            }
            (z0 + d.height * s, ri, ro)
        })
        .collect();
    (a, rows)
}

pub fn build_shell(d: &Design, nt: usize, nz: usize) -> Mesh {
    let (a, rows) = shell_rows(d, nt, nz);
    let mut m = tube(&a, &rows, [Tag::Outer, Tag::Inner, Tag::Rim]);
    apply_rim(&mut m, d);
    m
}

/// Stretches each column of the shell so the top rim follows `rim_height`.
/// Vertices keep their angle, so the mesh stays watertight.
pub fn apply_rim(m: &mut Mesh, d: &Design) {
    if d.rim_waves == 0 || d.rim_depth <= 0.0 {
        return;
    }
    let z0 = d.z0();
    for p in &mut m.pos {
        let s = (p.z - z0) / d.height;
        let th = p.y.atan2(p.x);
        p.z = z0 + s * d.rim_height(th);
    }
}

pub fn spoke_angles(d: &Design) -> Vec<f32> {
    let n = d.spokes.max(2);
    (0..n).map(|j| TAU * j as f32 / n as f32 + FRAC_PI_2).collect()
}

fn wrap_pi(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// Spider layers: the spoke ends follow the wall over the spider thickness.
const SPIDER_LAYERS: usize = 4;

/// Radius where a spoke ends at angle `th` and relative height `s`: the
/// middle of the wall at exactly that angle and height. Ribs, folds and
/// polygon facets across the spoke width are followed, so the spoke always
/// merges fully with the shade and never pokes out of the outer surface.
pub fn spoke_end(d: &Design, th: f32, s: f32) -> f32 {
    let (ri, ro) = d.radii(th, s);
    0.5 * (ri + ro)
}

/// Mount parts (closed meshes that overlap the shell; the slicer unions them).
pub fn build_mount(d: &Design, n: usize) -> Vec<Mesh> {
    let a = angles(n);
    let rh = d.hole_d * 0.5;
    let rr = rh + d.ring_width;
    let z0 = d.z0();
    let sa = spoke_angles(d);
    let half_w = d.spoke_width * 0.5;
    let contour = |s: f32| -> Vec<f32> {
        a.iter()
            .map(|&th| {
                let mut r = rr;
                for &phi in &sa {
                    let al = wrap_pi(th - phi);
                    if al.abs() < FRAC_PI_2 {
                        let sn = al.abs().sin();
                        let lim_w = if sn < 1e-6 { f32::INFINITY } else { half_w / sn };
                        if lim_w > rr {
                            r = r.max(lim_w.min(spoke_end(d, th, s)));
                        }
                    }
                }
                r
            })
            .collect()
    };
    let inner = vec![rh; n];
    let rows: Vec<crate::mesh::Row> = (0..=SPIDER_LAYERS)
        .map(|k| {
            let dz = d.mount_thickness * k as f32 / SPIDER_LAYERS as f32;
            (z0 + dz, inner.clone(), contour((dz / d.height).clamp(0.0, 1.0)))
        })
        .collect();
    let spider = tube(&a, &rows, [Tag::Mount; 3]);
    let mut parts = vec![spider];
    if d.mount == MountKind::Sleeve {
        let ring = vec![rr; n];
        let top = z0 + d.sleeve_height; // = socket top (z = 0)
        let mut rows = vec![(z0, inner.clone(), ring.clone()), (top, inner.clone(), ring.clone())];
        if d.ledge {
            // inward lip resting on the socket's top rim, built into the same
            // closed tube so there are no coincident faces
            let rl = (d.ledge_d * 0.5).min(rh - 0.5);
            let lip = vec![rl; n];
            let start = if d.ledge_chamfer {
                // 45° cone from the sleeve bore in to the ledge bore
                let c = rh - rl;
                rows.push((top + c, lip.clone(), ring.clone()));
                top + c
            } else {
                // flat underside (needs a short inward overhang)
                rows.push((top + 0.01, lip.clone(), ring.clone()));
                top + 0.01
            };
            rows.push((start + d.ledge_h, lip, ring));
        }
        parts.push(tube(&a, &rows, [Tag::Mount; 3]));
    }
    parts
}

/// Height of the sleeve ledge above the socket top (0 without ledge).
pub fn ledge_top(d: &Design) -> f32 {
    if d.mount != MountKind::Sleeve || !d.ledge {
        return 0.0;
    }
    let rh = d.hole_d * 0.5;
    let rl = (d.ledge_d * 0.5).min(rh - 0.5);
    let c = if d.ledge_chamfer { rh - rl } else { 0.01 };
    c + d.ledge_h
}

pub fn lamp_proxy(l: &LampBase) -> Mesh {
    let a = angles(64);
    let n = a.len();
    let zt = -l.height;
    let ring = |z: f32, ri: f32, ro: f32| -> Row { (z, vec![ri; n], vec![ro; n]) };
    let mut m = Mesh::default();
    let rb = l.base_d * 0.5;
    let rs = l.socket_d * 0.5;
    let stem_top = -l.socket_h;
    m.append(&tube(&a, &[ring(zt, 0.5, rb), ring(zt + 6.0, 0.5, rb)], [Tag::Proxy; 3]));
    if stem_top > zt + 6.0 {
        m.append(&tube(&a, &[ring(zt + 6.0, 0.5, 7.0), ring(stem_top, 0.5, 7.0)], [Tag::Proxy; 3]));
    }
    m.append(&tube(
        &a,
        &[ring(stem_top.max(zt + 6.0), 13.5, rs), ring(-0.2, 13.5, rs)],
        [Tag::Proxy; 3],
    ));
    m
}

/// Mesh resolution for preview / simulation; `scale` comes from the quality preset.
pub fn preview_res(d: &Design, scale: f32) -> (usize, usize) {
    let sc = |n: usize| (n as f32 * scale) as usize;
    let mut nt = sc(d.angular_detail() * 8).clamp(sc(160).max(96), sc(640));
    if d.sides >= 3 {
        nt = nt.div_ceil(d.sides as usize) * d.sides as usize;
    }
    let vert = d.waves as usize * 8 + (d.meander_freq * 16.0) as usize * (d.lobes > 0) as usize;
    let nz = sc((d.height / 1.5) as usize + vert).clamp(sc(60).max(40), sc(260));
    (nt, nz)
}

/// Mesh resolution for 3MF export.
pub fn export_res(d: &Design, fine: bool) -> (usize, usize) {
    let max_t = if fine { 1800 } else { 1000 };
    let mut nt = (d.angular_detail() * 16).max(d.sides as usize * 48).clamp(360, max_t);
    if d.sides >= 3 {
        nt = nt.div_ceil(d.sides as usize) * d.sides as usize;
    }
    let step = if fine { 0.4 } else { 0.8 };
    let nz = ((d.height / step) as usize + d.waves as usize * 16).clamp(80, 900);
    (nt, nz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::presets;

    /// Spoke ends must lie inside the wall: never beyond the outer surface,
    /// and past the inner surface wherever the wall allows it.
    #[test]
    fn spokes_stay_inside_the_wall() {
        let mut designs: Vec<(String, Design)> = presets().into_iter().map(|(n, d)| (n.to_string(), d)).collect();
        let p = crate::project::Project::default();
        for t in crate::random::Theme::ALL {
            for seed in 0..4 {
                designs.push((format!("{} #{seed}", t.name()), crate::random::generate(t, seed, &p).design));
            }
        }
        for (name, d) in designs {
            let spider = &build_mount(&d, 1440)[0];
            let rr = d.hole_d * 0.5 + d.ring_width;
            let z0 = d.z0();
            for v in &spider.pos {
                let r = v.truncate().length();
                if r <= rr + 0.01 {
                    continue;
                }
                let th = v.y.atan2(v.x);
                let s = ((v.z - z0) / d.height).clamp(0.0, 1.0);
                let (_, ro) = d.radii(th, s);
                assert!(r <= ro - 0.2, "{name}: spoke point r={r:.2} pokes out (outer {ro:.2}) at {th:.3}");
            }
            // every spoke reaches past the inner surface at every layer
            for phi in spoke_angles(&d) {
                for k in 0..=SPIDER_LAYERS {
                    let s = d.mount_thickness * k as f32 / SPIDER_LAYERS as f32 / d.height;
                    let (ri, _) = d.radii(phi, s);
                    assert!(spoke_end(&d, phi, s) > ri + 0.2, "{name}: spoke at {phi:.2} stops short");
                }
            }
        }
    }

    #[test]
    fn sleeve_with_ledge_is_closed() {
        for chamfer in [true, false] {
            let d = Design {
                mount: MountKind::Sleeve,
                hole_d: 40.3,
                ledge: true,
                ledge_d: 33.0,
                ledge_chamfer: chamfer,
                ..Design::default()
            };
            let parts = build_mount(&d, 720);
            assert_eq!(parts.len(), 2);
            for m in &parts {
                m.check_closed().unwrap();
                assert!(m.volume() > 0.0);
            }
            let top = parts[1].pos.iter().map(|p| p.z).fold(f32::MIN, f32::max);
            assert!((top - ledge_top(&d)).abs() < 1e-3, "{top} vs {}", ledge_top(&d));
        }
    }

    #[test]
    fn all_presets_produce_closed_meshes() {
        for (name, d) in presets() {
            let (nt, nz) = preview_res(&d, 1.0);
            let s = build_shell(&d, nt, nz);
            s.check_closed().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(s.volume() > 0.0, "{name}");
            for m in build_mount(&d, 720) {
                m.check_closed().unwrap_or_else(|e| panic!("{name} mount: {e}"));
                assert!(m.volume() > 0.0, "{name} mount");
            }
        }
    }
}
