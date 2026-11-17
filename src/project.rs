use crate::bulb::Bulb;
use crate::design::Design;
use crate::geometry::LampBase;
use crate::material::MaterialId;
use serde::{Deserialize, Serialize};

/// Printer presets (build volume). `Custom` uses `Project::custom_bed`.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Printer {
    A1Mini,
    A1,
    P1S,
    X1C,
    H2D,
    H2S,
    PrusaMini,
    PrusaMk4,
    PrusaCoreOne,
    PrusaXl,
    Ender3V3,
    CrealityK1,
    CrealityK1Max,
    CrealityK2Plus,
    Voron350,
    ElegooNeptune4Max,
    Custom,
}

impl Printer {
    pub const ALL: [Printer; 17] = [
        Printer::A1Mini,
        Printer::A1,
        Printer::P1S,
        Printer::X1C,
        Printer::H2D,
        Printer::H2S,
        Printer::PrusaMini,
        Printer::PrusaMk4,
        Printer::PrusaCoreOne,
        Printer::PrusaXl,
        Printer::Ender3V3,
        Printer::CrealityK1,
        Printer::CrealityK1Max,
        Printer::CrealityK2Plus,
        Printer::Voron350,
        Printer::ElegooNeptune4Max,
        Printer::Custom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Printer::A1Mini => "Bambu Lab A1 mini",
            Printer::A1 => "Bambu Lab A1",
            Printer::P1S => "Bambu Lab P1S / P1P",
            Printer::X1C => "Bambu Lab X1C / X1E",
            Printer::H2D => "Bambu Lab H2D",
            Printer::H2S => "Bambu Lab H2S",
            Printer::PrusaMini => "Prusa MINI+",
            Printer::PrusaMk4 => "Prusa MK4 / MK4S",
            Printer::PrusaCoreOne => "Prusa CORE One",
            Printer::PrusaXl => "Prusa XL",
            Printer::Ender3V3 => "Creality Ender-3 V3",
            Printer::CrealityK1 => "Creality K1 / K1C",
            Printer::CrealityK1Max => "Creality K1 Max",
            Printer::CrealityK2Plus => "Creality K2 Plus",
            Printer::Voron350 => "Voron 2.4 (350)",
            Printer::ElegooNeptune4Max => "Elegoo Neptune 4 Max",
            Printer::Custom => "Custom size",
        }
    }

    /// Build volume (x, y, z) in mm of a preset (`Custom` → `None`).
    pub fn preset_volume(self) -> Option<(f32, f32, f32)> {
        Some(match self {
            Printer::A1Mini => (180.0, 180.0, 180.0),
            Printer::A1 | Printer::P1S | Printer::X1C => (256.0, 256.0, 256.0),
            Printer::H2D => (325.0, 320.0, 325.0),
            Printer::H2S => (340.0, 320.0, 340.0),
            Printer::PrusaMini => (180.0, 180.0, 180.0),
            Printer::PrusaMk4 => (250.0, 210.0, 220.0),
            Printer::PrusaCoreOne => (250.0, 220.0, 270.0),
            Printer::PrusaXl => (360.0, 360.0, 360.0),
            Printer::Ender3V3 => (220.0, 220.0, 250.0),
            Printer::CrealityK1 => (220.0, 220.0, 250.0),
            Printer::CrealityK1Max => (300.0, 300.0, 300.0),
            Printer::CrealityK2Plus => (350.0, 350.0, 350.0),
            Printer::Voron350 => (350.0, 350.0, 340.0),
            Printer::ElegooNeptune4Max => (420.0, 420.0, 480.0),
            Printer::Custom => return None,
        })
    }
}

/// Does a round part of diameter `dia` and height `h` fit the volume (with margins)?
pub fn fits(vol: (f32, f32, f32), dia: f32, h: f32) -> (bool, bool) {
    (dia <= vol.0.min(vol.1) - 4.0, h <= vol.2 - 2.0)
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
    /// Build volume (x, y, z) used with Printer::Custom.
    pub custom_bed: [f32; 3],
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
            custom_bed: [256.0, 256.0, 256.0],
        }
    }
}

impl Project {
    /// Build volume (x, y, z) in mm of the selected printer.
    pub fn bed(&self) -> (f32, f32, f32) {
        self.printer.preset_volume().unwrap_or((self.custom_bed[0], self.custom_bed[1], self.custom_bed[2]))
    }

    /// Presets whose bed is wide enough for a shade of diameter `dia`
    /// (height is not considered: tall shades can be split).
    pub fn printers_fitting(dia: f32) -> Vec<Printer> {
        Printer::ALL.into_iter().filter(|p| p.preset_volume().is_some_and(|v| fits(v, dia, 0.0).0)).collect()
    }
    /// Equal apart from the printer and how the shade is split into parts
    /// (which change neither the preview nor the simulation).
    pub fn same_except_print_setup(&self, o: &Project) -> bool {
        let mut a = self.clone();
        a.printer = o.printer;
        a.custom_bed = o.custom_bed;
        a.design.split_parts = o.design.split_parts;
        a.design.split_joint = o.design.split_joint;
        a.design.split_overlap = o.design.split_overlap;
        a.design.split_gap = o.design.split_gap;
        a == *o
    }

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
