mod app;
mod bulb;
mod bvh;
mod checks;
mod cli;
mod design;
mod export;
mod geometry;
mod image_out;
mod material;
mod mesh;
mod project;
mod quality;
mod random;
mod render;
mod scene;
mod section;
mod sim;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1560.0, 940.0]).with_title("Lampdesigner"),
        ..Default::default()
    };
    eframe::run_native("Lampdesigner", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
