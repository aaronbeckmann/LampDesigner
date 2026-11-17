//! Right panel: simulation results, light distribution, checks, print tips.

use super::widgets::{faint, label, strong};
use super::State;
use crate::checks::{self, Level};
use rune::{canvas, col, el, row, text, Color, El, LineCap, Shape};
use rune_kit::Kit;

fn stat(k: &Kit, name: &str, value: String) -> El {
    row()
        .w_full()
        .gap(8.0)
        .child(label(k, name).w(150.0).truncate())
        .child(text(value).font(k.ty.text_small_medium).color(k.c.text_strong).wrap())
}

pub fn panel(s: &State, k: &Kit) -> El {
    let sim = &s.sim;
    let p = &s.p;
    let sc = &s.scene;
    let st = &sim.result.stats;
    let mass = p.mass_g(sc.volume_mm3);
    let price = mass / 1000.0 * p.material.props().price_eur_kg;

    let mut head = row()
        .gap(8.0)
        .center_y()
        .child(text("Simulation").font(k.ty.ui_medium).color(k.c.text_strong))
        .child(
            rune_kit::Switch::new(sim.enabled)
                .label("light")
                .on_toggle(|s: &mut State| s.toggle_simulation())
                .build(k),
        );
    if sim.enabled && sim.running() {
        head = head.child(rune_kit::Spinner::new().size(14.0).build(k)).child(faint(k, "tracing photons…"));
    }

    let mut stats = col().gap(3.0).w_full();
    if sim.enabled {
        stats = stats.child(stat(k, "Light output", format!("{:.0} % of {:.0} lm", st.efficiency * 100.0, p.bulb.lumens)));
        if sim.full {
            stats = stats
                .child(stat(
                    k,
                    "  up / down / wall",
                    format!("{:.0} / {:.0} / {:.0} %", st.via_top * 100.0, st.via_bottom * 100.0, st.via_wall * 100.0),
                ))
                .child(stat(k, "  absorbed", format!("{:.0} %", st.absorbed * 100.0)))
                .child(stat(k, "Table at 25 cm", format!("{:.0} lx", st.table_lux_25)));
        }
        let ratio = st.bulb_through / st.glow_mean.max(1.0);
        let vis = if ratio > 2.0 {
            "strong hotspot"
        } else if ratio > 0.3 {
            "visible"
        } else {
            "hidden"
        };
        stats = stats
            .child(stat(k, "Shade glow (mean)", format!("{:.0} cd/m²", st.glow_mean)))
            .child(stat(k, "Glow min / max", format!("{:.0} / {:.0} cd/m²", st.glow_min, st.glow_max)))
            .child(stat(k, "Bulb through wall", format!("{:.0} cd/m² ({vis})", st.bulb_through)))
            .child(stat(k, "Max wall temp (est.)", format!("{:.0} °C", st.max_temp)))
            .child(stat(k, "Mount temp (est.)", format!("{:.0} °C", st.mount_temp)));
    } else {
        stats = stats.child(stat(k, "Light simulation", "off".into()));
    }
    stats = stats
        .child(stat(k, "Filament", format!("{mass:.0} g ≈ {price:.2} €")))
        .child(stat(k, "Size", format!("Ø{:.0} × {:.0} mm", 2.0 * sc.max_r, p.design.height)))
        .child(stat(k, "Mesh", format!("{} triangles", sc.mesh.tris.len())));

    let mut c = col().gap(10.0).w_full().child(head).child(stats);
    if sim.enabled {
        c = c.child(strong(k, "Light distribution (cd)")).child(polar_plot(k, &sim.result.polar));
    }
    let mut list = col().gap(5.0).w_full();
    for (lvl, msg) in checks::run(p, sc, &sim.result) {
        let (icon, colour) = match lvl {
            Level::Ok => ("✔", Color { r: 0.16, g: 0.58, b: 0.20, a: 1.0 }),
            Level::Info => ("ℹ", Color { r: 0.26, g: 0.46, b: 0.79, a: 1.0 }),
            Level::Warn => ("⚠", Color { r: 0.87, g: 0.52, b: 0.08, a: 1.0 }),
            Level::Error => ("✖", Color { r: 0.86, g: 0.13, b: 0.10, a: 1.0 }),
        };
        list = list.child(
            row()
                .gap(6.0)
                .w_full()
                .child(text(icon).font(k.ty.text_small_medium).color(colour).w(14.0))
                .child(text(msg).font(k.ty.text_small).color(k.c.text_base).wrap().w(260.0)),
        );
    }
    c = c.child(strong(k, "Checks")).child(list);
    c.child(strong(k, "Print tips (Bambu Studio)"))
        .child(faint(k, "• Print upright; the mount spider lies on the plate. No supports."))
        .child(faint(k, "• Walls: set 'Wall loops' so they fill the wall (1.2 mm ≈ 3 loops at 0.42 mm). Infill isn't needed."))
        .child(faint(k, "• PETG Translucent: 2 loops, slow outer wall, glue stick on textured PEI."))
        .child(faint(k, "• LED bulbs only."))
}

/// Candela per polar angle, mirrored: 0° up.
fn polar_plot(k: &Kit, polar: &[f32]) -> El {
    let (w, h) = (280.0f32, 220.0f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let r = h.min(w) * 0.45;
    let max = polar.iter().cloned().fold(0.0f32, f32::max).max(1e-3);
    let grid = k.c.border_weak;
    let mut shape = Shape::new();
    for ring in 1..=4 {
        let rr = r * ring as f32 / 4.0;
        let pts: Vec<[f32; 2]> =
            (0..=64).map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 64.0;
                [cx + rr * a.cos(), cy + rr * a.sin()]
            }).collect();
        shape = shape.stroke(pts, 1.0, grid);
    }
    for a in (0..360).step_by(30) {
        let t = (a as f32).to_radians();
        shape = shape.line([cx, cy], [cx + t.sin() * r, cy - t.cos() * r], 1.0, LineCap::Butt, grid);
    }
    let n = polar.len();
    let mut pts = Vec::with_capacity(2 * n + 1);
    for side in [1.0f32, -1.0] {
        let order: Vec<usize> = if side > 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
        for b in order {
            let th = (b as f32 + 0.5) / n as f32 * std::f32::consts::PI;
            let rr = polar[b] / max * r;
            pts.push([cx + side * th.sin() * rr, cy - th.cos() * rr]);
        }
    }
    if let Some(&first) = pts.first() {
        pts.push(first);
    }
    shape = shape.stroke(pts, 2.0, Color { r: 1.0, g: 0.55, b: 0.12, a: 1.0 }).view_box(w, h);
    col()
        .gap(2.0)
        .child(canvas(shape).size(w, h))
        .child(row().gap(12.0).child(faint(k, "up = top, down = bottom")).child(faint(k, format!("max {max:.0} cd"))))
        .child(el().h(2.0))
}
