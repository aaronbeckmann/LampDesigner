//! Shared controls. Rune views are plain functions of the state, and every
//! handler gets `&mut State`, so a control that edits a field takes a small
//! accessor (`fn(&mut State) -> &mut T`) to reach it.

use super::State;
use rune::{col, el, row, text, Color, Drag, El, Phase};
use rune_kit::{Checkbox, Collapsible, Kit};
use std::ops::RangeInclusive;

pub const LABEL_W: f32 = 118.0;
pub const TRACK_W: f32 = 120.0;
pub const VALUE_W: f32 = 62.0;
const TRACK_H: f32 = 20.0;
const KNOB: f32 = 14.0;
const BAR: f32 = 4.0;

pub fn kit(dark: bool) -> &'static Kit {
    static D: std::sync::OnceLock<Kit> = std::sync::OnceLock::new();
    static L: std::sync::OnceLock<Kit> = std::sync::OnceLock::new();
    if dark { D.get_or_init(|| Kit::neutral(true)) } else { L.get_or_init(|| Kit::neutral(false)) }
}

/// sRGB bytes → Rune's linear colour.
pub fn srgb8(c: [u8; 3]) -> Color {
    use crate::material::srgb_to_linear as l;
    Color { r: l(c[0]), g: l(c[1]), b: l(c[2]), a: 1.0 }
}

/// Display sRGB floats (0..1) → Rune's linear colour.
pub fn srgbf(c: [f32; 3]) -> Color {
    let l = |v: f32| rune::color::srgb_decode(v.clamp(0.0, 1.0));
    Color { r: l(c[0]), g: l(c[1]), b: l(c[2]), a: 1.0 }
}

pub fn label(k: &Kit, s: impl Into<String>) -> El {
    text(s.into()).font(k.ty.text_small).color(k.c.text_base)
}

pub fn faint(k: &Kit, s: impl Into<String>) -> El {
    text(s.into()).font(k.ty.small).color(k.c.text_faint).wrap()
}

pub fn strong(k: &Kit, s: impl Into<String>) -> El {
    text(s.into()).font(k.ty.text_small_medium).color(k.c.text_strong)
}

/// A value a slider can edit.
pub trait SliderValue: Copy + 'static {
    fn to_f32(self) -> f32;
    fn from_f32(v: f32) -> Self;
    const INTEGER: bool;
}

impl SliderValue for f32 {
    fn to_f32(self) -> f32 {
        self
    }
    fn from_f32(v: f32) -> Self {
        v
    }
    const INTEGER: bool = false;
}

impl SliderValue for u32 {
    fn to_f32(self) -> f32 {
        self as f32
    }
    fn from_f32(v: f32) -> Self {
        v.round().max(0.0) as u32
    }
    const INTEGER: bool = true;
}

impl SliderValue for i32 {
    fn to_f32(self) -> f32 {
        self as f32
    }
    fn from_f32(v: f32) -> Self {
        v.round() as i32
    }
    const INTEGER: bool = true;
}

/// Decimals that suit a range: 2 below a span of 2, 1 below 20, else 0.
fn decimals<T: SliderValue>(lo: f32, hi: f32) -> usize {
    if T::INTEGER {
        0
    } else if hi - lo <= 2.0 {
        2
    } else if hi - lo <= 20.0 {
        1
    } else {
        0
    }
}

/// Label, track and value on one row. `get` reaches the edited field.
pub fn slider<T: SliderValue>(
    k: &Kit,
    name: &str,
    value: T,
    range: RangeInclusive<f32>,
    unit: &str,
    get: fn(&mut State) -> &mut T,
) -> El {
    let (lo, hi) = (*range.start(), *range.end());
    let v = value.to_f32();
    let dec = decimals::<T>(lo, hi);
    let step = if T::INTEGER { 1.0 } else { ((hi - lo) / 100.0).max(10f32.powi(-(dec as i32))) };
    let travel = TRACK_W - KNOB;
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let to_value = move |x: f32| lo + ((x - KNOB / 2.0) / travel).clamp(0.0, 1.0) * (hi - lo);
    let set = move |s: &mut State, x: f32| {
        *get(s) = T::from_f32(x.clamp(lo, hi));
        s.touch();
    };
    let key = format!("sl:{name}");
    let track = el()
        .key(key)
        .role("slider")
        .label(name.to_owned())
        .range_value(f64::from(v), f64::from(lo), f64::from(hi))
        .range_step(f64::from(step))
        .focusable(true)
        .size(TRACK_W, TRACK_H)
        .on_step(move |s: &mut State, steps, _| {
            let now = get(s).to_f32();
            set(s, now + steps as f32 * step);
        })
        .on_drag(move |s: &mut State, d: &mut Drag, tx| match d.phase {
            Phase::Down => {
                d.data = get(s).to_f32();
                set(s, to_value(d.x));
            }
            Phase::Move => {
                set(s, to_value(d.x));
                tx.instant();
            }
            Phase::Up => {
                if d.cancelled() {
                    set(s, d.data);
                }
            }
        })
        .child(
            el().key("track")
                .size(TRACK_W, BAR)
                .absolute(0.0, (TRACK_H - BAR) / 2.0)
                .radius(BAR / 2.0)
                .fill(k.c.border_base)
                .a11y_hidden(),
        )
        .child(
            el().key("fill")
                .size(KNOB / 2.0 + t * travel, BAR)
                .absolute(0.0, (TRACK_H - BAR) / 2.0)
                .radius(BAR / 2.0)
                .fill(k.c.bg_accent)
                .a11y_hidden(),
        )
        .child(
            el().key("knob")
                .size(KNOB, KNOB)
                .absolute(t * travel, (TRACK_H - KNOB) / 2.0)
                .radius(KNOB / 2.0)
                .fill(k.c.knob_on)
                .border(1.0, k.c.border_strong)
                .a11y_hidden(),
        );
    row()
        .gap(8.0)
        .center_y()
        .child(label(k, name).w(LABEL_W).truncate())
        .child(track)
        .child(text(format!("{v:.dec$}{unit}")).font(k.ty.text_small).color(k.c.text_muted).w(VALUE_W).truncate())
}

/// A checkbox bound to a bool field.
pub fn check(k: &Kit, name: &str, value: bool, get: fn(&mut State) -> &mut bool) -> El {
    Checkbox::new(value)
        .label(name.to_owned())
        .on_toggle(move |s: &mut State| {
            let b = get(s);
            *b = !*b;
            s.touch();
        })
        .build(k)
}

/// A collapsible panel section; `bit` is its flag in `State::open_sections`.
pub fn section(k: &Kit, title: &str, open: bool, bit: u32, content: impl FnOnce() -> El) -> El {
    let mut c = Collapsible::new(title.to_owned(), open).on_toggle(move |s: &mut State| s.ui.sections ^= bit);
    if open {
        c = c.child(col().w_full().gap(6.0).pb(8.0).child(content()));
    }
    c.build(k)
}

/// A horizontal colour scale with end labels.
pub fn legend(k: &Kit, lo: &str, hi: &str) -> El {
    let stops: Vec<(f32, Color)> = (0..8)
        .map(|i| {
            let t = i as f32 / 7.0;
            let c = crate::render::turbo(t);
            (t, srgbf([c.x, c.y, c.z]))
        })
        .collect();
    row()
        .gap(8.0)
        .center_y()
        .child(faint(k, lo))
        .child(el().size(200.0, 10.0).radius(3.0).linear_gradient(std::f32::consts::FRAC_PI_2, stops))
        .child(faint(k, hi))
}

/// A drop-down select. Its open state lives in `State::ui.open_select`; the
/// menu draws into the overlay `layer`.
#[allow(clippy::too_many_arguments)]
pub fn select(
    k: &Kit,
    layer: &mut Vec<El>,
    s: &State,
    key: &'static str,
    options: &[String],
    current: &str,
    width: f32,
    choose: fn(&mut State, &str),
) -> El {
    rune_kit::Select::new(key, options)
        .current(Some(current))
        .width(width)
        .open(s.ui.open_select == Some(key))
        .on_toggle(move |s: &mut State| s.ui.open_select = if s.ui.open_select == Some(key) { None } else { Some(key) })
        .on_close(|s: &mut State| s.ui.open_select = None)
        .on_choose(move |s: &mut State, v: &str, _: &mut rune::Tx| {
            choose(s, v);
            s.ui.open_select = None;
            s.touch();
        })
        .build(k, layer)
}

/// A segmented control picking one of `labels`.
pub fn segmented(k: &Kit, labels: &[&str], selected: usize, pick: fn(&mut State, usize)) -> El {
    rune_kit::SegmentedControl::new(labels, Some(selected))
        .fit()
        .on_pick(move |s: &mut State, i: usize, _: &mut rune::Tx| {
            pick(s, i);
            s.touch();
        })
        .build(k)
}

/// A small push button.
pub fn button(k: &Kit, name: &str, action: impl Fn(&mut State) + 'static) -> El {
    rune_kit::Button::new(name.to_owned()).control_size(rune_kit::ControlSize::Small).on_press(action).build(k)
}

/// A small push button whose action also gets the transaction (file dialogs).
pub fn button_tx(k: &Kit, name: &str, action: impl Fn(&mut State, &mut rune::Tx) + 'static) -> El {
    rune_kit::Button::new(name.to_owned()).control_size(rune_kit::ControlSize::Small).on_press_tx(action).build(k)
}
