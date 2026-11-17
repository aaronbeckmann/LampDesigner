//! Light simulation as seen by the UI: an instant direct-light result plus
//! photon tracing in the background.

use super::job::{Job, Wake};
use crate::bulb::Bulb;
use crate::material::Optics;
use crate::quality::Quality;
use crate::scene::Scene;
use crate::sim::{self, SimResult};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct SimState {
    /// Latest result (shared with render jobs).
    pub result: Arc<SimResult>,
    /// `result` includes the photon-traced indirect light.
    pub full: bool,
    /// User switch: off = no simulation at all (shape-only work).
    pub enabled: bool,
    job: Option<Job<SimResult>>,
}

impl SimState {
    pub fn new(wake: &Wake, scene: &Arc<Scene>, optics: Optics, bulb: &Bulb, q: Quality) -> SimState {
        let mut s = SimState { result: Arc::new(SimResult::empty(scene)), full: false, enabled: true, job: None };
        s.restart(wake, scene, optics, bulb, q);
        s
    }

    /// Recomputes for a changed scene/material/bulb: direct light right away,
    /// photons in the background. The new job replaces (and so cancels) a
    /// run that is still in progress.
    pub fn restart(&mut self, wake: &Wake, scene: &Arc<Scene>, optics: Optics, bulb: &Bulb, q: Quality) {
        if !self.enabled {
            self.job = None;
            self.result = Arc::new(SimResult::empty(scene));
            self.full = true;
            return;
        }
        let direct = sim::run(scene, &optics, bulb, 0, q.map_res(), &AtomicBool::new(false));
        self.result = Arc::new(direct.expect("the direct-light pass is never cancelled"));
        self.full = false;
        let (scene, bulb) = (Arc::clone(scene), bulb.clone());
        self.job = Some(Job::spawn(wake, move |cancel| sim::run(&scene, &optics, &bulb, q.photons(), q.map_res(), cancel)));
    }

    /// True when a photon-traced result just arrived.
    pub fn poll(&mut self) -> bool {
        match Job::poll(&mut self.job) {
            Some(r) => {
                self.result = Arc::new(r);
                self.full = true;
                true
            }
            None => false,
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }
}
