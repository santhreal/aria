//! Dressed states and invariants for the Viewer context menu (`PREVIEW_ITEMS`).
//!
//! Closes the class where a Viewer context menu row acts on a surface other
//! than the Viewer or the selected picture clip, or is offered in a state where
//! pressing it fails or changes nothing. `PREVIEW_ITEMS` previously offered
//! `Fit Timeline` (`zoom_fit_sequence`), which zoomed the Timeline instead of
//! the Viewer, and offered `Reset Transform` (`when: always()`) with no clip
//! selected (where `reset_selected_transform` changes nothing) or an audio clip
//! selected (where `set_transform` fails with `IncompatibleTrackKind`).
//!
//! Every row in `PREVIEW_ITEMS` is enumerated from the table at run time and
//! pressed in every stage of [`PREVIEW_STAGES`] that offers it. Each press must
//! succeed, leave the Timeline's zoom and scroll positions unchanged, and
//! change either [`AppState::preview_view`] or the selected [`UiClip`]. Every
//! [`UiClipKind`] in [`UiClipKind::ALL`] is also placed and selected through an
//! exhaustive match so a new clip kind fails to build until classified.
//!
//! Not covered: the menu card's pixel placement over the Viewer canvas, which
//! `panels::context_menu::place_tests` and `grid_tests` check.

use aria_models::{ClipId, Generator, Shape};

use super::Stage;
use crate::app::AppState;
use crate::app::fixtures;
use crate::app::premiere_tests::with_clip;
use crate::chrome::menu_items::items_for;
use crate::surfaces::context_menu::{MenuItem, PREVIEW_ITEMS, PreviewMenu};
use crate::types::{AppMenu, UiClipKind, UiClipTransform};

/// The Viewer menu opens over a zoomed Viewer with nothing selected, with an
/// audio clip selected, and with a displaced picture clip selected.
pub(super) const PREVIEW_STAGES: &[Stage<PreviewMenu>] = &[
    Stage {
        what: "a zoomed Viewer with nothing selected",
        dress: || {
            let (mut state, _) = with_clip();
            state.clear_selection();
            state.zoom = 200.0;
            state.scroll_x = 40.0;
            state.preview_view.zoom = Some(2.0);
            let facts = PreviewMenu::of(&state);
            (state, facts)
        },
    },
    Stage {
        what: "a zoomed Viewer with an audio clip selected",
        dress: || {
            let (mut state, _) = fixtures::with_audio();
            state.zoom = 200.0;
            state.scroll_x = 40.0;
            state.preview_view.zoom = Some(2.0);
            let facts = PreviewMenu::of(&state);
            (state, facts)
        },
    },
    Stage {
        what: "a zoomed Viewer with a displaced picture clip selected",
        dress: || {
            let (mut state, _) = with_clip();
            state.zoom = 200.0;
            state.scroll_x = 40.0;
            state.preview_view.zoom = Some(2.0);
            state
                .set_selected_transform(UiClipTransform {
                    position_x: 24.0,
                    ..UiClipTransform::default()
                })
                .expect("the fixture's clip accepts a transform");
            state.sync_from_engine();
            let facts = PreviewMenu::of(&state);
            (state, facts)
        },
    },
];

/// Place and select a clip of `kind` on `state`. Exhaustive over [`UiClipKind`]
/// so a new clip kind fails to build until given a fixture here.
fn select_kind(state: &mut AppState, kind: UiClipKind) {
    match kind {
        UiClipKind::Media => {
            let media = state
                .import_media(fixtures::asset("texture.png"))
                .expect("import the still fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the media clip");
        }
        UiClipKind::Audio => {
            let audio = state
                .import_media(fixtures::asset("fixture-sine-48k-2s.wav"))
                .expect("import the audio fixture");
            state
                .drop_media_on_timeline(audio, None, 0)
                .expect("place the audio clip");
        }
        UiClipKind::Text => {
            state
                .add_generated(Generator::text("Sample"))
                .expect("place the title clip");
        }
        UiClipKind::Shape => {
            state
                .add_generated(Generator::shape(Shape::Rectangle, [255, 255, 255, 255]))
                .expect("place the shape clip");
        }
        UiClipKind::Generator => {
            state
                .add_generated(Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                })
                .expect("place the solid clip");
        }
    }
    state.sync_from_engine();
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|clip| clip.kind == kind)
        .map(|clip| ClipId::from_raw(clip.id))
        .unwrap_or_else(|| panic!("no {kind:?} clip projected on the timeline"));
    state.select_clip(clip);
}

#[test]
fn every_preview_row_acts_on_the_viewer_or_selected_picture_and_leaves_the_timeline_alone() {
    let rows: Vec<_> = PREVIEW_ITEMS.iter().filter_map(MenuItem::row).collect();
    assert!(!rows.is_empty(), "PREVIEW_ITEMS declares no rows");
    for stage in PREVIEW_STAGES {
        let (_, opened_facts) = (stage.dress)();
        let offered: Vec<_> = rows
            .iter()
            .copied()
            .filter(|row| (row.when)(&opened_facts))
            .collect();
        assert!(
            !offered.is_empty(),
            "PREVIEW_ITEMS offers no row in {}",
            stage.what
        );
        for row in offered {
            let (mut state, facts) = (stage.dress)();
            let timeline_before = (
                state.zoom.to_bits(),
                state.scroll_x.to_bits(),
                state.video_scroll_y.to_bits(),
                state.audio_scroll_y.to_bits(),
            );
            let viewer_before = state.preview_view;
            let clip_before = state.selected_ui_clip();
            let outcome = (row.run)(&mut state, &facts);
            assert!(
                outcome.is_ok(),
                "{} failed in {}: {outcome:?}",
                row.id,
                stage.what
            );
            state.sync_from_engine();
            let timeline_after = (
                state.zoom.to_bits(),
                state.scroll_x.to_bits(),
                state.video_scroll_y.to_bits(),
                state.audio_scroll_y.to_bits(),
            );
            assert_eq!(
                timeline_after, timeline_before,
                "{} in {} moved the Timeline's zoom or scroll",
                row.id, stage.what
            );
            let changed_viewer = state.preview_view != viewer_before;
            let changed_clip = state.selected_ui_clip() != clip_before;
            assert!(
                changed_viewer || changed_clip,
                "{} in {} changed neither the Viewer nor the selected clip",
                row.id,
                stage.what
            );
        }
    }

    let (mut state, _) = with_clip();
    let view_fit_label = items_for(AppMenu::View, &state)
        .into_iter()
        .find(|item| item.id == "view-zoom-fit")
        .expect("View menu declares view-zoom-fit")
        .label;
    let preview_fit = super::row_in(PREVIEW_ITEMS, "ctx-fit-viewer");
    assert_eq!(
        (preview_fit.label)(&PreviewMenu::of(&state)),
        view_fit_label,
        "ctx-fit-viewer and view-zoom-fit draw different labels"
    );

    for kind in UiClipKind::ALL {
        select_kind(&mut state, kind);
        let facts = PreviewMenu::of(&state);
        let draws_picture = match kind {
            UiClipKind::Media | UiClipKind::Text | UiClipKind::Shape | UiClipKind::Generator => {
                true
            }
            UiClipKind::Audio => false,
        };
        assert_eq!(
            facts.picture, draws_picture,
            "PreviewMenu::of picture flag wrong for {kind:?}"
        );
    }
}
