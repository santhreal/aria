//! A value's track: a rail, the stretch of it the value covers, and a thumb
//! where the value stands.
//!
//! Every control that slides a value along a bounded travel draws through
//! [`track`]: an Effect Controls row, a mixer strip's pan and fader, and a
//! plugin panel's slider. The covered stretch runs from the travel's origin,
//! [`crate::types::Taper::origin`], so a value on a travel through zero fills
//! from zero to the value and a centered pan fills nothing.

use gpui::{Div, Hsla, InteractiveElement, ParentElement, Styled, div, px, relative};

use crate::types::Reach;

/// Which way a track runs from its start to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    /// Left to right.
    Across,
    /// Bottom to top, as a fader runs.
    Up,
}

/// A track's thickness: its rail's, and its thumb's diameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gauge {
    pub rail: f32,
    pub thumb: f32,
}

impl Gauge {
    /// Under an Effect Controls row and in a plugin panel.
    pub const ROW: Self = Self {
        rail: 3.0,
        thumb: 7.0,
    };
    /// A mixer strip's pan and fader.
    pub const STRIP: Self = Self {
        rail: 4.0,
        thumb: 11.0,
    };
}

/// The colors a track is drawn in: its rail, and its fill and thumb.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink {
    pub rail: Hsla,
    pub lit: Hsla,
}

/// A rail along `run`, the stretch of it `reach` covers, and the thumb where
/// the value stands. The track is the thumb's diameter thick across `run`; the
/// caller sets its length. `fill_selector` is the covered stretch's debug
/// selector.
pub fn track(
    reach: Reach,
    run: Run,
    gauge: Gauge,
    ink: Ink,
    fill_selector: impl FnOnce() -> String,
) -> Div {
    let inset = px((gauge.thumb - gauge.rail) / 2.0);
    let rail = px(gauge.rail);
    let thumb = px(gauge.thumb);
    // Centered on the value rather than starting at it, so the thumb still
    // marks the value at both ends of the travel.
    let hang = px(-gauge.thumb / 2.0);
    let bar = || div().absolute().rounded(px(gauge.rail / 2.0));
    let knob = div()
        .absolute()
        .size(thumb)
        .rounded(px(gauge.thumb / 2.0))
        .bg(ink.lit);
    match run {
        Run::Across => div()
            .relative()
            .h(thumb)
            .child(bar().top(inset).w_full().h(rail).bg(ink.rail))
            .child(
                bar()
                    .top(inset)
                    .left(relative(reach.start()))
                    .w(relative(reach.length()))
                    .h(rail)
                    .bg(ink.lit)
                    .debug_selector(fill_selector),
            )
            .child(knob.top_0().left(relative(reach.at)).ml(hang)),
        Run::Up => div()
            .relative()
            .w(thumb)
            .child(bar().left(inset).h_full().w(rail).bg(ink.rail))
            .child(
                bar()
                    .left(inset)
                    .bottom(relative(reach.start()))
                    .h(relative(reach.length()))
                    .w(rail)
                    .bg(ink.lit)
                    .debug_selector(fill_selector),
            )
            .child(knob.left_0().bottom(relative(reach.at)).mb(hang)),
    }
}
