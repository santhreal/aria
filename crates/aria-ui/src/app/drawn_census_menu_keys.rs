//! Dressed states for the keyframe and Path keyframe context menus
//! (`KEYFRAME_ITEMS`, `MASK_PATH_KEY_ITEMS`).

use aria_engine::EngineError;
use aria_models::{ClipParam, Easing};

use super::*;

/// A keyframe's menu opens on the middle of three Position keyframes: linear,
/// roving with keyframes copied, held, and under each easing a row writes.
pub(super) const KEYFRAME_STAGES: &[Stage<KeyframeMenu>] = &[
    Stage {
        what: "a linear keyframe between two others",
        dress: || fixtures::keyframe_menu(|_, _, _| {}),
    },
    Stage {
        what: "a roving keyframe, keyframes copied",
        dress: || {
            fixtures::keyframe_menu(|state, clip, tick| {
                state
                    .toggle_roving_keyframe(clip, ClipParam::Position, tick)
                    .expect("the middle keyframe roves");
                state.copy_selected_keyframes();
                state.status_message = None;
            })
        },
    },
    Stage {
        what: "a held keyframe",
        dress: || {
            fixtures::keyframe_menu(|state, _, _| {
                state
                    .toggle_hold_selected_or_playhead()
                    .expect("the keyframe holds");
            })
        },
    },
    Stage {
        what: "an eased keyframe",
        dress: || {
            fixtures::keyframe_menu(|s, _, _| key_ease(s, AppState::easy_ease_selected_or_playhead))
        },
    },
    Stage {
        what: "a keyframe eased in",
        dress: || {
            fixtures::keyframe_menu(|s, _, _| {
                key_ease(s, AppState::easy_ease_in_selected_or_playhead)
            })
        },
    },
    Stage {
        what: "a keyframe eased out",
        dress: || {
            fixtures::keyframe_menu(|s, _, _| {
                key_ease(s, AppState::easy_ease_out_selected_or_playhead)
            })
        },
    },
];

/// A Path keyframe's menu opens on the first of two keyframes, linear, held,
/// and under each easing a row writes.
pub(super) const PATH_KEY_STAGES: &[Stage<MaskPathKeyMenu>] = &[
    Stage {
        what: "a linear Path keyframe",
        dress: || fixtures::path_key_menu(|_, _, _| {}),
    },
    Stage {
        what: "an eased Path keyframe",
        dress: || fixtures::path_key_menu(|s, c, t| ease(s, c, t, Easing::EaseInOut)),
    },
    Stage {
        what: "a Path keyframe eased in",
        dress: || fixtures::path_key_menu(|s, c, t| ease(s, c, t, Easing::EaseIn)),
    },
    Stage {
        what: "a Path keyframe eased out",
        dress: || fixtures::path_key_menu(|s, c, t| ease(s, c, t, Easing::EaseOut)),
    },
    Stage {
        what: "a held Path keyframe",
        dress: || fixtures::path_key_menu(|s, c, t| ease(s, c, t, Easing::Hold)),
    },
];

/// Eases the segment that leaves the fixture's first Path keyframe.
fn ease(state: &mut AppState, clip: ClipId, tick: i64, easing: Easing) {
    state
        .ease_mask_path_key(clip, 0, tick, easing)
        .expect("the Path keyframe eases");
}

/// Runs one of the keyframe ease writes over the fixture's middle keyframe.
fn key_ease(state: &mut AppState, write: fn(&mut AppState) -> Result<(), EngineError>) {
    write(state).expect("the keyframe eases");
    state.status_message = None;
}
