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

/// The smallest share of the stack either side takes while both have lanes.
const MIN_SHARE: f32 = 0.15;

/// A side's height in the stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Side {
    /// This fraction of the stack.
    Share(f32),
    /// The stack less the other side's empty-lane strip.
    Rest,
    /// The empty-lane strip.
    Empty,
}

/// The video and audio sides for the lanes on each and the A/V `split`.
pub(super) fn sides(video_lanes: usize, audio_lanes: usize, split: f32) -> (Side, Side) {
    match (video_lanes, audio_lanes) {
        (1.., 0) => (Side::Rest, Side::Empty),
        (0, 1..) => (Side::Empty, Side::Rest),
        _ => {
            let video = split.clamp(MIN_SHARE, 1.0 - MIN_SHARE);
            (Side::Share(video), Side::Share(1.0 - video))
        }
    }
}

impl Side {
    /// The lane viewport this side gives its group in a stack `stack_h` tall.
    pub(super) fn viewport(self, stack_h: f32) -> f32 {
        let stack_h = stack_h.max(0.0);
        match self {
            Side::Share(share) => stack_h * share,
            Side::Rest => (stack_h - EMPTY_SIDE_H).max(0.0),
            Side::Empty => EMPTY_SIDE_H,
        }
    }

    pub(super) fn is_empty(self) -> bool {
        self == Side::Empty
    }
}

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
