//! "Compare materials": the same design simulated and rendered for every
//! filament with one shared exposure (CPU ray tracer, on a worker thread).

use super::job::{Job, Wake};
use super::widgets::{faint, label, strong};
use super::State;
use crate::material::{MaterialId, Optics};
use crate::project::Project;
use crate::quality::Quality;
use crate::render::{self, Camera, RenderSettings, ViewMode};
use crate::scene::Scene;
use crate::sim::{self, Stats};
use rune::{col, el, row, El};
use rune_kit::Kit;
use std::sync::Arc;

const COMPARE_PHOTONS: usize = 300_000;
const THUMB: (usize, usize) = (520, 640);

struct Rendered {
    id: MaterialId,
    stats: Stats,
    rgba: Vec<u8>,
}

#[derive(Default)]
pub struct Compare {
    pub open: bool,
    job: Option<Job<Vec<Rendered>>>,
    items: Vec<(MaterialId, Stats, rune::Pixels)>,
}

impl Compare {
    /// Opens the window and (re)starts the comparison; a running one is cancelled.
    pub fn start(&mut self, wake: &Wake, p: &Project, scene: &Arc<Scene>, cam: Camera, rs: RenderSettings, q: Quality) {
        self.open = true;
        self.items.clear();
        let (p, scene) = (p.clone(), Arc::clone(scene));
        let rs = RenderSettings { mode: ViewMode::Night, samples: q.samples().max(2), clip: None, ..rs };
        self.job = Some(Job::spawn(wake, move |cancel| {
            let mut out = Vec::new();
            // one shared exposure so brightness is comparable between materials
            let mut exposure = None;
            let photons = COMPARE_PHOTONS.max(q.photons() / 2);
            for id in MaterialId::ALL {
                let color = if id == p.material { p.color } else { id.props().palette[0].1 };
                let op = Optics::new(id, color);
                let r = sim::run(&scene, &op, &p.bulb, photons, q.map_res(), cancel)?;
                let f = render::render(&scene, &r, &op, &p.bulb, id.props().hdt, &cam, &rs, THUMB.0, THUMB.1, exposure);
                exposure.get_or_insert(f.exposure);
                out.push(Rendered { id, stats: r.stats.clone(), rgba: f.rgba });
            }
            Some(out)
        }));
    }

    /// Picks up a finished comparison. True when it arrived.
    pub fn poll(&mut self) -> bool {
        let Some(rendered) = Job::poll(&mut self.job) else { return false };
        self.items = rendered
            .into_iter()
            .map(|r| {
                let px = rune::Pixels::new();
                px.set(THUMB.0 as u32, THUMB.1 as u32, r.rgba);
                (r.id, r.stats, px)
            })
            .collect();
        true
    }

    fn close(&mut self) {
        self.open = false;
        // closing cancels a comparison that is still running
        self.job = None;
    }
}

pub fn dialog(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let c = &s.compare;
    if !c.open {
        return;
    }
    let body = if c.job.is_some() {
        row().gap(8.0).center_y().child(rune_kit::Spinner::new().build(k)).child(label(k, "Simulating all materials…"))
    } else {
        let mut r = row().gap(12.0);
        for (id, st, px) in &c.items {
            let id = *id;
            r = r.child(
                col()
                    .gap(3.0)
                    .w(260.0)
                    .child(strong(k, id.props().name))
                    .child(el().size(260.0, 320.0).radius(6.0).pixels(px))
                    .child(faint(k, format!("Output: {:.0} %", st.efficiency * 100.0)))
                    .child(faint(k, format!("Glow: {:.0} cd/m²", st.glow_mean)))
                    .child(faint(k, format!("Bulb through wall: {:.0} cd/m²", st.bulb_through)))
                    .child(faint(k, format!("Table @25 cm: {:.0} lx", st.table_lux_25)))
                    .child(faint(k, format!("Max wall: {:.0} °C (HDT {:.0})", st.max_temp, id.props().hdt)))
                    .child(super::widgets::button(k, "Use this material", move |s: &mut State| {
                        s.p.material = id;
                        s.p.color = id.props().palette[0].1;
                    })),
            );
        }
        r
    };
    rune_kit::Dialog::new("compare-dialog", "Material comparison (same design, bulb and exposure)")
        .open(true)
        .width(1140.0)
        .fit()
        .on_close(|s: &mut State| s.compare.close())
        .child(col().w_full().px(16.0).pb(16.0).child(body))
        .build(k, layer);
}
