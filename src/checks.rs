//! Printability / fit / safety checks.

use crate::design::MountKind;
use crate::project::Project;
use crate::scene::Scene;
use crate::sim::SimResult;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    Info,
    Warn,
    Error,
}

pub fn run(p: &Project, sc: &Scene, sim: &SimResult) -> Vec<(Level, String)> {
    let d = &p.design;
    let mat = p.material.props();
    let mut out = Vec::new();

    // bulb fit
    if sc.min_clearance < 0.0 {
        out.push((Level::Error, format!("Shade intersects the bulb ({:.0} mm overlap).", -sc.min_clearance)));
    } else if sc.min_clearance < 10.0 {
        out.push((Level::Warn, format!("Only {:.1} mm between bulb and wall; aim for ≥ 10 mm.", sc.min_clearance)));
    } else {
        out.push((Level::Ok, format!("Bulb clearance {:.0} mm.", sc.min_clearance)));
    }
    if d.mount == MountKind::FlatRing && d.hole_d < 34.0 && sc.top_opening_r < p.bulb.rx + 1.0 {
        out.push((
            Level::Error,
            format!(
                "Top opening (Ø{:.0} mm) is smaller than the bulb (Ø{:.0} mm): the bulb can't be screwed in through the shade.",
                2.0 * sc.top_opening_r,
                2.0 * p.bulb.rx
            ),
        ));
    }

    // mount
    match d.mount {
        MountKind::FlatRing => {
            if d.hole_d < 27.2 {
                out.push((Level::Error, "Mount hole is smaller than the E27 thread (27 mm).".into()));
            } else if d.hole_d < 34.0 {
                out.push((
                    Level::Info,
                    format!("Washer mount: clamped between bulb cap and socket rim. Hole Ø{:.1} mm.", d.hole_d),
                ));
                if d.hole_d > 31.0 {
                    out.push((Level::Warn, "Hole > 31 mm may slip over the bulb's cap.".into()));
                }
                if d.hole_d + 2.0 * d.ring_width < p.lamp.socket_d {
                    out.push((Level::Warn, "Washer ring is narrower than the socket rim; widen the ring.".into()));
                }
            }
        }
        MountKind::Sleeve => {
            let gap = d.hole_d - p.lamp.socket_d;
            if !(0.1..=0.8).contains(&gap) {
                out.push((
                    Level::Warn,
                    format!("Sleeve fit: hole is {:+.2} mm vs. socket Ø{:.1}. Use +0.2…+0.5 mm.", gap, p.lamp.socket_d),
                ));
            }
            if d.sleeve_height > p.lamp.height - 3.0 {
                out.push((
                    Level::Error,
                    format!(
                        "The {:.0} mm sleeve reaches the table (socket top is {:.0} mm above it).",
                        d.sleeve_height, p.lamp.height
                    ),
                ));
            }
            sleeve_window_check(p, &mut out);
            if !d.ledge {
                out.push((Level::Warn, "Sleeve has no ledge: only friction holds the shade on the socket.".into()));
            } else {
                let rim = (p.lamp.socket_d - d.ledge_d) * 0.5;
                if d.ledge_d < 27.5 {
                    out.push((Level::Error, format!("Ledge hole Ø{:.1} mm is smaller than the E27 cap (27 mm).", d.ledge_d)));
                } else if d.ledge_d < 31.0 {
                    out.push((
                        Level::Warn,
                        format!("Ledge hole Ø{:.1} mm may hit the bulb's neck; measure it just above the socket.", d.ledge_d),
                    ));
                }
                if rim < 1.0 {
                    out.push((
                        Level::Error,
                        format!("Ledge overlaps the socket rim by only {:.1} mm: it will slip over. Reduce the ledge Ø.", rim.max(0.0)),
                    ));
                } else {
                    out.push((
                        Level::Info,
                        format!(
                            "Ledge rests on the socket top ({:.1} mm rim overlap, {} underside).",
                            rim,
                            if d.ledge_chamfer { "45° chamfered" } else { "flat" }
                        ),
                    ));
                }
                let lt = crate::geometry::ledge_top(d);
                let glass_r = p.bulb.radius_at(lt - crate::scene::bulb_center_z(d, &p.bulb));
                if glass_r > d.ledge_d * 0.5 - 1.0 {
                    out.push((Level::Error, "Ledge collides with the bulb glass; lower the ledge or enlarge its hole.".into()));
                }
                if !d.ledge_chamfer && rim > 2.0 {
                    out.push((
                        Level::Warn,
                        format!("Flat ledge is a {:.1} mm inward overhang; enable the chamfer to print without supports.", (d.hole_d - d.ledge_d) * 0.5),
                    ));
                }
            }
        }
    }
    if d.vase_mode {
        let layers = (d.mount_thickness / 0.2).ceil() as u32;
        out.push((
            Level::Info,
            if crate::geometry::has_insert(d) {
                format!(
                    "Vase mode: print the shade with Spiral vase (Bottom shell layers ≥ {layers} at 0.2 mm). \
                     Print the mount insert (second 3MF) normally, flange down, then push it up into the ring and glue the flange to the ring."
                )
            } else {
                format!("Vase mode: the ring and spokes print as the bottom layers (Bottom shell layers ≥ {layers} at 0.2 mm).")
            },
        ));
        out.push((
            Level::Info,
            "Vase mode prints one wall line: the wall-thickness setting and the inner relief are not printed.".into(),
        ));
    }
    let r_inner0 = (0..360)
        .map(|k| d.radii((k as f32).to_radians(), 0.0).0)
        .fold(f32::INFINITY, f32::min);
    if crate::geometry::spider_radii(d).1 + 2.0 > r_inner0 {
        out.push((Level::Error, "Mount ring is wider than the shade's bottom opening.".into()));
    }
    if sim.stats.mount_temp > mat.hdt - 5.0 {
        out.push((
            if sim.stats.mount_temp > mat.hdt { Level::Error } else { Level::Warn },
            format!(
                "Mount touches the bulb cap (~{:.0} °C est.) close to {}'s softening point ({:.0} °C). Prefer PETG or the sleeve mount.",
                sim.stats.mount_temp, mat.name, mat.hdt
            ),
        ));
    }

    // heat
    if sim.stats.max_temp > mat.hdt - 10.0 {
        out.push((
            Level::Error,
            format!(
                "Wall reaches ~{:.0} °C (est.), too close to {}'s heat-deflection temperature ({:.0} °C).",
                sim.stats.max_temp, mat.name, mat.hdt
            ),
        ));
    }
    if !p.bulb.led {
        out.push((Level::Warn, "Use LED bulbs only with 3D-printed shades.".into()));
    }

    // printability
    let nozzle_walls = sc.min_thickness / 0.42;
    if sc.min_thickness < 0.8 {
        out.push((
            Level::Warn,
            format!("Thinnest wall {:.2} mm (< 2 lines with a 0.4 mm nozzle).", sc.min_thickness),
        ));
    } else {
        out.push((
            Level::Ok,
            format!("Wall {:.2}–{:.2} mm (≈ {:.0}+ lines at 0.42 mm).", sc.min_thickness, sc.max_thickness, nozzle_walls.round()),
        ));
    }
    if sc.max_overhang > 60.0 {
        out.push((
            Level::Error,
            format!(
                "Overhang up to {:.0}° ({:.0}% of the wall > 45°): it will sag without supports.",
                sc.max_overhang,
                sc.overhang_frac * 100.0
            ),
        ));
    } else if sc.max_overhang > 45.0 {
        out.push((
            Level::Warn,
            format!("Overhang up to {:.0}°: printable but may look rough. Slow down outer walls.", sc.max_overhang),
        ));
    } else {
        out.push((Level::Ok, format!("Max overhang {:.0}°: prints without supports.", sc.max_overhang)));
    }
    fit_checks(p, sc, &mut out);
    out
}

/// Bed size and the split into parts along the height.
fn fit_checks(p: &Project, sc: &Scene, out: &mut Vec<(Level, String)>) {
    use crate::geometry::{ledge_top, split_cuts, split_has_joint, split_heights, split_parts_for};
    let d = &p.design;
    let (bx, by, bz) = p.bed();
    let name = p.printer.name();
    let dia = 2.0 * sc.max_r;
    if !crate::project::fits((bx, by, bz), dia, 0.0).0 {
        let fitting: Vec<&str> = Project::printers_fitting(dia).iter().map(|p| p.name()).collect();
        let hint = if fitting.is_empty() {
            "No preset bed is that big: make the shade narrower.".to_string()
        } else {
            format!("Make it narrower, or pick a larger printer: {}.", fitting.join(", "))
        };
        out.push((Level::Error, format!("Ø{dia:.0} mm is wider than the {name} bed ({bx:.0} × {by:.0} mm). {hint}")));
    }
    let heights = split_heights(d);
    let tallest = heights.iter().cloned().fold(0.0, f32::max);
    if tallest > bz - 2.0 {
        let msg = match split_parts_for(d, bz) {
            Some(n) => format!(
                "{:.0} mm{} is taller than the {name} ({bz:.0} mm). Split it into {n} parts under Printer & split.",
                tallest,
                if heights.len() > 1 { " (tallest part)" } else { "" }
            ),
            None => format!("Too tall for the {name} ({bz:.0} mm) even in 8 parts."),
        };
        out.push((Level::Error, msg));
    } else if heights.len() > 1 {
        let cuts: Vec<String> = split_cuts(d).iter().map(|c| format!("{:.0}", c * d.height)).collect();
        let hs: Vec<String> = heights.iter().map(|h| format!("{h:.0}")).collect();
        out.push((
            Level::Info,
            format!(
                "Printed in {} parts, cut at {} mm above the bottom (part heights {} mm). Each part is its own 3MF; {}.",
                heights.len(),
                cuts.join(" / "),
                hs.join(" / "),
                if split_has_joint(d) {
                    "the upper part's skirt slides over the lower part's lip: glue the overlap (CA or epoxy)"
                } else {
                    "glue the flat joints"
                }
            ),
        ));
    }
    if heights.len() > 1 {
        let cuts = split_cuts(d);
        let z0 = d.z0();
        let first = z0 + cuts[0] * d.height;
        let mount_top = ledge_top(d).max(z0 + d.mount_thickness);
        if first < mount_top + 3.0 {
            out.push((Level::Error, "The lowest cut runs through the mount: use fewer parts.".into()));
        }
        let lip_top = cuts[cuts.len() - 1] * d.height + if split_has_joint(d) { d.split_overlap + 2.0 } else { 0.0 };
        let rim_low = d.height - if d.rim_waves > 0 { d.rim_depth.min(d.height * 0.5) } else { 0.0 };
        if lip_top > rim_low - 5.0 {
            out.push((Level::Error, "The top cut is too close to the wavy rim: use fewer parts.".into()));
        }
        if split_has_joint(d) {
            // thinnest half wall at a cut
            let half = cuts
                .iter()
                .flat_map(|&c| (0..90).map(move |k| (k as f32 * 4.0f32).to_radians()).map(move |th| (th, c)))
                .map(|(th, c)| {
                    let (i, o) = d.radii(th, c);
                    (o - i) * 0.5 - d.split_gap * 0.5
                })
                .fold(f32::INFINITY, f32::min);
            if half < 0.8 {
                out.push((
                    Level::Warn,
                    format!(
                        "The joint halves the wall to {half:.2} mm at the cuts (one line or less). Use a thicker wall (≥ 2 mm) or a flat joint."
                    ),
                ));
            }
        }
    }
}

/// Weight-saving sleeve windows: how much they save, or why there are none.
fn sleeve_window_check(p: &Project, out: &mut Vec<(Level, String)>) {
    use crate::geometry::{build_insert, build_mount};
    let d = &p.design;
    if d.sleeve_windows < 2 {
        return;
    }
    let sleeve = |d: &crate::design::Design| -> (usize, f64) {
        let parts = match build_insert(d, 240) {
            Some(insert) => insert,
            None => build_mount(d, 240).split_off(1),
        };
        (parts.len(), parts.iter().map(|m| m.volume()).sum())
    };
    let solid = crate::design::Design { sleeve_windows: 0, ..d.clone() };
    let (n, v) = sleeve(d);
    if n == 1 {
        out.push((
            Level::Info,
            format!(
                "Sleeve windows need more height or fewer, narrower struts: the {:.0} mm sleeve stays solid.",
                d.sleeve_height
            ),
        ));
        return;
    }
    let saved = p.mass_g(sleeve(&solid).1 - v);
    out.push((Level::Ok, format!("Sleeve windows: {} struts, saves ≈ {saved:.0} g.", d.sleeve_windows)));
}