//! What the replay's substitutions do to the steps that follow.
//!
//! The classification is gated by the compiler: `Made::of` is exhaustive over
//! `EditOutcome`, so an outcome that creates a new kind of entity stops this
//! crate compiling until it says which map it belongs in. What is left to check
//! here is that a substitution reaches the step that names it, and that a
//! non-substitution does not invent one.

use aria_commands::EditOutcome;
use aria_models::{ClipId, MarkerId, TrackId};

use super::{Ids, Made};
use crate::WireCommand;
use crate::wire::{AddEffect, ExtractAudio, SetMarker};

#[test]
fn only_a_created_entity_is_a_substitution() {
    assert_eq!(
        Made::of(&EditOutcome::Created(ClipId::from_raw(7))),
        Made::Clip(7)
    );
    assert_eq!(
        Made::of(&EditOutcome::CreatedTrack(TrackId::from_raw(3))),
        Made::Track(3)
    );
    assert_eq!(
        Made::of(&EditOutcome::CreatedMarker(MarkerId::from_raw(9))),
        Made::Marker(9)
    );
    // Touching an entity is not creating one: the id in the step already names
    // the thing the replay touched.
    assert_eq!(
        Made::of(&EditOutcome::Updated(ClipId::from_raw(7))),
        Made::Nothing
    );
    assert_eq!(
        Made::of(&EditOutcome::UpdatedMarker(MarkerId::from_raw(9))),
        Made::Nothing
    );
    assert_eq!(Made::of(&EditOutcome::UpdatedCanvas), Made::Nothing);
}

#[test]
fn a_later_step_names_what_the_replay_created() {
    let mut ids = Ids::default();
    ids.note(
        Some(EditOutcome::Created(ClipId::from_raw(10))),
        EditOutcome::Created(ClipId::from_raw(400)),
    );
    ids.note(
        Some(EditOutcome::CreatedTrack(TrackId::from_raw(11))),
        EditOutcome::CreatedTrack(TrackId::from_raw(401)),
    );
    ids.note(
        Some(EditOutcome::CreatedMarker(MarkerId::from_raw(12))),
        EditOutcome::CreatedMarker(MarkerId::from_raw(402)),
    );

    let mut extract = WireCommand::ExtractAudio(ExtractAudio {
        clip: 10,
        track: 11,
    });
    ids.rewrite(&mut extract);
    assert_eq!(
        extract,
        WireCommand::ExtractAudio(ExtractAudio {
            clip: 400,
            track: 401,
        })
    );

    let mut marker = WireCommand::SetMarker(SetMarker {
        marker: 12,
        at: None,
        name: Some("cut".into()),
        color: None,
        comment: None,
    });
    ids.rewrite(&mut marker);
    let WireCommand::SetMarker(marker) = marker else {
        panic!("rewriting a step does not change which command it is");
    };
    assert_eq!(marker.marker, 402);
}

#[test]
fn an_id_nothing_replaced_is_left_alone() {
    let mut ids = Ids::default();
    assert!(ids.is_empty());
    // A step that created nothing, and a step whose replay produced another
    // kind of entity: neither is an id standing in for an id.
    ids.note(None, EditOutcome::Created(ClipId::from_raw(400)));
    ids.note(
        Some(EditOutcome::Updated(ClipId::from_raw(10))),
        EditOutcome::Updated(ClipId::from_raw(10)),
    );
    ids.note(
        Some(EditOutcome::Created(ClipId::from_raw(10))),
        EditOutcome::CreatedTrack(TrackId::from_raw(401)),
    );
    assert!(ids.is_empty());

    let mut effect = WireCommand::AddEffect(AddEffect {
        clip: 10,
        effect: "vignette".into(),
    });
    ids.rewrite(&mut effect);
    assert_eq!(
        effect,
        WireCommand::AddEffect(AddEffect {
            clip: 10,
            effect: "vignette".into(),
        })
    );
}
