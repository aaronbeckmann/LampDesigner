//! Simulation / render quality presets.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Draft,
    Normal,
    High,
    Ultra,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Draft, Quality::Normal, Quality::High, Quality::Ultra];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Draft => "Draft",
            Quality::Normal => "Normal",
            Quality::High => "High",
            Quality::Ultra => "Ultra",
        }
    }

    pub fn parse(s: &str) -> Option<Quality> {
        Quality::ALL.into_iter().find(|q| q.name().eq_ignore_ascii_case(s))
    }

    /// Multiplier for the preview/simulation mesh resolution.
    pub fn mesh_scale(self) -> f32 {
        match self {
            Quality::Draft => 0.6,
            Quality::Normal => 1.0,
            Quality::High => 2.0,
            Quality::Ultra => 3.0,
        }
    }

    /// Photon count is the same for every level: quality only refines the mesh.
    pub fn photons(self) -> usize {
        400_000
    }

    /// Cells per side of the table/wall light maps (same for every level).
    pub fn map_res(self) -> usize {
        140
    }

    /// Anti-aliasing samples per pixel axis for still frames.
    pub fn samples(self) -> u32 {
        match self {
            Quality::Draft => 1,
            Quality::Normal => 2,
            Quality::High => 3,
            Quality::Ultra => 4,
        }
    }
}
