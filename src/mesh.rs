//! Triangle mesh container and the closed "tube" builder used for every part.

use glam::Vec3;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    /// Outside surface of the shade wall.
    Outer,
    /// Inside surface of the shade wall (faces the bulb).
    Inner,
    /// Top/bottom end faces of the wall.
    Rim,
    /// Mount ring + spokes.
    Mount,
    /// Lamp base (not exported, only for simulation/rendering).
    Proxy,
}

#[derive(Clone, Default)]
pub struct Mesh {
    pub pos: Vec<Vec3>,
    pub tris: Vec<[u32; 3]>,
    pub tags: Vec<Tag>,
}

impl Mesh {
    pub fn append(&mut self, o: &Mesh) -> usize {
        let off = self.pos.len() as u32;
        self.pos.extend_from_slice(&o.pos);
        for (t, g) in o.tris.iter().zip(&o.tags) {
            self.tris.push([t[0] + off, t[1] + off, t[2] + off]);
            self.tags.push(*g);
        }
        off as usize
    }

    /// Signed volume in mm³ (positive for outward-oriented closed meshes).
    pub fn volume(&self) -> f64 {
        self.tris
            .iter()
            .map(|t| {
                let a = self.pos[t[0] as usize].as_dvec3();
                let b = self.pos[t[1] as usize].as_dvec3();
                let c = self.pos[t[2] as usize].as_dvec3();
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    pub fn face_normal(&self, i: usize) -> Vec3 {
        let t = self.tris[i];
        let a = self.pos[t[0] as usize];
        let b = self.pos[t[1] as usize];
        let c = self.pos[t[2] as usize];
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// Every directed edge must have exactly one opposite partner
    /// (closed, consistently oriented 2-manifold).
    pub fn check_closed(&self) -> Result<(), String> {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::with_capacity(self.tris.len() * 3);
        for t in &self.tris {
            for k in 0..3 {
                let a = t[k];
                let b = t[(k + 1) % 3];
                *edges.entry((a, b)).or_insert(0) += 1;
            }
        }
        for (&(a, b), &n) in &edges {
            if n != 1 {
                return Err(format!("edge {a}-{b} used {n} times in the same direction"));
            }
            if edges.get(&(b, a)) != Some(&1) {
                return Err(format!("edge {a}-{b} has no opposite partner (hole)"));
            }
        }
        Ok(())
    }
}

/// One horizontal ring of a tube: z, inner radii, outer radii (one per angle).
pub type Row = (f32, Vec<f32>, Vec<f32>);

/// Builds a closed tube from rings of (inner, outer) radii sampled at
/// `angles` (increasing, full circle). Result is watertight and outward
/// oriented. `tags` = [outer, inner, rim].
pub fn tube(angles: &[f32], rows: &[Row], tags: [Tag; 3]) -> Mesh {
    let nt = angles.len();
    let nz = rows.len();
    assert!(nt >= 3 && nz >= 2);
    let mut m = Mesh::default();
    m.pos.reserve(2 * nt * nz);
    for (z, _, ro) in rows {
        for (j, &a) in angles.iter().enumerate() {
            m.pos.push(Vec3::new(ro[j] * a.cos(), ro[j] * a.sin(), *z));
        }
    }
    for (z, ri, ro) in rows {
        for (j, &a) in angles.iter().enumerate() {
            let r = ri[j].min(ro[j] - 0.05).max(0.01);
            m.pos.push(Vec3::new(r * a.cos(), r * a.sin(), *z));
        }
    }
    let o = |iz: usize, it: usize| (iz * nt + it % nt) as u32;
    let i = |iz: usize, it: usize| (nt * nz + iz * nt + it % nt) as u32;
    let mut push = |t: [u32; 3], g: Tag| {
        m.tris.push(t);
        m.tags.push(g);
    };
    for iz in 0..nz - 1 {
        for it in 0..nt {
            let (a, b, c, d) = (o(iz, it), o(iz, it + 1), o(iz + 1, it + 1), o(iz + 1, it));
            push([a, b, c], tags[0]);
            push([a, c, d], tags[0]);
            let (a, b, c, d) = (i(iz, it), i(iz, it + 1), i(iz + 1, it + 1), i(iz + 1, it));
            push([a, c, b], tags[1]);
            push([a, d, c], tags[1]);
        }
    }
    let top = nz - 1;
    for it in 0..nt {
        let (a, b, c, d) = (o(top, it), o(top, it + 1), i(top, it + 1), i(top, it));
        push([a, b, c], tags[2]);
        push([a, c, d], tags[2]);
        let (a, b, c, d) = (o(0, it), o(0, it + 1), i(0, it + 1), i(0, it));
        push([a, c, b], tags[2]);
        push([a, d, c], tags[2]);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn tube_is_closed_and_positive() {
        let angles: Vec<f32> = (0..32).map(|i| TAU * i as f32 / 32.0).collect();
        let rows: Vec<Row> = (0..5)
            .map(|k| (k as f32 * 10.0, vec![9.0; 32], vec![10.0; 32]))
            .collect();
        let m = tube(&angles, &rows, [Tag::Outer, Tag::Inner, Tag::Rim]);
        m.check_closed().unwrap();
        let v = m.volume();
        let expected = std::f64::consts::PI * (100.0 - 81.0) * 40.0;
        assert!(v > 0.0);
        assert!((v - expected).abs() / expected < 0.02, "{v} vs {expected}");
    }
}
