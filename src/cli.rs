//! Headless commands:
//!   lampdesigner export   <project.json|default> <out.3mf> [--coarse]
//!   lampdesigner simulate [project.json] [--quality draft|normal|high|ultra]
//!   lampdesigner render   <project.json|default> <out.png> [mode] [--quality q] [--size WxH]

use crate::checks;
use crate::export::export_project;
use crate::material::{MaterialId, Optics};
use crate::project::Project;
use crate::quality::Quality;
use crate::render::{self, Camera, RenderSettings, ViewMode};
use crate::sim;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn load(arg: Option<&String>) -> Result<Project, String> {
    match arg.map(|s| s.as_str()) {
        None | Some("default") => Ok(Project::default()),
        Some(p) if p.starts_with("--") => Ok(Project::default()),
        Some(p) if p.starts_with("preset:") => {
            let name = &p["preset:".len()..];
            crate::design::presets()
                .into_iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, design)| Project { design, ..Project::default() })
                .ok_or_else(|| format!("unknown preset '{name}'"))
        }
        Some(p) => Project::load(Path::new(p)),
    }
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(|s| s.as_str())
}

fn quality(args: &[String], default: Quality) -> Quality {
    opt(args, "--quality").and_then(Quality::parse).unwrap_or(default)
}

pub fn run(args: &[String]) -> Option<i32> {
    match args.get(1).map(|s| s.as_str()) {
        Some("export") => {
            let Some(out) = args.get(3) else {
                eprintln!("usage: lampdesigner export <project.json|default> <out.3mf> [--coarse]");
                return Some(2);
            };
            let fine = !args.iter().any(|a| a == "--coarse");
            let r = load(args.get(2)).and_then(|p| export_project(&p, Path::new(out), fine).map(|i| (p, i)));
            match r {
                Ok((p, info)) => {
                    println!(
                        "wrote {out}: {} triangles, {:.1} cm³, {:.0} g {}",
                        info.triangles,
                        info.volume_mm3 / 1000.0,
                        p.mass_g(info.volume_mm3),
                        p.material.props().name
                    );
                    Some(0)
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    Some(1)
                }
            }
        }
        Some("simulate") => {
            let p = match load(args.get(2)) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return Some(1);
                }
            };
            let q = quality(args, Quality::Normal);
            let t0 = std::time::Instant::now();
            let sc = crate::app::build_scene(&p, q);
            println!("scene: {} triangles in {:?} (quality {})", sc.mesh.tris.len(), t0.elapsed(), q.name());
            println!("bulb: {}", p.bulb.name);
            println!(
                "{:<24} {:>7} {:>6} {:>6} {:>6} {:>9} {:>9} {:>8} {:>7}",
                "material", "output", "up", "down", "wall", "glow", "bulb-thr", "table25", "maxT"
            );
            for id in MaterialId::ALL {
                let op = Optics::new(id, if id == p.material { p.color } else { id.props().palette[0].1 });
                let t = std::time::Instant::now();
                let r = sim::run(&sc, &op, &p.bulb, q.photons(), q.map_res(), &AtomicBool::new(false)).unwrap();
                let s = &r.stats;
                println!(
                    "{:<24} {:>6.0}% {:>5.0}% {:>5.0}% {:>5.0}% {:>6.0}cd {:>7.0}cd {:>6.0}lx {:>5.0}°C  ({:?})",
                    id.props().name,
                    s.efficiency * 100.0,
                    s.via_top * 100.0,
                    s.via_bottom * 100.0,
                    s.via_wall * 100.0,
                    s.glow_mean,
                    s.bulb_through,
                    s.table_lux_25,
                    s.max_temp,
                    t.elapsed()
                );
                if id == p.material {
                    for (lvl, msg) in checks::run(&p, &sc, &r) {
                        println!("    [{lvl:?}] {msg}");
                    }
                }
            }
            Some(0)
        }
        Some("render") => {
            let (Ok(p), Some(out)) = (load(args.get(2)), args.get(3)) else {
                eprintln!("usage: lampdesigner render <project.json|default> <out.png> [mode] [--quality q] [--size WxH]");
                return Some(2);
            };
            let mode = match args.get(4).map(|s| s.as_str()) {
                Some("shape") => ViewMode::Shape,
                Some("day") => ViewMode::Day,
                Some("thickness") => ViewMode::Thickness,
                Some("overhang") => ViewMode::Overhang,
                Some("temperature") => ViewMode::Temperature,
                _ => ViewMode::Night,
            };
            let q = quality(args, Quality::High);
            let (w, h) = opt(args, "--size")
                .and_then(|s| s.split_once(['x', 'X']))
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                .unwrap_or((1280usize, 1440usize));
            let t0 = std::time::Instant::now();
            let sc = crate::app::build_scene(&p, q);
            let op = Optics::new(p.material, p.color);
            let needs_sim = matches!(mode, ViewMode::Night | ViewMode::Temperature);
            let r = if needs_sim {
                sim::run(&sc, &op, &p.bulb, q.photons(), q.map_res(), &AtomicBool::new(false)).unwrap()
            } else {
                sim::SimResult::empty(&sc)
            };
            let t1 = t0.elapsed();
            let cam = Camera::for_scene(&sc);
            let rs = RenderSettings {
                mode,
                room_lux: if mode == ViewMode::Day { 200.0 } else { 3.0 },
                ev: 0.0,
                show_room: true,
                samples: q.samples(),
                clay: p.clay(),
                    clip: None,
            };
            let f = render::render(&sc, &r, &op, &p.bulb, p.material.props().hdt, &cam, &rs, w, h, None);
            println!(
                "{w}×{h}, quality {}: simulation {:?}, render {:?}",
                q.name(),
                t1,
                t0.elapsed() - t1
            );
            match std::fs::write(out, crate::image_out::png(f.w, f.h, &f.rgba)) {
                Ok(()) => Some(0),
                Err(e) => {
                    eprintln!("error: {e}");
                    Some(1)
                }
            }
        }
        Some("section") => {
            // lampdesigner section <project> <x|y|z> <pos> [out.png] [--keep-below]
            let axis = match args.get(3).map(|s| s.to_ascii_lowercase()) {
                Some(a) if a == "x" => 0,
                Some(a) if a == "y" => 1,
                Some(a) if a == "z" => 2,
                _ => {
                    eprintln!("usage: lampdesigner section <project> <x|y|z> <pos mm> [out.png] [--keep-below]");
                    return Some(2);
                }
            };
            let Some(pos) = args.get(4).and_then(|s| s.parse::<f32>().ok()) else {
                eprintln!("usage: lampdesigner section <project> <x|y|z> <pos mm> [out.png] [--keep-below]");
                return Some(2);
            };
            let p = match load(args.get(2)) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return Some(1);
                }
            };
            let sc = crate::app::build_scene(&p, quality(args, Quality::High));
            let s = crate::section::compute(&sc, axis, pos);
            for line in s.summary() {
                println!("{line}");
            }
            if let Some(m) = s.min_sample {
                println!("thinnest point at ({:.1}, {:.1}) in the section plane", m.outer.x, m.outer.y);
            }
            if let Some(out) = args.get(5).filter(|a| !a.starts_with("--")) {
                let keep_above = if args.iter().any(|a| a == "--keep-below") {
                    false
                } else {
                    axis != 2
                };
                let rs = RenderSettings {
                    mode: ViewMode::Shape,
                    room_lux: 3.0,
                    ev: 0.0,
                    show_room: true,
                    samples: 2,
                    clay: p.clay(),
                    clip: Some(render::Clip { axis, pos, keep_above }),
                };
                let op = Optics::new(p.material, p.color);
                let mut cam = Camera::for_scene(&sc);
                if axis == 2 {
                    cam.pitch = 0.75;
                }
                let f = render::render(
                    &sc,
                    &sim::SimResult::empty(&sc),
                    &op,
                    &p.bulb,
                    p.material.props().hdt,
                    &cam,
                    &rs,
                    800,
                    900,
                    None,
                );
                if let Err(e) = std::fs::write(out, crate::image_out::png(f.w, f.h, &f.rgba)) {
                    eprintln!("error: {e}");
                    return Some(1);
                }
            }
            Some(0)
        }
        Some("random") => {
            // lampdesigner random <theme> <seed> [out.png] [--save project.json]
            let theme = args.get(2).and_then(|s| crate::random::Theme::parse(s));
            let seed = args.get(3).and_then(|s| s.parse::<u64>().ok());
            let (Some(theme), Some(seed)) = (theme, seed) else {
                eprintln!("usage: lampdesigner random <theme> <seed> [out.png] [--save project.json]");
                eprintln!(
                    "themes: {}",
                    crate::random::Theme::ALL.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ")
                );
                return Some(2);
            };
            let base = Project::default();
            let g = crate::random::generate(theme, seed, &base);
            let p = Project { design: g.design, ..base };
            println!(
                "{} #{seed}: {} tries, {}",
                theme.name(),
                g.tries,
                if g.problems.is_empty() { "valid".to_string() } else { g.problems.join(", ") }
            );
            if let Some(path) = opt(args, "--save") {
                if let Err(e) = p.save(Path::new(path)) {
                    eprintln!("error: {e}");
                    return Some(1);
                }
            }
            if let Some(out) = args.get(4).filter(|a| !a.starts_with("--")) {
                let sc = crate::app::build_scene(&p, Quality::Normal);
                let op = Optics::new(p.material, p.color);
                let rs = RenderSettings {
                    mode: ViewMode::Shape,
                    room_lux: 3.0,
                    ev: 0.0,
                    show_room: true,
                    samples: 2,
                    clay: p.clay(),
                    clip: None,
                };
                let f = render::render(
                    &sc,
                    &sim::SimResult::empty(&sc),
                    &op,
                    &p.bulb,
                    p.material.props().hdt,
                    &Camera::for_scene(&sc),
                    &rs,
                    360,
                    440,
                    None,
                );
                if let Err(e) = std::fs::write(out, crate::image_out::png(f.w, f.h, &f.rgba)) {
                    eprintln!("error: {e}");
                    return Some(1);
                }
            }
            Some(0)
        }
        Some("--help") | Some("-h") => {
            println!("lampdesigner                 start the GUI");
            println!("lampdesigner export <project.json|default> <out.3mf> [--coarse]");
            println!("lampdesigner simulate [project.json] [--quality draft|normal|high|ultra]");
            println!("lampdesigner render <project.json|default> <out.png> [shape|night|day|thickness|overhang|temperature]");
            println!("                    [--quality draft|normal|high|ultra] [--size WxH]");
            println!("lampdesigner random <theme> <seed> [out.png] [--save project.json]");
            println!("lampdesigner section <project> <x|y|z> <pos mm> [out.png] [--keep-below]");
            Some(0)
        }
        _ => None,
    }
}
