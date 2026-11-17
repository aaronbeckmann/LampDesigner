//! The GUI, built with Rune (a retained, GPU-rendered UI framework).
//!
//! Rune calls [`view`] to build the element tree from the state and runs
//! handlers (`&mut State`) between frames; [`poll`] runs at the start of every
//! frame. Everything derived from the project happens in `poll`: rebuilding
//! the scene after an edit, collecting background results and sending the 3D
//! view to the GPU renderer (`crate::gpu`), which publishes its picture
//! through `rune::Pixels` without re-running the view.
//!
//! The state is split by concern, so a panel borrows only what it shows:
//! [`viewport::ViewState`], [`sim_state::SimState`], [`random::RandomGen`],
//! [`section::SectionTool`], [`compare::Compare`] and [`Files`].

mod compare;
mod job;
mod params;
mod random;
mod results;
mod section;
mod sim_state;
mod viewport;
mod widgets;

use crate::bulb::{self, Bulb};
use crate::gpu::{self, FrameParams, Request};
use crate::material::Optics;
use crate::project::Project;
use crate::quality::Quality;
use crate::render::{self, RenderSettings};
use crate::scene::{self, Scene};
use job::{Job, Wake};
use rune::{col, row, text, AppSpec, Cx, El, FileDialogOptions, ScrollSpec, Tx};
use rune_kit::Kit;
use sim_state::SimState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use viewport::{FrameKey, ViewState};

/// Open / save / export state and their background jobs.
pub struct Files {
    pub status: String,
    pub project_path: Option<PathBuf>,
    pub export_fine: bool,
    pub image_width: usize,
    pub export: Option<Job<String>>,
    pub image: Option<Job<String>>,
}

/// UI-only state (open menus and sections).
pub struct UiFlags {
    pub open_select: Option<&'static str>,
    /// Open panel sections (`params::S_*` bits).
    pub sections: u32,
    /// The custom colour sliders, synced with `Project::color` in `poll`.
    pub custom_rgb: [u32; 3],
    custom_seen: [u32; 3],
}

pub struct State {
    /// The project being edited.
    pub p: Project,
    /// `p` as of the last rebuild, to detect edits.
    pub prev: Project,
    pub bulbs: Vec<Bulb>,
    /// Quality chosen by the user; `scene_q` is what the scene was built with
    /// (lower while dragging sliders at High/Ultra).
    pub quality: Quality,
    pub scene_q: Quality,
    pub scene: Arc<Scene>,
    pub optics: Optics,
    pub sim: SimState,
    pub view: ViewState,
    pub random: random::RandomGen,
    pub section: section::SectionTool,
    pub compare: compare::Compare,
    pub files: Files,
    pub ui: UiFlags,
    pub wake: Wake,
}

fn rgb_u32(c: [u8; 3]) -> [u32; 3] {
    c.map(u32::from)
}

fn init() -> State {
    let p = Project::default();
    let q = Quality::Normal;
    let scene = Arc::new(scene::build_for(&p, q));
    let optics = Optics::new(p.material, p.color);
    let wake = Wake::new(None);
    let sim = SimState::new(&wake, &scene, optics, &p.bulb, q);
    let view = ViewState::new(&scene, p.clay());
    let rgb = rgb_u32(p.color);
    State {
        prev: p.clone(),
        p,
        bulbs: bulb::presets(),
        quality: q,
        scene_q: q,
        scene,
        optics,
        sim,
        view,
        random: random::RandomGen::default(),
        section: section::SectionTool::default(),
        compare: compare::Compare::default(),
        files: Files {
            status: "Ready".into(),
            project_path: None,
            export_fine: true,
            image_width: 2560,
            export: None,
            image: None,
        },
        ui: UiFlags { open_select: None, sections: params::DEFAULT_OPEN, custom_rgb: rgb, custom_seen: rgb },
        wake,
    }
}

/// Named starting states (Rune's `--scene` for headless snapshots; the windowed
/// app also starts in one with `LAMPDESIGNER_SCENE=<name>`, for screenshots).
type NamedScene = (&'static str, fn(&mut State));
const SCENES: &[NamedScene] = &[
    ("section", |s| {
        s.section.on = true;
        s.section.window_open = true;
        s.view.rs.mode = crate::render::ViewMode::Shape;
    }),
    ("thickness", |s| s.view.rs.mode = crate::render::ViewMode::Thickness),
    ("windows", |s| {
        s.p.design.sleeve_height = 80.0;
        s.p.design.sleeve_windows = 6;
        s.p.design.height = 260.0;
        s.section.on = true;
        s.view.rs.mode = crate::render::ViewMode::Shape;
        s.ui.sections = params::S_MOUNT;
    }),    ("split", |s| {
        s.p.design.height = 330.0;
        s.p.design.wall = 2.0;
        s.p.printer = crate::project::Printer::PrusaMk4;
        s.p.design.split_parts = 2;
        s.ui.sections = params::S_PRINT;
    }),
    ("crown", |s| {
        s.p.design = crate::design::presets().into_iter().find(|(n, _)| *n == "Flowing crown").map(|(_, d)| d).unwrap_or_default();
    }),
];

fn init_scene() -> State {
    let mut s = init();
    if let Ok(name) = std::env::var("LAMPDESIGNER_SCENE") {
        if let Some((_, apply)) = SCENES.iter().find(|(n, _)| *n == name) {
            apply(&mut s);
        }
    }
    s
}

/// Rune hands over the waker once: from now on finished jobs wake `poll`.
fn setup(s: &mut State, waker: rune::Waker) {
    s.wake = Wake::new(Some(waker));
    // the first photon run started before there was a waker
    s.rebuild(false);
}

impl State {
    /// A user change: frames are coarse for a moment, then refined.
    pub fn touch(&mut self) {
        self.view.touched = Instant::now();
    }

    /// Quality for the scene right now: while the user drags, heavy presets
    /// fall back to Normal and are restored when the drag ends.
    fn live_quality(&self) -> Quality {
        if self.view.interacting() && matches!(self.quality, Quality::High | Quality::Ultra) {
            Quality::Normal
        } else {
            self.quality
        }
    }

    /// Recomputes what depends on the project; `geometry` = shape, bulb or lamp changed.
    pub fn rebuild(&mut self, geometry: bool) {
        self.p.design.sanitize();
        let q = self.live_quality();
        if geometry || q != self.scene_q {
            self.scene = Arc::new(scene::build_for(&self.p, q));
            self.scene_q = q;
        }
        self.optics = Optics::new(self.p.material, self.p.color);
        self.view.rs.clay = self.p.clay();
        self.sim.restart(&self.wake, &self.scene, self.optics, &self.p.bulb, q);
    }

    /// Applies edits made by handlers since the last frame. True when anything changed.
    fn sync_changes(&mut self) -> bool {
        // custom colour sliders ↔ project colour
        if self.ui.custom_rgb != self.ui.custom_seen {
            self.p.color = self.ui.custom_rgb.map(|v| v.min(255) as u8);
            self.ui.custom_seen = self.ui.custom_rgb;
        } else if rgb_u32(self.p.color) != self.ui.custom_rgb {
            self.ui.custom_rgb = rgb_u32(self.p.color);
            self.ui.custom_seen = self.ui.custom_rgb;
        }
        if self.p == self.prev {
            // drag finished: upgrade the reduced live scene to the chosen quality
            if self.scene_q != self.live_quality() {
                self.rebuild(true);
                return true;
            }
            return false;
        }
        if self.p.same_except_print_setup(&self.prev) {
            // printer / split only change the checks and the export, not the scene
            self.prev.clone_from(&self.p);
            return true;
        }
        let geo = self.p.design != self.prev.design || self.p.bulb != self.prev.bulb || self.p.lamp != self.prev.lamp;
        self.rebuild(geo);
        self.prev.clone_from(&self.p);
        true
    }

    pub fn toggle_simulation(&mut self) {
        self.sim.enabled = !self.sim.enabled;
        if !self.sim.enabled {
            self.view.leave_light_views();
        }
        self.rebuild(false);
    }

    /// Sends the 3D view to the GPU when anything it shows changed: coarse
    /// while the user interacts, supersampled once they stop.
    fn schedule_render(&mut self, tx: &mut Tx) {
        let [w, h] = self.view.size_px;
        if w < 16 || h < 16 {
            return;
        }
        let interacting = self.view.interacting();
        let ssaa = if interacting { 1 } else { viewport::ssaa_for(self.quality) };
        if !self.view.lock_exposure {
            self.view.exposure = gpu::auto_exposure(&self.scene, &self.sim.result, self.view.rs.mode, self.view.rs.room_lux, &self.optics);
        }
        let rs = RenderSettings { clip: self.section.clip(), ..self.view.rs };
        let key = FrameKey::new(
            Arc::as_ptr(&self.scene) as usize,
            Arc::as_ptr(&self.sim.result) as usize,
            self.view.cam,
            rs,
            [w, h],
            ssaa,
            self.view.exposure,
        );
        if self.view.last_key.as_ref() != Some(&key) {
            self.view.gpu.request(Request {
                scene: Arc::clone(&self.scene),
                sim: Arc::clone(&self.sim.result),
                params: FrameParams {
                    width: w,
                    height: h,
                    ssaa,
                    cam: self.view.cam,
                    rs,
                    exposure: self.view.exposure,
                    optics: self.optics,
                    bulb: self.p.bulb.clone(),
                    hdt: self.p.material.props().hdt,
                },
            });
            self.view.last_key = Some(key);
        }
        if interacting {
            // come back when the interaction has settled: refine, restore quality
            tx.wake_after(Duration::from_millis(220));
        }
    }

    fn open_project(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Open project".into(), ..Default::default() }.filter("Lampdesigner project", &["json"]);
        tx.open_file(options, |s: &mut State, result, _| match result {
            Ok(Some(path)) => match Project::load(&path) {
                Ok(p) => {
                    s.p = p;
                    s.files.status = format!("Opened {}", path.display());
                    s.files.project_path = Some(path);
                }
                Err(e) => s.files.status = format!("Open failed: {e}"),
            },
            Ok(None) => {}
            Err(e) => s.files.status = format!("Open failed: {e}"),
        });
    }

    fn save_project(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Save project".into(), file_name: Some("lampshade.json".into()), ..Default::default() }
            .filter("Lampdesigner project", &["json"]);
        tx.save_file(options, |s: &mut State, result, _| match result {
            Ok(Some(path)) => {
                s.files.status = match s.p.save(&path) {
                    Ok(()) => format!("Saved {}", path.display()),
                    Err(e) => format!("Save failed: {e}"),
                };
                s.files.project_path = Some(path);
            }
            Ok(None) => {}
            Err(e) => s.files.status = format!("Save failed: {e}"),
        });
    }

    fn export_3mf(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Export 3MF".into(), file_name: Some("lampshade.3mf".into()), ..Default::default() }
            .filter("3MF", &["3mf"]);
        tx.save_file(options, |s: &mut State, result, _| {
            let Ok(Some(path)) = result else { return };
            let (p, fine) = (s.p.clone(), s.files.export_fine);
            s.files.export = Some(Job::spawn(&s.wake, move |_| {
                Some(match crate::export::export_project(&p, &path, fine) {
                    Ok(info) => format!(
                        "Exported {}{}{} ({} triangles, {:.0} g {})",
                        path.display(),
                        if info.part_paths.is_empty() { String::new() } else { format!(" + {} more parts", info.part_paths.len()) },
                        info.insert_path.map(|i| format!(" + mount insert {}", i.display())).unwrap_or_default(),
                        info.triangles,
                        p.mass_g(info.volume_mm3),
                        p.material.props().name
                    ),
                    Err(e) => format!("Export failed: {e}"),
                })
            }));
            s.files.status = "Exporting…".into();
        });
    }

    /// Renders the current view at high resolution (CPU ray tracer) and saves a PNG.
    fn save_image(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Save image".into(), file_name: Some("lampshade.png".into()), ..Default::default() }
            .filter("PNG image", &["png"]);
        tx.save_file(options, |s: &mut State, result, _| {
            let Ok(Some(path)) = result else { return };
            let w = s.files.image_width;
            let h = ((w as f32 / s.view.aspect.max(0.2)) as usize).clamp(64, 8192);
            let (scene, sim, optics, bulb, cam) =
                (Arc::clone(&s.scene), Arc::clone(&s.sim.result), s.optics, s.p.bulb.clone(), s.view.cam);
            let hdt = s.p.material.props().hdt;
            let rs = RenderSettings { samples: s.scene_q.samples().max(3), clip: s.section.clip(), ..s.view.rs };
            let exposure = s.view.exposure;
            s.files.image = Some(Job::spawn(&s.wake, move |_| {
                let t0 = Instant::now();
                let f = render::render(&scene, &sim, &optics, &bulb, hdt, &cam, &rs, w, h, Some(exposure));
                Some(match std::fs::write(&path, crate::image_out::png(f.w, f.h, &f.rgba)) {
                    Ok(()) => format!("Saved {}×{} image to {} ({:.1} s)", w, h, path.display(), t0.elapsed().as_secs_f32()),
                    Err(e) => format!("Image save failed: {e}"),
                })
            }));
            s.files.status = format!("Rendering {w}×{h} image…");
        });
    }
}

/// Start of every frame: apply edits, collect background work, render the view.
fn poll(s: &mut State, _t: f64, tx: &mut Tx) -> bool {
    let mut changed = s.sync_changes();
    changed |= s.sim.poll();
    for done in [Job::poll(&mut s.files.export), Job::poll(&mut s.files.image)].into_iter().flatten() {
        s.files.status = done;
        changed = true;
    }
    changed |= s.compare.poll();
    changed |= s.section.update(&s.scene);
    s.schedule_render(tx);
    changed
}

fn top_bar(s: &State, k: &Kit) -> El {
    let exporting = s.files.export.is_some();
    let mut r = row()
        .w_full()
        .gap(10.0)
        .center_y()
        .padding(10.0)
        .fill(k.c.surface_base)
        .child(text("💡 Lampdesigner").font(k.ty.ui_medium).color(k.c.text_strong))
        .child(widgets::button_tx(k, "📂 Open…", |s: &mut State, tx| s.open_project(tx)))
        .child(widgets::button_tx(k, "💾 Save…", |s: &mut State, tx| s.save_project(tx)))
        .child(
            rune_kit::Button::new("📦 Export 3MF…")
                .control_size(rune_kit::ControlSize::Small)
                .disabled(exporting)
                .on_press_tx(|s: &mut State, tx: &mut Tx| s.export_3mf(tx))
                .build(k),
        )
        .child(widgets::check(k, "fine mesh", s.files.export_fine, |s| &mut s.files.export_fine));
    if exporting {
        r = r.child(rune_kit::Spinner::new().size(14.0).build(k));
    }
    r.child(widgets::button(k, "⚖ Compare materials", |s: &mut State| {
        let State { compare, wake, p, scene, view, scene_q, .. } = s;
        compare.start(wake, p, scene, view.cam, view.rs, *scene_q);
    }))
    .child(widgets::faint(k, s.files.status.clone()).truncate())
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let k = widgets::kit(cx.system_dark());
    let scale = cx.env().scale;
    let mut layer = Vec::new();
    let open = |bit: u32| s.ui.sections & bit != 0;

    let left_content = col()
        .w_full()
        .gap(4.0)
        .padding(12.0)
        .child(widgets::section(k, "🎲 Random generator", open(params::S_RANDOM), params::S_RANDOM, || {
            random::panel(s, k, &mut layer)
        }))
        .child(params::panel(s, k, &mut layer));
    let left = col()
        .key("left")
        .w(360.0)
        .h_full()
        .min_h(0.0)
        .fill(k.c.surface_base)
        .child(col().key("left-scroll").w_full().h_full().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(left_content));

    let center = col().key("center").grow(1.0).h_full().min_h(0.0).min_w(0.0).child(viewport::panel(s, k, &mut layer, scale));

    let right = col()
        .key("right")
        .w(340.0)
        .h_full()
        .min_h(0.0)
        .fill(k.c.surface_base)
        .child(
            col()
                .key("right-scroll")
                .w_full()
                .h_full()
                .min_h(0.0)
                .scroll(ScrollSpec { stick_bottom: false })
                .child(col().w_full().padding(12.0).child(results::panel(s, k))),
        );

    section::dialog(s, k, &mut layer);
    compare::dialog(s, k, &mut layer);

    let root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(k.c.page)
        .child(top_bar(s, k))
        .child(row().key("main").w_full().grow(1.0).min_h(0.0).child(left).child(center).child(right));
    rune_kit::root(root, layer)
}

pub fn run() {
    rune::run(AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        min_size: Some((1100.0, 700.0)),
        scenes: SCENES,
        ..AppSpec::new("Lampdesigner", (1600.0, 960.0), init_scene, view)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn runtime() -> Runtime<State> {
        let mut rt = Runtime::new(init(), view, (1600.0, 960.0), Env { system_dark: true, scale: 1.0 });
        rt.frame(0.0);
        rt
    }

    fn settle(rt: &mut Runtime<State>) {
        for _ in 0..4 {
            rt.frame(1.0 / 60.0);
        }
    }

    #[test]
    fn dragging_a_slider_edits_the_design() {
        let mut rt = runtime();
        let before = rt.state.p.design.height;
        let (x, y) = rt.find("sl:height").expect("height slider");
        rt.pointer_down(x, y);
        rt.pointer_move(x + 30.0, y);
        rt.pointer_up(x + 30.0, y);
        settle(&mut rt);
        assert!(rt.state.p.design.height > before + 5.0, "{} → {}", before, rt.state.p.design.height);
        // the slider keeps the value inside its range at the end of the track
        rt.pointer_down(x, y);
        rt.pointer_move(x + 500.0, y);
        rt.pointer_up(x + 500.0, y);
        settle(&mut rt);
        assert!((rt.state.p.design.height - 800.0).abs() < 1e-3);
    }

    #[test]
    fn orbit_and_zoom_move_the_camera() {
        let mut rt = runtime();
        let cam = rt.state.view.cam;
        let (x, y) = rt.find("view3d").expect("3D view");
        rt.pointer_down(x, y);
        rt.pointer_move(x + 80.0, y + 20.0);
        rt.pointer_up(x + 80.0, y + 20.0);
        settle(&mut rt);
        let after = rt.state.view.cam;
        assert!((after.yaw - cam.yaw).abs() > 0.1, "orbit changes yaw");
        assert!((after.pitch - cam.pitch).abs() > 0.01, "orbit changes pitch");
        // the platform wheel path (Runtime::wheel only scrolls scroll views)
        let wheel = |rt: &mut Runtime<State>, dy: f32| {
            rt.wheel_event(rune::WheelEvent::new(0.0, dy).at(x, y));
            settle(rt);
        };
        wheel(&mut rt, 120.0);
        let d1 = rt.state.view.cam.dist;
        assert!(d1 < after.dist, "wheel up (content down) zooms in: {} → {d1}", after.dist);
        wheel(&mut rt, -120.0);
        wheel(&mut rt, -120.0);
        let d2 = rt.state.view.cam.dist;
        assert!((d2 - after.dist) * (d1 - after.dist) < 0.0, "opposite wheel directions zoom opposite ways: {d1} / {d2}");
        // the view reported its size for the renderer
        assert!(rt.state.view.size_px[0] > 100 && rt.state.view.size_px[1] > 100);
    }

    #[test]
    fn sections_collapse() {
        let mut rt = runtime();
        assert!(rt.find("sl:height").is_some());
        rt.state.ui.sections &= !params::S_SHAPE;
        settle(&mut rt);
        assert!(rt.find("sl:height").is_none(), "a closed section builds no controls");
    }
}
