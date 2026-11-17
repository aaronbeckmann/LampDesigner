//! Parametric lampshade description. All lengths in millimetres.
//!
//! Coordinate system: z = 0 is the top rim of the lamp socket, +z points up
//! (the GENNAKER holds the bulb pointing up). The shade wall is described by
//! a radius r(θ, s) per angle θ and relative height s ∈ [0,1]:
//!
//!   silhouette (bulge curve or free spline) × polygon factor
//!   + flowing folds (lobes that twist and meander with height)
//!   + surface texture (ribs / grooves / waves)
//!
//! and an optional wavy top rim. It prints upright with the mount spider
//! lying on the build plate.

use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum TextureMode {
    /// Inner wall smooth, texture only on the outside: thickness varies, so
    /// ribs show up as light/dark bands when lit (lithophane effect).
    Relief,
    /// Whole wall follows the texture with constant thickness (vase look).
    Corrugated,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum MountKind {
    /// Flat ring with spokes. Small hole: washer clamped between bulb cap and
    /// socket. ~40 mm hole: standard E27 shade-ring thread.
    FlatRing,
    /// Tube that slides over the socket body (friction fit).
    Sleeve,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum ProfileKind {
    /// Bottom/top radius with a sine bulge.
    Bulge,
    /// Free curve through `SPLINE_POINTS` radii evenly spaced in height.
    Spline,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum RibStyle {
    /// Raised ridges.
    Ridges,
    /// Narrow cut-in grooves on a smooth surface.
    Grooves,
}

pub const SPLINE_POINTS: usize = 6;

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Design {
    // --- silhouette ---
    pub height: f32,
    pub profile_kind: ProfileKind,
    pub r_bottom: f32,
    pub r_top: f32,
    /// Extra radius added in a sine bump (negative = waist).
    pub bulge: f32,
    /// Relative height (0..1) of the bulge maximum.
    pub bulge_pos: f32,
    /// Radii of the free curve, bottom to top.
    pub spline: [f32; SPLINE_POINTS],
    /// Polygon cross-section, 0 = round.
    pub sides: u32,
    pub poly_amount: f32,
    pub poly_twist_deg: f32,
    // --- flowing folds ---
    pub lobes: u32,
    /// Fold depth (radial amplitude, mm) at the bottom.
    pub lobe_amp: f32,
    /// Fold depth multiplier at the top (1 = constant).
    pub lobe_growth: f32,
    pub lobe_sharpness: f32,
    pub lobe_twist_deg: f32,
    /// Side-to-side meander of the folds (deg of rotation).
    pub meander_deg: f32,
    /// Meander waves over the height.
    pub meander_freq: f32,
    // --- wall ---
    pub wall: f32,
    pub texture_mode: TextureMode,
    // --- ribs ---
    pub ribs: u32,
    pub rib_style: RibStyle,
    pub rib_depth: f32,
    pub rib_sharpness: f32,
    pub twist_deg: f32,
    pub cross_ribs: bool,
    /// Ribs follow the fold flow (twist + meander) instead of their own twist.
    pub ribs_follow_folds: bool,
    // --- horizontal waves ---
    pub waves: u32,
    pub wave_depth: f32,
    pub wave_spiral: i32,
    /// Texture fades out within this distance of the rims.
    pub edge_band: f32,
    // --- top rim ---
    pub rim_waves: u32,
    pub rim_depth: f32,
    // --- mount ---
    pub mount: MountKind,
    pub hole_d: f32,
    pub ring_width: f32,
    pub mount_thickness: f32,
    pub sleeve_height: f32,
    /// Weight saving for tall sleeves: this many struts with windows between
    /// them (0 = solid sleeve). Window tops are 45° points (no supports).
    pub sleeve_windows: u32,
    /// Width of a strut (arc length at the sleeve's outside).
    pub sleeve_strut_w: f32,
    /// Solid sleeve kept at the bottom and the top (around the ledge).
    pub sleeve_band: f32,
    /// Sleeve only: inward ledge at the top of the sleeve that rests on the
    /// socket's top rim so the shade can't slide down.
    pub ledge: bool,
    /// Inner diameter of the ledge (must clear the bulb's E27 cap/neck).
    pub ledge_d: f32,
    /// Thickness of the flat part of the ledge above the socket.
    pub ledge_h: f32,
    /// 45° chamfer under the ledge so it prints without supports.
    pub ledge_chamfer: bool,
    /// Spiral-vase print: the shade keeps only ring + spokes in its bottom
    /// layers; the socket sleeve becomes a separate insert that is pushed up
    /// into the ring from below and glued (sleeve mount only).
    pub vase_mode: bool,
    /// Diametral clearance between the insert's sleeve and the ring hole (glue gap).
    pub glue_gap: f32,
    /// How far the insert's glue flange reaches past the ring hole (radial).
    pub glue_ledge_width: f32,
    /// Thickness of the insert's glue flange.
    pub glue_ledge_h: f32,
    /// Print the shade in this many parts stacked along the height (1 = one
    /// piece), for shades taller than the printer. Cuts are evenly spaced.
    pub split_parts: u32,
    /// Overlapping (rabbet) joint at each cut: the lower part carries the
    /// inner half of the wall up as a lip, the upper part the outer half
    /// down over it. Off = flat butt joint.
    pub split_joint: bool,
    /// Height of the overlap at a cut.
    pub split_overlap: f32,
    /// Radial clearance in the joint (glue gap).
    pub split_gap: f32,
    pub spokes: u32,
    pub spoke_width: f32,
}

impl Default for Design {
    fn default() -> Self {
        Design {
            height: 150.0,
            profile_kind: ProfileKind::Bulge,
            r_bottom: 62.0,
            r_top: 46.0,
            bulge: 14.0,
            bulge_pos: 0.4,
            spline: [60.0, 72.0, 76.0, 70.0, 58.0, 46.0],
            sides: 0,
            poly_amount: 1.0,
            poly_twist_deg: 0.0,
            lobes: 0,
            lobe_amp: 8.0,
            lobe_growth: 1.0,
            lobe_sharpness: 1.0,
            lobe_twist_deg: 0.0,
            meander_deg: 0.0,
            meander_freq: 1.0,
            wall: 1.2,
            texture_mode: TextureMode::Relief,
            ribs: 28,
            rib_style: RibStyle::Ridges,
            rib_depth: 2.0,
            rib_sharpness: 1.0,
            twist_deg: 90.0,
            cross_ribs: false,
            ribs_follow_folds: false,
            waves: 0,
            wave_depth: 0.0,
            wave_spiral: 0,
            edge_band: 5.0,
            rim_waves: 0,
            rim_depth: 0.0,
            // IKEA GENNAKER: sleeve over the socket with a ledge on its top rim
            mount: MountKind::Sleeve,
            hole_d: 42.75,
            ring_width: 2.0,
            mount_thickness: 2.0,
            sleeve_height: 12.0,
            sleeve_windows: 0,
            sleeve_strut_w: 5.0,
            sleeve_band: 6.0,
            ledge: true,
            ledge_d: 33.0,
            ledge_h: 2.0,
            ledge_chamfer: true,
            vase_mode: false,
            glue_gap: 0.4,
            glue_ledge_width: 4.0,
            glue_ledge_h: 1.6,
            split_parts: 1,
            split_joint: true,
            split_overlap: 6.0,
            split_gap: 0.15,
            spokes: 3,
            spoke_width: 6.0,
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Uniform Catmull-Rom through evenly spaced points.
fn catmull_rom(p: &[f32], s: f32) -> f32 {
    let n = p.len();
    let x = s.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = (x.floor() as usize).min(n - 2);
    let t = x - i as f32;
    let g = |k: isize| p[(k.clamp(0, n as isize - 1)) as usize];
    let (p0, p1, p2, p3) = (g(i as isize - 1), g(i as isize), g(i as isize + 1), g(i as isize + 2));
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}

impl Design {
    /// z of the shade's bottom rim (= build plate).
    pub fn z0(&self) -> f32 {
        match self.mount {
            MountKind::FlatRing => 0.0,
            // vase mode: the shade's ring rests on the insert's glue flange
            MountKind::Sleeve if self.vase_mode => -self.sleeve_height + self.glue_ledge_h,
            MountKind::Sleeve => -self.sleeve_height,
        }
    }

    /// z of the highest point of the top rim.
    pub fn z1(&self) -> f32 {
        self.z0() + self.height
    }

    /// Base silhouette radius at relative height s in [0,1].
    pub fn profile(&self, s: f32) -> f32 {
        let s = s.clamp(0.0, 1.0);
        let r = match self.profile_kind {
            ProfileKind::Bulge => {
                let base = self.r_bottom + (self.r_top - self.r_bottom) * s;
                let p = self.bulge_pos.clamp(0.05, 0.95);
                let k = 0.5f32.ln() / p.ln();
                base + self.bulge * (PI * s.powf(k)).sin()
            }
            ProfileKind::Spline => catmull_rom(&self.spline, s),
        };
        r.max(8.0)
    }

    /// Rotation (rad) of the fold pattern at height s.
    pub fn flow(&self, s: f32) -> f32 {
        self.lobe_twist_deg.to_radians() * s
            + self.meander_deg.to_radians() * (TAU * self.meander_freq * s).sin()
    }

    /// Polygon radius multiplier (1 at the corners).
    pub fn poly(&self, theta: f32, s: f32) -> f32 {
        if self.sides < 3 || self.poly_amount <= 0.0 {
            return 1.0;
        }
        let n = self.sides as f32;
        let seg = TAU / n;
        let th = theta + self.poly_twist_deg.to_radians() * s;
        let a = th.rem_euclid(seg) - seg * 0.5;
        let f = (PI / n).cos() / a.cos();
        1.0 + self.poly_amount.clamp(0.0, 1.0) * (f - 1.0)
    }

    /// Radial offset of the flowing folds (mm, roughly -amp..+amp).
    pub fn folds(&self, theta: f32, s: f32) -> f32 {
        if self.lobes == 0 || self.lobe_amp == 0.0 {
            return 0.0;
        }
        let k = self.lobes as f32;
        let g = (0.5 + 0.5 * (k * (theta - self.flow(s))).cos()).powf(self.lobe_sharpness.max(0.2));
        let amp = self.lobe_amp * (1.0 + (self.lobe_growth - 1.0) * s);
        amp * (2.0 * g - 1.0)
    }

    /// Outward texture displacement (mm, >= 0).
    pub fn texture(&self, theta: f32, s: f32) -> f32 {
        let mut d = 0.0;
        if self.ribs > 0 && self.rib_depth > 0.0 {
            let (a1, a2) = if self.ribs_follow_folds {
                let f = self.flow(s);
                (theta - f, theta + f)
            } else {
                let tw = self.twist_deg.to_radians() * s;
                (theta + tw, theta - tw)
            };
            let n = self.ribs as f32;
            let sharp = self.rib_sharpness.max(0.1);
            let style = self.rib_style;
            let g = |u: f32| {
                let v = (0.5 + 0.5 * u.cos()).powf(sharp);
                match style {
                    RibStyle::Ridges => v,
                    RibStyle::Grooves => 1.0 - v,
                }
            };
            let mut v = g(n * a1);
            if self.cross_ribs {
                v = match self.rib_style {
                    RibStyle::Ridges => v.max(g(n * a2)),
                    RibStyle::Grooves => v.min(g(n * a2)),
                };
            }
            d += self.rib_depth * v;
        }
        if self.waves > 0 && self.wave_depth > 0.0 {
            let u = TAU * self.waves as f32 * s + self.wave_spiral as f32 * theta;
            d += self.wave_depth * (0.5 + 0.5 * u.cos());
        }
        if self.edge_band > 0.0 {
            let z = s * self.height;
            let dist = z.min(self.height - z);
            d *= smoothstep(0.0, self.edge_band, dist);
        }
        d
    }

    /// Wall height at angle theta (wavy top rim; crests sit on the folds).
    pub fn rim_height(&self, theta: f32) -> f32 {
        if self.rim_waves == 0 || self.rim_depth <= 0.0 {
            return self.height;
        }
        let u = self.rim_waves as f32 * (theta - self.flow(1.0));
        let depth = self.rim_depth.min(self.height * 0.5);
        self.height - depth * (0.5 - 0.5 * u.cos())
    }

    /// (inner radius, outer radius) of the wall at angle theta, relative height s.
    pub fn radii(&self, theta: f32, s: f32) -> (f32, f32) {
        let base = |th: f32, s: f32| (self.profile(s) * self.poly(th, s) + self.folds(th, s)).max(8.0);
        // surface the inner wall runs parallel to
        let follow = |th: f32, s: f32| match self.texture_mode {
            TextureMode::Relief => base(th, s),
            TextureMode::Corrugated => base(th, s) + self.texture(th, s),
        };
        let rb = base(theta, s);
        let ro = rb + self.texture(theta, s);
        let g = follow(theta, s);
        // offset along the surface normal (not radially) so steep flanks keep
        // the nominal wall thickness
        let (ht, hs) = (1e-3f32, 1e-3f32);
        let dr_dth = (follow(theta + ht, s) - follow(theta - ht, s)) / (2.0 * ht * g.max(1.0));
        let dr_dz = (follow(theta, (s + hs).min(1.0)) - follow(theta, (s - hs).max(0.0)))
            / (((s + hs).min(1.0) - (s - hs).max(0.0)) * self.height);
        let stretch = (1.0 + dr_dth * dr_dth + dr_dz * dr_dz).sqrt().min(3.0);
        let ri = (g - self.wall * stretch).max(1.0);
        (ri, ro.max(ri + 0.3))
    }

    pub fn sanitize(&mut self) {
        self.height = self.height.clamp(20.0, 800.0);
        self.r_bottom = self.r_bottom.clamp(15.0, 200.0);
        self.r_top = self.r_top.clamp(15.0, 200.0);
        for r in &mut self.spline {
            *r = r.clamp(10.0, 200.0);
        }
        self.wall = self.wall.clamp(0.4, 5.0);
        self.spokes = self.spokes.clamp(2, 8);
        self.hole_d = self.hole_d.clamp(10.0, 80.0);
        self.ring_width = self.ring_width.clamp(2.0, 40.0);
        self.mount_thickness = self.mount_thickness.clamp(0.6, 6.0);
        self.sleeve_height = self.sleeve_height.clamp(3.0, 150.0);
        self.sleeve_windows = self.sleeve_windows.min(16);
        self.sleeve_strut_w = self.sleeve_strut_w.clamp(2.0, 20.0);
        self.sleeve_band = self.sleeve_band.clamp(2.0, 30.0);
        if self.mount == MountKind::Sleeve {
            self.ledge_d = self.ledge_d.clamp(20.0, (self.hole_d - 1.0).max(20.0));
        }
        self.ledge_h = self.ledge_h.clamp(0.8, 10.0);
        self.glue_gap = self.glue_gap.clamp(0.0, 2.0);
        self.glue_ledge_width = self.glue_ledge_width.clamp(1.0, 15.0);
        self.glue_ledge_h = self.glue_ledge_h.clamp(0.6, 5.0);
        self.sleeve_height = self.sleeve_height.max(self.glue_ledge_h + 2.0);
        self.split_parts = self.split_parts.clamp(1, 8);
        self.split_overlap = self.split_overlap.clamp(2.0, 20.0);
        self.split_gap = self.split_gap.clamp(0.0, 0.6);
        self.spoke_width = self.spoke_width.clamp(2.0, 30.0);
        self.bulge_pos = self.bulge_pos.clamp(0.05, 0.95);
        self.lobe_growth = self.lobe_growth.clamp(0.0, 5.0);
        self.meander_freq = self.meander_freq.clamp(0.0, 6.0);
    }

    /// Angular frequency the mesh must resolve (features per revolution).
    pub fn angular_detail(&self) -> usize {
        let rib = self.ribs as usize
            * match (self.cross_ribs, self.rib_style) {
                (_, RibStyle::Grooves) => 3,
                (true, _) => 3,
                _ => 2,
            }
            / 2;
        rib.max(self.sides as usize * 2)
            .max(self.wave_spiral.unsigned_abs() as usize * 2)
            .max(self.lobes as usize * 2)
            .max(self.rim_waves as usize * 2)
    }
}

pub fn presets() -> Vec<(&'static str, Design)> {
    let base = Design::default();
    vec![
        ("Twisted ribs", base.clone()),
        (
            "Smooth bell",
            Design { height: 140.0, r_bottom: 80.0, r_top: 40.0, bulge: 10.0, bulge_pos: 0.6, ribs: 0, ..base.clone() },
        ),
        (
            "Diamond lattice",
            Design {
                height: 160.0,
                r_bottom: 60.0,
                r_top: 58.0,
                bulge: 14.0,
                bulge_pos: 0.5,
                ribs: 18,
                rib_depth: 2.5,
                rib_sharpness: 2.5,
                twist_deg: 120.0,
                cross_ribs: true,
                ..base.clone()
            },
        ),
        (
            "Wave globe",
            Design {
                height: 165.0,
                r_bottom: 58.0,
                r_top: 46.0,
                bulge: 20.0,
                bulge_pos: 0.45,
                ribs: 0,
                waves: 10,
                wave_depth: 2.0,
                ..base.clone()
            },
        ),
        (
            "Faceted hex",
            Design {
                height: 150.0,
                r_bottom: 66.0,
                r_top: 50.0,
                bulge: 8.0,
                sides: 6,
                poly_twist_deg: 60.0,
                ribs: 0,
                texture_mode: TextureMode::Corrugated,
                ..base.clone()
            },
        ),
        (
            "Spiral corrugated",
            Design {
                texture_mode: TextureMode::Corrugated,
                ribs: 36,
                rib_depth: 3.0,
                twist_deg: 180.0,
                wall: 1.0,
                ..base.clone()
            },
        ),
        (
            // tall column whose oval cross-section rotates and sways, fine grooves along the flow
            "Organic twist",
            Design {
                height: 240.0,
                profile_kind: ProfileKind::Spline,
                spline: [58.0, 64.0, 60.0, 56.0, 58.0, 52.0],
                lobes: 2,
                lobe_amp: 9.0,
                lobe_growth: 1.2,
                lobe_sharpness: 1.0,
                lobe_twist_deg: 220.0,
                meander_deg: 25.0,
                meander_freq: 1.0,
                ribs: 60,
                rib_style: RibStyle::Grooves,
                rib_depth: 0.8,
                rib_sharpness: 6.0,
                ribs_follow_folds: true,
                texture_mode: TextureMode::Relief,
                wall: 1.0,
                edge_band: 3.0,
                ..base.clone()
            },
        ),
        (
            // flared shade with deep meandering folds, scalloped top rim, flowing grooves
            "Flowing crown",
            Design {
                height: 115.0,
                profile_kind: ProfileKind::Spline,
                spline: [56.0, 68.0, 78.0, 85.0, 89.0, 91.0],
                lobes: 9,
                lobe_amp: 3.5,
                lobe_growth: 2.3,
                lobe_sharpness: 1.4,
                lobe_twist_deg: 25.0,
                meander_deg: 9.0,
                meander_freq: 1.5,
                ribs: 110,
                rib_style: RibStyle::Grooves,
                rib_depth: 0.6,
                rib_sharpness: 5.0,
                ribs_follow_folds: true,
                rim_waves: 9,
                rim_depth: 12.0,
                texture_mode: TextureMode::Relief,
                wall: 1.0,
                edge_band: 2.0,
                ..base
            },
        ),
    ]
}
