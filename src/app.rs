use crate::bulb::{self, Bulb};
use crate::checks::{self, Level};
use crate::design::{self, MountKind, ProfileKind, RibStyle, TextureMode};
use crate::export;
use crate::geometry::preview_res;
use crate::material::{MaterialId, Optics};
use crate::project::{Printer, Project};
use crate::quality::Quality;
use crate::random::{self, Theme};
use crate::design::Design;
use crate::render::{self, Camera, RenderSettings, ViewMode};
use crate::scene::{self, Scene};
use crate::section;
use crate::sim::{self, SimResult, Stats};
use eframe::egui::{self, Color32, RichText};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const COMPARE_PHOTONS: usize = 300_000;

pub fn build_scene(p: &Project, q: Quality) -> Scene {
    let (nt, nz) = preview_res(&p.design, q.mesh_scale());
    scene::build(&p.design, &p.bulb, &p.lamp, nt, nz)
}

struct CompareItem {
    id: MaterialId,
    stats: Stats,
    image: egui::ColorImage,
}

#[derive(Default)]
struct Compare {
    open: bool,
    rx: Option<mpsc::Receiver<Vec<CompareItem>>>,
    items: Vec<(MaterialId, Stats, egui::TextureHandle)>,
}

pub struct App {
    ctx: egui::Context,
    p: Project,
    prev: Project,
    scene: Arc<Scene>,
    optics: Optics,
    sim: Arc<SimResult>,
    sim_full: bool,
    gen: u64,
    cancel: Arc<AtomicBool>,
    sim_rx: Option<mpsc::Receiver<(u64, SimResult)>>,
    bulbs: Vec<Bulb>,
    cam: Camera,
    rs: RenderSettings,
    lock_exposure: bool,
    exposure: f32,
    tex: Option<egui::TextureHandle>,
    dirty: bool,
    rendered_full: bool,
    last_size: [usize; 2],
    last_interact: Instant,
    render_ms: f32,
    status: String,
    export_fine: bool,
    export_rx: Option<mpsc::Receiver<String>>,
    image_rx: Option<mpsc::Receiver<String>>,
    image_width: usize,
    rand_theme: Theme,
    rand_seed: u64,
    rand_color: bool,
    rand_msg: String,
    history: Vec<(Design, [u8; 3])>,
    sim_enabled: bool,
    section_on: bool,
    section_win: bool,
    clip: render::Clip,
    section_cache: Option<((usize, usize, u32), Arc<section::Section>)>,
    view_aspect: f32,
    rendered_aa: bool,
    view_gen: u64,
    aa_rx: Option<mpsc::Receiver<(u64, render::Frame)>>,
    last_render: Instant,
    quality: Quality,
    scene_q: Quality,
    project_path: Option<PathBuf>,
    compare: Compare,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let p = Project::default();
        let scene = Arc::new(build_scene(&p, Quality::Normal));
        let optics = Optics::new(p.material, p.color);
        let sim = Arc::new(sim::run(&scene, &optics, &p.bulb, 0, Quality::Normal.map_res(), &AtomicBool::new(false)).expect("direct sim"));
        let cam = Camera::for_scene(&scene);
        let p_clay = p.clay();
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            prev: p.clone(),
            p,
            scene,
            optics,
            sim,
            sim_full: false,
            gen: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            sim_rx: None,
            bulbs: bulb::presets(),
            cam,
            rs: RenderSettings {
                mode: ViewMode::Night,
                room_lux: 3.0,
                ev: 0.0,
                show_room: true,
                samples: 1,
                clay: p_clay,
                clip: None,
            },
            section_on: false,
            section_win: false,
            clip: render::Clip { axis: 1, pos: 0.0, keep_above: true },
            section_cache: None,
            sim_enabled: true,
            lock_exposure: false,
            exposure: 1.0,
            tex: None,
            dirty: true,
            rendered_full: false,
            last_size: [0, 0],
            last_interact: Instant::now(),
            render_ms: 0.0,
            status: "Ready".into(),
            export_fine: true,
            export_rx: None,
            image_rx: None,
            image_width: 2560,
            rand_theme: Theme::Surprise,
            rand_seed: 1,
            rand_color: false,
            rand_msg: String::new(),
            history: Vec::new(),
            view_aspect: 1.0,
            rendered_aa: false,
            view_gen: 0,
            aa_rx: None,
            last_render: Instant::now(),
            quality: Quality::Normal,
            scene_q: Quality::Normal,
            project_path: None,
            compare: Compare::default(),
        };
        app.start_sim();
        app
    }

    fn interacting(&self) -> bool {
        self.last_interact.elapsed() < Duration::from_millis(180)
    }

    /// Quality used for the scene right now: while the user drags sliders,
    /// heavy presets fall back to Normal and are restored on release.
    fn live_quality(&self) -> Quality {
        if self.interacting() && matches!(self.quality, Quality::High | Quality::Ultra) {
            Quality::Normal
        } else {
            self.quality
        }
    }

    fn rebuild(&mut self, geometry: bool) {
        self.p.design.sanitize();
        let q = self.live_quality();
        if geometry || q != self.scene_q {
            self.scene = Arc::new(build_scene(&self.p, q));
            self.scene_q = q;
        }
        self.optics = Optics::new(self.p.material, self.p.color);
        self.rs.clay = self.p.clay();
        self.dirty = true;
        if !self.sim_enabled {
            self.cancel.store(true, Ordering::Relaxed);
            self.sim_rx = None;
            self.sim = Arc::new(SimResult::empty(&self.scene));
            self.sim_full = true;
            return;
        }
        self.sim = Arc::new(
            sim::run(&self.scene, &self.optics, &self.p.bulb, 0, q.map_res(), &AtomicBool::new(false))
                .expect("direct sim"),
        );
        self.sim_full = false;
        self.start_sim();
    }

    fn start_sim(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.gen += 1;
        let (tx, rx) = mpsc::channel();
        let (scene, optics, bulb, cancel, gen, ctx) =
            (self.scene.clone(), self.optics, self.p.bulb.clone(), self.cancel.clone(), self.gen, self.ctx.clone());
        let q = self.scene_q;
        std::thread::spawn(move || {
            if let Some(r) = sim::run(&scene, &optics, &bulb, q.photons(), q.map_res(), &cancel) {
                let _ = tx.send((gen, r));
                ctx.request_repaint();
            }
        });
        self.sim_rx = Some(rx);
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.sim_rx {
            if let Ok((gen, r)) = rx.try_recv() {
                if gen == self.gen {
                    self.sim = Arc::new(r);
                    self.sim_full = true;
                    self.dirty = true;
                    self.sim_rx = None;
                }
            }
        }
        if let Some(rx) = &self.export_rx {
            if let Ok(msg) = rx.try_recv() {
                self.status = msg;
                self.export_rx = None;
            }
        }
        if let Some(rx) = &self.image_rx {
            if let Ok(msg) = rx.try_recv() {
                self.status = msg;
                self.image_rx = None;
            }
        }
        if let Some(rx) = &self.compare.rx {
            if let Ok(items) = rx.try_recv() {
                self.compare.items = items
                    .into_iter()
                    .map(|it| {
                        let tex = self.ctx.load_texture(
                            format!("cmp-{:?}", it.id),
                            it.image,
                            egui::TextureOptions::LINEAR,
                        );
                        (it.id, it.stats, tex)
                    })
                    .collect();
                self.compare.rx = None;
            }
        }
    }

    fn sync_changes(&mut self) {
        if self.p == self.prev {
            // drag finished: upgrade the reduced live scene to the chosen quality
            if self.scene_q != self.live_quality() {
                self.rebuild(true);
            }
            return;
        }
        let geo = self.p.design != self.prev.design || self.p.bulb != self.prev.bulb || self.p.lamp != self.prev.lamp;
        self.rebuild(geo);
        self.prev = self.p.clone();
    }

    // ---------------------------------------------------------------- top bar
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("💡 Lampdesigner");
            ui.separator();
            if ui.button("📂 Open…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Lampdesigner project", &["json"]).pick_file() {
                    match Project::load(&path) {
                        Ok(p) => {
                            self.p = p;
                            self.project_path = Some(path);
                            self.status = "Project loaded".into();
                        }
                        Err(e) => self.status = format!("Open failed: {e}"),
                    }
                }
            }
            if ui.button("💾 Save…").clicked() {
                let mut dlg = rfd::FileDialog::new().add_filter("Lampdesigner project", &["json"]);
                dlg = dlg.set_file_name("lampshade.json");
                if let Some(path) = dlg.save_file() {
                    self.status = match self.p.save(&path) {
                        Ok(()) => format!("Saved {}", path.display()),
                        Err(e) => format!("Save failed: {e}"),
                    };
                    self.project_path = Some(path);
                }
            }
            ui.separator();
            let exporting = self.export_rx.is_some();
            if ui.add_enabled(!exporting, egui::Button::new("📦 Export 3MF…")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("3MF", &["3mf"])
                    .set_file_name("lampshade.3mf")
                    .save_file()
                {
                    let (tx, rx) = mpsc::channel();
                    let p = self.p.clone();
                    let fine = self.export_fine;
                    let ctx = self.ctx.clone();
                    std::thread::spawn(move || {
                        let msg = match export::export_project(&p, &path, fine) {
                            Ok(info) => format!(
                                "Exported {} ({} triangles, {:.0} g {})",
                                path.display(),
                                info.triangles,
                                p.mass_g(info.volume_mm3),
                                p.material.props().name
                            ),
                            Err(e) => format!("Export failed: {e}"),
                        };
                        let _ = tx.send(msg);
                        ctx.request_repaint();
                    });
                    self.export_rx = Some(rx);
                    self.status = "Exporting…".into();
                }
            }
            ui.checkbox(&mut self.export_fine, "fine mesh");
            if exporting {
                ui.spinner();
            }
            ui.separator();
            if ui.button("⚖ Compare materials").clicked() {
                self.start_compare();
            }
            ui.separator();
            ui.label(RichText::new(&self.status).weak());
        });
    }

    fn start_compare(&mut self) {
        self.compare.open = true;
        self.compare.items.clear();
        let (tx, rx) = mpsc::channel();
        let p = self.p.clone();
        let scene = self.scene.clone();
        let cam = self.cam;
        let q = self.scene_q;
        let rs = RenderSettings { mode: ViewMode::Night, samples: q.samples().max(2), ..self.rs };
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let mut out = Vec::new();
            // one shared exposure so brightness is comparable between materials
            let mut exposure = None;
            let photons = COMPARE_PHOTONS.max(q.photons() / 2);
            for id in MaterialId::ALL {
                let color = if id == p.material { p.color } else { id.props().palette[0].1 };
                let op = Optics::new(id, color);
                let Some(r) = sim::run(&scene, &op, &p.bulb, photons, q.map_res(), &cancel) else { continue };
                // rendered at 2× the display size so thumbnails stay sharp on HiDPI screens
                let f = render::render(&scene, &r, &op, &p.bulb, id.props().hdt, &cam, &rs, 520, 640, exposure);
                exposure.get_or_insert(f.exposure);
                out.push(CompareItem {
                    id,
                    stats: r.stats.clone(),
                    image: egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba),
                });
            }
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        self.compare.rx = Some(rx);
    }

    // ----------------------------------------------------- random generator
    fn random_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("🎲 Random generator").default_open(true).show(ui, |ui| {
            egui::ComboBox::from_id_salt("theme")
                .width(200.0)
                .selected_text(self.rand_theme.name())
                .show_ui(ui, |ui| {
                    for t in Theme::ALL {
                        ui.selectable_value(&mut self.rand_theme, t, t.name()).on_hover_text(t.hint());
                    }
                });
            ui.label(RichText::new(self.rand_theme.hint()).weak());
            let mut go: Option<u64> = None;
            ui.horizontal(|ui| {
                if ui.button(RichText::new("🎲 Generate").strong()).clicked() {
                    let t = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(1);
                    go = Some(t % 1_000_000);
                }
                ui.add_enabled_ui(!self.history.is_empty(), |ui| {
                    if ui.button("◀ Back").on_hover_text("Restore the previous design").clicked() {
                        if let Some((d, c)) = self.history.pop() {
                            self.p.design = d;
                            self.p.color = c;
                            self.rand_msg = "Restored previous design".into();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("seed");
                ui.add(egui::DragValue::new(&mut self.rand_seed).range(0..=999_999));
                if ui.button("Use seed").on_hover_text("Same theme + seed = same design").clicked() {
                    go = Some(self.rand_seed);
                }
                if ui.small_button("▶").on_hover_text("Next seed").clicked() {
                    go = Some(self.rand_seed + 1);
                }
            });
            ui.checkbox(&mut self.rand_color, "also pick a filament colour");
            if let Some(seed) = go {
                self.generate(seed);
            }
            if !self.rand_msg.is_empty() {
                ui.label(RichText::new(&self.rand_msg).weak());
            }
        });
    }

    fn generate(&mut self, seed: u64) {
        self.rand_seed = seed;
        let g = random::generate(self.rand_theme, seed, &self.p);
        self.history.push((self.p.design.clone(), self.p.color));
        if self.history.len() > 50 {
            self.history.remove(0);
        }
        self.p.design = g.design;
        if self.rand_color {
            let pal = self.p.material.props().palette;
            // skip near-black colours: they block most of the light
            let usable: Vec<[u8; 3]> =
                pal.iter().map(|c| c.1).filter(|c| c.iter().map(|&v| v as u32).sum::<u32>() > 120).collect();
            if !usable.is_empty() {
                self.p.color = usable[(seed as usize * 7 + 3) % usable.len()];
            }
        }
        self.rand_msg = if g.problems.is_empty() {
            format!("#{seed}: printable design ({} {})", g.tries, if g.tries == 1 { "try" } else { "tries" })
        } else {
            format!("#{seed}: best of {} tries, still: {}", g.tries, g.problems.join(", "))
        };
    }

    // --------------------------------------------------------- left panel
    fn params(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.p;
        egui::CollapsingHeader::new("Presets").default_open(false).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (name, d) in design::presets() {
                    if ui.button(name).clicked() {
                        p.design = d;
                    }
                }
            });
        });

        egui::CollapsingHeader::new("Lamp & bulb").default_open(true).show(ui, |ui| {
            egui::ComboBox::from_id_salt("bulb")
                .width(280.0)
                .selected_text(p.bulb.name.clone())
                .show_ui(ui, |ui| {
                    for b in &self.bulbs {
                        if ui.selectable_label(p.bulb.name == b.name, &b.name).clicked() {
                            p.bulb = b.clone();
                        }
                    }
                });
            egui::CollapsingHeader::new("Bulb details").default_open(false).show(ui, |ui| {
                slider(ui, &mut p.bulb.lumens, 50.0..=2500.0, "lumens", " lm");
                slider(ui, &mut p.bulb.watts, 1.0..=100.0, "power", " W");
                ui.checkbox(&mut p.bulb.led, "LED");
                slider(ui, &mut p.bulb.cct, 1800.0..=6500.0, "colour temp.", " K");
                slider(ui, &mut p.bulb.rx, 15.0..=90.0, "glass radius", " mm");
                slider(ui, &mut p.bulb.rz, 15.0..=90.0, "glass half-height", " mm");
                slider(ui, &mut p.bulb.center_h, 20.0..=150.0, "centre above socket", " mm");
                slider(ui, &mut p.bulb.dark_cone_deg, 0.0..=90.0, "dark cone", "°");
            });
            egui::CollapsingHeader::new("Lamp base (GENNAKER)").default_open(false).show(ui, |ui| {
                ui.label(RichText::new("IKEA lists Ø120 mm foot, 100 mm height. Measure your socket.").weak());
                slider(ui, &mut p.lamp.height, 40.0..=300.0, "table → socket top", " mm");
                slider(ui, &mut p.lamp.socket_d, 25.0..=60.0, "socket Ø", " mm");
                slider(ui, &mut p.lamp.socket_h, 10.0..=80.0, "socket height", " mm");
                slider(ui, &mut p.lamp.base_d, 60.0..=250.0, "foot Ø", " mm");
            });
        });

        egui::CollapsingHeader::new("Shape").default_open(true).show(ui, |ui| {
            let d = &mut p.design;
            slider(ui, &mut d.height, 40.0..=300.0, "height", " mm");
            ui.horizontal(|ui| {
                ui.label("silhouette:");
                ui.radio_value(&mut d.profile_kind, ProfileKind::Bulge, "Bulge");
                ui.radio_value(&mut d.profile_kind, ProfileKind::Spline, "Free curve");
            });
            match d.profile_kind {
                ProfileKind::Bulge => {
                    slider(ui, &mut d.r_bottom, 20.0..=150.0, "bottom radius", " mm");
                    slider(ui, &mut d.r_top, 20.0..=150.0, "top radius", " mm");
                    slider(ui, &mut d.bulge, -40.0..=60.0, "bulge", " mm");
                    slider(ui, &mut d.bulge_pos, 0.05..=0.95, "bulge position", "");
                }
                ProfileKind::Spline => {
                    let n = d.spline.len();
                    for k in (0..n).rev() {
                        let label = format!("radius at {:.0} %", 100.0 * k as f32 / (n - 1) as f32);
                        slider(ui, &mut d.spline[k], 10.0..=150.0, &label, " mm");
                    }
                }
            }
            ui.add(egui::Slider::new(&mut d.sides, 0..=12).text("polygon sides (0 = round)"));
            if d.sides >= 3 {
                slider(ui, &mut d.poly_amount, 0.0..=1.0, "polygon amount", "");
                slider(ui, &mut d.poly_twist_deg, -360.0..=360.0, "polygon twist", "°");
            }
        });

        egui::CollapsingHeader::new("Folds & flow").default_open(true).show(ui, |ui| {
            let d = &mut p.design;
            ui.add(egui::Slider::new(&mut d.lobes, 0..=24).text("folds (0 = off, 2–3 = organic twist)"));
            if d.lobes > 0 {
                slider(ui, &mut d.lobe_amp, 0.0..=30.0, "fold depth", " mm");
                slider(ui, &mut d.lobe_growth, 0.0..=4.0, "depth at top ×", "");
                slider(ui, &mut d.lobe_sharpness, 0.3..=4.0, "fold sharpness", "");
                slider(ui, &mut d.lobe_twist_deg, -540.0..=540.0, "fold twist", "°");
                slider(ui, &mut d.meander_deg, 0.0..=60.0, "meander", "°");
                slider(ui, &mut d.meander_freq, 0.0..=4.0, "meander waves", "");
            }
            ui.add(egui::Slider::new(&mut d.rim_waves, 0..=24).text("wavy top rim"));
            if d.rim_waves > 0 {
                slider(ui, &mut d.rim_depth, 0.0..=40.0, "rim wave depth", " mm");
            }
        });

        egui::CollapsingHeader::new("Surface texture").default_open(true).show(ui, |ui| {
            let d = &mut p.design;
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.texture_mode, TextureMode::Relief, "Relief")
                    .on_hover_text("Inside smooth, texture outside: ribs glow as light/dark bands");
                ui.radio_value(&mut d.texture_mode, TextureMode::Corrugated, "Corrugated")
                    .on_hover_text("Whole wall follows the texture, constant thickness");
            });
            ui.add(egui::Slider::new(&mut d.ribs, 0..=160).text("ribs / grooves"));
            if d.ribs > 0 {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.rib_style, RibStyle::Ridges, "Ridges");
                    ui.radio_value(&mut d.rib_style, RibStyle::Grooves, "Grooves");
                });
                slider(ui, &mut d.rib_depth, 0.0..=8.0, "depth", " mm");
                slider(ui, &mut d.rib_sharpness, 0.2..=8.0, "sharpness", "");
                ui.checkbox(&mut d.ribs_follow_folds, "follow fold flow");
                if !d.ribs_follow_folds {
                    slider(ui, &mut d.twist_deg, -360.0..=360.0, "twist", "°");
                }
                ui.checkbox(&mut d.cross_ribs, "cross ribs (lattice)");
            }
            ui.add(egui::Slider::new(&mut d.waves, 0..=40).text("horizontal waves"));
            if d.waves > 0 {
                slider(ui, &mut d.wave_depth, 0.0..=8.0, "wave depth", " mm");
                ui.add(egui::Slider::new(&mut d.wave_spiral, -12..=12).text("wave spiral"));
            }
            slider(ui, &mut d.edge_band, 0.0..=30.0, "smooth rim band", " mm");
        });

        egui::CollapsingHeader::new("Wall & mount").default_open(true).show(ui, |ui| {
            let d = &mut p.design;
            slider(ui, &mut d.wall, 0.4..=4.0, "wall thickness", " mm");
            ui.horizontal(|ui| {
                let was = d.mount;
                ui.radio_value(&mut d.mount, MountKind::FlatRing, "Flat ring");
                ui.radio_value(&mut d.mount, MountKind::Sleeve, "Socket sleeve");
                if d.mount != was {
                    match d.mount {
                        MountKind::Sleeve => {
                            d.hole_d = p.lamp.socket_d + 0.35;
                            d.ring_width = 4.0;
                        }
                        MountKind::FlatRing => {
                            d.hole_d = 28.5;
                            d.ring_width = 9.0;
                        }
                    }
                }
            });
            match d.mount {
                MountKind::FlatRing => {
                    ui.horizontal(|ui| {
                        if ui.small_button("Bulb washer Ø28.5").clicked() {
                            d.hole_d = 28.5;
                        }
                        if ui.small_button("E27 shade ring Ø40.5").clicked() {
                            d.hole_d = 40.5;
                        }
                    });
                }
                MountKind::Sleeve => {
                    slider(ui, &mut d.sleeve_height, 3.0..=40.0, "sleeve height", " mm")
                        .on_hover_text("How far the sleeve reaches down around the socket");
                    ui.checkbox(&mut d.ledge, "ledge on socket top")
                        .on_hover_text("Inward ring at the top of the sleeve that rests on the socket rim");
                    if d.ledge {
                        let max_d = (d.hole_d - 1.0).max(21.0);
                        slider(ui, &mut d.ledge_d, 20.0..=max_d, "ledge hole Ø", " mm")
                            .on_hover_text("Must clear the bulb's E27 cap and neck; smaller than the socket Ø");
                        slider(ui, &mut d.ledge_h, 0.8..=6.0, "ledge height", " mm");
                        ui.checkbox(&mut d.ledge_chamfer, "45° chamfer underneath (no supports)");
                    }
                }
            }
            slider(ui, &mut d.hole_d, 20.0..=70.0, "hole Ø", " mm");
            slider(ui, &mut d.ring_width, 2.0..=25.0, "ring width", " mm");
            slider(ui, &mut d.mount_thickness, 0.8..=5.0, "mount thickness", " mm");
            ui.add(egui::Slider::new(&mut d.spokes, 2..=8).text("spokes"));
            slider(ui, &mut d.spoke_width, 2.0..=20.0, "spoke width", " mm");
        });

        egui::CollapsingHeader::new("Material").default_open(true).show(ui, |ui| {
            let before = p.material;
            egui::ComboBox::from_id_salt("material")
                .width(220.0)
                .selected_text(p.material.props().name)
                .show_ui(ui, |ui| {
                    for id in MaterialId::ALL {
                        ui.selectable_value(&mut p.material, id, id.props().name);
                    }
                });
            if p.material != before {
                p.color = p.material.props().palette[0].1;
            }
            let props = p.material.props();
            ui.label(RichText::new(props.note).weak());
            ui.horizontal_wrapped(|ui| {
                for (name, c) in props.palette {
                    let col = Color32::from_rgb(c[0], c[1], c[2]);
                    let selected = p.color == *c;
                    let btn = egui::Button::new(if selected { "✔" } else { "" })
                        .fill(col)
                        .min_size(egui::vec2(22.0, 22.0));
                    if ui.add(btn).on_hover_text(*name).clicked() {
                        p.color = *c;
                    }
                }
                ui.color_edit_button_srgb(&mut p.color).on_hover_text("Custom colour");
            });
            egui::ComboBox::from_id_salt("printer")
                .width(220.0)
                .selected_text(p.printer.name())
                .show_ui(ui, |ui| {
                    for pr in Printer::ALL {
                        ui.selectable_value(&mut p.printer, pr, pr.name());
                    }
                });
        });
    }

    // -------------------------------------------------------- right panel
    fn set_sim_enabled(&mut self, on: bool) {
        self.sim_enabled = on;
        if !on && matches!(self.rs.mode, ViewMode::Night | ViewMode::Temperature) {
            self.rs.mode = ViewMode::Shape;
        }
        self.rebuild(false);
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let mut toggled = false;
        ui.horizontal(|ui| {
            ui.heading("Simulation");
            toggled = ui
                .checkbox(&mut self.sim_enabled, "light")
                .on_hover_text("Off: no photon tracing, fast shape-only preview")
                .changed();
            if self.sim_enabled && !self.sim_full {
                ui.spinner();
                ui.label(RichText::new("tracing photons…").weak());
            }
        });
        if toggled {
            self.set_sim_enabled(self.sim_enabled);
        }
        let sim_on = self.sim_enabled;
        let st = &self.sim.stats;
        let sc = &self.scene;
        let mass = self.p.mass_g(sc.volume_mm3);
        let price = mass / 1000.0 * self.p.material.props().price_eur_kg;
        egui::Grid::new("stats").num_columns(2).striped(true).show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                ui.label(k);
                ui.label(RichText::new(v).strong());
                ui.end_row();
            };
            if !sim_on {
                row("Light simulation", "off".into());
            }
            if sim_on {
            row("Light output", format!("{:.0} % of {:.0} lm", st.efficiency * 100.0, self.p.bulb.lumens));
            if self.sim_full {
                row("  up / down / through wall", format!("{:.0} / {:.0} / {:.0} %", st.via_top * 100.0, st.via_bottom * 100.0, st.via_wall * 100.0));
                row("  absorbed", format!("{:.0} %", st.absorbed * 100.0));
                row("Table at 25 cm", format!("{:.0} lx", st.table_lux_25));
            }
            row("Shade glow (mean)", format!("{:.0} cd/m²", st.glow_mean));
            row("Glow min / max", format!("{:.0} / {:.0} cd/m²", st.glow_min, st.glow_max));
            let ratio = st.bulb_through / st.glow_mean.max(1.0);
            let vis = if ratio > 2.0 {
                "strong hotspot"
            } else if ratio > 0.3 {
                "visible"
            } else {
                "hidden"
            };
            row("Bulb through wall", format!("{:.0} cd/m² ({vis})", st.bulb_through));
            row("Max wall temp (est.)", format!("{:.0} °C", st.max_temp));
            row("Mount temp (est.)", format!("{:.0} °C", st.mount_temp));
            }
            row("Filament", format!("{:.0} g ≈ {:.2} €", mass, price));
            row("Size", format!("Ø{:.0} × {:.0} mm", 2.0 * sc.max_r, self.p.design.height));
            row("Mesh", format!("{} triangles", sc.mesh.tris.len()));
        });

        if sim_on {
            ui.add_space(6.0);
            ui.label(RichText::new("Light distribution (cd)").strong());
            polar_plot(ui, &self.sim.polar);
        }

        ui.add_space(6.0);
        ui.label(RichText::new("Checks").strong());
        for (lvl, msg) in checks::run(&self.p, &self.scene, &self.sim) {
            let (icon, col) = match lvl {
                Level::Ok => ("✔", Color32::from_rgb(110, 200, 120)),
                Level::Info => ("ℹ", Color32::from_rgb(140, 180, 230)),
                Level::Warn => ("⚠", Color32::from_rgb(240, 190, 80)),
                Level::Error => ("✖", Color32::from_rgb(240, 100, 90)),
            };
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(col, icon);
                ui.label(msg);
            });
        }

        ui.add_space(6.0);
        egui::CollapsingHeader::new("Print tips (Bambu Studio)").default_open(false).show(ui, |ui| {
            ui.label("• Print upright; the mount spider lies on the plate. No supports.");
            ui.label("• Walls: set 'Wall loops' so they fill the wall (1.2 mm ≈ 3 loops at 0.42 mm). Infill isn't needed.");
            ui.label("• PETG Translucent: 2 loops, slow outer wall, glue stick on textured PEI.");
            ui.label("• LED bulbs only.");
        });
    }

    // ------------------------------------------------------------ viewport
    fn viewport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let before = self.rs.mode;
            let sim_on = self.sim_enabled;
            for (m, label, needs_sim) in [
                (ViewMode::Shape, "◆ Shape", false),
                (ViewMode::Night, "🌙 Lamp on", true),
                (ViewMode::Day, "☀ Daylight", false),
                (ViewMode::Thickness, "Thickness", false),
                (ViewMode::Overhang, "Overhang", false),
                (ViewMode::Temperature, "Temperature", true),
            ] {
                let r = ui.add_enabled(
                    sim_on || !needs_sim,
                    egui::SelectableLabel::new(self.rs.mode == m, label),
                );
                if r.clicked() {
                    self.rs.mode = m;
                }
                if needs_sim && !sim_on {
                    r.on_disabled_hover_text("Turn on light simulation in the right panel");
                }
            }
            if self.rs.mode != before {
                if self.rs.mode == ViewMode::Day {
                    self.rs.room_lux = 200.0;
                } else if self.rs.mode == ViewMode::Night {
                    self.rs.room_lux = 3.0;
                }
                self.dirty = true;
            }
            ui.separator();
            let mut changed = false;
            changed |= ui
                .add(egui::Slider::new(&mut self.rs.room_lux, 0.0..=500.0).logarithmic(true).text("room lx"))
                .changed();
            changed |= ui.add(egui::Slider::new(&mut self.rs.ev, -4.0..=4.0).text("EV")).changed();
            changed |= ui
                .checkbox(&mut self.lock_exposure, "lock exposure")
                .on_hover_text("Keep brightness fixed to compare materials/bulbs fairly")
                .changed();
            changed |= ui.checkbox(&mut self.rs.show_room, "room").changed();
            if ui.button("⟲ view").clicked() {
                self.cam = Camera::for_scene(&self.scene);
                changed = true;
            }
            if changed {
                self.dirty = true;
                self.last_interact = Instant::now();
            }
        });
        ui.horizontal(|ui| {
            let before = self.quality;
            egui::ComboBox::from_id_salt("quality")
                .selected_text(format!("Quality: {}", self.quality.name()))
                .show_ui(ui, |ui| {
                    for q in Quality::ALL {
                        let txt = format!("{}  ({}× mesh, {}× AA)", q.name(), q.mesh_scale(), q.samples() * q.samples());
                        ui.selectable_value(&mut self.quality, q, txt);
                    }
                })
                .response
                .on_hover_text("Higher = finer mesh (smoother silhouette and ribs) and anti-aliasing. Photon count stays the same.");
            if self.quality != before {
                self.rebuild(true);
            }
            ui.separator();
            egui::ComboBox::from_id_salt("imgw")
                .selected_text(format!("{} px wide", self.image_width))
                .show_ui(ui, |ui| {
                    for w in [1920usize, 2560, 3840, 5120] {
                        ui.selectable_value(&mut self.image_width, w, format!("{w} px"));
                    }
                });
            let busy = self.image_rx.is_some();
            if ui.add_enabled(!busy, egui::Button::new("📷 Save image…")).clicked() {
                self.save_image(ui.available_size_before_wrap().x.max(1.0), ui.ctx().available_rect().height());
            }
            if busy {
                ui.spinner();
            }
            if self.aa_rx.is_some() {
                ui.label(RichText::new("refining…").weak());
            }
        });
        self.section_toolbar(ui);
        match self.rs.mode {
            ViewMode::Thickness => legend(ui, "0 mm", &format!("{:.1} mm", self.scene.max_thickness)),
            ViewMode::Overhang => legend(ui, "0°", "90° (> 45° needs care)"),
            ViewMode::Temperature => legend(
                ui,
                "25 °C",
                &format!("{:.0} °C (softening {:.0} °C)", render::temp_scale_max(self.p.material.props().hdt), self.p.material.props().hdt),
            ),
            _ => {}
        }

        let avail = ui.available_size();
        self.view_aspect = avail.x.max(1.0) / avail.y.max(1.0);
        let ppp = self.ctx.pixels_per_point();
        let interacting = self.interacting();
        let scale = if interacting { 0.5 } else { 1.0 };
        let w = ((avail.x * ppp * scale) as usize).clamp(64, 3840);
        let h = ((avail.y * ppp * scale) as usize).clamp(64, 2400);
        let full = !interacting;

        // background anti-aliasing pass finished?
        if let Some(rx) = &self.aa_rx {
            if let Ok((gen, f)) = rx.try_recv() {
                if gen == self.view_gen && !self.dirty {
                    let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
                    if let Some(t) = &mut self.tex {
                        t.set(img, egui::TextureOptions::LINEAR);
                    }
                    self.rendered_aa = true;
                }
                self.aa_rx = None;
            }
        }

        let need = self.dirty || self.tex.is_none() || (full && !self.rendered_full) || (full && self.last_size != [w, h]);
        if need {
            let t0 = Instant::now();
            let fixed = if self.lock_exposure { Some(self.exposure) } else { None };
            let mut rs = self.rs;
            rs.samples = 1;
            let f = render::render(
                &self.scene,
                &self.sim,
                &self.optics,
                &self.p.bulb,
                self.p.material.props().hdt,
                &self.cam,
                &rs,
                w,
                h,
                fixed,
            );
            if !self.lock_exposure {
                self.exposure = f.exposure;
            }
            let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
            match &mut self.tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => self.tex = Some(self.ctx.load_texture("render", img, egui::TextureOptions::LINEAR)),
            }
            self.render_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.dirty = false;
            self.rendered_full = full;
            self.rendered_aa = self.scene_q.samples() <= 1;
            self.last_size = [w, h];
            self.view_gen += 1;
            self.aa_rx = None;
            self.last_render = Instant::now();
        } else if full && !self.rendered_aa && self.aa_rx.is_none() {
            if self.last_render.elapsed() > Duration::from_millis(350) {
                self.start_aa(w, h);
            } else {
                self.ctx.request_repaint_after(Duration::from_millis(360));
            }
        }
        if interacting {
            self.ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(tex) = &self.tex {
            let resp = ui.add(
                egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), avail))
                    .sense(egui::Sense::click_and_drag()),
            );
            let mut moved = false;
            if resp.dragged_by(egui::PointerButton::Primary) {
                let dlt = resp.drag_delta();
                self.cam.yaw -= dlt.x * 0.008;
                self.cam.pitch = (self.cam.pitch + dlt.y * 0.008).clamp(-1.4, 1.4);
                moved = true;
            }
            if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                let dlt = resp.drag_delta();
                self.cam.target.z += dlt.y * self.cam.dist * 0.002;
                moved = true;
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    self.cam.dist = (self.cam.dist * (1.0 - scroll * 0.0015)).clamp(100.0, 3000.0);
                    moved = true;
                }
            }
            if moved {
                self.dirty = true;
                self.last_interact = Instant::now();
            }
            ui.painter().text(
                resp.rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format!("drag: orbit · right-drag: up/down · wheel: zoom   ({:.0} ms)", self.render_ms),
                egui::FontId::proportional(11.0),
                Color32::from_white_alpha(120),
            );
        }
    }

    // ------------------------------------------------------------- section
    fn section_range(&self, axis: usize) -> (f32, f32) {
        let sc = &self.scene;
        if axis == 2 {
            (sc.z_table, sc.shade_z1 + 10.0)
        } else {
            let r = sc.max_r.max(self.p.lamp.base_d * 0.5) + 5.0;
            (-r, r)
        }
    }

    fn section_toolbar(&mut self, ui: &mut egui::Ui) {
        let before = (self.section_on, self.clip);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.section_on, "✂ Section").on_hover_text("Cut the 3D view with a plane");
            ui.add_enabled_ui(self.section_on, |ui| {
                let old_axis = self.clip.axis;
                for (k, name) in section::AXIS_NAMES.iter().enumerate() {
                    ui.radio_value(&mut self.clip.axis, k, *name);
                }
                if self.clip.axis != old_axis {
                    // sensible defaults: keep the back half / the lower part
                    self.clip.keep_above = self.clip.axis != 2;
                    let (a, b) = self.section_range(self.clip.axis);
                    self.clip.pos = if self.clip.axis == 2 {
                        (self.scene.shade_z0 + self.scene.shade_z1) * 0.5
                    } else {
                        0.0f32.clamp(a, b)
                    };
                }
                let (a, b) = self.section_range(self.clip.axis);
                ui.add(
                    egui::Slider::new(&mut self.clip.pos, a..=b)
                        .text(format!("{} mm", section::AXIS_NAMES[self.clip.axis]))
                        .step_by(0.1),
                );
                ui.checkbox(&mut self.clip.keep_above, "keep +side")
                    .on_hover_text("Which side of the plane stays visible");
                if ui.selectable_label(self.section_win, "📐 Analysis").clicked() {
                    self.section_win = !self.section_win;
                }
            });
        });
        if (self.section_on, self.clip) != before {
            self.dirty = true;
            self.last_interact = Instant::now();
            if self.section_on && !before.0 {
                self.section_win = true;
            }
        }
        self.rs.clip = if self.section_on { Some(self.clip) } else { None };
    }

    fn section_data(&mut self) -> Arc<section::Section> {
        let key = (Arc::as_ptr(&self.scene) as usize, self.clip.axis, self.clip.pos.to_bits());
        if let Some((k, s)) = &self.section_cache {
            if *k == key {
                return s.clone();
            }
        }
        let s = Arc::new(section::compute(&self.scene, self.clip.axis, self.clip.pos));
        self.section_cache = Some((key, s.clone()));
        s
    }

    fn section_window(&mut self) {
        let mut open = self.section_win;
        let s = self.section_data();
        egui::Window::new(format!("📐 Section {} = {:.1} mm", section::AXIS_NAMES[s.axis], s.pos))
            .open(&mut open)
            .default_width(520.0)
            .default_height(640.0)
            .show(&self.ctx, |ui| {
                for line in s.summary() {
                    ui.label(line);
                }
                if s.t_min > 0.0 && s.t_min < 0.8 {
                    ui.colored_label(Color32::from_rgb(240, 100, 90), "⚠ thinner than 2 lines (0.8 mm) in this section");
                }
                ui.label(
                    RichText::new("Thickness is measured inside the cut plane: an oblique cut reads thicker than the true wall.")
                        .weak()
                        .small(),
                );
                section_plot(ui, &s);
                ui.add_space(4.0);
                ui.label(RichText::new(if s.axis == 2 { "Wall thickness around the circumference" } else { "Wall thickness over height" }).strong());
                thickness_graph(ui, &s);
            });
        self.section_win = open;
    }

    /// Anti-aliased re-render of the current view on a worker thread.
    fn start_aa(&mut self, w: usize, h: usize) {
        let (tx, rx) = mpsc::channel();
        let (scene, sim, optics, bulb, cam, ctx) =
            (self.scene.clone(), self.sim.clone(), self.optics, self.p.bulb.clone(), self.cam, self.ctx.clone());
        let hdt = self.p.material.props().hdt;
        let rs = RenderSettings { samples: self.scene_q.samples(), ..self.rs };
        let exposure = self.exposure;
        let gen = self.view_gen;
        std::thread::spawn(move || {
            let f = render::render(&scene, &sim, &optics, &bulb, hdt, &cam, &rs, w, h, Some(exposure));
            let _ = tx.send((gen, f));
            ctx.request_repaint();
        });
        self.aa_rx = Some(rx);
    }

    /// Renders the current view at high resolution and saves it as PNG.
    fn save_image(&mut self, _ui_w: f32, _ui_h: f32) {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("lampshade.png").save_file()
        else {
            return;
        };
        let w = self.image_width;
        let h = ((w as f32 / self.view_aspect.max(0.2)) as usize).clamp(64, 8192);
        let (tx, rx) = mpsc::channel();
        let (scene, sim, optics, bulb, cam, ctx) =
            (self.scene.clone(), self.sim.clone(), self.optics, self.p.bulb.clone(), self.cam, self.ctx.clone());
        let hdt = self.p.material.props().hdt;
        let rs = RenderSettings { samples: self.scene_q.samples().max(3), ..self.rs };
        let exposure = self.exposure;
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let f = render::render(&scene, &sim, &optics, &bulb, hdt, &cam, &rs, w, h, Some(exposure));
            let msg = match std::fs::write(&path, crate::image_out::png(f.w, f.h, &f.rgba)) {
                Ok(()) => format!("Saved {}×{} image to {} ({:.1} s)", w, h, path.display(), t0.elapsed().as_secs_f32()),
                Err(e) => format!("Image save failed: {e}"),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.image_rx = Some(rx);
        self.status = format!("Rendering {w}×{h} image…");
    }

    fn compare_window(&mut self) {
        let mut open = self.compare.open;
        egui::Window::new("Material comparison (same design, bulb and exposure)")
            .open(&mut open)
            .default_width(1120.0)
            .show(&self.ctx, |ui| {
                if self.compare.rx.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Simulating all materials…");
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    for (id, st, tex) in &self.compare.items {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(id.props().name).strong());
                            ui.image((tex.id(), egui::vec2(260.0, 320.0)));
                            ui.label(format!("Output: {:.0} %", st.efficiency * 100.0));
                            ui.label(format!("Glow: {:.0} cd/m²", st.glow_mean));
                            ui.label(format!("Bulb through wall: {:.0} cd/m²", st.bulb_through));
                            ui.label(format!("Table @25 cm: {:.0} lx", st.table_lux_25));
                            ui.label(format!("Max wall: {:.0} °C (HDT {:.0})", st.max_temp, id.props().hdt));
                            if ui.button("Use this material").clicked() {
                                self.p.material = *id;
                                self.p.color = id.props().palette[0].1;
                            }
                        });
                    }
                });
            });
        self.compare.open = open;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        egui::TopBottomPanel::top("top").show(ctx, |ui| self.top_bar(ui));
        egui::SidePanel::left("params").resizable(true).default_width(340.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.random_ui(ui);
                self.params(ui);
            });
        });
        if ctx.input(|i| i.pointer.any_down()) && self.p != self.prev {
            self.last_interact = Instant::now();
        }
        self.sync_changes();
        egui::SidePanel::right("results").resizable(true).default_width(330.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.results(ui));
        });
        if self.compare.open {
            self.compare_window();
        }
        egui::CentralPanel::default().show(ctx, |ui| self.viewport(ui));
        if self.section_on && self.section_win {
            self.section_window();
        }
    }
}

fn slider(ui: &mut egui::Ui, v: &mut f32, r: RangeInclusive<f32>, label: &str, suffix: &str) -> egui::Response {
    ui.add(egui::Slider::new(v, r).text(label).suffix(suffix))
}

fn legend(ui: &mut egui::Ui, lo: &str, hi: &str) {
    ui.horizontal(|ui| {
        ui.label(lo);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 12.0), egui::Sense::hover());
        let n = 40;
        for k in 0..n {
            let c = render::turbo(k as f32 / (n - 1) as f32);
            let x0 = rect.left() + rect.width() * k as f32 / n as f32;
            let x1 = rect.left() + rect.width() * (k + 1) as f32 / n as f32;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
                0.0,
                Color32::from_rgb((c.x * 255.0) as u8, (c.y * 255.0) as u8, (c.z * 255.0) as u8),
            );
        }
        ui.label(hi);
    });
}

fn tag_color(tag: crate::mesh::Tag) -> Color32 {
    use crate::mesh::Tag;
    match tag {
        Tag::Outer => Color32::from_rgb(250, 150, 60),
        Tag::Inner => Color32::from_rgb(250, 200, 90),
        Tag::Rim => Color32::from_rgb(250, 170, 70),
        Tag::Mount => Color32::from_rgb(90, 150, 250),
        Tag::Proxy => Color32::from_gray(120),
    }
}

/// 2D drawing of the cut with grid, bulb, thinnest spot and hover readout.
fn section_plot(ui: &mut egui::Ui, s: &section::Section) {
    let w = ui.available_width().max(200.0);
    let (resp, painter) = ui.allocate_painter(egui::vec2(w, (w * 0.95).min(560.0)), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, Color32::from_gray(22));
    if s.segs.is_empty() {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "plane misses the model", egui::FontId::proportional(14.0), Color32::GRAY);
        return;
    }
    let mut lo = s.lo;
    let mut hi = s.hi;
    for p in &s.bulb {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let span = (hi - lo).max(glam::Vec2::splat(10.0));
    let margin = 0.08;
    let scale = ((rect.width() * (1.0 - 2.0 * margin)) / span.x).min((rect.height() * (1.0 - 2.0 * margin)) / span.y);
    let mid = (lo + hi) * 0.5;
    let c = rect.center();
    let to_screen = |p: glam::Vec2| egui::pos2(c.x + (p.x - mid.x) * scale, c.y - (p.y - mid.y) * scale);
    let to_mm = |q: egui::Pos2| glam::Vec2::new((q.x - c.x) / scale + mid.x, -(q.y - c.y) / scale + mid.y);

    // grid: 10 mm, labelled every 50 mm
    let a = to_mm(rect.left_bottom());
    let b = to_mm(rect.right_top());
    let small = egui::Stroke::new(1.0, Color32::from_white_alpha(14));
    let big = egui::Stroke::new(1.0, Color32::from_white_alpha(40));
    let f = egui::FontId::proportional(10.0);
    let mut x = (a.x / 10.0).floor() * 10.0;
    while x <= b.x {
        let major = (x.round() as i32) % 50 == 0;
        let sx = to_screen(glam::Vec2::new(x, 0.0)).x;
        painter.line_segment([egui::pos2(sx, rect.top()), egui::pos2(sx, rect.bottom())], if major { big } else { small });
        if major {
            painter.text(egui::pos2(sx + 2.0, rect.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, format!("{x:.0}"), f.clone(), Color32::from_white_alpha(110));
        }
        x += 10.0;
    }
    let mut y = (a.y / 10.0).floor() * 10.0;
    while y <= b.y {
        let major = (y.round() as i32) % 50 == 0;
        let sy = to_screen(glam::Vec2::new(0.0, y)).y;
        painter.line_segment([egui::pos2(rect.left(), sy), egui::pos2(rect.right(), sy)], if major { big } else { small });
        if major {
            painter.text(egui::pos2(rect.left() + 2.0, sy - 1.0), egui::Align2::LEFT_BOTTOM, format!("{y:.0}"), f.clone(), Color32::from_white_alpha(110));
        }
        y += 10.0;
    }
    let (hname, vname) = match s.axis {
        0 => ("Y", "Z"),
        1 => ("X", "Z"),
        _ => ("X", "Y"),
    };
    painter.text(rect.right_bottom() + egui::vec2(-4.0, -14.0), egui::Align2::RIGHT_BOTTOM, format!("{hname} →  {vname} ↑  (mm)"), f.clone(), Color32::from_white_alpha(120));

    // bulb
    if s.bulb.len() > 1 {
        let pts: Vec<egui::Pos2> = s.bulb.iter().map(|p| to_screen(*p)).collect();
        painter.add(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.0, Color32::from_rgb(255, 240, 160)), 4.0, 3.0));
    }
    // geometry
    for seg in &s.segs {
        painter.line_segment([to_screen(seg.a), to_screen(seg.b)], egui::Stroke::new(1.6, tag_color(seg.tag)));
    }
    // bulb clearance
    if let Some((g, bp, wp)) = s.clearance {
        let (p0, p1) = (to_screen(bp), to_screen(wp));
        painter.add(egui::Shape::dashed_line(&[p0, p1], egui::Stroke::new(1.2, Color32::from_rgb(110, 220, 230)), 3.0, 2.0));
        painter.text(p1 + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("gap {g:.1}"), f.clone(), Color32::from_rgb(110, 220, 230));
    }
    // thinnest spot
    if let Some(m) = s.min_sample {
        let (p0, p1) = (to_screen(m.outer), to_screen(m.inner));
        let red = Color32::from_rgb(255, 80, 80);
        painter.circle_stroke(p0, 7.0, egui::Stroke::new(1.5, red));
        painter.line_segment([p0, p1], egui::Stroke::new(2.0, red));
        painter.text(p0 + egui::vec2(9.0, -9.0), egui::Align2::LEFT_BOTTOM, format!("min {:.2} mm", m.t), egui::FontId::proportional(12.0), red);
    }
    // hover: coordinates + nearest thickness sample
    if let Some(hp) = resp.hover_pos() {
        let mm = to_mm(hp);
        let mut txt = format!("{hname} {:.1}  {vname} {:.1} mm", mm.x, mm.y);
        if let Some(n) = s
            .samples
            .iter()
            .min_by(|a, b| a.outer.distance(mm).partial_cmp(&b.outer.distance(mm)).unwrap_or(std::cmp::Ordering::Equal))
        {
            if n.outer.distance(mm) < 8.0 {
                let (p0, p1) = (to_screen(n.outer), to_screen(n.inner));
                painter.line_segment([p0, p1], egui::Stroke::new(2.0, Color32::WHITE));
                painter.circle_filled(p0, 3.0, Color32::WHITE);
                txt += &format!("   wall {:.2} mm", n.t);
            }
        }
        painter.text(rect.left_top() + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP, txt, egui::FontId::proportional(12.0), Color32::WHITE);
    }
}

/// Thickness along the cut (angle for Z cuts, height for X/Y cuts).
fn thickness_graph(ui: &mut egui::Ui, s: &section::Section) {
    let w = ui.available_width().max(200.0);
    let (resp, painter) = ui.allocate_painter(egui::vec2(w, 130.0), egui::Sense::hover());
    let rect = resp.rect.shrink2(egui::vec2(34.0, 14.0));
    painter.rect_filled(resp.rect, 4.0, Color32::from_gray(22));
    if s.samples.len() < 2 {
        return;
    }
    let (p0, p1) = if s.axis == 2 {
        (0.0, 360.0)
    } else {
        let a = s.samples.first().unwrap().param;
        let b = s.samples.last().unwrap().param;
        (a, b.max(a + 1.0))
    };
    let t_top = (s.t_max * 1.15).max(1.0);
    let map = |param: f32, t: f32| {
        egui::pos2(
            rect.left() + (param - p0) / (p1 - p0) * rect.width(),
            rect.bottom() - (t / t_top) * rect.height(),
        )
    };
    let f = egui::FontId::proportional(10.0);
    let weak = Color32::from_white_alpha(110);
    // 0.8 mm limit
    let y08 = map(p0, 0.8).y;
    painter.add(egui::Shape::dashed_line(
        &[egui::pos2(rect.left(), y08), egui::pos2(rect.right(), y08)],
        egui::Stroke::new(1.0, Color32::from_rgb(240, 90, 80)),
        4.0,
        3.0,
    ));
    painter.text(egui::pos2(rect.left() - 4.0, y08), egui::Align2::RIGHT_CENTER, "0.8", f.clone(), Color32::from_rgb(240, 90, 80));
    painter.text(egui::pos2(rect.left() - 4.0, rect.top()), egui::Align2::RIGHT_CENTER, format!("{t_top:.1}"), f.clone(), weak);
    painter.text(egui::pos2(rect.left(), rect.bottom() + 2.0), egui::Align2::LEFT_TOP, format!("{p0:.0}"), f.clone(), weak);
    painter.text(
        egui::pos2(rect.right(), rect.bottom() + 2.0),
        egui::Align2::RIGHT_TOP,
        if s.axis == 2 { "360°".to_string() } else { format!("{p1:.0} mm") },
        f.clone(),
        weak,
    );
    // vertical cuts hit the wall twice (both sides): plot both as points
    let col = Color32::from_rgb(250, 170, 80);
    if s.axis == 2 {
        let pts: Vec<egui::Pos2> = s.samples.iter().map(|x| map(x.param, x.t)).collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, col)));
    } else {
        for x in &s.samples {
            painter.circle_filled(map(x.param, x.t), 1.4, col);
        }
    }
    if let Some(hp) = resp.hover_pos() {
        if rect.contains(hp) {
            let param = p0 + (hp.x - rect.left()) / rect.width() * (p1 - p0);
            if let Some(n) = s
                .samples
                .iter()
                .min_by(|a, b| (a.param - param).abs().partial_cmp(&(b.param - param).abs()).unwrap_or(std::cmp::Ordering::Equal))
            {
                let q = map(n.param, n.t);
                painter.circle_stroke(q, 4.0, egui::Stroke::new(1.5, Color32::WHITE));
                let label = if s.axis == 2 { format!("{:.0}°: {:.2} mm", n.param, n.t) } else { format!("{:.1} mm: {:.2} mm", n.param, n.t) };
                painter.text(q + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, label, f, Color32::WHITE);
            }
        }
    }
}

fn polar_plot(ui: &mut egui::Ui, polar: &[f32]) {
    let size = egui::vec2(ui.available_width().min(300.0), 220.0);
    let (resp, painter) = ui.allocate_painter(size, egui::Sense::hover());
    let rect = resp.rect;
    let c = rect.center();
    let r = rect.height().min(rect.width()) * 0.45;
    let grid = egui::Stroke::new(1.0, Color32::from_white_alpha(40));
    let max = polar.iter().cloned().fold(0.0f32, f32::max).max(1e-3);
    for k in 1..=4 {
        painter.circle_stroke(c, r * k as f32 / 4.0, grid);
    }
    for a in (0..360).step_by(30) {
        let t = (a as f32).to_radians();
        painter.line_segment([c, c + egui::vec2(t.sin(), -t.cos()) * r], grid);
    }
    let n = polar.len();
    let mut pts = Vec::new();
    for side in [1.0f32, -1.0] {
        let range: Vec<usize> = if side > 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
        for b in range {
            let th = (b as f32 + 0.5) / n as f32 * std::f32::consts::PI;
            let rr = polar[b] / max * r;
            pts.push(c + egui::vec2(side * th.sin() * rr, -th.cos() * rr));
        }
    }
    if let Some(first) = pts.first().copied() {
        pts.push(first);
    }
    painter.add(egui::Shape::line(pts, egui::Stroke::new(2.0, Color32::from_rgb(255, 200, 90))));
    let txt = Color32::from_white_alpha(140);
    let f = egui::FontId::proportional(10.0);
    painter.text(c + egui::vec2(0.0, -r - 2.0), egui::Align2::CENTER_BOTTOM, "up", f.clone(), txt);
    painter.text(c + egui::vec2(0.0, r + 2.0), egui::Align2::CENTER_TOP, "down", f.clone(), txt);
    painter.text(rect.right_top(), egui::Align2::RIGHT_TOP, format!("max {:.0} cd", max), f, txt);
}
