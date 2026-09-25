//! One clip as a project of its own, so an ordinary export renders it.
//!
//! Render and replace has the same requirement a proxy has, for the same
//! reason: the file that stands in for the clip must be produced by the render
//! and export path everything else goes through, because a second encoder would
//! be a second opinion about colour, orientation, stills and rate conversion,
//! and the first frame where the two disagreed would be a preview showing a
//! picture the export does not have.
//!
//! So a bake pass is the clip's own timeline with nothing else on it, moved to
//! tick `0`, on the canvas it was composited on. Rendering that is an ordinary
//! export, and frame `0` of the result is the tick the clip started on — which
//! is exactly the mapping [`Clip::unbaked`](aria_models::Clip::unbaked) inverts.
//!
//! What is deliberately left out of the pass is everything the clip is not: the
//! neighbours it abuts, the transitions those junctions carried, and the lane
//! bars above it. A lane bar reprocesses whatever is beneath it, so folding one
//! into a single clip would bake a grade that belongs to the programme, and it
//! would stay baked after the bar was deleted. Only nests are kept, because a
//! nest clip's picture *is* another timeline.
//!
//! A clip on a sound lane bakes the same way, to a file with no picture in it.
//! The same argument applies once more, to the mix rather than to the picture: a
//! lane's fader, its mute, its solo, its sends and where it points are all
//! programme, and they are applied again when the intermediate plays back. So
//! the sound pass neutralizes them, and the file holds the clip's own sound and
//! nothing else.

use aria_models::{
    BakeMedium, ClipId, ModelError, Project, RationalTime, SequenceId, TrackId, TrackOutput,
};

use crate::error::EngineError;

/// A clip alone, as the project an ordinary export renders.
#[derive(Debug)]
pub struct BakePass {
    /// What kind of file rendering it writes.
    pub medium: BakeMedium,
    /// The clip's own timeline, with nothing else on it, at tick `0`.
    pub project: Project,
}

/// A project that renders exactly `clip`'s timeline span, one-to-one at its
/// sequence's rate, on the canvas the clip is composited on.
///
/// Refuses what a render cannot stand in for: a clip that is already a render,
/// a lane bar (both refused by the model, so there is one answer), and a clip on
/// a lane that is neither picture nor sound.
pub fn bake_project(project: &Project, clip: ClipId) -> Result<BakePass, EngineError> {
    let source = project.clip(clip).ok_or(ModelError::UnknownClip(clip))?;
    let span = source.timeline;
    let home = home_of(project, clip).ok_or(ModelError::UnknownClip(clip))?;
    let timeline = match home {
        None => project.root_timeline(),
        Some(id) => {
            &project
                .sequence(id)
                .ok_or(ModelError::UnknownSequence(id))?
                .timeline
        }
    };
    let track = timeline
        .track_of(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    // What kind of file this becomes, and whether one is worth making, are the
    // model's answer: the same one the assistant's tool row reads before it asks
    // for a render at all.
    let medium = project.bake_medium(clip)?;

    let mut baking = Project::clone(project);
    // The exporter renders the open timeline, so the clip's own timeline becomes
    // the open one whether it lives in the root or inside a nest.
    baking.set_active_sequence(home)?;

    let others: Vec<ClipId> = baking
        .timeline()
        .tracks_ordered()
        .flat_map(|track| track.clips())
        .map(|other| other.id)
        .filter(|other| *other != clip)
        .collect();
    let timeline = baking.timeline_mut();
    for other in others {
        timeline.remove_clip(other);
    }
    // Every junction this clip had is gone with its neighbours, and a transition
    // with one side missing renders nothing.
    baking.prune_dead_transitions();
    baking.move_clip(clip, track, RationalTime::new(0, span.start.rate))?;
    if medium == BakeMedium::Sound {
        silence_the_mix_desk(&mut baking);
    }
    Ok(BakePass {
        medium,
        project: baking,
    })
}

/// Take every lane in the open timeline off the mix desk: unity fader, nothing
/// muted or soloed, no sends, straight to the master.
///
/// The sound half of "the pass is the clip and nothing else". A fader is
/// programme: it is applied when the intermediate plays back, so folding it in
/// would apply it twice, and it would stay folded in after the fader was moved.
/// Mute and solo are the same argument at its extreme, where the file would be
/// silence.
///
/// Every lane rather than only the clip's own, because a send or a bus route
/// elsewhere in the timeline can still colour what reaches the master.
fn silence_the_mix_desk(baking: &mut Project) {
    let lanes: Vec<TrackId> = baking
        .timeline()
        .tracks_ordered()
        .map(|track| track.id)
        .collect();
    let timeline = baking.timeline_mut();
    for lane in lanes {
        let Some(lane) = timeline.track_mut(lane) else {
            continue;
        };
        lane.gain = 1.0;
        lane.muted = false;
        lane.solo = false;
        lane.sends.clear();
        lane.output = TrackOutput::Master;
    }
}

/// Which timeline holds `clip`: `None` for the root, `Some(id)` for a nest.
fn home_of(project: &Project, clip: ClipId) -> Option<Option<SequenceId>> {
    if project.root_timeline().clip(clip).is_some() {
        return Some(None);
    }
    project
        .sequences()
        .find(|sequence| sequence.timeline.clip(clip).is_some())
        .map(|sequence| Some(sequence.id))
}

#[cfg(test)]
#[path = "bake_tests.rs"]
mod bake_tests;

#[cfg(test)]
#[path = "bake_audio_tests.rs"]
mod audio;
