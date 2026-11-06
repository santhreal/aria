//! Framing block of the Transform tab: the crop insets, mirroring, and motion
//! blur.
//!
//! Crop rows are generated from `app::clip_frame::CROP_EDGES`, so the panel
//! cannot drift from the rect the model validates. They read the rect in force
//! under the playhead, which is where an edit lands. The crop is one animatable
//! property, as a position is, so each inset row draws the same stopwatch,
//! navigator and time area, as Position X and Position Y share theirs.

use aria_models::ClipParam;
use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::clip_frame::{CROP_EDGES, crop_inset};
use crate::app::param_units::Units;
use crate::components::{self, section_label};
use crate::motion::MotionKey;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::{
    Columns, Keys, Readout, Stopwatch, filing, stepper_row, switch_line, unkeyed_row,
};

pub fn framing_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // Every frame of an animated crop has its own rect; the rows show the one
    // under the playhead, not the clip's first frame the projection holds.
    let (here, watch) = {
        let state = state.read(cx);
        let here = state
            .selected_framing_here()
            .map_or(clip.crop, |(crop, ..)| crop);
        (here, crop_stopwatch(state))
    };
    // A crop inset reads, drags and types in the units a crop prints in.
    let units = Units::of(ClipParam::Crop, &ClipParam::Crop.range());
    let scale = units.number.scale;
    let crops: Vec<_> = CROP_EDGES
        .iter()
        .map(|(id, label, ..)| {
            let edge = *id;
            let current = crop_inset(&here, edge);
            let keys = watch
                .clone()
                .map_or(Keys::Unkeyed(Columns::Keyed), Keys::Watch);
            stepper_row(
                theme,
                state,
                MotionKey::new("insp.row", edge),
                *label,
                Readout::of(units, current),
                None,
                keys,
                move |state, by| {
                    state.nudge_selected_crop_inset(edge, scale.stored_delta(current, by))
                },
                cx,
            )
            .into_any_element()
        })
        .collect();

    let blur = clip.motion_blur;
    // The reset ends the Crop heading's line, as the block's own ends the
    // Transform heading's, and puts back the mirroring under it too.
    let reset = filing(
        components::icon_button("crop-clear", crate::icons::ROTATE_CCW, theme)
            .debug_selector(|| "crop-clear".into()),
        cx,
        |state| inspector_pick::apply(state, Pick::ClearCrop),
    );

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(components::section_heading(theme, "Crop", reset))
        .children(crops)
        .child(section_label(theme, "Mirror"))
        .child(switch_line(
            theme,
            MotionKey::new("insp.row", "crop-flip-h"),
            "Horizontal",
            Columns::Keyed,
            "flip-h",
            Some(clip.flip_h),
            cx,
            |state| inspector_pick::apply(state, Pick::FlipHorizontal),
        ))
        .child(switch_line(
            theme,
            MotionKey::new("insp.row", "crop-flip-v"),
            "Vertical",
            Columns::Keyed,
            "flip-v",
            Some(clip.flip_v),
            cx,
            |state| inspector_pick::apply(state, Pick::FlipVertical),
        ))
        .child(section_label(theme, "Motion blur"))
        .child(switch_line(
            theme,
            MotionKey::new("insp.row", "blur-on"),
            "Enabled",
            Columns::Keyed,
            "blur-toggle",
            Some(blur.enabled),
            cx,
            |state| inspector_pick::apply(state, Pick::ToggleMotionBlur),
        ))
        .child(unkeyed_row(
            theme,
            state,
            MotionKey::new("insp.row", "blur-shutter"),
            "Shutter",
            Readout::printed(
                format!("{:.0}°", blur.shutter_deg),
                blur.shutter_deg,
                0,
                1.0,
            ),
            Columns::Keyed,
            |state, by| state.nudge_selected_shutter(by),
            cx,
        ))
        .child(unkeyed_row(
            theme,
            state,
            MotionKey::new("insp.row", "blur-samples"),
            "Samples",
            Readout::number(blur.samples as f32, 0, 1.0),
            Columns::Keyed,
            |state, by| state.nudge_selected_blur_samples(by.round() as i32),
            cx,
        ))
}

/// The stopwatch every crop inset row draws: the crop's keys, read once for
/// the four rows. `None` when the selection offers no crop, and the rows then
/// stand blank in the keyed columns.
fn crop_stopwatch(state: &crate::app::AppState) -> Option<Stopwatch> {
    let row = state.key_param(ClipParam::Crop)?;
    let at = state.playhead.value;
    Some(Stopwatch {
        param: ClipParam::Crop,
        animated: !row.marks.is_empty(),
        at_playhead: row.keyed_at(at),
        keys_around: row.keys_around(at),
        off_default: state.param_off_default(ClipParam::Crop),
        driver: None,
        pick_target: false,
        strip: state.key_strip(ClipParam::Crop),
    })
}
