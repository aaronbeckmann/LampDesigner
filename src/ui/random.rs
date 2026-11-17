//! Random design generator: state, logic and panel.

use super::widgets::{button, check, faint, label, select, slider};
use super::State;
use crate::design::Design;
use crate::project::Project;
use crate::random::{self, GenOptions, Theme};
use rune::{col, row, text, El};
use rune_kit::Kit;
use std::collections::VecDeque;

const HISTORY: usize = 50;

pub struct RandomGen {
    pub theme: Theme,
    pub seed: u64,
    pub pick_color: bool,
    pub opts: GenOptions,
    /// Slider values, kept while "fixed height/width" is unticked.
    pub fixed_height: bool,
    pub height: f32,
    pub fixed_width: bool,
    pub width: f32,
    pub msg: String,
    /// Previous designs for "◀ Back" (oldest dropped first).
    history: VecDeque<(Design, [u8; 3])>,
}

impl Default for RandomGen {
    fn default() -> Self {
        RandomGen {
            theme: Theme::Surprise,
            seed: 1,
            pick_color: false,
            opts: GenOptions::default(),
            fixed_height: false,
            height: 150.0,
            fixed_width: false,
            width: 140.0,
            msg: String::new(),
            history: VecDeque::new(),
        }
    }
}

impl RandomGen {
    /// The constraints as the generator takes them.
    fn options(&self) -> GenOptions {
        GenOptions {
            height: self.fixed_height.then_some(self.height),
            width: self.fixed_width.then_some(self.width),
            ..self.opts
        }
    }

    pub fn back(&mut self, p: &mut Project) {
        if let Some((d, c)) = self.history.pop_back() {
            p.design = d;
            p.color = c;
            self.msg = "Restored previous design".into();
        }
    }

    pub fn generate(&mut self, seed: u64, p: &mut Project) {
        self.seed = seed;
        let g = random::generate(self.theme, seed, p, &self.options());
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        // the generated design replaces the current one: move it into the history
        self.history.push_back((std::mem::replace(&mut p.design, g.design), p.color));
        if self.pick_color {
            // skip near-black colours: they block most of the light
            let usable: Vec<[u8; 3]> = p
                .material
                .props()
                .palette
                .iter()
                .map(|c| c.1)
                .filter(|c| c.iter().map(|&v| v as u32).sum::<u32>() > 120)
                .collect();
            if !usable.is_empty() {
                p.color = usable[(seed as usize * 7 + 3) % usable.len()];
            }
        }
        self.msg = if g.problems.is_empty() {
            format!("#{seed}: printable design ({} {})", g.tries, if g.tries == 1 { "try" } else { "tries" })
        } else {
            format!("#{seed}: best of {} tries, still: {}", g.tries, g.problems.join(", "))
        };
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }
}

/// Seed from the clock for "Generate".
fn time_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        % 1_000_000
}

fn generate(s: &mut State, seed: u64) {
    let State { random, p, .. } = s;
    random.generate(seed, p);
    s.touch();
}

pub fn panel(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let g = &s.random;
    let themes: Vec<String> = Theme::ALL.iter().map(|t| t.name().to_owned()).collect();
    let mut c = col()
        .gap(6.0)
        .child(select(k, layer, s, "theme", &themes, g.theme.name(), 220.0, |s, v| {
            if let Some(t) = Theme::ALL.into_iter().find(|t| t.name() == v) {
                s.random.theme = t;
            }
        }))
        .child(faint(k, g.theme.hint()))
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    rune_kit::Button::new("🎲 Generate")
                        .on_press(|s: &mut State| generate(s, time_seed()))
                        .build(k),
                )
                .child(
                    rune_kit::Button::new("◀ Back")
                        .control_size(rune_kit::ControlSize::Small)
                        .disabled(!g.can_go_back())
                        .help("Restore the previous design")
                        .on_press(|s: &mut State| {
                            let State { random, p, .. } = s;
                            random.back(p);
                        })
                        .build(k),
                ),
        )
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(label(k, "seed"))
                .child(text(format!("{}", g.seed)).font(k.ty.text_small_medium).color(k.c.text_strong).w(60.0))
                .child(button(k, "◀", |s: &mut State| s.random.seed = s.random.seed.saturating_sub(1)))
                .child(button(k, "▶", |s: &mut State| s.random.seed = (s.random.seed + 1) % 1_000_000))
                .child(button(k, "Use seed", |s: &mut State| {
                    let seed = s.random.seed;
                    generate(s, seed);
                })),
        )
        .child(slider(k, "max overhang", g.opts.max_overhang, 20.0..=60.0, "°", |s| &mut s.random.opts.max_overhang))
        // ticking a size constraint starts from the current design's size
        .child(
            rune_kit::Checkbox::new(g.fixed_height)
                .label("fixed height")
                .on_toggle(|s: &mut State| {
                    s.random.fixed_height = !s.random.fixed_height;
                    if s.random.fixed_height {
                        s.random.height = s.p.design.height;
                    }
                })
                .build(k),
        );
    if g.fixed_height {
        c = c.child(slider(k, "height", g.height, 60.0..=600.0, " mm", |s| &mut s.random.height));
    }
    c = c.child(
        rune_kit::Checkbox::new(g.fixed_width)
            .label("fixed width (largest Ø)")
            .on_toggle(|s: &mut State| {
                s.random.fixed_width = !s.random.fixed_width;
                if s.random.fixed_width {
                    s.random.width = (2.0 * s.scene.max_r).round();
                }
            })
            .build(k),
    );
    if g.fixed_width {
        c = c
            .child(slider(k, "width Ø", g.width, 80.0..=300.0, " mm", |s| &mut s.random.width))
            .child(faint(k, "The bulb needs 10 mm of air all round: with a 60 mm bulb, about Ø 110 mm or more works for every theme."));
    }
    c = c.child(check(k, "also pick a filament colour", g.pick_color, |s| &mut s.random.pick_color));
    if !g.msg.is_empty() {
        c = c.child(faint(k, g.msg.clone()));
    }
    c
}
