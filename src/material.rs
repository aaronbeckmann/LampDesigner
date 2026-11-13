//! Optical/thermal models of Bambu Lab filaments.
//!
//! A printed wall is treated as a scattering slab (Kubelka–Munk) plus an
//! unscattered ("ballistic") part that lets you see the bulb through
//! translucent material. Coefficients are estimates tuned to typical
//! behaviour (e.g. white PLA at 1 mm transmits roughly 15–20 % diffusely);
//! real filament batches and colours vary.

use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum MaterialId {
    PlaBasic,
    PlaMatte,
    PetgHf,
    PetgTranslucent,
}

pub struct MaterialProps {
    pub name: &'static str,
    /// Kubelka–Munk scattering coefficient S (1/mm).
    pub scatter: f32,
    /// Base absorption K (1/mm) of the uncoloured polymer.
    pub absorb: f32,
    /// How strongly the chosen colour adds absorption.
    pub pigment: f32,
    /// Fraction of unscattered light that layer lines still diffuse.
    pub haze: f32,
    pub ior: f32,
    /// g/cm³
    pub density: f32,
    /// Heat deflection temperature (°C), from Bambu Lab TDS (approx.).
    pub hdt: f32,
    pub price_eur_kg: f32,
    pub note: &'static str,
    pub palette: &'static [(&'static str, [u8; 3])],
}

static PLA_BASIC: MaterialProps = MaterialProps {
    name: "Bambu PLA Basic",
    scatter: 4.0,
    absorb: 0.04,
    pigment: 0.9,
    haze: 0.9,
    ior: 1.46,
    density: 1.26,
    hdt: 57.0,
    price_eur_kg: 20.0,
    note: "Even, diffuse glow. Softens around 57 °C: LED bulbs only.",
    palette: &[
        ("Jade White", [246, 246, 240]),
        ("Beige", [242, 226, 196]),
        ("Light Gray", [192, 194, 196]),
        ("Yellow", [250, 216, 52]),
        ("Orange", [255, 116, 26]),
        ("Red", [216, 28, 32]),
        ("Pink", [245, 154, 178]),
        ("Magenta", [216, 28, 128]),
        ("Cyan", [26, 166, 216]),
        ("Blue", [26, 76, 204]),
        ("Mistletoe Green", [40, 128, 76]),
        ("Black", [14, 14, 14]),
    ],
};

static PLA_MATTE: MaterialProps = MaterialProps {
    name: "Bambu PLA Matte",
    scatter: 6.0,
    absorb: 0.05,
    pigment: 1.0,
    haze: 0.95,
    ior: 1.46,
    density: 1.32,
    hdt: 55.0,
    price_eur_kg: 22.0,
    note: "Chalk-like fillers: more opaque than PLA Basic, very soft light.",
    palette: &[
        ("Ivory White", [242, 240, 230]),
        ("Latte Brown", [192, 154, 116]),
        ("Sakura Pink", [242, 192, 204]),
        ("Lemon Yellow", [242, 230, 116]),
        ("Mandarin Orange", [242, 140, 64]),
        ("Ice Blue", [166, 204, 230]),
        ("Grass Green", [102, 178, 90]),
        ("Ash Gray", [154, 154, 154]),
        ("Charcoal", [38, 38, 38]),
    ],
};

static PETG_HF: MaterialProps = MaterialProps {
    name: "Bambu PETG HF",
    scatter: 2.2,
    absorb: 0.03,
    pigment: 0.8,
    haze: 0.85,
    ior: 1.57,
    density: 1.27,
    hdt: 69.0,
    price_eur_kg: 20.0,
    note: "Brighter than PLA, slight gloss, tolerates more heat.",
    palette: &[
        ("White", [242, 242, 242]),
        ("Cream", [242, 230, 204]),
        ("Yellow", [250, 204, 40]),
        ("Orange", [250, 110, 30]),
        ("Red", [204, 30, 36]),
        ("Blue", [30, 80, 190]),
        ("Green", [40, 150, 80]),
        ("Gray", [128, 130, 132]),
        ("Black", [16, 16, 16]),
    ],
};

static PETG_TRANSLUCENT: MaterialProps = MaterialProps {
    name: "Bambu PETG Translucent",
    scatter: 0.12,
    absorb: 0.01,
    pigment: 0.45,
    haze: 0.55,
    ior: 1.57,
    density: 1.25,
    hdt: 69.0,
    price_eur_kg: 22.0,
    note: "Glass-like: very bright, the bulb stays visible as a hotspot.",
    palette: &[
        ("Clear", [248, 248, 248]),
        ("Translucent Gray", [154, 154, 158]),
        ("Translucent Light Blue", [178, 216, 242]),
        ("Translucent Olive", [166, 178, 90]),
        ("Translucent Brown", [178, 128, 90]),
        ("Translucent Teal", [102, 192, 192]),
        ("Translucent Orange", [250, 154, 64]),
        ("Translucent Purple", [166, 116, 204]),
        ("Translucent Pink", [242, 166, 192]),
    ],
};

impl MaterialId {
    pub const ALL: [MaterialId; 4] =
        [MaterialId::PlaBasic, MaterialId::PlaMatte, MaterialId::PetgHf, MaterialId::PetgTranslucent];

    pub fn props(self) -> &'static MaterialProps {
        match self {
            MaterialId::PlaBasic => &PLA_BASIC,
            MaterialId::PlaMatte => &PLA_MATTE,
            MaterialId::PetgHf => &PETG_HF,
            MaterialId::PetgTranslucent => &PETG_TRANSLUCENT,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Optics {
    pub k: Vec3,
    pub s: f32,
    pub haze: f32,
    pub ior: f32,
    pub f0: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Slab {
    /// Unscattered transmission (you can see through this part).
    pub ballistic: Vec3,
    /// Diffuse transmission.
    pub t_diff: Vec3,
    /// Diffuse reflection.
    pub r_diff: Vec3,
}

pub fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Kubelka–Munk reflectance and transmittance of a slab (black background).
fn km(k: f64, s: f64, t: f64) -> (f64, f64) {
    let s = s.max(1e-6);
    let k = k.max(1e-5);
    let a = 1.0 + k / s;
    let b = (a * a - 1.0).sqrt().max(1e-9);
    let x = (b * s * t).min(40.0);
    let (sh, ch) = (x.sinh(), x.cosh());
    let den = a * sh + b * ch;
    (sh / den, b / den)
}

impl Optics {
    pub fn new(id: MaterialId, color: [u8; 3]) -> Optics {
        let p = id.props();
        let k = |c: u8| p.absorb + p.pigment * -(srgb_to_linear(c).clamp(0.02, 1.0)).ln();
        let f0 = ((p.ior - 1.0) / (p.ior + 1.0)).powi(2);
        Optics { k: Vec3::new(k(color[0]), k(color[1]), k(color[2])), s: p.scatter, haze: p.haze, ior: p.ior, f0 }
    }

    /// `path`: geometric path length through the wall, `t`: wall thickness.
    pub fn slab(&self, path: f32, t: f32) -> Slab {
        let mut b = [0f32; 3];
        let mut td = [0f32; 3];
        let mut rd = [0f32; 3];
        for c in 0..3 {
            let k = self.k[c];
            let e = (-(k + self.s) * path).exp();
            let (r, tt) = km(k as f64, self.s as f64, t as f64);
            let rem = 1.0 - e;
            b[c] = (1.0 - self.haze) * e;
            td[c] = self.haze * e + rem * tt as f32;
            rd[c] = rem * r as f32;
        }
        Slab { ballistic: Vec3::from(b), t_diff: Vec3::from(td), r_diff: Vec3::from(rd) }
    }

    pub fn ballistic(&self, path: f32) -> Vec3 {
        let f = |k: f32| (1.0 - self.haze) * (-(k + self.s) * path).exp();
        Vec3::new(f(self.k.x), f(self.k.y), f(self.k.z))
    }

    pub fn fresnel(&self, cos_i: f32) -> f32 {
        self.f0 + (1.0 - self.f0) * (1.0 - cos_i.clamp(0.0, 1.0)).powi(5)
    }

    /// Cosine of the refracted angle inside the wall.
    pub fn cos_t(&self, cos_i: f32) -> f32 {
        (1.0 - (1.0 - cos_i * cos_i) / (self.ior * self.ior)).max(0.05).sqrt()
    }

    /// Reflectance of a thick (2 mm) piece, used for the mount spider.
    pub fn albedo(&self) -> Vec3 {
        self.slab(2.0, 2.0).r_diff
    }
}

pub fn lum(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slab_conserves_energy() {
        for id in MaterialId::ALL {
            for &col in id.props().palette {
                let o = Optics::new(id, col.1);
                for t in [0.4f32, 0.8, 1.2, 2.0, 4.0] {
                    let s = o.slab(t, t);
                    let sum = s.ballistic + s.t_diff + s.r_diff;
                    assert!(sum.max_element() <= 1.0001, "{} {t}: {sum}", id.props().name);
                    assert!(s.ballistic.min_element() >= 0.0 && s.t_diff.min_element() >= 0.0);
                }
            }
        }
    }

    #[test]
    fn white_pla_transmits_plausibly() {
        let o = Optics::new(MaterialId::PlaBasic, [246, 246, 240]);
        let s = o.slab(1.0, 1.0);
        let t = lum(s.t_diff + s.ballistic);
        assert!((0.1..0.3).contains(&t), "{t}");
        let o = Optics::new(MaterialId::PetgTranslucent, [248, 248, 248]);
        let s = o.slab(1.0, 1.0);
        assert!(lum(s.t_diff + s.ballistic) > 0.8);
        assert!(lum(s.ballistic) > 0.3);
    }
}
