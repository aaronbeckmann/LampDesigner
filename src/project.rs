use crate::bulb::Bulb;
use crate::design::Design;
use crate::geometry::LampBase;
use crate::material::MaterialId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Printer {
    A1Mini,
    A1,
    P1S,
    X1C,
    H2D,
}

impl Printer {
    pub const ALL: [Printer; 5] = [Printer::A1Mini, Printer::A1, Printer::P1S, Printer::X1C, Printer::H2D];

    pub fn name(self) -> &'static str {
        match self {
            Printer::A1Mini => "Bambu Lab A1 mini",
            Printer::A1 => "Bambu Lab A1",
            Printer::P1S => "Bambu Lab P1S / P1P",
            Printer::X1C => "Bambu Lab X1C / X1E",
            Printer::H2D => "Bambu Lab H2D",
        }
    }

    /// Build volume (x, y, z) in mm.
    pub fn volume(self) -> (f32, f32, f32) {
        match self {
            Printer::A1Mini => (180.0, 180.0, 180.0),
            Printer::A1 | Printer::P1S | Printer::X1C => (256.0, 256.0, 256.0),
            Printer::H2D => (325.0, 320.0, 325.0),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Project {
    pub design: Design,
    pub material: MaterialId,
    /// sRGB filament colour.
    pub color: [u8; 3],
    pub bulb: Bulb,
    pub lamp: LampBase,
    pub printer: Printer,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            design: Design::default(),
            material: MaterialId::PlaBasic,
            color: MaterialId::PlaBasic.props().palette[0].1,
            bulb: Bulb::default(),
            lamp: LampBase::default(),
            printer: Printer::P1S,
        }
    }
}

impl Project {
    pub fn load(path: &std::path::Path) -> Result<Project, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut p: Project = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        p.design.sanitize();
        Ok(p)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, s).map_err(|e| e.to_string())
    }

    /// Filament colour in linear RGB (for the Shape view).
    pub fn clay(&self) -> glam::Vec3 {
        use crate::material::srgb_to_linear;
        glam::Vec3::new(srgb_to_linear(self.color[0]), srgb_to_linear(self.color[1]), srgb_to_linear(self.color[2]))
            .max(glam::Vec3::splat(0.03))
    }

    pub fn mass_g(&self, volume_mm3: f64) -> f32 {
        (volume_mm3 / 1000.0) as f32 * self.material.props().density
    }
}
