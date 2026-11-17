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
    apply_rim_above(m, d, 0.0);
}

/// Like pply_rim, but only the wall above relative height s_lo is
/// stretched (the top part of a split shade keeps its flat joint).
fn apply_rim_above(m: &mut Mesh, d: &Design, s_lo: f32) {
    if d.rim_waves == 0 || d.rim_depth <= 0.0 {
        return;
    }
    let zl = d.z0() + d.height * s_lo;
    let span = d.height * (1.0 - s_lo);
    for p in &mut m.pos {
        if p.z <= zl {
            continue;
        }
        let th = p.y.atan2(p.x);
        p.z = zl + (p.z - zl) * (d.rim_height(th) - d.height * s_lo) / span;
    }
}

/// Relative heights (0..1) of the cuts between split parts (evenly spaced).
pub fn split_cuts(d: &Design) -> Vec<f32> {
    let n = d.split_parts.max(1);
    (1..n).map(|k| k as f32 / n as f32).collect()
}

/// Whether the cuts get the overlapping joint (a spiral-vase wall is a
/// single line and can't be halved, so vase mode uses butt joints).
pub fn split_has_joint(d: &Design) -> bool {
    d.split_joint && !d.vase_mode
}

/// Printed height of every part (mm), lips included. One entry when not split.
pub fn split_heights(d: &Design) -> Vec<f32> {
    let mut b = vec![0.0];
    b.extend(split_cuts(d));
    b.push(1.0);
    let lip = if split_has_joint(d) { d.split_overlap } else { 0.0 };
    let n = b.len() - 1;
    (0..n).map(|k| (b[k + 1] - b[k]) * d.height + if k + 1 < n { lip } else { 0.0 }).collect()
}

/// Fewest parts (1..=8) whose heights all fit a build height z.
pub fn split_parts_for(d: &Design, bz: f32) -> Option<u32> {
    (1..=8).find(|&n| {
        let mut t = d.clone();
        t.split_parts = n;
        split_heights(&t).iter().all(|&h| h <= bz - 2.0)
    })
}

#[derive(Clone, Copy)]
enum Half {
    Full,
    /// Inner or outer half of the wall (split joint).
    Half,
}

/// The shade cut into d.split_parts closed meshes, in place (world z).
/// Part 0 is the bottom one (it carries the mount). Each cut gets an
/// overlapping joint unless split_has_joint is false.
pub fn build_shell_parts(d: &Design, nt: usize, nz: usize) -> Vec<Mesh> {
    if d.split_parts <= 1 {
        return vec![build_shell(d, nt, nz)];
    }
    let a = angles(nt);
    let (z0, h) = (d.z0(), d.height);
    let joint = split_has_joint(d);
    let hs = d.split_overlap / h;
    let g = d.split_gap * 0.5;
    let row = |s: f32, dz: f32, inner: Half, outer: Half| -> Row {
        let s = s.clamp(0.0, 1.0);
        let mut ri = Vec::with_capacity(nt);
        let mut ro = Vec::with_capacity(nt);
        for &th in &a {
            let (i, o) = d.radii(th, s);
            let mid = 0.5 * (i + o);
            ri.push(match inner {
                Half::Full => i,
                Half::Half => mid + g,
            });
            ro.push(match outer {
                Half::Full => o,
                Half::Half => mid - g,
            });
        }
        (z0 + h * s + dz, ri, ro)
    };
    // rows over s0..=s1; irst = z offset of the first row, None = skip it
    let span = |rows: &mut Vec<Row>, s0: f32, s1: f32, inner: Half, outer: Half, first: Option<f32>| {
        let n = (((nz - 1) as f32) * (s1 - s0)).ceil().max(1.0) as usize;
        for i in 0..=n {
            let s = s0 + (s1 - s0) * i as f32 / n as f32;
            match (i, first) {
                (0, None) => {}
                (0, Some(dz)) => rows.push(row(s, dz, inner, outer)),
                _ => rows.push(row(s, 0.0, inner, outer)),
            }
        }
    };
    let mut b = vec![0.0];
    b.extend(split_cuts(d));
    b.push(1.0);
    let n = b.len() - 1;
    let mut parts = Vec::with_capacity(n);
    for k in 0..n {
        let (sa, sb) = (b[k], b[k + 1]);
        let mut rows: Vec<Row> = Vec::new();
        let mut body_start = sa;
        if k > 0 && joint {
            // skirt: outer half of the wall, slides over the lower part's lip
            span(&mut rows, sa, sa + hs, Half::Half, Half::Full, Some(0.0));
            // 45° chamfer from the skirt's bore back to the full wall (no overhang)
            let top = &rows[rows.len() - 1];
            let s_top = sa + hs;
            let width = a
                .iter()
                .enumerate()
                .map(|(j, &th)| top.1[j] - d.radii(th, s_top).0)
                .fold(0.0f32, f32::max);
            body_start = s_top + width.max(0.05) / h;
            rows.push(row(body_start, 0.0, Half::Full, Half::Full));
            span(&mut rows, body_start, sb, Half::Full, Half::Full, None);
        } else {
            span(&mut rows, sa, sb, Half::Full, Half::Full, Some(0.0));
        }
        if k + 1 < n && joint {
            // lip: inner half of the wall above the cut
            span(&mut rows, sb, sb + hs, Half::Full, Half::Half, Some(0.02));
        }
        let mut m = tube(&a, &rows, [Tag::Outer, Tag::Inner, Tag::Rim]);
        if k + 1 == n {
            apply_rim_above(&mut m, d, if k > 0 { body_start } else { 0.0 });
        }
        parts.push(m);
    }
    parts
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

/// Vase mode with the socket sleeve: the sleeve is printed separately as a
/// glue-in insert.
pub fn has_insert(d: &Design) -> bool {
    d.vase_mode && d.mount == MountKind::Sleeve
}

/// Inner and outer radius of the shade's mount ring (the part the spokes hold).
///
/// Normally the ring is the mount itself (hole = socket/bulb fit). With a
/// glue-in insert the ring is a seat instead: its hole fits around the
/// insert's sleeve with the glue gap, and it is wide enough to cover the
/// insert's glue flange underneath.
pub fn spider_radii(d: &Design) -> (f32, f32) {
    let rh = d.hole_d * 0.5;
    if has_insert(d) {
        let sleeve_outer = rh + d.ring_width;
        (sleeve_outer + d.glue_gap * 0.5, sleeve_outer + d.glue_ledge_width + 1.0)
    } else {
        (rh, rh + d.ring_width)
    }
}

/// Rows of the inward ledge at the top of a sleeve (bore `rh`, outer `rr`),
/// starting at the socket top `top`. Part of the sleeve's closed tube, so
/// there are no coincident faces.
fn ledge_rows(d: &Design, n: usize, rh: f32, rr: f32, top: f32) -> Vec<crate::mesh::Row> {
    if !d.ledge {
        return Vec::new();
    }
    let ring = vec![rr; n];
    let rl = (d.ledge_d * 0.5).min(rh - 0.5);
    let lip = vec![rl; n];
    // 45° cone from the sleeve bore in to the ledge bore, or a flat underside
    // (needs a short inward overhang)
    let start = top + if d.ledge_chamfer { rh - rl } else { 0.01 };
    vec![(start, lip.clone(), ring.clone()), (start + d.ledge_h, lip, ring)]
}

/// Glue-in mount insert for vase mode (printed on its own, flange down):
/// glue flange, sleeve around the socket, ledge on the socket top. It slides
/// up into the shade's ring from below until the flange meets the ring.
pub fn build_insert(d: &Design, n: usize) -> Option<Vec<Mesh>> {
    if !has_insert(d) {
        return None;
    }
    let rh = d.hole_d * 0.5;
    let rr = rh + d.ring_width;
    let rf = rr + d.glue_ledge_width;
    let zb = -d.sleeve_height;
    let zf = zb + d.glue_ledge_h;
    let row = |z: f32, ro: f32| -> crate::mesh::Row { (z, vec![rh; n], vec![ro; n]) };
    Some(sleeve_meshes(d, n, vec![row(zb, rf), row(zf, rf), row(zf + 0.01, rr)], 0.0))
}

/// Strut layout of a windowed sleeve whose plain part runs from `zs` to the
/// socket top `top`: (struts, strut half-angle, window-closing half-angle,
/// window bottom, window apex, taper height). None = solid sleeve (off, or
/// no room for windows).
pub fn sleeve_window_layout(d: &Design, zs: f32, top: f32) -> Option<(u32, f32, f32, f32, f32, f32)> {
    let n = d.sleeve_windows;
    if n < 2 || d.mount != MountKind::Sleeve {
        return None;
    }
    let rr = d.hole_d * 0.5 + d.ring_width;
    let pitch = TAU / n as f32;
    let window = pitch * rr - d.sleeve_strut_w;
    if window < 3.0 {
        return None;
    }
    let taper = window * 0.5; // 45° pointed window top
    let (z_lo, z_hi) = (zs + d.sleeve_band, top - d.sleeve_band);
    if z_hi - z_lo < taper + 4.0 {
        return None;
    }
    Some((n, d.sleeve_strut_w * 0.5 / rr, pitch * 0.5 * 0.995, z_lo, z_hi, taper))
}

/// Socket sleeve meshes: `foot` rows at the bottom (the last one starts the
/// plain sleeve), the sleeve up to the socket top `top`, then the ledge.
/// With windows: solid bands at both ends joined by struts (separate closed
/// meshes that overlap; the slicer unions them).
fn sleeve_meshes(d: &Design, n: usize, foot: Vec<Row>, top: f32) -> Vec<Mesh> {
    let a = angles(n);
    let rh = d.hole_d * 0.5;
    let rr = rh + d.ring_width;
    let row = |z: f32| -> Row { (z, vec![rh; n], vec![rr; n]) };
    let zs = foot.last().map(|r| r.0).unwrap_or(top);
    let ledge = ledge_rows(d, n, rh, rr, top);
    let Some((count, half, full, z_lo, z_hi, taper)) = sleeve_window_layout(d, zs, top) else {
        let mut rows = foot;
        rows.push(row(top));
        rows.extend(ledge);
        return vec![tube(&a, &rows, [Tag::Mount; 3])];
    };
    let mut lower = foot;
    lower.push(row(z_lo));
    let mut upper = vec![row(z_hi), row(top)];
    upper.extend(ledge);
    let mut out = vec![tube(&a, &lower, [Tag::Mount; 3]), tube(&a, &upper, [Tag::Mount; 3])];
    let segs = ((n as f32 * 2.0 * full / TAU).ceil() as usize).max(2);
    for k in 0..count {
        let phi = TAU * k as f32 / count as f32;
        // struts widen at 45° near the top until neighbours meet: pointed windows
        let rows = [(z_lo - 0.5, half), (z_hi - taper, half), (z_hi, full), (z_hi + 0.5, full)];
        out.push(sector(&rows.map(|(z, h)| (z, phi - h, phi + h)), rh, rr, segs));
    }
    out
}

/// Closed annular-sector prism: rows of (z, start angle, end angle) between
/// radii `ri` and `ro`, `segs` segments across.
fn sector(rows: &[(f32, f32, f32)], ri: f32, ro: f32, segs: usize) -> Mesh {
    let mut m = Mesh::default();
    let w = segs + 1;
    for &(z, a0, a1) in rows {
        for r in [ro, ri] {
            for j in 0..w {
                let th = a0 + (a1 - a0) * j as f32 / segs as f32;
                m.pos.push(glam::Vec3::new(r * th.cos(), r * th.sin(), z));
            }
        }
    }
    let o = |iz: usize, j: usize| (iz * 2 * w + j) as u32;
    let i = |iz: usize, j: usize| (iz * 2 * w + w + j) as u32;
    let mut push = |t: [u32; 3]| {
        m.tris.push(t);
        m.tags.push(Tag::Mount);
    };
    let nz = rows.len();
    for iz in 0..nz - 1 {
        for j in 0..segs {
            let (a, b, c, e) = (o(iz, j), o(iz, j + 1), o(iz + 1, j + 1), o(iz + 1, j));
            push([a, b, c]);
            push([a, c, e]);
            let (a, b, c, e) = (i(iz, j), i(iz, j + 1), i(iz + 1, j + 1), i(iz + 1, j));
            push([a, c, b]);
            push([a, e, c]);
        }
        // side at the start angle (faces backwards) and at the end angle
        push([o(iz, 0), o(iz + 1, 0), i(iz + 1, 0)]);
        push([o(iz, 0), i(iz + 1, 0), i(iz, 0)]);
        push([o(iz, segs), i(iz + 1, segs), o(iz + 1, segs)]);
        push([o(iz, segs), i(iz, segs), i(iz + 1, segs)]);
    }
    let top = nz - 1;
    for j in 0..segs {
        push([o(top, j), o(top, j + 1), i(top, j + 1)]);
        push([o(top, j), i(top, j + 1), i(top, j)]);
        push([o(0, j), i(0, j + 1), o(0, j + 1)]);
        push([o(0, j), i(0, j), i(0, j + 1)]);
    }
    m
}

/// Mount parts printed with the shade (closed meshes that overlap the
/// shell; the slicer unions them): the spider, plus the socket sleeve unless
/// it is a separate vase-mode insert.
pub fn build_mount(d: &Design, n: usize) -> Vec<Mesh> {
    let a = angles(n);
    let (rh, rr) = spider_radii(d);
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
    if d.mount == MountKind::Sleeve && !has_insert(d) {
        let top = z0 + d.sleeve_height; // = socket top (z = 0)
        parts.extend(sleeve_meshes(d, n, vec![(z0, inner, vec![rr; n])], top));
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

/// Largest overhang from vertical (deg) of the shell's inner and outer
/// surfaces, sampled on an `nt` × `nz` grid straight from the design (no mesh
/// or BVH needed, rows in parallel). Same normal estimate as the scene; the
/// bottom row is ignored because it lies on the build plate.
pub fn max_overhang(d: &Design, nt: usize, nz: usize) -> f32 {
    use glam::Vec3;
    use rayon::prelude::*;
    let a = angles(nt);
    let z0 = d.z0();
    let rows: Vec<(Vec<Vec3>, Vec<Vec3>)> = (0..nz)
        .into_par_iter()
        .map(|iz| {
            let s = iz as f32 / (nz - 1) as f32;
            let mut o = Vec::with_capacity(nt);
            let mut i = Vec::with_capacity(nt);
            for &th in &a {
                let (ri, ro) = d.radii(th, s);
                let z = z0 + s * d.rim_height(th);
                let (sn, cs) = th.sin_cos();
                o.push(Vec3::new(ro * cs, ro * sn, z));
                i.push(Vec3::new(ri * cs, ri * sn, z));
            }
            (o, i)
        })
        .collect();
    let oh = |n: Vec3| (-n.z).clamp(0.0, 1.0).asin().to_degrees();
    (1..nz)
        .into_par_iter()
        .map(|iz| {
            let up = (iz + 1).min(nz - 1);
            let dn = iz - 1;
            let mut m: f32 = 0.0;
            for it in 0..nt {
                let (l, r) = ((it + nt - 1) % nt, (it + 1) % nt);
                let n_o = (rows[iz].0[r] - rows[iz].0[l]).cross(rows[up].0[it] - rows[dn].0[it]).normalize_or_zero();
                let n_i = -(rows[iz].1[r] - rows[iz].1[l]).cross(rows[up].1[it] - rows[dn].1[it]).normalize_or_zero();
                m = m.max(oh(n_o)).max(oh(n_i));
            }
            m
        })
        .reduce(|| 0.0, f32::max)
}

/// Mesh resolution for preview / simulation; `scale` comes from the quality preset.
pub fn preview_res(d: &Design, scale: f32) -> (usize, usize) {
    let sc = |n: usize| (n as f32 * scale) as usize;
    let mut nt = sc(d.angular_detail() * 8).clamp(sc(160).max(96), sc(640));
    if d.sides >= 3 {
        nt = nt.div_ceil(d.sides as usize) * d.sides as usize;
    }
    let vert = d.waves as usize * 8 + (d.meander_freq * 16.0) as usize * (d.lobes > 0) as usize;
    let nz = sc((d.height / 1.5) as usize + vert).clamp(sc(60).max(40), sc(420));
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
    let nz = ((d.height / step) as usize + d.waves as usize * 16).clamp(80, 1400);
    (nt, nz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeve_windows_save_material_and_stay_closed() {
        let solid = Design { sleeve_height: 80.0, ..Design::default() };
        let windowed = Design { sleeve_windows: 6, ..solid.clone() };
        let vol = |d: &Design| build_mount(d, 720)[1..].iter().map(|m| m.volume()).sum::<f64>();
        let parts = build_mount(&windowed, 720);
        assert_eq!(parts.len(), 1 + 2 + 6, "spider, two bands, six struts");
        for m in &parts {
            m.check_closed().unwrap();
            assert!(m.volume() > 0.0);
        }
        assert!(vol(&windowed) < vol(&solid) * 0.6, "{} vs {}", vol(&windowed), vol(&solid));
        // struts stay inside the sleeve wall
        let (rh, rr) = (windowed.hole_d * 0.5, windowed.hole_d * 0.5 + windowed.ring_width);
        for p in parts[3..].iter().flat_map(|m| &m.pos) {
            let r = p.truncate().length();
            assert!(r > rh - 1e-3 && r < rr + 1e-3);
        }
        // a short sleeve has no room for windows and stays solid
        let short = Design { sleeve_windows: 6, sleeve_height: 12.0, ..Design::default() };
        assert_eq!(build_mount(&short, 720).len(), 2);
        // the vase-mode insert gets them too
        let insert = build_insert(&Design { vase_mode: true, ..windowed }, 720).unwrap();
        assert_eq!(insert.len(), 8);
        insert.iter().for_each(|m| m.check_closed().unwrap());
    }

    #[test]
    fn split_parts_are_closed_and_keep_the_volume() {
        let mut d = Design { rim_waves: 6, rim_depth: 8.0, ..Design::default() };
        d.height = 300.0;
        let whole = build_shell(&d, 180, 160).volume();
        for joint in [true, false] {
            d.split_parts = 3;
            d.split_joint = joint;
            d.split_gap = 0.0;
            let parts = build_shell_parts(&d, 180, 160);
            assert_eq!(parts.len(), 3);
            let mut v = 0.0;
            for p in &parts {
                p.check_closed().unwrap();
                assert!(p.volume() > 0.0);
                v += p.volume();
            }
            // the joint adds a little (chamfer cone); a butt joint is exact
            assert!((v - whole).abs() / whole < 0.03, "joint {joint}: {v} vs {whole}");
        }
        let hs = split_heights(&d);
        assert_eq!(hs.len(), 3);
        assert!((hs.iter().sum::<f32>() - 300.0).abs() < 0.1);
        d.split_joint = true;
        assert_eq!(split_parts_for(&d, 256.0), Some(2));
        assert_eq!(split_parts_for(&d, 180.0), Some(2));
        assert_eq!(split_parts_for(&d, 140.0), Some(3));
    }
    use crate::design::presets;

    /// Spoke ends must lie inside the wall: never beyond the outer surface,
    /// and past the inner surface wherever the wall allows it.
    #[test]
    fn spokes_stay_inside_the_wall() {
        let mut designs: Vec<(String, Design)> = presets().into_iter().map(|(n, d)| (n.to_string(), d)).collect();
        let p = crate::project::Project::default();
        for t in crate::random::Theme::ALL {
            for seed in 0..4 {
                designs.push((format!("{} #{seed}", t.name()), crate::random::generate(t, seed, &p, &Default::default()).design));
            }
        }
        for (name, d) in designs {
            let spider = &build_mount(&d, 1440)[0];
            let rr = spider_radii(&d).1;
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

    /// Vase mode: the insert and the shade's ring fit together.
    #[test]
    fn vase_mode_insert_fits_the_ring() {
        let d = Design { vase_mode: true, ..Design::default() };
        assert_eq!(d.mount, MountKind::Sleeve, "GENNAKER default is the sleeve");
        let insert = build_insert(&d, 720).expect("insert in vase mode");
        assert_eq!(insert.len(), 1, "solid sleeve: one mesh");
        let insert = &insert[0];
        insert.check_closed().unwrap();
        assert!(insert.volume() > 0.0);
        let shade_parts = build_mount(&d, 720);
        assert_eq!(shade_parts.len(), 1, "the shade keeps only ring + spokes");

        let (hole, ring) = spider_radii(&d);
        let rh = d.hole_d * 0.5;
        let sleeve_r = rh + d.ring_width;
        let flange_r = sleeve_r + d.glue_ledge_width;
        let flange_top = -d.sleeve_height + d.glue_ledge_h;
        // sleeve passes through the ring hole with the glue gap
        assert!((hole - sleeve_r - d.glue_gap * 0.5).abs() < 1e-4);
        let r_max_above_flange = insert
            .pos
            .iter()
            .filter(|p| p.z > flange_top + 0.05)
            .map(|p| p.truncate().length())
            .fold(0.0f32, f32::max);
        assert!(r_max_above_flange <= sleeve_r + 1e-3, "sleeve wider than planned: {r_max_above_flange}");
        // the ring sits on the flange and covers it
        let spider_bottom = shade_parts[0].pos.iter().map(|p| p.z).fold(f32::MAX, f32::min);
        assert!((spider_bottom - flange_top).abs() < 1e-4, "ring rests on the flange");
        assert!((d.z0() - flange_top).abs() < 1e-4);
        assert!(ring > flange_r, "ring covers the glue flange");
        // the insert reaches the socket top and carries the ledge
        let top = insert.pos.iter().map(|p| p.z).fold(f32::MIN, f32::max);
        assert!((top - ledge_top(&d)).abs() < 1e-3);
        // defaults asked for: 2 mm sleeve wall, 2 mm ledge
        assert_eq!((d.ring_width, d.ledge_h), (2.0, 2.0));
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
