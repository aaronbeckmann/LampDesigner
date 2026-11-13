//! Simulation/render scene: shade + mount + lamp proxy, with per-vertex
//! analysis data (wall thickness, overhang) and fit checks.

use crate::bulb::Bulb;
use crate::bvh::Bvh;
use crate::design::{Design, MountKind};
use crate::geometry::{apply_rim, build_mount, lamp_proxy, shell_rows, LampBase};
use crate::mesh::{tube, Mesh, Tag};
use glam::Vec3;

#[derive(Clone, Copy)]
pub struct Grid {
    pub nt: usize,
    pub nz: usize,
    pub outer0: usize,
    pub inner0: usize,
}

impl Grid {
    pub fn o(&self, iz: usize, it: usize) -> usize {
        self.outer0 + iz * self.nt + it % self.nt
    }
    pub fn i(&self, iz: usize, it: usize) -> usize {
        self.inner0 + iz * self.nt + it % self.nt
    }
    pub fn shell_verts(&self) -> usize {
        2 * self.nt * self.nz
    }
}

pub struct Scene {
    pub mesh: Mesh,
    pub fnormal: Vec<Vec3>,
    pub vnormal: Vec<Vec3>,
    pub grid: Grid,
    pub bvh: Bvh,
    /// Wall thickness measured along the surface normal (mm).
    pub thickness: Vec<f32>,
    /// Overhang angle from vertical (deg).
    pub overhang: Vec<f32>,
    /// Vertex area (mm²), rim faces excluded.
    pub varea: Vec<f32>,
    pub bulb_c: Vec3,
    pub bulb_r: Vec3,
    pub z_table: f32,
    pub wall_y: f32,
    pub shade_z0: f32,
    pub shade_z1: f32,
    pub max_r: f32,
    /// Smallest gap between bulb glass and inner wall (mm, negative = collision).
    pub min_clearance: f32,
    /// Smallest inner radius of the top opening.
    pub top_opening_r: f32,
    pub min_thickness: f32,
    pub max_thickness: f32,
    pub max_overhang: f32,
    /// Fraction of wall area steeper than 45°.
    pub overhang_frac: f32,
    pub volume_mm3: f64,
    pub mount_hole_r: f32,
}

pub fn bulb_center_z(d: &Design, bulb: &Bulb) -> f32 {
    let lift = if d.mount == MountKind::FlatRing && d.hole_d < 34.0 { d.mount_thickness } else { 0.0 };
    bulb.center_h + lift
}

pub fn build(d: &Design, bulb: &Bulb, lamp: &LampBase, nt: usize, nz: usize) -> Scene {
    let (angles, rows) = shell_rows(d, nt, nz);
    let mut shell = tube(&angles, &rows, [Tag::Outer, Tag::Inner, Tag::Rim]);
    apply_rim(&mut shell, d);
    let mut volume = shell.volume();
    let mut mesh = Mesh::default();
    mesh.append(&shell);
    let grid = Grid { nt, nz, outer0: 0, inner0: nt * nz };
    for m in build_mount(d, 720) {
        volume += m.volume();
        mesh.append(&m);
    }
    mesh.append(&lamp_proxy(lamp));

    let fnormal: Vec<Vec3> = (0..mesh.tris.len()).map(|i| mesh.face_normal(i)).collect();

    // area-weighted vertex normals and vertex areas (rims excluded from area)
    let nv = mesh.pos.len();
    let mut vnormal = vec![Vec3::ZERO; nv];
    let mut varea = vec![0f32; nv];
    for (ti, t) in mesh.tris.iter().enumerate() {
        let a = mesh.pos[t[0] as usize];
        let b = mesh.pos[t[1] as usize];
        let c = mesh.pos[t[2] as usize];
        let cr = (b - a).cross(c - a);
        let area = cr.length() * 0.5;
        for &v in t {
            vnormal[v as usize] += cr;
            if mesh.tags[ti] != Tag::Rim {
                varea[v as usize] += area / 3.0;
            }
        }
    }
    for n in &mut vnormal {
        *n = n.normalize_or_zero();
    }

    // shell: analytic-ish normals from grid differences (ignores rim faces)
    let mut thickness = vec![d.mount_thickness; nv];
    let mut overhang = vec![0f32; nv];
    let dz = d.height / (nz - 1) as f32;
    let dth = std::f32::consts::TAU / nt as f32;
    let mut min_t = f32::INFINITY;
    let mut max_t: f32 = 0.0;
    let mut max_oh: f32 = 0.0;
    let mut oh_area = 0.0f32;
    let mut tot_area = 0.0f32;
    for iz in 0..nz {
        let up = (iz + 1).min(nz - 1);
        let dn = iz.saturating_sub(1);
        for it in 0..nt {
            let tz_o = mesh.pos[grid.o(up, it)] - mesh.pos[grid.o(dn, it)];
            let tt_o = mesh.pos[grid.o(iz, it + 1)] - mesh.pos[grid.o(iz, it + nt - 1)];
            let n_o = tt_o.cross(tz_o).normalize_or_zero();
            let tz_i = mesh.pos[grid.i(up, it)] - mesh.pos[grid.i(dn, it)];
            let tt_i = mesh.pos[grid.i(iz, it + 1)] - mesh.pos[grid.i(iz, it + nt - 1)];
            let n_i = -tt_i.cross(tz_i).normalize_or_zero();
            vnormal[grid.o(iz, it)] = n_o;
            vnormal[grid.i(iz, it)] = n_i;

            let ro = &rows[iz].2;
            let ri = &rows[iz].1;
            let tr = (ro[it] - ri[it]).max(0.0);
            let drdz = (rows[up].2[it] - rows[dn].2[it]) / (dz * (up - dn).max(1) as f32);
            let drdt = (ro[(it + 1) % nt] - ro[(it + nt - 1) % nt]) / (2.0 * dth * ro[it].max(1.0));
            let tn = tr / (1.0 + drdz * drdz + drdt * drdt).sqrt();
            thickness[grid.o(iz, it)] = tn;
            thickness[grid.i(iz, it)] = tn;
            min_t = min_t.min(tn);
            max_t = max_t.max(tn);

            let oh = |n: Vec3| (-n.z).max(0.0).min(1.0).asin().to_degrees();
            let o = if iz == 0 { 0.0 } else { oh(n_o).max(oh(n_i)) };
            overhang[grid.o(iz, it)] = o;
            overhang[grid.i(iz, it)] = o;
            max_oh = max_oh.max(o);
            let a = varea[grid.o(iz, it)];
            tot_area += a;
            if o > 45.0 {
                oh_area += a;
            }
        }
    }

    let bz = bulb_center_z(d, bulb);
    let bulb_c = Vec3::new(0.0, 0.0, bz);
    let mut min_clear = f32::INFINITY;
    let mut max_r: f32 = 0.0;
    for (z, ri, ro) in &rows {
        let rb = bulb.radius_at(z - bz);
        for k in 0..nt {
            max_r = max_r.max(ro[k]);
            if rb > 0.0 {
                min_clear = min_clear.min(ri[k] - rb);
            }
        }
    }
    // bulb sticking out of the top: check the opening
    let top_r = rows[nz - 1].1.iter().cloned().fold(f32::INFINITY, f32::min);
    let top_dz = d.z1() - bz;
    if top_dz < bulb.rz {
        min_clear = min_clear.min(top_r - bulb.radius_at(top_dz));
    }

    let bvh = Bvh::build(&mesh.pos, &mesh.tris);
    Scene {
        bvh,
        fnormal,
        vnormal,
        grid,
        thickness,
        overhang,
        varea,
        bulb_c,
        bulb_r: Vec3::new(bulb.rx, bulb.rx, bulb.rz),
        z_table: -lamp.height,
        wall_y: max_r + 250.0,
        shade_z0: d.z0(),
        shade_z1: d.z1(),
        max_r,
        min_clearance: min_clear,
        top_opening_r: top_r,
        min_thickness: min_t,
        max_thickness: max_t,
        max_overhang: max_oh,
        overhang_frac: if tot_area > 0.0 { oh_area / tot_area } else { 0.0 },
        volume_mm3: volume,
        mount_hole_r: d.hole_d * 0.5,
        mesh,
    }
}

impl Scene {
    /// Ray vs. bulb ellipsoid, returns distance.
    /// Both ray/bulb intersection distances (may be negative).
    pub fn hit_bulb_both(&self, o: Vec3, d: Vec3) -> Option<(f32, f32)> {
        let oc = (o - self.bulb_c) / self.bulb_r;
        let dd = d / self.bulb_r;
        let a = dd.dot(dd);
        let b = 2.0 * oc.dot(dd);
        let c = oc.dot(oc) - 1.0;
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let sq = disc.sqrt();
        Some(((-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)))
    }

    pub fn hit_bulb(&self, o: Vec3, d: Vec3) -> Option<f32> {
        let oc = (o - self.bulb_c) / self.bulb_r;
        let dd = d / self.bulb_r;
        let a = dd.dot(dd);
        let b = 2.0 * oc.dot(dd);
        let c = oc.dot(oc) - 1.0;
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let sq = disc.sqrt();
        let t0 = (-b - sq) / (2.0 * a);
        let t1 = (-b + sq) / (2.0 * a);
        if t0 > 1e-3 {
            Some(t0)
        } else if t1 > 1e-3 {
            Some(t1)
        } else {
            None
        }
    }

    pub fn interp<T>(&self, tri: usize, u: f32, v: f32, data: &[T]) -> T
    where
        T: Copy + std::ops::Mul<f32, Output = T> + std::ops::Add<Output = T>,
    {
        let t = self.mesh.tris[tri];
        data[t[0] as usize] * (1.0 - u - v) + data[t[1] as usize] * u + data[t[2] as usize] * v
    }
}
