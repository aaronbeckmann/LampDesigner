//! Themed random design generator. Each candidate is checked for
//! printability and fit (bulb clearance, overhang, wall, bed size, mount);
//! failing candidates are re-rolled.

use crate::design::{Design, MountKind, ProfileKind, RibStyle, TextureMode, SPLINE_POINTS};
use crate::geometry::preview_res;
use crate::project::Project;
use crate::scene;
use crate::sim::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Surprise,
    Minimal,
    Organic,
    Floral,
    Geometric,
    Crystal,
    Waves,
    Fluted,
    Twisted,
}

impl Theme {
    pub const ALL: [Theme; 9] = [
        Theme::Surprise,
        Theme::Minimal,
        Theme::Organic,
        Theme::Floral,
        Theme::Geometric,
        Theme::Crystal,
        Theme::Waves,
        Theme::Fluted,
        Theme::Twisted,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Theme::Surprise => "🎲 Surprise me",
            Theme::Minimal => "Minimal",
            Theme::Organic => "Organic",
            Theme::Floral => "Floral",
            Theme::Geometric => "Geometric",
            Theme::Crystal => "Crystal",
            Theme::Waves => "Waves",
            Theme::Fluted => "Fluted / Art Deco",
            Theme::Twisted => "Twisted",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Theme::Surprise => "any theme",
            Theme::Minimal => "smooth calm silhouettes, at most soft ribs",
            Theme::Organic => "swaying columns, twisting folds, flowing grooves",
            Theme::Floral => "flared crowns with petal folds and scalloped rims",
            Theme::Geometric => "polygons, twisted prisms, sharp ridges",
            Theme::Crystal => "faceted polygons with diamond lattices",
            Theme::Waves => "horizontal and spiral ripples",
            Theme::Fluted => "straight fluted columns and cones",
            Theme::Twisted => "spiral ribs and lattices",
        }
    }

    pub fn parse(s: &str) -> Option<Theme> {
        let s = s.to_ascii_lowercase();
        Theme::ALL.into_iter().find(|t| {
            let n = t.name().to_ascii_lowercase();
            n.contains(&s) || (s == "surprise" && *t == Theme::Surprise)
        })
    }
}

struct R(Rng);

impl R {
    fn f(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.0.f()
    }
    fn i(&mut self, a: i32, b: i32) -> i32 {
        a + ((b - a + 1) as f32 * self.0.f()).floor().min((b - a) as f32) as i32
    }
    fn u(&mut self, a: u32, b: u32) -> u32 {
        self.i(a as i32, b as i32) as u32
    }
    fn chance(&mut self, p: f32) -> bool {
        self.0.f() < p
    }
    fn sign(&mut self) -> f32 {
        if self.chance(0.5) {
            1.0
        } else {
            -1.0
        }
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.0.f() * v.len() as f32) as usize % v.len()]
    }
}

#[derive(Clone, Copy)]
enum Silhouette {
    Column,
    Vase,
    Globe,
    Hourglass,
    Flare,
    Cone,
}

fn spline(r: &mut R, kind: Silhouette, base_range: (f32, f32)) -> [f32; SPLINE_POINTS] {
    let base = r.f(base_range.0, base_range.1);
    // relative radius factors bottom → top
    let f: [f32; SPLINE_POINTS] = match kind {
        Silhouette::Column => {
            let w = r.f(0.0, 0.08);
            [1.0, 1.0 + w, 1.0 + w * 0.5, 1.0 - w * 0.3, 1.0 + w * 0.4, 0.96]
        }
        Silhouette::Vase => {
            let b = r.f(0.12, 0.28);
            [0.95, 1.0 + b * 0.7, 1.0 + b, 1.0 + b * 0.6, 0.95, 0.85]
        }
        Silhouette::Globe => {
            let b = r.f(0.18, 0.32);
            [0.85, 1.0 + b * 0.6, 1.0 + b, 1.0 + b * 0.85, 0.95 + b * 0.2, 0.78]
        }
        Silhouette::Hourglass => {
            let w = r.f(0.1, 0.2);
            [1.05, 1.0, 1.0 - w, 1.0 - w * 0.6, 1.02, 1.08]
        }
        Silhouette::Flare => {
            let g = r.f(0.35, 0.65);
            [0.9, 0.9 + g * 0.35, 0.9 + g * 0.65, 0.9 + g * 0.85, 0.9 + g * 0.96, 0.9 + g]
        }
        Silhouette::Cone => {
            let t = r.f(-0.3, 0.25);
            let mut a = [0.0; SPLINE_POINTS];
            for (k, v) in a.iter_mut().enumerate() {
                *v = 1.0 + t * k as f32 / (SPLINE_POINTS - 1) as f32;
            }
            a
        }
    };
    let mut out = [0.0; SPLINE_POINTS];
    for k in 0..SPLINE_POINTS {
        out[k] = (base * f[k] * r.f(0.97, 1.03)).clamp(40.0, 120.0);
    }
    out
}

/// Fresh shape parameters, keeping the user's mount.
fn blank(cur: &Design) -> Design {
    Design {
        mount: cur.mount,
        hole_d: cur.hole_d,
        ring_width: cur.ring_width,
        mount_thickness: cur.mount_thickness,
        sleeve_height: cur.sleeve_height,
        ledge: cur.ledge,
        ledge_d: cur.ledge_d,
        ledge_h: cur.ledge_h,
        ledge_chamfer: cur.ledge_chamfer,
        spokes: cur.spokes,
        spoke_width: cur.spoke_width,
        profile_kind: ProfileKind::Spline,
        ribs: 0,
        rib_depth: 0.0,
        twist_deg: 0.0,
        lobes: 0,
        rim_waves: 0,
        rim_depth: 0.0,
        waves: 0,
        wave_depth: 0.0,
        sides: 0,
        edge_band: 4.0,
        wall: 1.2,
        ..Design::default()
    }
}

fn roll(theme: Theme, r: &mut R, cur: &Design) -> Design {
    let mut d = blank(cur);
    match theme {
        Theme::Surprise => {
            let t = r.pick(&Theme::ALL[1..]);
            return roll(t, r, cur);
        }
        Theme::Minimal => {
            d.height = r.f(120.0, 190.0);
            let k = r.pick(&[Silhouette::Vase, Silhouette::Column, Silhouette::Globe, Silhouette::Cone]);
            d.spline = spline(r, k, (50.0, 66.0));
            d.wall = r.f(1.2, 1.6);
            if r.chance(0.35) {
                d.ribs = r.u(12, 30);
                d.rib_depth = r.f(0.6, 1.2);
                d.rib_sharpness = r.f(0.5, 1.0);
                d.twist_deg = if r.chance(0.5) { 0.0 } else { r.sign() * r.f(30.0, 90.0) };
            }
            if r.chance(0.2) {
                d.sides = r.u(5, 8);
                d.poly_amount = r.f(0.3, 0.7);
            }
        }
        Theme::Organic => {
            d.height = r.f(150.0, 240.0);
            let k = r.pick(&[Silhouette::Column, Silhouette::Vase, Silhouette::Hourglass]);
            d.spline = spline(r, k, (52.0, 62.0));
            d.lobes = r.u(2, 4);
            d.lobe_amp = r.f(5.0, 11.0) / (d.lobes as f32 * 0.5).max(1.0);
            d.lobe_growth = r.f(0.7, 1.5);
            d.lobe_sharpness = r.f(0.8, 1.3);
            d.lobe_twist_deg = r.sign() * r.f(90.0, 320.0);
            d.meander_deg = r.f(0.0, 35.0);
            d.meander_freq = r.f(0.5, 1.8);
            d.ribs_follow_folds = true;
            if r.chance(0.7) {
                d.rib_style = RibStyle::Grooves;
                d.ribs = r.u(40, 90);
                d.rib_depth = r.f(0.6, 1.0);
                d.rib_sharpness = r.f(4.0, 7.0);
            } else {
                d.ribs = r.u(24, 48);
                d.rib_depth = r.f(1.0, 2.0);
                d.rib_sharpness = r.f(1.0, 2.0);
            }
            d.wall = 1.0;
            d.edge_band = 3.0;
        }
        Theme::Floral => {
            d.height = r.f(95.0, 145.0);
            d.spline = spline(r, Silhouette::Flare, (50.0, 58.0));
            d.lobes = r.u(6, 12);
            d.lobe_amp = r.f(2.5, 4.5);
            d.lobe_growth = r.f(1.8, 2.8);
            d.lobe_sharpness = r.f(1.0, 1.6);
            d.lobe_twist_deg = r.sign() * r.f(0.0, 40.0);
            d.meander_deg = r.f(4.0, 12.0);
            d.meander_freq = r.f(1.0, 2.0);
            if r.chance(0.75) {
                d.rim_waves = if r.chance(0.7) { d.lobes } else { d.lobes * 2 };
                d.rim_depth = r.f(6.0, 16.0);
            }
            if r.chance(0.65) {
                d.ribs = r.u(70, 130);
                d.rib_style = RibStyle::Grooves;
                d.rib_depth = r.f(0.5, 0.8);
                d.rib_sharpness = r.f(4.0, 6.0);
                d.ribs_follow_folds = true;
            }
            d.wall = 1.0;
            d.edge_band = 2.0;
        }
        Theme::Geometric => {
            d.height = r.f(120.0, 200.0);
            let k = r.pick(&[Silhouette::Column, Silhouette::Cone, Silhouette::Vase, Silhouette::Hourglass]);
            d.spline = spline(r, k, (54.0, 68.0));
            d.sides = r.u(3, 8);
            d.poly_amount = r.f(0.6, 1.0);
            d.poly_twist_deg = if r.chance(0.25) { 0.0 } else { r.sign() * r.f(30.0, 180.0) };
            if r.chance(0.5) {
                d.texture_mode = TextureMode::Corrugated;
            }
            if r.chance(0.4) {
                d.ribs = d.sides * r.u(2, 5);
                d.rib_depth = r.f(1.0, 2.5);
                d.rib_sharpness = r.f(2.0, 4.0);
                d.twist_deg = d.poly_twist_deg;
            }
        }
        Theme::Crystal => {
            d.height = r.f(120.0, 190.0);
            let k = r.pick(&[Silhouette::Globe, Silhouette::Vase, Silhouette::Column]);
            d.spline = spline(r, k, (52.0, 64.0));
            d.sides = r.u(5, 10);
            d.poly_amount = r.f(0.7, 1.0);
            d.poly_twist_deg = r.sign() * r.f(30.0, 120.0);
            d.ribs = r.u(8, 20);
            d.cross_ribs = true;
            d.rib_depth = r.f(1.5, 3.0);
            d.rib_sharpness = r.f(2.0, 4.0);
            d.twist_deg = r.sign() * r.f(60.0, 160.0);
        }
        Theme::Waves => {
            d.height = r.f(120.0, 200.0);
            let k = r.pick(&[Silhouette::Globe, Silhouette::Vase, Silhouette::Column]);
            d.spline = spline(r, k, (50.0, 62.0));
            d.waves = r.u(5, 14);
            d.wave_depth = r.f(1.2, 2.8);
            d.wave_spiral = if r.chance(0.5) { 0 } else { r.i(-4, 4) };
            if r.chance(0.5) {
                d.texture_mode = TextureMode::Corrugated;
                d.wave_depth = d.wave_depth.min(2.2);
            }
            if r.chance(0.3) {
                d.ribs = r.u(16, 36);
                d.rib_depth = r.f(0.6, 1.2);
            }
        }
        Theme::Fluted => {
            d.height = r.f(130.0, 210.0);
            let k = r.pick(&[Silhouette::Column, Silhouette::Cone, Silhouette::Cone, Silhouette::Vase]);
            d.spline = spline(r, k, (52.0, 66.0));
            d.ribs = r.u(16, 40);
            d.rib_depth = r.f(2.0, 4.0);
            d.rib_sharpness = r.f(0.4, 0.9);
            d.texture_mode = TextureMode::Corrugated;
            d.edge_band = r.f(8.0, 15.0);
            d.wall = 1.0;
        }
        Theme::Twisted => {
            d.height = r.f(130.0, 210.0);
            let k = r.pick(&[Silhouette::Vase, Silhouette::Globe, Silhouette::Column, Silhouette::Cone]);
            d.spline = spline(r, k, (52.0, 66.0));
            d.ribs = r.u(18, 48);
            d.rib_depth = r.f(1.5, 3.5);
            d.rib_sharpness = r.f(0.8, 2.5);
            d.twist_deg = r.sign() * r.f(60.0, 300.0);
            d.cross_ribs = r.chance(0.25);
            if r.chance(0.5) {
                d.texture_mode = TextureMode::Corrugated;
                d.wall = 1.0;
            }
        }
    }
    d.sanitize();
    d
}

/// Problems with a design (empty = printable and fits).
pub fn evaluate(p: &Project) -> Vec<String> {
    let d = &p.design;
    let (nt, nz) = preview_res(d, 0.6);
    let sc = scene::build(d, &p.bulb, &p.lamp, nt, nz);
    let mut bad = Vec::new();
    if sc.min_clearance < 10.0 {
        bad.push(format!("bulb clearance {:.0} mm", sc.min_clearance));
    }
    if sc.max_overhang > 50.0 {
        bad.push(format!("overhang {:.0}°", sc.max_overhang));
    }
    if sc.min_thickness < 0.8 {
        bad.push(format!("wall {:.2} mm", sc.min_thickness));
    }
    if d.mount == MountKind::FlatRing && d.hole_d < 34.0 && sc.top_opening_r < p.bulb.rx + 3.0 {
        bad.push("top opening too small for the bulb".into());
    }
    let r_in0 = (0..72).map(|k| d.radii((k as f32 * 5.0).to_radians(), 0.0).0).fold(f32::INFINITY, f32::min);
    if d.hole_d * 0.5 + d.ring_width + 3.0 > r_in0 {
        bad.push("mount ring too wide".into());
    }
    let (bx, by, bz) = p.printer.volume();
    if 2.0 * sc.max_r > bx.min(by) - 6.0 || d.height > bz - 2.0 {
        bad.push("does not fit the printer".into());
    }
    bad
}

pub struct Generated {
    pub design: Design,
    pub tries: u32,
    /// Remaining problems if no fully valid candidate was found.
    pub problems: Vec<String>,
}

/// Generates a design for `theme` from `seed`, keeping the project's mount,
/// bulb, material and printer.
pub fn generate(theme: Theme, seed: u64, p: &Project) -> Generated {
    let mut r = R(Rng::new(seed ^ 0xA5A5_1234_5678_9ABC));
    let mut best: Option<(Design, Vec<String>)> = None;
    for k in 1..=60 {
        let d = roll(theme, &mut r, &p.design);
        let cand = Project { design: d.clone(), ..p.clone() };
        let bad = evaluate(&cand);
        if bad.is_empty() {
            return Generated { design: d, tries: k, problems: bad };
        }
        if best.as_ref().map_or(true, |(_, b)| bad.len() < b.len()) {
            best = Some((d, bad));
        }
    }
    let (design, problems) = best.expect("at least one candidate");
    Generated { design, tries: 60, problems }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_finds_valid_designs() {
        let p = Project::default();
        for theme in Theme::ALL {
            let mut ok = 0;
            for seed in 0..6u64 {
                let g = generate(theme, seed, &p);
                if g.problems.is_empty() {
                    ok += 1;
                }
                // generated meshes must always be exportable
                let (nt, nz) = crate::geometry::export_res(&g.design, false);
                crate::geometry::build_shell(&g.design, nt, nz).check_closed().unwrap();
            }
            assert!(ok >= 5, "{}: only {ok}/6 valid", theme.name());
        }
    }

    #[test]
    fn same_seed_same_design() {
        let p = Project::default();
        assert_eq!(generate(Theme::Organic, 42, &p).design, generate(Theme::Organic, 42, &p).design);
    }
}
