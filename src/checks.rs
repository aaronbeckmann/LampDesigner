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
    let r_inner0 = (0..360)
        .map(|k| d.radii((k as f32).to_radians(), 0.0).0)
        .fold(f32::INFINITY, f32::min);
    if d.hole_d * 0.5 + d.ring_width + 2.0 > r_inner0 {
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
    let (bx, by, bz) = p.printer.volume();
    let dia = 2.0 * sc.max_r;
    if dia > bx.min(by) - 4.0 || d.height > bz - 2.0 {
        out.push((
            Level::Error,
            format!("Doesn't fit {} ({:.0} × {:.0} mm vs. {:.0} × {:.0} mm).", p.printer.name(), dia, d.height, bx.min(by), bz),
        ));
    }
    out
}
