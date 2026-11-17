//! E27 bulb models (geometry, photometry, rough heat output).

use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Bulb {
    pub name: String,
    /// Horizontal glass radius (mm).
    pub rx: f32,
    /// Vertical glass half-height (mm).
    pub rz: f32,
    /// Height of the glass centre above the socket top (mm).
    pub center_h: f32,
    pub lumens: f32,
    pub watts: f32,
    pub led: bool,
    /// Colour temperature (K).
    pub cct: f32,
    /// Half-angle of the dark cone towards the socket (deg).
    pub dark_cone_deg: f32,
}

impl Default for Bulb {
    fn default() -> Self {
        presets().remove(0)
    }
}

/// Table helper for the preset list below (one argument per column).
#[allow(clippy::too_many_arguments)]
fn b(name: &str, rx: f32, rz: f32, center_h: f32, lumens: f32, watts: f32, led: bool, cct: f32, cone: f32) -> Bulb {
    Bulb { name: name.into(), rx, rz, center_h, lumens, watts, led, cct, dark_cone_deg: cone }
}

pub fn presets() -> Vec<Bulb> {
    vec![
        b("IKEA SOLHETTA LED 470 lm 2.9 W (globe opal Ø60)", 30.0, 31.0, 55.0, 470.0, 2.9, true, 2700.0, 45.0),
        b("LED A60 806 lm (60 W equiv.)", 30.0, 31.0, 55.0, 806.0, 7.0, true, 2700.0, 45.0),
        b("LED A60 1055 lm (75 W equiv.)", 30.0, 31.0, 55.0, 1055.0, 9.5, true, 2700.0, 45.0),
        b("LED A60 470 lm (40 W equiv.)", 30.0, 31.0, 55.0, 470.0, 4.5, true, 2700.0, 45.0),
        b("LED G95 globe 806 lm", 47.5, 47.5, 65.0, 806.0, 7.0, true, 2700.0, 35.0),
        b("LED G125 globe 1055 lm", 62.5, 62.5, 85.0, 1055.0, 9.5, true, 2700.0, 35.0),
        b("Decorative ellipsoid Ø150 (MOLNART-like, approx.)", 75.0, 55.0, 80.0, 250.0, 4.0, true, 2200.0, 30.0),
        b("Incandescent A60 60 W (heat reference)", 30.0, 31.0, 55.0, 710.0, 60.0, false, 2700.0, 30.0),
    ]
}

impl Bulb {
    /// Linear RGB with luminance 1 (Tanner Helland blackbody approximation).
    pub fn color(&self) -> Vec3 {
        let t = (self.cct / 100.0).clamp(10.0, 400.0);
        let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
        let g = if t <= 66.0 {
            99.470_8 * t.ln() - 161.119_57
        } else {
            288.122_17 * (t - 60.0).powf(-0.075_514_85)
        };
        let bl = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.517_73 * (t - 10.0).ln() - 305.044_8
        };
        let lin = |v: f32| (v.clamp(0.0, 255.0) / 255.0).powf(2.2);
        let c = Vec3::new(lin(r), lin(g), lin(bl));
        c / crate::material::lum(c).max(1e-4)
    }

    /// Glass surface area in m² (Knud Thomsen ellipsoid approximation).
    pub fn area_m2(&self) -> f32 {
        let (a, c) = (self.rx / 1000.0, self.rz / 1000.0);
        let p = 1.6075f32;
        let ap = a.powf(p);
        let cp = c.powf(p);
        4.0 * PI * ((ap * ap + 2.0 * ap * cp) / 3.0).powf(1.0 / p)
    }

    /// Mean glass luminance (cd/m²), Lambertian emitter.
    pub fn luminance(&self) -> f32 {
        self.lumens / (PI * self.area_m2())
    }

    /// Luminous intensity (cd) for a direction with z-component `dir_z`.
    pub fn intensity(&self, dir_z: f32) -> f32 {
        let cb = self.dark_cone_deg.to_radians().cos();
        if dir_z >= -cb {
            self.lumens / (2.0 * PI * (1.0 + cb))
        } else {
            0.0
        }
    }

    /// (infrared/radiated heat emitted from the bulb centre, convective heat) in W.
    pub fn heat(&self) -> (f32, f32) {
        let p_vis = self.lumens / 300.0;
        if self.led {
            let tb: f32 = 273.15 + 55.0;
            let ta = 298.15f32;
            let p_rad = (0.9 * 5.67e-8 * (tb.powi(4) - ta.powi(4)) * self.area_m2()).min(0.4 * self.watts);
            (p_rad, (self.watts - p_vis - p_rad).max(0.0))
        } else {
            let p_ir = 0.8 * self.watts;
            (p_ir, (self.watts - p_vis - p_ir).max(0.0))
        }
    }

    /// Rough temperature of the bulb cap / neck (what a clamp washer touches).
    pub fn cap_temp(&self) -> f32 {
        if self.led {
            25.0 + 3.5 * self.watts
        } else {
            25.0 + 1.6 * self.watts
        }
    }

    /// Glass radius at height dz above the glass centre (0 outside the bulb).
    pub fn radius_at(&self, dz: f32) -> f32 {
        let q = dz / self.rz;
        if q.abs() >= 1.0 {
            0.0
        } else {
            self.rx * (1.0 - q * q).sqrt()
        }
    }
}
