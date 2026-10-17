//! Capabilities the editor shipped without a way to ask for them.
//!
//! One of the tables [`super::BUILTIN_COMMANDS`] joins, plus the
//! actions behind it. Every row here closes a measured hole: the `AppState` verb
//! existed and was tested, and no panel, menu, or key sent it. The census that
//! found them compared the verbs in `crate::app` against the call sites in this
//! crate's registries, so a row here is not a convenience — before it, that verb
//! had no operator path at all.
//!
//! The action bodies are functions rather than closures inside the table because
//! the Sequence menu offers the same actions: `chrome::menu_sequence`
//! calls these, so a palette row and a menu row cannot drift into doing two
//! different things. One owner per action.
//!
//! The keyframe rows are the fail-by-default half. `ClipParam::class` declares
//! which parameters the animatable-property catalog addresses directly, and
//! `super`'s `every_direct_clip_param_has_a_keyframe_palette_row` reads
//! that declaration at run time: a new directly-addressed property is red until
//! it has a row here. Three of the eight had one before.

use aria_models::ClipParam;

use crate::app::AppState;

use super::host::CommandEntry;

/// One second of the sequence, in ticks.
///
/// Every "by a second" action below asks the sequence rather than assuming a
/// rate, which is what keeps the gesture the same size on a 24 and a 60 fps
/// timeline.
fn one_second(state: &AppState) -> i64 {
    i64::from(state.store.sequence.frame_rate.num.max(1))
}

/// Pin one property's current value at the playhead.
///
/// Inert off the clip and inert on a property the clip does not carry, which is
/// `add_keyframe_at_playhead`'s own rule — a row for a property an image clip has
/// no volume for stays quiet rather than erroring.
pub(crate) fn keyframe(state: &mut AppState, param: ClipParam) {
    if let Err(err) = state.add_keyframe_at_playhead(param) {
        state.report_error(&err);
    }
}

/// Shift the selected clip's whole lane by one second, from its first clip on.
pub(crate) fn shift_lane(state: &mut AppState, later: bool) {
    let Some(track) = state.selected_track() else {
        state.status_message = Some("Select a clip on the lane to shift".into());
        return;
    };
    let second = one_second(state);
    let delta = if later { second } else { -second };
    if let Err(err) = state.shift_track(track, delta) {
        state.report_error(&err);
    }
}

/// Insert the selected pool items at the playhead through source patching,
/// rippling the patched and sync-locked lanes: the `,` key with Program
/// focused, offered by name.
pub(crate) fn ripple_insert(state: &mut AppState) {
    if let Err(err) = state.place_pool_selection(crate::types::EditMode::Ripple) {
        state.report_error(&err);
    }
}

/// Move the marker nearest the playhead onto it.
///
/// Within one second, the same window every other "near the playhead" action
/// uses, so a marker two lanes of zoom away is not dragged across the sequence by
/// a keypress.
pub(crate) fn snap_marker(state: &mut AppState) {
    let within = one_second(state);
    let at = state.playhead.value;
    let Some(marker) = state.marker_near(at, within).map(|marker| marker.id) else {
        state.status_message = Some("No marker near the playhead".into());
        return;
    };
    if let Err(err) = state.move_marker(marker, at) {
        state.report_error(&err);
    }
}

/// Return every parameter of every effect on the selected clip to its catalog
/// default.
pub(crate) fn reset_effects(state: &mut AppState) {
    // The chain's shape is read once, before the first write: every reset
    // rebuilds the projected clip, so a borrow held across the loop would
    // describe the chain as it was.
    let Some(ui) = state.selected_ui_clip() else {
        return;
    };
    let slots: Vec<usize> = ui
        .effects
        .iter()
        .map(|effect| effect.values.len())
        .collect();
    for (index, count) in slots.into_iter().enumerate() {
        for param in 0..count {
            // A slot answers to exactly one of the two: the scalar reset is inert
            // on a colour or vector slot and the non-scalar reset is inert on a
            // scalar one, so both are asked.
            if let Err(err) = state.reset_selected_effect_param(index, param) {
                state.report_error(&err);
                return;
            }
            if let Err(err) = state.reset_selected_effect_non_scalar(index, param) {
                state.report_error(&err);
                return;
            }
        }
    }
}

/// Put the canvas background back to what a fresh project starts with.
///
/// The value comes from the model's own default, so the action cannot drift from
/// the default it claims to restore.
pub(crate) fn reset_canvas_background(state: &mut AppState) {
    let background = aria_models::CanvasSettings::default().background;
    if let Err(err) = state.set_canvas_background(background) {
        state.report_error(&err);
    }
}

/// Step the selected title's typeface. Inert off a title, which is where the
/// inspector hides the picker too.
pub(crate) fn cycle_font(state: &mut AppState, forward: bool) {
    if let Err(err) = state.cycle_selected_font(forward) {
        state.report_error(&err);
    }
}

/// New Sequence, from the File menu, the palette, and Ctrl+N.
pub(crate) fn new_sequence(state: &mut AppState) {
    if let Err(err) = state.new_sequence() {
        state.report_error(&err);
    }
}

/// Nest, open, leave, place. Four actions over one report path so a refusal
/// reads the same whether it came from the palette or the Sequence menu.
pub(crate) fn nest_selection(state: &mut AppState) {
    if let Err(err) = state.nest_selection_named_in_order() {
        state.report_error(&err);
    }
}

pub(crate) fn open_selected_nest(state: &mut AppState) {
    if let Err(err) = state.open_selected_nest() {
        state.report_error(&err);
    }
}

pub(crate) fn leave_sequence(state: &mut AppState) {
    if let Err(err) = state.leave_sequence() {
        state.report_error(&err);
    }
}

pub(crate) fn place_selected_nest(state: &mut AppState) {
    if let Err(err) = state.place_selected_nest() {
        state.report_error(&err);
    }
}

/// Rows that were reachable from no surface at all before this table existed.
pub(super) const REACH_ROWS: &[CommandEntry] = &[
    // --- keyframes: one row per directly-addressed animatable property -------
    CommandEntry {
        id: "pal-key-anchor-point",
        label: "Keyframe anchor point at playhead",
        run: |state| keyframe(state, ClipParam::AnchorPoint),
    },
    CommandEntry {
        id: "pal-key-rotation",
        label: "Keyframe rotation at playhead",
        run: |state| keyframe(state, ClipParam::Rotation),
    },
    CommandEntry {
        id: "pal-key-crop",
        label: "Keyframe crop at playhead",
        run: |state| keyframe(state, ClipParam::Crop),
    },
    CommandEntry {
        id: "pal-key-volume",
        label: "Keyframe volume at playhead",
        run: |state| keyframe(state, ClipParam::Volume),
    },
    CommandEntry {
        id: "pal-key-pan",
        label: "Keyframe pan at playhead",
        run: |state| keyframe(state, ClipParam::Pan),
    },
    // --- ducking: the dip was reachable, its shape was not ------------------
    CommandEntry {
        id: "pal-duck-amount",
        label: "Cycle duck amount",
        run: |state| state.cycle_duck_amount(),
    },
    CommandEntry {
        id: "pal-duck-attack",
        label: "Cycle duck attack",
        run: |state| state.cycle_duck_attack(),
    },
    CommandEntry {
        id: "pal-duck-release",
        label: "Cycle duck release",
        run: |state| state.cycle_duck_release(),
    },
    // --- lanes, markers, pool, canvas ---------------------------------------
    CommandEntry {
        id: "pal-lane-shift-later",
        label: "Shift lane one second later",
        run: |state| shift_lane(state, true),
    },
    CommandEntry {
        id: "pal-lane-shift-earlier",
        label: "Shift lane one second earlier",
        run: |state| shift_lane(state, false),
    },
    CommandEntry {
        id: "pal-ripple-insert",
        label: "Insert pool selection at playhead",
        run: ripple_insert,
    },
    CommandEntry {
        id: "pal-marker-snap",
        label: "Move nearest marker to playhead",
        run: snap_marker,
    },
    CommandEntry {
        id: "pal-effect-reset",
        label: "Reset effect parameters",
        run: reset_effects,
    },
    CommandEntry {
        id: "pal-canvas-background",
        label: "Reset canvas background",
        run: reset_canvas_background,
    },
    CommandEntry {
        id: "pal-media-filter-clear",
        label: "Clear pool filter",
        run: |state| state.clear_media_query(),
    },
    CommandEntry {
        id: "pal-font-next",
        label: "Next font on title",
        run: |state| cycle_font(state, true),
    },
    CommandEntry {
        id: "pal-font-prev",
        label: "Previous font on title",
        run: |state| cycle_font(state, false),
    },
    CommandEntry {
        id: "pal-proxy-clear-all",
        label: "Decode everything from originals",
        run: |state| state.clear_all_media_proxies(),
    },
    CommandEntry {
        id: "pal-proxy-create-all",
        label: "Proxy every pool item",
        run: |state| state.proxy_whole_pool(),
    },
    // --- render and replace -------------------------------------------------
    CommandEntry {
        id: "pal-bake-clip",
        label: "Render and Replace",
        run: |state| {
            if let Err(err) = state.render_and_replace() {
                state.status_message = Some(err);
            }
        },
    },
    CommandEntry {
        id: "pal-bake-restore",
        label: "Restore Unrendered",
        run: |state| {
            if let Err(err) = state.restore_unrendered() {
                state.status_message = Some(err);
            }
        },
    },
    // --- nested sequences: three operator commands with no surface ----------
    CommandEntry {
        id: "pal-new-sequence",
        label: "New sequence",
        run: new_sequence,
    },
    CommandEntry {
        id: "pal-nest-selection",
        label: "Nest selection into a sequence",
        run: nest_selection,
    },
    CommandEntry {
        id: "pal-nest-open",
        label: "Open the selected nested sequence",
        run: open_selected_nest,
    },
    CommandEntry {
        id: "pal-nest-leave",
        label: "Leave the open nested sequence",
        run: leave_sequence,
    },
    CommandEntry {
        id: "pal-nest-place",
        label: "Place the selected nest at the playhead",
        run: place_selected_nest,
    },
];
