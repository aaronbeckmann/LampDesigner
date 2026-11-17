//! The 3D view: its state, toolbars and the GPU-rendered picture.

use super::widgets::{faint, legend, select, slider};
use super::State;
use crate::gpu::GpuView;
use crate::quality::Quality;
use crate::render::{self, Camera, RenderSettings, ViewMode};
use crate::scene::Scene;
use rune::{col, el, row, Drag, El, Phase, PointerButton, Tx, WheelEvent};
use rune_kit::Kit;
use std::time::{Duration, Instant};

/// What a sent frame was rendered from (resend only when it changes).
#[derive(Clone, PartialEq)]
pub struct FrameKey {
    scene: usize,
    sim: usize,
    cam: Camera,
    rs: RenderSettings,
    size: [u32; 2],
    ssaa: u32,
    exposure: u32,
}

impl FrameKey {
    pub fn new(scene: usize, sim: usize, cam: Camera, rs: RenderSettings, size: [u32; 2], ssaa: u32, exposure: f32) -> FrameKey {
        FrameKey { scene, sim, cam, rs, size, ssaa, exposure: exposure.to_bits() }
    }
}

pub struct ViewState {
    pub gpu: GpuView,
    pub cam: Camera,
    pub rs: RenderSettings,
    pub lock_exposure: bool,
    pub exposure: f32,
    /// Size of the view in device pixels (from layout).
    pub size_px: [u32; 2],
    /// Width / height of the view (for saved images).
    pub aspect: f32,
    /// Last user change: frames are coarse for a moment after it.
    pub touched: Instant,
    pub last_key: Option<FrameKey>,
    /// Camera at the start of a drag.
    drag_cam: Camera,
}

impl ViewState {
    pub fn new(scene: &Scene, clay: glam::Vec3) -> ViewState {
        let cam = Camera::for_scene(scene);
        ViewState {
            gpu: GpuView::new(),
            cam,
            rs: RenderSettings { mode: ViewMode::Night, room_lux: 3.0, ev: 0.0, show_room: true, samples: 1, clay, clip: None },
            lock_exposure: false,
            exposure: 1.0,
            size_px: [0, 0],
            aspect: 1.0,
            touched: Instant::now(),
            last_key: None,
            drag_cam: cam,
        }
    }

    pub fn interacting(&self) -> bool {
        self.touched.elapsed() < Duration::from_millis(180)
    }

    /// Light-only views are unavailable without the simulation.
    pub fn leave_light_views(&mut self) {
        if matches!(self.rs.mode, ViewMode::Night | ViewMode::Temperature) {
            self.rs.mode = ViewMode::Shape;
        }
    }
}

/// Supersampling of the settled (not interacting) frame per quality.
pub fn ssaa_for(q: Quality) -> u32 {
    match q {
        Quality::Draft => 1,
        Quality::Normal | Quality::High => 2,
        Quality::Ultra => 3,
    }
}

const MODES: [(ViewMode, &str, bool); 6] = [
    (ViewMode::Shape, "◆ Shape", false),
    (ViewMode::Night, "🌙 Lamp on", true),
    (ViewMode::Day, "☀ Daylight", false),
    (ViewMode::Thickness, "Thickness", false),
    (ViewMode::Overhang, "Overhang", false),
    (ViewMode::Temperature, "Temperature", true),
];

fn mode_row(s: &State, k: &Kit) -> El {
    let labels: Vec<&str> = MODES.iter().map(|m| m.1).collect();
    let current = MODES.iter().position(|m| m.0 == s.view.rs.mode).unwrap_or(0);
    let mut seg = rune_kit::SegmentedControl::new(&labels, Some(current)).fit().on_pick(|s: &mut State, i: usize, _: &mut Tx| {
        let (mode, _, needs_sim) = MODES[i];
        if needs_sim && !s.sim.enabled {
            return;
        }
        s.view.rs.mode = mode;
        match mode {
            ViewMode::Day => s.view.rs.room_lux = 200.0,
            ViewMode::Night => s.view.rs.room_lux = 3.0,
            _ => {}
        }
    });
    if !s.sim.enabled {
        for (i, m) in MODES.iter().enumerate() {
            if m.2 {
                seg = seg.segment_state(i, rune_kit::State::Disabled);
            }
        }
    }
    let v = &s.view;
    row()
        .gap(10.0)
        .center_y()
        .flex_wrap()
        .child(seg.build(k))
        .child(slider(k, "room lx", v.rs.room_lux, 0.0..=500.0, "", |s| &mut s.view.rs.room_lux))
        .child(slider(k, "EV", v.rs.ev, -4.0..=4.0, "", |s| &mut s.view.rs.ev))
        .child(super::widgets::check(k, "lock exposure", v.lock_exposure, |s| &mut s.view.lock_exposure))
        .child(super::widgets::check(k, "room", v.rs.show_room, |s| &mut s.view.rs.show_room))
        .child(super::widgets::button(k, "⟲ view", |s: &mut State| s.view.cam = Camera::for_scene(&s.scene)))
}

fn quality_row(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let names: Vec<String> = Quality::ALL
        .iter()
        .map(|q| format!("{}  ({}× mesh, {}× AA)", q.name(), q.mesh_scale(), ssaa_for(*q) * ssaa_for(*q)))
        .collect();
    let current = names[Quality::ALL.iter().position(|q| *q == s.quality).unwrap_or(1)].clone();
    let widths: Vec<String> = [1920usize, 2560, 3840, 5120].iter().map(|w| format!("{w} px wide")).collect();
    let status = s.view.gpu.status();
    let mut r = row()
        .gap(10.0)
        .center_y()
        .flex_wrap()
        .child(select(k, layer, s, "quality", &names, &current, 260.0, |s, v| {
            if let Some(q) = Quality::ALL.into_iter().find(|q| v.starts_with(q.name())) {
                if q != s.quality {
                    s.quality = q;
                    s.rebuild(true);
                }
            }
        }))
        .child(select(k, layer, s, "image-width", &widths, &format!("{} px wide", s.files.image_width), 150.0, |s, v| {
            if let Some(w) = v.split(' ').next().and_then(|w| w.parse().ok()) {
                s.files.image_width = w;
            }
        }));
    let busy = s.files.image.is_some();
    r = r.child(
        rune_kit::Button::new("📷 Save image…")
            .control_size(rune_kit::ControlSize::Small)
            .disabled(busy)
            .on_press_tx(|s: &mut State, tx: &mut Tx| s.save_image(tx))
            .build(k),
    );
    if busy {
        r = r.child(rune_kit::Spinner::new().size(14.0).build(k));
    }
    r.child(faint(k, format!("{} · {:.0} ms", status.backend, status.last_ms)))
}

pub fn panel(s: &State, k: &Kit, layer: &mut Vec<El>, scale: f32) -> El {
    let mut c = col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(6.0)
        .padding(10.0)
        .child(mode_row(s, k))
        .child(quality_row(s, k, layer))
        .child(super::section::toolbar(s, k));
    let hdt = s.p.material.props().hdt;
    match s.view.rs.mode {
        ViewMode::Thickness => c = c.child(legend(k, "0 mm", &format!("{:.1} mm", s.scene.max_thickness))),
        ViewMode::Overhang => c = c.child(legend(k, "0°", "90° (> 45° needs care)")),
        ViewMode::Temperature => {
            c = c.child(legend(k, "25 °C", &format!("{:.0} °C (softening {:.0} °C)", render::temp_scale_max(hdt), hdt)))
        }
        _ => {}
    }
    let view = el()
        .key("view3d")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .radius(6.0)
        .fill(k.c.bg_deep)
        .pixels(&s.view.gpu.pixels)
        .on_layout(move |s: &mut State, r: [f32; 4]| {
            s.view.size_px = [(r[2] * scale).round().max(1.0) as u32, (r[3] * scale).round().max(1.0) as u32];
            s.view.aspect = r[2].max(1.0) / r[3].max(1.0);
        })
        .pointer_buttons([PointerButton::Middle])
        .on_drag(|s: &mut State, d: &mut Drag, tx: &mut Tx| {
            match d.phase {
                Phase::Down => s.view.drag_cam = s.view.cam,
                Phase::Move => {
                    let start = s.view.drag_cam;
                    if tx.button() == PointerButton::Middle {
                        s.view.cam.target.z = start.target.z + d.dy * start.dist * 0.002;
                    } else {
                        s.view.cam.yaw = start.yaw - d.dx * 0.008;
                        s.view.cam.pitch = (start.pitch + d.dy * 0.008).clamp(-1.4, 1.4);
                    }
                    s.touch();
                }
                Phase::Up => {}
            }
            // the camera only changes the picture, not the view tree
            tx.live_only();
        })
        .on_wheel(|s: &mut State, e: WheelEvent, tx: &mut Tx| {
            // e.dy > 0 moves content down (wheel turned up / away): zoom in
            s.view.cam.dist = (s.view.cam.dist * (1.0 - e.dy * 0.002)).clamp(100.0, 3000.0);
            s.touch();
            tx.live_only();
            true
        });
    c.child(view).child(faint(k, "drag: orbit · middle-drag: up/down · wheel: zoom"))
}
