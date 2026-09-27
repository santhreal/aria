//! Loaders: the pulse loader, the gradient matrix spinner, the progress bar, the
//! delay before any of them appears, and the boot splash.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use gpui::{
    AnyElement, App, EntityId, InteractiveElement, IntoElement, ParentElement, SharedString,
    Styled, div, px,
};

use gpui::motion::loaders::{
    self, GSPIN_DIM, GSPIN_ROW_TINTS, MARK_CELLS, MATRIX_SIDE, MINI_RING, MINI_RING_LEN,
    PULSE_STAGGER, ZERON_CELLS,
};
use gpui::motion::presets::{GRADIENT_SPIN, ZERON_PULSE};

use crate::motion;
use crate::theme::Theme;

// The progress bar. A bar says how far, a loader says that something is
// happening, and a wait that knows both draws both.
#[path = "loaders_progress.rs"]
mod progress;
pub use progress::{progress_bar, progress_row};
// When a loader may appear at all. A wait shorter than the span leaves no trace.
#[path = "loaders_delay.rs"]
mod delay;
pub use delay::worth_showing;

/// The pulsing block a panel shows while it waits. Painted in the busy colour,
/// which exists to say "something is running" without saying it failed.
///
/// `id` offsets the phase, so two panels waiting at once do not pulse as one.
pub fn pulse_loader(
    id: &'static str,
    theme: &Theme,
    cell_px: f32,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let color = theme.busy;
    let slot = cell_px;
    let delta = motion::pulse_delta(&ZERON_PULSE, view, cx) + motion::key_phase(id);
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(slot / 2.0))
        .children((0..ZERON_CELLS).map(move |i| {
            div()
                .size(px(slot))
                .flex()
                .items_center()
                .justify_center()
                .child({
                    let phase = loaders::staggered_phase(delta, i, PULSE_STAGGER);
                    div()
                        .rounded(px(slot / 4.0))
                        .bg(color)
                        .opacity(loaders::pulse_opacity(phase))
                        .size(px(slot * loaders::pulse_scale(phase)))
                })
        }))
}

/// The gradient matrix, for a wait with room for it.
///
/// `id` offsets the phase, as everywhere else here.
pub fn gradient_spinner(
    id: &'static str,
    _theme: &Theme,
    cell_px: f32,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let delta = motion::pulse_delta(&GRADIENT_SPIN, view, cx) + motion::key_phase(id);
    div()
        .flex()
        .flex_col()
        .gap(px(cell_px / 2.0))
        .children((0..MATRIX_SIDE).map(move |row| {
            let tint: gpui::Hsla = gpui::rgb(GSPIN_ROW_TINTS[row]).into();
            div()
                .flex()
                .flex_row()
                .gap(px(cell_px / 2.0))
                .children((0..MATRIX_SIDE).map(move |col| {
                    let phase = loaders::gspin_cell_phase(row, col);
                    div()
                        .size(px(cell_px))
                        .rounded(px(cell_px / 2.0))
                        .bg(tint)
                        .opacity(loaders::gspin_opacity(delta + phase, GSPIN_DIM))
                }))
        }))
}

pub fn mini_gradient_spinner(
    id: impl Into<motion::ControlId>,
    cell_px: f32,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    // The ring order and its length live with the rest of the pulse arithmetic:
    // a second copy here is a second owner of which cell lights when.
    const ROWS: usize = MINI_RING.len();
    const COLS: usize = MINI_RING[0].len();
    let id = id.into();
    let delta = motion::pulse_delta(&GRADIENT_SPIN, view, cx) + id.phase();
    div()
        .id(id.element("mini-spinner"))
        .flex()
        .flex_col()
        .gap(px(cell_px / 2.0))
        .children((0..ROWS).map(move |row| {
            let tint: gpui::Hsla = gpui::rgb(GSPIN_ROW_TINTS[row]).into();
            div()
                .flex()
                .flex_row()
                .gap(px(cell_px / 2.0))
                .children((0..COLS).map(move |col| {
                    let phase = MINI_RING[row][col] as f32 / MINI_RING_LEN;
                    div()
                        .size(px(cell_px))
                        .rounded(px(cell_px / 2.0))
                        .bg(tint)
                        .opacity(loaders::gspin_opacity(delta + phase, GSPIN_DIM))
                }))
        }))
}

/// A line that says something is still happening, with a loader that proves it.
///
/// Every pending surface draws through this rather than suffixing a label with
/// an ellipsis: a string that stopped updating and a string that is still
/// working look identical, which is the failure the loaders exist to prevent.
/// `id` must be unique per row: the spinner takes its phase from it, so two
/// rows sharing an id pulse as one animation drawn twice, and the same id is
/// what ages the wait.
///
/// The spinner waits for [`worth_showing`] before it appears, and its width is
/// held either way, so a load that resolves inside a settle shows a line of text
/// that never moved rather than a spinner that flashed.
pub fn pending_row(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let id = id.into();
    let text = label.into();
    let color = theme.text_muted;
    // The spinner's own geometry, so a change to the ring cannot leave a gap the
    // wrong width: the row must not reflow on the frame the spinner arrives.
    const CELL: f32 = 3.0;
    const COLS: f32 = MINI_RING[0].len() as f32;
    const LEADER_W: f32 = COLS * CELL + (COLS - 1.0) * CELL / 2.0;
    let leader = if worth_showing(id) {
        mini_gradient_spinner(id, CELL, view, cx).into_any_element()
    } else {
        div().w(px(LEADER_W)).into_any_element()
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(leader)
        .child(div().text_size(px(12.0)).text_color(color).child(text))
}

/// The cell-grid pulse, at whatever height the caller has room for.
///
/// A loader rather than a brand mark: the shipped mark is raster art drawn by
/// [`crate::icons::brand_mark`], and this is what a surface shows while it is
/// still coming up. `id` offsets the phase.
pub fn aria_mark_loader(
    id: &'static str,
    theme: &Theme,
    height_px: f32,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let color = theme.text;
    let scale = height_px / 940.0;
    let cell = 100.0 * scale;
    let delta = motion::pulse_delta(&ZERON_PULSE, view, cx) + motion::key_phase(id);
    gpui::canvas(
        |_, _, _| (),
        move |bounds: gpui::Bounds<gpui::Pixels>, _, window, _| {
            let origin_x = f32::from(bounds.origin.x);
            let origin_y = f32::from(bounds.origin.y);
            for &(x, y) in &MARK_CELLS {
                let phase = loaders::mark_phase(delta, x, y);
                let side = cell * loaders::pulse_scale(phase);
                let inset = (cell - side) * 0.5;
                let bounds = gpui::Bounds {
                    origin: gpui::point(
                        px(origin_x + x * scale + inset),
                        px(origin_y + y * scale + inset),
                    ),
                    size: gpui::size(px(side), px(side)),
                };
                window.paint_quad(gpui::quad(
                    bounds,
                    px(16.0 * scale),
                    color.opacity(loaders::pulse_opacity(phase)),
                    0.0,
                    color,
                    gpui::BorderStyle::default(),
                ));
            }
        },
    )
    .w(px(820.0 * scale))
    .h(px(height_px))
}

pub fn splash_overlay(theme: &Theme, fading: bool) -> AnyElement {
    let content = div()
        .absolute()
        .inset_0()
        .bg(theme.glass())
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(28.0))
        .child(hero_wordmark(theme))
        .child(loading_word(theme));
    if fading {
        motion::splash_out("boot-splash-out", content).into_any_element()
    } else {
        content.into_any_element()
    }
}

/// The mark over the word, as the boot splash and nothing else draws it.
///
/// The mark is the shipped art rather than the pulse loader beside it: the
/// splash is the first frame of a session, and the product's own mark is what
/// it should be showing while the window comes up.
fn hero_wordmark(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .child(crate::icons::brand_mark(72.0))
        .child(
            div()
                .text_size(px(28.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(SharedString::from("Aria")),
        )
        .into_any_element()
}

pub fn loading_word(theme: &Theme) -> impl IntoElement {
    div()
        .text_size(px(11.0))
        .text_color(theme.text_muted.opacity(0.7))
        .child(SharedString::from(
            "L\u{2009}O\u{2009}A\u{2009}D\u{2009}I\u{2009}N\u{2009}G",
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two loaders on screen must not pulse as one, and one loader must not
    /// jump between frames. Both are properties of `key_phase` alone, so they
    /// are asserted here rather than through an element tree: the keys come
    /// from the shipped call sites, and every distinct key has to land on a
    /// distinct phase inside the unit interval.
    #[test]
    fn a_loader_key_decides_its_phase_and_keeps_it() {
        let keys = [
            "ld-mark-sm",
            "ld-mark",
            "pending-import",
            "pending-export",
            "pending-analysis",
        ];
        let mut seen: Vec<f32> = Vec::new();
        for key in keys {
            let phase = motion::key_phase(key);
            assert!((0.0..1.0).contains(&phase), "{key} -> {phase}");
            assert_eq!(phase, motion::key_phase(key), "{key} moved between frames");
            assert!(
                seen.iter().all(|other| (other - phase).abs() > 1e-6),
                "{key} shares a phase with another loader on screen"
            );
            seen.push(phase);
        }
    }
}
