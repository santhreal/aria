//! How the video and audio lane groups share the stack.
//!
//! With lanes on both sides the A/V split sets each side's share. A side with
//! no lanes is one empty-lane strip, `EMPTY_SIDE_H` tall, that states the
//! missing side; the side with lanes takes the rest of the stack, so no band of
//! the panel is left without lanes or a label.
//!
//! A side's frame is drawn at the height [`Side::viewport`] gives its lane
//! group, so the group places its lanes in the box it is drawn in: the video
//! lanes stand on the divider at every panel height.

use gpui::{Div, IntoElement, ParentElement, Styled, div, px};

use super::EMPTY_SIDE_H;
use crate::theme::Theme;

pub(super) use crate::app::property_rows::DEFAULT_PANEL_H;
#[cfg(test)]
pub(super) use crate::app::property_rows::{MIN_SHARE, Side, sides};

/// The box a side's lane group draws in, `viewport` high: the height
/// [`Side::viewport`] gave the group.
pub(super) fn frame(viewport: f32) -> Div {
    div()
        .w_full()
        .h(px(viewport))
        .flex_none()
        .relative()
        .overflow_hidden()
}

/// The label over an empty side's outline column, on the track names' edge.
pub(super) fn empty_label(theme: &Theme, is_audio: bool) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left(px(Theme::SPACE_SM))
        .h(px(EMPTY_SIDE_H))
        .flex()
        .items_center()
        .text_size(Theme::TEXT_SMALL)
        .text_color(theme.text_faint)
        .child(if is_audio {
            "No audio tracks"
        } else {
            "No video tracks"
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A side with no lanes is the strip and never a share of the stack,
    /// whatever the split; with lanes on both sides the split holds.
    #[test]
    fn a_side_without_lanes_is_one_strip() {
        for split in [0.0, 0.15, 0.5, 0.85, 1.0] {
            assert_eq!(sides(2, 0, split), (Side::Rest, Side::Empty));
            assert_eq!(sides(0, 3, split), (Side::Empty, Side::Rest));
            let (video, audio) = sides(1, 1, split);
            let Side::Share(v) = video else {
                panic!("video takes a share with lanes on both sides");
            };
            assert_eq!(audio, Side::Share(1.0 - v));
            assert!((MIN_SHARE..=1.0 - MIN_SHARE).contains(&v), "{v}");
        }
        assert_eq!(Side::Empty.viewport(400.0), EMPTY_SIDE_H);
        assert_eq!(Side::Rest.viewport(400.0), 400.0 - EMPTY_SIDE_H);
        // A stack too short for the strip leaves the side with lanes nothing,
        // never a viewport taller than the stack.
        assert_eq!(Side::Rest.viewport(12.0), 0.0);
        assert_eq!(Side::Share(0.4).viewport(-5.0), 0.0);
        assert_eq!(Side::Share(0.4).viewport(100.0), 40.0);
    }
}
