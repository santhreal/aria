//! Golden JSON for what an applied command reports back.
use super::*;

// --- outcomes ---------------------------------------------------------------

#[test]
fn edit_outcome_wire_format() {
    let samples: Vec<(EditOutcome, Value)> = vec![
        (
            EditOutcome::Created(clip(7)),
            json!({"type": "Created", "id": 7}),
        ),
        (
            EditOutcome::CreatedTrack(track(3)),
            json!({"type": "CreatedTrack", "id": 3}),
        ),
        (
            EditOutcome::Updated(clip(7)),
            json!({"type": "Updated", "id": 7}),
        ),
        (
            EditOutcome::Removed(clip(7)),
            json!({"type": "Removed", "id": 7}),
        ),
        (
            EditOutcome::RemovedTrack(track(3)),
            json!({"type": "RemovedTrack", "id": 3}),
        ),
        (
            EditOutcome::ShiftedTrack(track(3)),
            json!({"type": "ShiftedTrack", "id": 3}),
        ),
        (
            EditOutcome::UpdatedTrack(track(3)),
            json!({"type": "UpdatedTrack", "id": 3}),
        ),
        (
            EditOutcome::CreatedMarker(MarkerId::from_raw(9)),
            json!({"type": "CreatedMarker", "id": 9}),
        ),
        (
            EditOutcome::UpdatedMarker(MarkerId::from_raw(9)),
            json!({"type": "UpdatedMarker", "id": 9}),
        ),
        (
            EditOutcome::RemovedMarker(MarkerId::from_raw(9)),
            json!({"type": "RemovedMarker", "id": 9}),
        ),
        (EditOutcome::UpdatedCanvas, json!({"type": "UpdatedCanvas"})),
        (
            EditOutcome::UpdatedSequenceRate,
            json!({"type": "UpdatedSequenceRate"}),
        ),
        (
            EditOutcome::UpdatedAudioLayout,
            json!({"type": "UpdatedAudioLayout"}),
        ),
        (
            EditOutcome::UpdatedTimeDisplay,
            json!({"type": "UpdatedTimeDisplay"}),
        ),
        (EditOutcome::UpdatedMaster, json!({"type": "UpdatedMaster"})),
        (
            EditOutcome::CreatedBin(aria_models::BinId::from_raw(5)),
            json!({"type": "CreatedBin", "id": 5}),
        ),
        (EditOutcome::UpdatedPool, json!({"type": "UpdatedPool"})),
        (
            EditOutcome::UpdatedSourcePatch,
            json!({"type": "UpdatedSourcePatch"}),
        ),
    ];
    for (outcome, expected) in samples {
        assert_eq!(serde_json::to_value(outcome).unwrap(), expected);
        let back: EditOutcome = serde_json::from_value(expected).unwrap();
        assert_eq!(back, outcome);
    }
}
