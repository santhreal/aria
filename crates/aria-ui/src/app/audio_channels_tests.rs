//! Clip > Audio Channels.
//!
//! # Why
//!
//! A choice that leaves an old channel tool behind plays two mappings at once;
//! one placed after the clip's other effects changes what those effects hear;
//! one that takes several undo steps leaves a half-applied mapping after one
//! Undo. These cases walk every choice from every other choice on a clip that
//! already has an effect, and undo each.
//!
//! What they do not catch: the channel tools' sample processing, which
//! `aria_render::audio_fx` tests.

use aria_models::{AudioInsertTarget, ClipId};
use gpui::AppContext;

use super::AudioChannels;
use crate::app::AppState;
use crate::app::fixtures::with_audio;
use crate::app::premiere_tests::with_clip;
use crate::chrome::menu_items::{MenuItem, items_of};
use crate::types::AppMenu;

fn chain(state: &AppState, clip: ClipId) -> Vec<String> {
    state
        .engine
        .lock()
        .project()
        .audio_inserts(AudioInsertTarget::Clip(clip))
        .expect("the chain")
        .iter()
        .map(|insert| insert.effect_id.clone())
        .collect()
}

#[test]
fn every_choice_replaces_the_last_at_the_head_of_the_chain_as_one_undo_step() {
    let (mut state, clip) = with_audio();
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::AddAudioEffect {
                target: AudioInsertTarget::Clip(clip),
                effect_id: "gain".to_owned(),
            },
        ))
        .expect("a gain insert");
    assert_eq!(
        state.selection_audio_channels(),
        Some(AudioChannels::Stereo)
    );

    for from in AudioChannels::ALL {
        for to in AudioChannels::ALL {
            state.set_audio_channels(from).expect("set from");
            let before = chain(&state, clip);
            state.set_audio_channels(to).expect("set to");
            assert_eq!(
                state.selection_audio_channels(),
                Some(to),
                "{from:?} -> {to:?}"
            );
            let expected: Vec<String> = to
                .tool()
                .into_iter()
                .map(str::to_owned)
                .chain(["gain".to_owned()])
                .collect();
            assert_eq!(chain(&state, clip), expected, "{from:?} -> {to:?}");
            if before != expected {
                state.undo();
                assert_eq!(
                    chain(&state, clip),
                    before,
                    "one Undo after {from:?} -> {to:?}"
                );
                assert_eq!(state.selection_audio_channels(), Some(from));
            }
        }
    }
}

#[test]
fn a_clip_without_sound_is_left_alone() {
    let (mut state, clip) = with_clip();
    state
        .set_audio_channels(AudioChannels::Mono)
        .expect("no refusal");
    assert_eq!(
        state.status_message.as_deref(),
        Some("Select a clip with sound.")
    );
    assert!(chain(&state, clip).is_empty());
    assert_eq!(state.selection_audio_channels(), None);
    for row in channel_rows(&state) {
        assert_eq!(
            row.blocked_by(&state),
            Some("Select a clip with sound."),
            "{}",
            row.id
        );
    }
}

/// The Clip > Audio Channels rows, in menu order.
fn channel_rows(state: &AppState) -> Vec<MenuItem> {
    items_of(AppMenu::Clip, state)
        .into_iter()
        .filter(|item| item.under == Some("Audio Channels"))
        .collect()
}

#[test]
fn each_row_picks_its_choice_and_is_the_only_one_checked() {
    let (state, _) = with_audio();
    let labels: Vec<&str> = channel_rows(&state).iter().map(|row| row.label).collect();
    assert_eq!(labels, AudioChannels::ALL.map(AudioChannels::label));
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    // Mono first, so the walk back to Stereo removes a tool.
    for choice in [AudioChannels::Mono].into_iter().chain(AudioChannels::ALL) {
        let checked = cx.update(|app| {
            entity.update(app, |state: &mut AppState, cx| {
                let row = channel_rows(state)
                    .into_iter()
                    .find(|row| row.id == choice.row())
                    .expect("the row");
                assert_eq!(row.blocked_by(state), None, "{choice:?}");
                (row.run)(row.id, state, cx);
                channel_rows(state)
                    .into_iter()
                    .filter(|row| row.is_checked(state))
                    .map(|row| row.id)
                    .collect::<Vec<_>>()
            })
        });
        assert_eq!(checked, [choice.row()], "{choice:?}");
    }
}
