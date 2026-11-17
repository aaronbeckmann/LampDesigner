//! Left panel: design, lamp, bulb and material parameters.

use super::widgets::{check, faint, label, section, segmented, select, slider, srgb8};
use super::State;
use crate::design::{self, MountKind, ProfileKind, RibStyle, TextureMode, SPLINE_POINTS};
use crate::material::MaterialId;
use crate::project::Printer;
use rune::{col, el, row, El};
use rune_kit::Kit;

pub const S_PRESETS: u32 = 1 << 0;
pub const S_LAMP: u32 = 1 << 1;
pub const S_BULB: u32 = 1 << 2;
pub const S_BASE: u32 = 1 << 3;
pub const S_SHAPE: u32 = 1 << 4;
pub const S_FOLDS: u32 = 1 << 5;
pub const S_TEXTURE: u32 = 1 << 6;
pub const S_MOUNT: u32 = 1 << 7;
pub const S_MATERIAL: u32 = 1 << 8;
pub const S_RANDOM: u32 = 1 << 9;
pub const S_CUSTOM_COLOR: u32 = 1 << 10;
pub const S_PRINT: u32 = 1 << 11;
/// Open at start.
pub const DEFAULT_OPEN: u32 = S_LAMP | S_SHAPE | S_FOLDS | S_TEXTURE | S_MOUNT | S_MATERIAL | S_PRINT | S_RANDOM;

pub fn panel(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let open = |bit: u32| s.ui.sections & bit != 0;
    let d = &s.p.design;
    let mut c = col().w_full().gap(4.0);

    c = c.child(section(k, "Presets", open(S_PRESETS), S_PRESETS, || {
        let mut r = row().gap(6.0).flex_wrap();
        for (i, (name, _)) in design::presets().into_iter().enumerate() {
            r = r.child(super::widgets::button(k, name, move |s: &mut State| {
                s.p.design = design::presets().swap_remove(i).1;
            }));
        }
        r
    }));

    c = c.child(section(k, "Lamp & bulb", open(S_LAMP), S_LAMP, || {
        let names: Vec<String> = s.bulbs.iter().map(|b| b.name.clone()).collect();
        let current = s.p.bulb.name.clone();
        col()
            .gap(6.0)
            .child(select(k, layer, s, "bulb", &names, &current, 300.0, |s, v| {
                if let Some(b) = s.bulbs.iter().find(|b| b.name == v) {
                    s.p.bulb = b.clone();
                }
            }))
            .child(section(k, "Bulb details", open(S_BULB), S_BULB, || {
                let b = &s.p.bulb;
                col()
                    .gap(4.0)
                    .child(slider(k, "lumens", b.lumens, 50.0..=2500.0, " lm", |s| &mut s.p.bulb.lumens))
                    .child(slider(k, "power", b.watts, 1.0..=100.0, " W", |s| &mut s.p.bulb.watts))
                    .child(check(k, "LED", b.led, |s| &mut s.p.bulb.led))
                    .child(slider(k, "colour temp.", b.cct, 1800.0..=6500.0, " K", |s| &mut s.p.bulb.cct))
                    .child(slider(k, "glass radius", b.rx, 15.0..=90.0, " mm", |s| &mut s.p.bulb.rx))
                    .child(slider(k, "glass half-height", b.rz, 15.0..=90.0, " mm", |s| &mut s.p.bulb.rz))
                    .child(slider(k, "centre above socket", b.center_h, 20.0..=150.0, " mm", |s| &mut s.p.bulb.center_h))
                    .child(slider(k, "dark cone", b.dark_cone_deg, 0.0..=90.0, "°", |s| &mut s.p.bulb.dark_cone_deg))
            }))
            .child(section(k, "Lamp base (GENNAKER)", open(S_BASE), S_BASE, || {
                let l = &s.p.lamp;
                col()
                    .gap(4.0)
                    .child(faint(k, "IKEA lists Ø120 mm foot, 100 mm height. Measure your socket."))
                    .child(slider(k, "table → socket top", l.height, 40.0..=300.0, " mm", |s| &mut s.p.lamp.height))
                    .child(slider(k, "socket Ø", l.socket_d, 25.0..=60.0, " mm", |s| &mut s.p.lamp.socket_d))
                    .child(slider(k, "socket height", l.socket_h, 10.0..=80.0, " mm", |s| &mut s.p.lamp.socket_h))
                    .child(slider(k, "foot Ø", l.base_d, 60.0..=250.0, " mm", |s| &mut s.p.lamp.base_d))
            }))
    }));

    c = c.child(section(k, "Shape", open(S_SHAPE), S_SHAPE, || {
        let mut col_ = col()
            .gap(4.0)
            .child(slider(k, "height", d.height, 40.0..=800.0, " mm", |s| &mut s.p.design.height))
            .child(row().gap(8.0).center_y().child(label(k, "silhouette")).child(segmented(
                k,
                &["Bulge", "Free curve"],
                usize::from(d.profile_kind == ProfileKind::Spline),
                |s, i| s.p.design.profile_kind = if i == 0 { ProfileKind::Bulge } else { ProfileKind::Spline },
            )));
        match d.profile_kind {
            ProfileKind::Bulge => {
                col_ = col_
                    .child(slider(k, "bottom radius", d.r_bottom, 20.0..=150.0, " mm", |s| &mut s.p.design.r_bottom))
                    .child(slider(k, "top radius", d.r_top, 20.0..=150.0, " mm", |s| &mut s.p.design.r_top))
                    .child(slider(k, "bulge", d.bulge, -40.0..=60.0, " mm", |s| &mut s.p.design.bulge))
                    .child(slider(k, "bulge position", d.bulge_pos, 0.05..=0.95, "", |s| &mut s.p.design.bulge_pos));
            }
            ProfileKind::Spline => {
                // one accessor per curve point (handlers need plain fn pointers)
                const POINT: [fn(&mut State) -> &mut f32; SPLINE_POINTS] = [
                    |s| &mut s.p.design.spline[0],
                    |s| &mut s.p.design.spline[1],
                    |s| &mut s.p.design.spline[2],
                    |s| &mut s.p.design.spline[3],
                    |s| &mut s.p.design.spline[4],
                    |s| &mut s.p.design.spline[5],
                ];
                for i in (0..SPLINE_POINTS).rev() {
                    let name = format!("radius at {:.0} %", 100.0 * i as f32 / (SPLINE_POINTS - 1) as f32);
                    col_ = col_.child(slider(k, &name, d.spline[i], 10.0..=150.0, " mm", POINT[i]));
                }
            }
        }
        col_ = col_.child(slider(k, "polygon sides", d.sides, 0.0..=12.0, "", |s| &mut s.p.design.sides));
        if d.sides >= 3 {
            col_ = col_
                .child(slider(k, "polygon amount", d.poly_amount, 0.0..=1.0, "", |s| &mut s.p.design.poly_amount))
                .child(slider(k, "polygon twist", d.poly_twist_deg, -360.0..=360.0, "°", |s| &mut s.p.design.poly_twist_deg));
        }
        col_
    }));

    c = c.child(section(k, "Folds & flow", open(S_FOLDS), S_FOLDS, || {
        let mut col_ = col().gap(4.0).child(slider(k, "folds", d.lobes, 0.0..=24.0, "", |s| &mut s.p.design.lobes));
        if d.lobes > 0 {
            col_ = col_
                .child(slider(k, "fold depth", d.lobe_amp, 0.0..=30.0, " mm", |s| &mut s.p.design.lobe_amp))
                .child(slider(k, "depth at top ×", d.lobe_growth, 0.0..=4.0, "", |s| &mut s.p.design.lobe_growth))
                .child(slider(k, "fold sharpness", d.lobe_sharpness, 0.3..=4.0, "", |s| &mut s.p.design.lobe_sharpness))
                .child(slider(k, "fold twist", d.lobe_twist_deg, -540.0..=540.0, "°", |s| &mut s.p.design.lobe_twist_deg))
                .child(slider(k, "meander", d.meander_deg, 0.0..=60.0, "°", |s| &mut s.p.design.meander_deg))
                .child(slider(k, "meander waves", d.meander_freq, 0.0..=4.0, "", |s| &mut s.p.design.meander_freq));
        } else {
            col_ = col_.child(faint(k, "0 = off, 2–3 = organic twist, 8–12 = flowing crown"));
        }
        col_ = col_.child(slider(k, "wavy top rim", d.rim_waves, 0.0..=24.0, "", |s| &mut s.p.design.rim_waves));
        if d.rim_waves > 0 {
            col_ = col_.child(slider(k, "rim wave depth", d.rim_depth, 0.0..=40.0, " mm", |s| &mut s.p.design.rim_depth));
        }
        col_
    }));

    c = c.child(section(k, "Surface texture", open(S_TEXTURE), S_TEXTURE, || {
        let mut col_ = col()
            .gap(4.0)
            .child(segmented(k, &["Relief", "Corrugated"], usize::from(d.texture_mode == TextureMode::Corrugated), |s, i| {
                s.p.design.texture_mode = if i == 0 { TextureMode::Relief } else { TextureMode::Corrugated }
            }))
            .child(slider(k, "ribs / grooves", d.ribs, 0.0..=160.0, "", |s| &mut s.p.design.ribs));
        if d.ribs > 0 {
            col_ = col_
                .child(segmented(k, &["Ridges", "Grooves"], usize::from(d.rib_style == RibStyle::Grooves), |s, i| {
                    s.p.design.rib_style = if i == 0 { RibStyle::Ridges } else { RibStyle::Grooves }
                }))
                .child(slider(k, "depth", d.rib_depth, 0.0..=8.0, " mm", |s| &mut s.p.design.rib_depth))
                .child(slider(k, "sharpness", d.rib_sharpness, 0.2..=8.0, "", |s| &mut s.p.design.rib_sharpness))
                .child(check(k, "follow fold flow", d.ribs_follow_folds, |s| &mut s.p.design.ribs_follow_folds));
            if !d.ribs_follow_folds {
                col_ = col_.child(slider(k, "twist", d.twist_deg, -360.0..=360.0, "°", |s| &mut s.p.design.twist_deg));
            }
            col_ = col_.child(check(k, "cross ribs (lattice)", d.cross_ribs, |s| &mut s.p.design.cross_ribs));
        }
        col_ = col_.child(slider(k, "horizontal waves", d.waves, 0.0..=40.0, "", |s| &mut s.p.design.waves));
        if d.waves > 0 {
            col_ = col_
                .child(slider(k, "wave depth", d.wave_depth, 0.0..=8.0, " mm", |s| &mut s.p.design.wave_depth))
                .child(slider(k, "wave spiral", d.wave_spiral, -12.0..=12.0, "", |s| &mut s.p.design.wave_spiral));
        }
        col_.child(slider(k, "smooth rim band", d.edge_band, 0.0..=30.0, " mm", |s| &mut s.p.design.edge_band))
    }));

    c = c.child(section(k, "Wall & mount", open(S_MOUNT), S_MOUNT, || mount_controls(s, k)));
    c = c.child(section(k, "Material", open(S_MATERIAL), S_MATERIAL, || material_controls(s, k, layer)));
    c = c.child(section(k, "Printer & split", open(S_PRINT), S_PRINT, || print_controls(s, k, layer)));
    c
}

fn mount_controls(s: &State, k: &Kit) -> El {
    let d = &s.p.design;
    let sleeve = d.mount == MountKind::Sleeve;
    let mut c = col()
        .gap(4.0)
        .child(slider(k, "wall thickness", d.wall, 0.4..=4.0, " mm", |s| &mut s.p.design.wall))
        .child(segmented(k, &["Flat ring", "Socket sleeve"], usize::from(sleeve), |s, i| {
            let new = if i == 0 { MountKind::FlatRing } else { MountKind::Sleeve };
            if new == s.p.design.mount {
                return;
            }
            let d = &mut s.p.design;
            d.mount = new;
            match new {
                MountKind::Sleeve => {
                    d.hole_d = s.p.lamp.socket_d + 0.35;
                    d.ring_width = 2.0;
                    d.ledge_h = 2.0;
                }
                MountKind::FlatRing => {
                    d.hole_d = 28.5;
                    d.ring_width = 9.0;
                }
            }
        }));
    if sleeve {
        c = c
            .child(slider(k, "sleeve height", d.sleeve_height, 3.0..=150.0, " mm", |s| &mut s.p.design.sleeve_height))
            .child(slider(k, "sleeve windows", d.sleeve_windows, 0.0..=12.0, "", |s| &mut s.p.design.sleeve_windows));
        if d.sleeve_windows >= 2 {
            c = c
                .child(slider(k, "strut width", d.sleeve_strut_w, 2.0..=15.0, " mm", |s| &mut s.p.design.sleeve_strut_w))
                .child(slider(k, "solid bands", d.sleeve_band, 2.0..=20.0, " mm", |s| &mut s.p.design.sleeve_band))
                .child(faint(k, "Saves weight on tall sleeves: struts between a solid band at the bottom and at the top, with 45° pointed windows (no supports)."));
        }
        c = c.child(check(k, "ledge on socket top", d.ledge, |s| &mut s.p.design.ledge));
        if d.ledge {
            let max_d = (d.hole_d - 1.0).max(21.0);
            c = c
                .child(slider(k, "ledge hole Ø", d.ledge_d, 20.0..=max_d, " mm", |s| &mut s.p.design.ledge_d))
                .child(slider(k, "ledge height", d.ledge_h, 0.8..=6.0, " mm", |s| &mut s.p.design.ledge_h))
                .child(check(k, "45° chamfer underneath", d.ledge_chamfer, |s| &mut s.p.design.ledge_chamfer));
        }
    } else {
        c = c.child(
            row()
                .gap(6.0)
                .child(super::widgets::button(k, "Bulb washer Ø28.5", |s: &mut State| s.p.design.hole_d = 28.5))
                .child(super::widgets::button(k, "E27 shade ring Ø40.5", |s: &mut State| s.p.design.hole_d = 40.5)),
        );
    }
    c = c
        .child(slider(k, "hole Ø", d.hole_d, 20.0..=70.0, " mm", |s| &mut s.p.design.hole_d))
        .child(slider(k, if sleeve { "sleeve wall" } else { "ring width" }, d.ring_width, 1.2..=25.0, " mm", |s| {
            &mut s.p.design.ring_width
        }))
        .child(check(k, "vase mode (spiral vase print)", d.vase_mode, |s| &mut s.p.design.vase_mode));
    if d.vase_mode && sleeve {
        c = c
            .child(slider(k, "glue gap (Ø)", d.glue_gap, 0.0..=1.0, " mm", |s| &mut s.p.design.glue_gap))
            .child(slider(k, "glue flange width", d.glue_ledge_width, 1.0..=10.0, " mm", |s| &mut s.p.design.glue_ledge_width))
            .child(slider(k, "glue flange height", d.glue_ledge_h, 0.6..=4.0, " mm", |s| &mut s.p.design.glue_ledge_h));
    }
    c = c.child(slider(
        k,
        if d.vase_mode { "ring & spokes thickness" } else { "mount thickness" },
        d.mount_thickness,
        0.8..=5.0,
        " mm",
        |s| &mut s.p.design.mount_thickness,
    ));
    if d.vase_mode {
        c = c.child(faint(
            k,
            format!(
                "Bambu Studio: enable Spiral vase and set Bottom shell layers ≥ {} (0.2 mm layers) so the ring and spokes print solid.",
                (d.mount_thickness / 0.2).ceil() as u32
            ),
        ));
    }
    c.child(slider(k, "spokes", d.spokes, 2.0..=8.0, "", |s| &mut s.p.design.spokes))
        .child(slider(k, "spoke width", d.spoke_width, 2.0..=20.0, " mm", |s| &mut s.p.design.spoke_width))
}

fn material_controls(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let p = &s.p;
    let props = p.material.props();
    let names: Vec<String> = MaterialId::ALL.iter().map(|m| m.props().name.to_owned()).collect();
    let mut swatches = row().gap(6.0).flex_wrap();
    for (name, c) in props.palette {
        let selected = p.color == *c;
        let c = *c;
        swatches = swatches.child(
            el().key(format!("sw:{name}"))
                .role("radio")
                .label(*name)
                .checked(selected)
                .focusable(true)
                .size(22.0, 22.0)
                .radius(5.0)
                .fill(srgb8(c))
                .border(if selected { 2.0 } else { 1.0 }, if selected { k.c.text_strong } else { k.c.border_base })
                .help(*name)
                .on_press(move |s: &mut State| s.p.color = c),
        );
    }
    let open = s.ui.sections & S_CUSTOM_COLOR != 0;
    col()
        .gap(6.0)
        .child(select(k, layer, s, "material", &names, props.name, 240.0, |s, v| {
            if let Some(id) = MaterialId::ALL.into_iter().find(|m| m.props().name == v) {
                if id != s.p.material {
                    s.p.material = id;
                    s.p.color = id.props().palette[0].1;
                }
            }
        }))
        .child(faint(k, props.note))
        .child(swatches)
        .child(section(k, "Custom colour", open, S_CUSTOM_COLOR, || {
            let c = s.p.color;
            col()
                .gap(4.0)
                .child(el().size(60.0, 18.0).radius(4.0).fill(srgb8(c)).border(1.0, k.c.border_base))
                .child(slider(k, "red", c[0] as u32, 0.0..=255.0, "", |s| &mut s.ui.custom_rgb[0]))
                .child(slider(k, "green", c[1] as u32, 0.0..=255.0, "", |s| &mut s.ui.custom_rgb[1]))
                .child(slider(k, "blue", c[2] as u32, 0.0..=255.0, "", |s| &mut s.ui.custom_rgb[2]))
        }))
}

fn print_controls(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    use crate::geometry::{split_has_joint, split_heights, split_parts_for};
    let p = &s.p;
    let d = &p.design;
    let printers: Vec<String> = Printer::ALL.iter().map(|p| p.name().to_owned()).collect();
    let (bx, by, bz) = p.bed();
    let mut c = col().gap(4.0).child(select(k, layer, s, "printer", &printers, p.printer.name(), 240.0, |s, v| {
        if let Some(pr) = Printer::ALL.into_iter().find(|p| p.name() == v) {
            if pr == Printer::Custom {
                // start the custom size from the printer picked before
                let (x, y, z) = s.p.bed();
                s.p.custom_bed = [x, y, z];
            }
            s.p.printer = pr;
        }
    }));
    if p.printer == Printer::Custom {
        c = c
            .child(slider(k, "bed X", p.custom_bed[0], 100.0..=500.0, " mm", |s| &mut s.p.custom_bed[0]))
            .child(slider(k, "bed Y", p.custom_bed[1], 100.0..=500.0, " mm", |s| &mut s.p.custom_bed[1]))
            .child(slider(k, "build height", p.custom_bed[2], 100.0..=500.0, " mm", |s| &mut s.p.custom_bed[2]));
    } else {
        c = c.child(faint(k, format!("Build volume {bx:.0} × {by:.0} × {bz:.0} mm")));
    }
    let auto = split_parts_for(d, bz);
    c = c.child(slider(k, "split into parts", d.split_parts, 1.0..=8.0, "", |s| &mut s.p.design.split_parts)).child(
        row()
            .gap(6.0)
            .child(super::widgets::button(k, "Fit printer height", |s: &mut State| {
                if let Some(n) = split_parts_for(&s.p.design, s.p.bed().2) {
                    s.p.design.split_parts = n;
                }
            }))
            .child(faint(
                k,
                match auto {
                    Some(1) => "fits in one piece".to_string(),
                    Some(n) => format!("needs {n} parts"),
                    None => "too tall even in 8 parts".to_string(),
                },
            )),
    );
    if d.split_parts > 1 {
        c = c.child(check(k, "overlap joint (rabbet)", d.split_joint, |s| &mut s.p.design.split_joint));
        if split_has_joint(d) {
            c = c
                .child(slider(k, "overlap height", d.split_overlap, 2.0..=20.0, " mm", |s| &mut s.p.design.split_overlap))
                .child(slider(k, "joint gap", d.split_gap, 0.0..=0.6, " mm", |s| &mut s.p.design.split_gap));
        } else if d.split_joint && d.vase_mode {
            c = c.child(faint(k, "Vase mode prints a single-line wall, so the parts get flat (butt) joints."));
        }
        let hs: Vec<String> = split_heights(d).iter().map(|h| format!("{h:.0}")).collect();
        c = c.child(faint(
            k,
            format!(
                "Even cuts, part heights {} mm. Export writes part 1 (with the mount) to the chosen file and the others as -part2.3mf, -part3.3mf, … Print each upright, glue bottom to top.",
                hs.join(" / ")
            ),
        ));
    }
    c
}
