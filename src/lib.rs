//! Lampdesigner: parametric 3D-printable lampshades with light simulation
//! and 3MF export. Shared by the GUI (`lampdesigner`) and the console tool
//! (`lampdesigner-cli`).

pub mod bulb;
pub mod bvh;
pub mod checks;
pub mod cli;
pub mod design;
pub mod export;
pub mod geometry;
pub mod gpu;
pub mod image_out;
pub mod material;
pub mod mesh;
pub mod project;
pub mod quality;
pub mod random;
pub mod render;
pub mod scene;
pub mod section;
pub mod sim;
pub mod ui;

/// Starts the GUI.
pub fn run_gui() {
    ui::run();
}
