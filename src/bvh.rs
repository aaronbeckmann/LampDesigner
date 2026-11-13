//! Simple median-split BVH for ray/triangle queries.

use glam::Vec3;

#[derive(Clone, Copy)]
struct Node {
    bmin: Vec3,
    bmax: Vec3,
    /// leaf: first index into `order`; inner: index of right child
    a: u32,
    /// leaf: triangle count (>0); inner: 0
    count: u32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
    v0: Vec<Vec3>,
    e1: Vec<Vec3>,
    e2: Vec<Vec3>,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub tri: u32,
    pub u: f32,
    pub v: f32,
}

impl Bvh {
    pub fn build(pos: &[Vec3], tris: &[[u32; 3]]) -> Bvh {
        let n = tris.len();
        let mut v0 = Vec::with_capacity(n);
        let mut e1 = Vec::with_capacity(n);
        let mut e2 = Vec::with_capacity(n);
        let mut lo = Vec::with_capacity(n);
        let mut hi = Vec::with_capacity(n);
        let mut cen = Vec::with_capacity(n);
        for t in tris {
            let a = pos[t[0] as usize];
            let b = pos[t[1] as usize];
            let c = pos[t[2] as usize];
            v0.push(a);
            e1.push(b - a);
            e2.push(c - a);
            let l = a.min(b).min(c);
            let h = a.max(b).max(c);
            lo.push(l);
            hi.push(h);
            cen.push((l + h) * 0.5);
        }
        let mut order: Vec<u32> = (0..n as u32).collect();
        let mut nodes = Vec::with_capacity(n / 2 + 1);
        if n > 0 {
            build_rec(&mut nodes, &mut order, 0, &lo, &hi, &cen);
        }
        Bvh { nodes, order, v0, e1, e2 }
    }

    /// Closest hit in (tmin, tmax). With `any`, returns the first hit found.
    pub fn intersect(&self, o: Vec3, d: Vec3, tmin: f32, tmax: f32, any: bool) -> Option<Hit> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = Vec3::new(safe_inv(d.x), safe_inv(d.y), safe_inv(d.z));
        let mut best: Option<Hit> = None;
        let mut tmax = tmax;
        let mut stack = [0u32; 96];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let ni = stack[sp] as usize;
            let node = &self.nodes[ni];
            let t1 = (node.bmin - o) * inv;
            let t2 = (node.bmax - o) * inv;
            let tn = t1.min(t2).max_element().max(tmin);
            let tf = t1.max(t2).min_element().min(tmax);
            if tn > tf {
                continue;
            }
            if node.count > 0 {
                let s = node.a as usize;
                for k in s..s + node.count as usize {
                    let ti = self.order[k] as usize;
                    if let Some((t, u, v)) = tri_hit(o, d, self.v0[ti], self.e1[ti], self.e2[ti]) {
                        if t > tmin && t < tmax {
                            tmax = t;
                            best = Some(Hit { t, tri: ti as u32, u, v });
                            if any {
                                return best;
                            }
                        }
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = (ni + 1) as u32;
                stack[sp + 1] = node.a;
                sp += 2;
            }
        }
        best
    }

    pub fn occluded(&self, o: Vec3, d: Vec3, dist: f32) -> bool {
        self.intersect(o, d, 1e-4, dist, true).is_some()
    }
}

fn safe_inv(x: f32) -> f32 {
    if x.abs() < 1e-12 {
        1e12f32.copysign(x)
    } else {
        1.0 / x
    }
}

#[inline]
fn tri_hit(o: Vec3, d: Vec3, v0: Vec3, e1: Vec3, e2: Vec3) -> Option<(f32, f32, f32)> {
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - v0;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some((e2.dot(q) * inv, u, v))
}

fn build_rec(
    nodes: &mut Vec<Node>,
    order: &mut [u32],
    start: usize,
    lo: &[Vec3],
    hi: &[Vec3],
    cen: &[Vec3],
) -> u32 {
    let idx = nodes.len();
    let mut bmin = Vec3::splat(f32::INFINITY);
    let mut bmax = Vec3::splat(f32::NEG_INFINITY);
    let mut cmin = bmin;
    let mut cmax = bmax;
    for &t in order.iter() {
        let t = t as usize;
        bmin = bmin.min(lo[t]);
        bmax = bmax.max(hi[t]);
        cmin = cmin.min(cen[t]);
        cmax = cmax.max(cen[t]);
    }
    nodes.push(Node { bmin, bmax, a: start as u32, count: order.len() as u32 });
    let ext = cmax - cmin;
    if order.len() <= 4 || ext.max_element() < 1e-7 {
        return idx as u32;
    }
    let axis = if ext.x >= ext.y && ext.x >= ext.z {
        0
    } else if ext.y >= ext.z {
        1
    } else {
        2
    };
    let mid = order.len() / 2;
    order.select_nth_unstable_by(mid, |&a, &b| {
        cen[a as usize][axis].partial_cmp(&cen[b as usize][axis]).unwrap_or(std::cmp::Ordering::Equal)
    });
    let (l, r) = order.split_at_mut(mid);
    build_rec(nodes, l, start, lo, hi, cen);
    let right = build_rec(nodes, r, start + mid, lo, hi, cen);
    nodes[idx].a = right;
    nodes[idx].count = 0;
    idx as u32
}
