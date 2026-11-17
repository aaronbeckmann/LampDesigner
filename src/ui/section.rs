//! Section tool: cuts the 3D view with a plane and shows the 2D analysis.

use super::widgets::{faint, label, segmented, slider, strong};
use super::State;
use crate::mesh::Tag;
use crate::render::Clip;
use crate::scene::Scene;
use crate::section::{self, Section};
use glam::Vec2;
use rune::{canvas, col, el, row, text, Color, El, LineCap, PointerEvent, Shape};
use rune_kit::Kit;
use std::sync::Arc;

/// Cached cut, valid for one scene and plane.
struct Cached {
    /// Holding the scene keeps its address from being reused by a new one.
    scene: Arc<Scene>,
    axis: usize,
    pos: f32,
    section: Section,
}

pub struct SectionTool {
    pub on: bool,
    pub window_open: bool,
    pub clip: Clip,
    cache: Option<Cached>,
    /// Pointer over the 2D plot (plot points), for the readout.
    hover: Option<[f32; 2]>,
    /// Pointer over the thickness graph (graph points).
    graph_hover: Option<f32>,
}

impl Default for SectionTool {
    fn default() -> Self {
        SectionTool {
            on: false,
            window_open: false,
            clip: Clip { axis: 1, pos: 0.0, keep_above: true },
            cache: None,
            hover: None,
            graph_hover: None,
        }
    }
}

impl SectionTool {
    /// The plane that cuts the 3D view, if the tool is on.
    pub fn clip(&self) -> Option<Clip> {
        self.on.then_some(self.clip)
    }

    /// Recomputes the cut when the window shows it and scene or plane changed
    /// (from `poll`: the view itself can't change state). True when it did.
    pub fn update(&mut self, scene: &Arc<Scene>) -> bool {
        if !self.on || !self.window_open {
            return false;
        }
        let (axis, pos) = (self.clip.axis, self.clip.pos);
        let valid = self.cache.as_ref().is_some_and(|c| Arc::ptr_eq(&c.scene, scene) && c.axis == axis && c.pos == pos);
        if !valid {
            self.cache = Some(Cached { scene: Arc::clone(scene), axis, pos, section: section::compute(scene, axis, pos) });
        }
        !valid
    }

    fn section(&self) -> Option<&Section> {
        self.cache.as_ref().map(|c| &c.section)
    }
}

/// Slider range for the plane position on `axis`.
fn range(scene: &Scene, base_d: f32, axis: usize) -> (f32, f32) {
    if axis == 2 {
        (scene.z_table, scene.shade_z1 + 10.0)
    } else {
        let r = scene.max_r.max(base_d * 0.5) + 5.0;
        (-r, r)
    }
}

pub fn toolbar(s: &State, k: &Kit) -> El {
    let t = &s.section;
    let mut r = row().gap(8.0).center_y().child(
        rune_kit::Checkbox::new(t.on)
            .label("✂ Section")
            .on_toggle(|s: &mut State| {
                s.section.on = !s.section.on;
                if s.section.on {
                    s.section.window_open = true;
                }
                s.touch();
            })
            .build(k),
    );
    if t.on {
        let (a, b) = range(&s.scene, s.p.lamp.base_d, t.clip.axis);
        r = r
            .child(segmented(k, &section::AXIS_NAMES, t.clip.axis, |s, axis| {
                if axis == s.section.clip.axis {
                    return;
                }
                let (a, b) = range(&s.scene, s.p.lamp.base_d, axis);
                // sensible defaults: keep the back half / the lower part
                s.section.clip = Clip {
                    axis,
                    keep_above: axis != 2,
                    pos: if axis == 2 { (s.scene.shade_z0 + s.scene.shade_z1) * 0.5 } else { 0.0f32.clamp(a, b) },
                };
            }))
            .child(slider(k, section::AXIS_NAMES[t.clip.axis], t.clip.pos, a..=b, " mm", |s| &mut s.section.clip.pos))
            .child(
                rune_kit::Checkbox::new(t.clip.keep_above)
                    .label("keep + side")
                    .on_toggle(|s: &mut State| {
                        s.section.clip.keep_above = !s.section.clip.keep_above;
                        s.touch();
                    })
                    .build(k),
            )
            .child(super::widgets::button(k, "📐 Analysis", |s: &mut State| s.section.window_open = !s.section.window_open));
    }
    r
}

/// The 2D analysis dialog.
pub fn dialog(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let t = &s.section;
    if !t.on || !t.window_open {
        return;
    }
    let mut body = col().gap(6.0).w_full();
    match t.section() {
        Some(sec) => {
            for line in sec.summary() {
                body = body.child(label(k, line));
            }
            if sec.t_min > 0.0 && sec.t_min < 0.8 {
                body = body.child(
                    text("⚠ thinner than 2 lines (0.8 mm) in this section")
                        .font(k.ty.text_small_medium)
                        .color(Color { r: 0.86, g: 0.13, b: 0.10, a: 1.0 }),
                );
            }
            body = body
                .child(faint(k, "Thickness is measured inside the cut plane: an oblique cut reads thicker than the true wall."))
                .child(section_plot(k, sec, t.hover))
                .child(strong(k, if sec.axis == 2 { "Wall thickness around the circumference" } else { "Wall thickness over height" }))
                .child(thickness_graph(k, sec, t.graph_hover));
        }
        None => body = body.child(faint(k, "computing…")),
    }
    let title = format!("📐 Section {} = {:.1} mm", section::AXIS_NAMES[t.clip.axis], t.clip.pos);
    rune_kit::Dialog::new("section-dialog", title)
        .open(true)
        .width(560.0)
        .fit()
        .click_outside(false)
        .on_close(|s: &mut State| s.section.window_open = false)
        .child(col().w_full().px(16.0).pb(16.0).child(body))
        .build(k, layer);
}

fn tag_color(tag: Tag) -> Color {
    match tag {
        Tag::Outer => Color { r: 0.95, g: 0.30, b: 0.05, a: 1.0 },
        Tag::Inner => Color { r: 0.95, g: 0.58, b: 0.10, a: 1.0 },
        Tag::Rim => Color { r: 0.95, g: 0.40, b: 0.06, a: 1.0 },
        Tag::Mount => Color { r: 0.10, g: 0.30, b: 0.95, a: 1.0 },
        Tag::Proxy => Color { r: 0.20, g: 0.20, b: 0.20, a: 1.0 },
    }
}

const PLOT_W: f32 = 520.0;
const PLOT_H: f32 = 380.0;
const RED: Color = Color { r: 0.90, g: 0.08, b: 0.08, a: 1.0 };
const CYAN: Color = Color { r: 0.05, g: 0.55, b: 0.65, a: 1.0 };

/// Maps section mm to plot points (y up) and back.
struct Frame2 {
    mid: Vec2,
    scale: f32,
}

impl Frame2 {
    fn new(sec: &Section) -> Frame2 {
        let mut lo = sec.lo;
        let mut hi = sec.hi;
        for p in &sec.bulb {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let span = (hi - lo).max(Vec2::splat(10.0));
        let scale = (PLOT_W * 0.84 / span.x).min(PLOT_H * 0.84 / span.y);
        Frame2 { mid: (lo + hi) * 0.5, scale }
    }
    fn to_plot(&self, p: Vec2) -> [f32; 2] {
        [PLOT_W / 2.0 + (p.x - self.mid.x) * self.scale, PLOT_H / 2.0 - (p.y - self.mid.y) * self.scale]
    }
    fn to_mm(&self, q: [f32; 2]) -> Vec2 {
        Vec2::new((q[0] - PLOT_W / 2.0) / self.scale + self.mid.x, -(q[1] - PLOT_H / 2.0) / self.scale + self.mid.y)
    }
}

fn circle(c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    (0..=24).map(|i| {
        let a = std::f32::consts::TAU * i as f32 / 24.0;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    }).collect()
}

/// A dashed polyline, drawn as short strokes.
fn dashed(mut shape: Shape, pts: &[[f32; 2]], width: f32, color: Color, dash: f32) -> Shape {
    let mut on = true;
    let mut left = dash;
    for w in pts.windows(2) {
        let (a, b) = (Vec2::from(w[0]), Vec2::from(w[1]));
        let len = a.distance(b);
        let mut t = 0.0;
        while t < len {
            let step = left.min(len - t);
            if on {
                let p0 = a + (b - a) * (t / len);
                let p1 = a + (b - a) * ((t + step) / len);
                shape = shape.line(p0.into(), p1.into(), width, LineCap::Butt, color);
            }
            t += step;
            left -= step;
            if left <= 1e-4 {
                on = !on;
                left = dash;
            }
        }
    }
    shape
}

fn section_plot(k: &Kit, sec: &Section, hover: Option<[f32; 2]>) -> El {
    let mut frame = el().key("section-plot").size(PLOT_W, PLOT_H).radius(6.0).fill(k.c.surface_raised_base).border(1.0, k.c.border_weak);
    if sec.segs.is_empty() {
        return frame.center().child(faint(k, "the plane misses the model"));
    }
    let f = Frame2::new(sec);
    let a = f.to_mm([0.0, PLOT_H]);
    let b = f.to_mm([PLOT_W, 0.0]);
    let mut shape = Shape::new();
    let mut labels: Vec<El> = Vec::new();
    // grid: 10 mm, labelled every 50 mm
    let mut x = (a.x / 10.0).floor() * 10.0;
    while x <= b.x {
        let major = (x.round() as i32) % 50 == 0;
        let px = f.to_plot(Vec2::new(x, 0.0))[0];
        shape = shape.line([px, 0.0], [px, PLOT_H], 1.0, LineCap::Butt, if major { k.c.border_base } else { k.c.border_weak });
        if major {
            labels.push(faint(k, format!("{x:.0}")).absolute(px + 2.0, PLOT_H - 16.0));
        }
        x += 10.0;
    }
    let mut y = (a.y / 10.0).floor() * 10.0;
    while y <= b.y {
        let major = (y.round() as i32) % 50 == 0;
        let py = f.to_plot(Vec2::new(0.0, y))[1];
        shape = shape.line([0.0, py], [PLOT_W, py], 1.0, LineCap::Butt, if major { k.c.border_base } else { k.c.border_weak });
        if major {
            labels.push(faint(k, format!("{y:.0}")).absolute(4.0, py - 14.0));
        }
        y += 10.0;
    }
    let (hname, vname) = match sec.axis {
        0 => ("Y", "Z"),
        1 => ("X", "Z"),
        _ => ("X", "Y"),
    };
    labels.push(faint(k, format!("{hname} →  {vname} ↑  (mm)")).absolute(PLOT_W - 120.0, 6.0));
    // bulb
    if sec.bulb.len() > 1 {
        let pts: Vec<[f32; 2]> = sec.bulb.iter().map(|p| f.to_plot(*p)).collect();
        shape = dashed(shape, &pts, 1.2, Color { r: 0.80, g: 0.62, b: 0.0, a: 1.0 }, 5.0);
    }
    // geometry
    for s in &sec.segs {
        shape = shape.line(f.to_plot(s.a), f.to_plot(s.b), 1.6, LineCap::Round, tag_color(s.tag));
    }
    // bulb clearance
    if let Some((g, bp, wp)) = sec.clearance {
        let (p0, p1) = (f.to_plot(bp), f.to_plot(wp));
        shape = dashed(shape, &[p0, p1], 1.2, CYAN, 3.0);
        labels.push(text(format!("gap {g:.1}")).font(k.ty.small).color(CYAN).absolute(p1[0] + 4.0, p1[1] - 7.0));
    }
    // thinnest spot
    if let Some(m) = sec.min_sample {
        let (p0, p1) = (f.to_plot(m.outer), f.to_plot(m.inner));
        shape = shape.stroke(circle(p0, 7.0), 1.5, RED).line(p0, p1, 2.0, LineCap::Round, RED);
        labels.push(text(format!("min {:.2} mm", m.t)).font(k.ty.text_small_medium).color(RED).absolute(p0[0] + 9.0, p0[1] - 22.0));
    }
    // hover readout: coordinates and the nearest wall sample
    if let Some(q) = hover {
        let mm = f.to_mm(q);
        let mut readout = format!("{hname} {:.1}  {vname} {:.1} mm", mm.x, mm.y);
        if let Some(n) = sec
            .samples
            .iter()
            .min_by(|a, b| a.outer.distance(mm).partial_cmp(&b.outer.distance(mm)).unwrap_or(std::cmp::Ordering::Equal))
        {
            if n.outer.distance(mm) < 8.0 {
                let (p0, p1) = (f.to_plot(n.outer), f.to_plot(n.inner));
                shape = shape.line(p0, p1, 2.0, LineCap::Round, k.c.text_strong).stroke(circle(p0, 3.0), 2.0, k.c.text_strong);
                readout += &format!("   wall {:.2} mm", n.t);
            }
        }
        labels.push(text(readout).font(k.ty.text_small_medium).color(k.c.text_strong).absolute(8.0, 6.0));
    }
    frame = frame
        .on_pointer_move_local(|s: &mut State, e: PointerEvent, _| s.section.hover = Some([e.x, e.y]))
        .on_hover_change(|s: &mut State, inside: bool, _| {
            if !inside {
                s.section.hover = None;
            }
        })
        .child(canvas(shape.view_box(PLOT_W, PLOT_H)).size(PLOT_W, PLOT_H).absolute(0.0, 0.0));
    for l in labels {
        frame = frame.child(l);
    }
    frame
}

const GRAPH_W: f32 = 520.0;
const GRAPH_H: f32 = 110.0;
const PAD_X: f32 = 34.0;
const PAD_Y: f32 = 14.0;

/// Thickness along the cut (angle for Z cuts, height for X/Y cuts).
fn thickness_graph(k: &Kit, sec: &Section, hover: Option<f32>) -> El {
    let mut frame = el().key("section-graph").size(GRAPH_W, GRAPH_H).radius(6.0).fill(k.c.surface_raised_base).border(1.0, k.c.border_weak);
    if sec.samples.len() < 2 {
        return frame;
    }
    let (p0, p1) = if sec.axis == 2 {
        (0.0, 360.0)
    } else {
        let a = sec.samples.first().map_or(0.0, |s| s.param);
        let b = sec.samples.last().map_or(1.0, |s| s.param);
        (a, b.max(a + 1.0))
    };
    let t_top = (sec.t_max * 1.15).max(1.0);
    let (iw, ih) = (GRAPH_W - 2.0 * PAD_X, GRAPH_H - 2.0 * PAD_Y);
    let map = |param: f32, t: f32| [PAD_X + (param - p0) / (p1 - p0) * iw, PAD_Y + ih - (t / t_top) * ih];
    let mut shape = Shape::new();
    let y08 = map(p0, 0.8)[1];
    shape = dashed(shape, &[[PAD_X, y08], [PAD_X + iw, y08]], 1.0, RED, 4.0);
    let warm = Color { r: 0.95, g: 0.45, b: 0.05, a: 1.0 };
    if sec.axis == 2 {
        let pts: Vec<[f32; 2]> = sec.samples.iter().map(|s| map(s.param, s.t)).collect();
        shape = shape.stroke(pts, 1.5, warm);
    } else {
        // vertical cuts hit the wall twice (both sides): plot both as points
        for s in &sec.samples {
            let q = map(s.param, s.t);
            shape = shape.line(q, [q[0] + 0.01, q[1]], 2.6, LineCap::Round, warm);
        }
    }
    let mut labels = vec![
        text("0.8").font(k.ty.small).color(RED).absolute(4.0, y08 - 7.0),
        faint(k, format!("{t_top:.1}")).absolute(4.0, PAD_Y - 7.0),
        faint(k, format!("{p0:.0}")).absolute(PAD_X, GRAPH_H - PAD_Y),
        faint(k, if sec.axis == 2 { "360°".to_string() } else { format!("{p1:.0} mm") }).absolute(GRAPH_W - PAD_X - 40.0, GRAPH_H - PAD_Y),
    ];
    if let Some(hx) = hover {
        let param = p0 + (hx - PAD_X) / iw * (p1 - p0);
        if let Some(n) = sec
            .samples
            .iter()
            .min_by(|a, b| (a.param - param).abs().partial_cmp(&(b.param - param).abs()).unwrap_or(std::cmp::Ordering::Equal))
        {
            let q = map(n.param, n.t);
            shape = shape.stroke(circle(q, 4.0), 1.5, k.c.text_strong);
            let txt = if sec.axis == 2 { format!("{:.0}°: {:.2} mm", n.param, n.t) } else { format!("{:.1} mm: {:.2} mm", n.param, n.t) };
            labels.push(text(txt).font(k.ty.small).color(k.c.text_strong).absolute((q[0] + 6.0).min(GRAPH_W - 110.0), (q[1] - 18.0).max(0.0)));
        }
    }
    frame = frame
        .on_pointer_move_local(|s: &mut State, e: PointerEvent, _| s.section.graph_hover = Some(e.x))
        .on_hover_change(|s: &mut State, inside: bool, _| {
            if !inside {
                s.section.graph_hover = None;
            }
        })
        .child(canvas(shape.view_box(GRAPH_W, GRAPH_H)).size(GRAPH_W, GRAPH_H).absolute(0.0, 0.0));
    for l in labels {
        frame = frame.child(l);
    }
    frame
}
