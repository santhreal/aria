//! Which filmstrip spans an applied command invalidates.
//!
//! `apply_command` captures the clips, tracks and media a command touches
//! before it runs, then compares their spans after it ran, so a picture edit
//! repaints only the frames it moved and a picture-neutral edit repaints none.
//! It also compares where every picture lane plays each clip, so an edit that
//! moves clips on lanes its payload does not name, such as the sync-locked
//! lanes a ripple carries along, repaints the frames those clips left and
//! reached.

use super::*;

pub(super) enum FilmstripInvalidation {
    Full,
    Spans(Vec<(i64, i64)>),
}

pub(super) struct FilmstripImpact {
    full: bool,
    picture: bool,
    clips: Vec<aria_models::ClipId>,
    tracks: Vec<aria_models::TrackId>,
    media: Option<aria_models::MediaId>,
    before: Vec<(i64, i64)>,
    before_tracks: Vec<(i64, i64)>,
    wide_tracks: bool,
    placed: Option<Vec<Placement>>,
}

/// Where a picture lane plays a clip: the clip, its lane, its span, and the
/// source frame it opens on.
type Placement = (
    aria_models::ClipId,
    aria_models::TrackId,
    i64,
    i64,
    Option<i64>,
);

impl FilmstripImpact {
    pub(super) fn capture(command: &Command, project: &aria_models::Project) -> Self {
        let mut impact = Self {
            full: false,
            picture: false,
            clips: Vec::new(),
            tracks: Vec::new(),
            media: None,
            before: Vec::new(),
            before_tracks: Vec::new(),
            wide_tracks: false,
            placed: None,
        };
        match command {
            Command::Project(
                aria_commands::ProjectCommand::Open { .. }
                | aria_commands::ProjectCommand::Load { .. }
                | aria_commands::ProjectCommand::ApplyTemplate { .. },
            ) => impact.full = true,
            Command::Project(aria_commands::ProjectCommand::RelinkMedia { media, .. }) => {
                impact.picture = true;
                impact.media = Some(*media);
            }
            Command::Project(
                aria_commands::ProjectCommand::Import { .. }
                | aria_commands::ProjectCommand::RemoveMedia { .. }
                | aria_commands::ProjectCommand::Save { .. }
                | aria_commands::ProjectCommand::SetMediaLabel { .. }
                | aria_commands::ProjectCommand::Export { .. }
                | aria_commands::ProjectCommand::SaveTemplate { .. },
            ) => {}
            Command::Edit(edit) => {
                let targets = edit.targets();
                impact.picture =
                    !picture_neutral_edit(edit) && targets_touch_picture(project, &targets);
                if impact.picture {
                    impact.clips = targets.clips;
                    impact.tracks = targets.tracks;
                    if let EditCommand::SetTrackSolo { track, .. } = edit
                        && let Some(kind) = project.timeline().track(*track).map(|track| track.kind)
                    {
                        impact.wide_tracks = true;
                        impact.tracks = project
                            .timeline()
                            .tracks_ordered()
                            .filter(|track| track.kind == kind)
                            .map(|track| track.id)
                            .collect();
                    }
                }
                if !picture_neutral_edit(edit) {
                    impact.placed = Some(placements(project));
                }
            }
        }
        impact.before = impact.spans_in(project);
        impact.before_tracks = impact.track_spans_in(project);
        impact
    }

    pub(super) fn finish(
        mut self,
        outcome: &ApplyOutcome,
        project: &aria_models::Project,
    ) -> FilmstripInvalidation {
        if self.full {
            return FilmstripInvalidation::Full;
        }
        if let Some(before) = self.placed.take() {
            moved_spans(&before, &placements(project), &mut self.before);
        }
        if !self.picture {
            return FilmstripInvalidation::Spans(tidy(self.before));
        }
        let after = self.spans_in(project);
        self.before.extend(after);
        if self.wide_tracks {
            self.before.extend(&self.before_tracks);
            let after_tracks = self.track_spans_in(project);
            self.before.extend(after_tracks);
        }
        match outcome {
            ApplyOutcome::Opened
            | ApplyOutcome::Loaded
            | ApplyOutcome::AppliedTemplate
            | ApplyOutcome::Edited(
                aria_commands::EditOutcome::UpdatedCanvas
                | aria_commands::EditOutcome::UpdatedSequenceRate
                | aria_commands::EditOutcome::OpenedSequence,
            ) => return FilmstripInvalidation::Full,
            ApplyOutcome::Edited(aria_commands::EditOutcome::Created(clip))
            | ApplyOutcome::Edited(aria_commands::EditOutcome::Updated(clip)) => {
                push_clip_span(project, *clip, &mut self.before);
            }
            ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(track))
            | ApplyOutcome::Edited(aria_commands::EditOutcome::ShiftedTrack(track))
            | ApplyOutcome::Edited(aria_commands::EditOutcome::UpdatedTrack(track)) => {
                if !self.wide_tracks {
                    self.before.extend(&self.before_tracks);
                    let after_tracks = self.track_spans_in(project);
                    self.before.extend(after_tracks);
                }
                push_track_spans(project, *track, &mut self.before);
            }
            ApplyOutcome::Edited(aria_commands::EditOutcome::RemovedTrack(_)) => {
                self.before.extend(&self.before_tracks);
            }
            ApplyOutcome::Imported { .. }
            | ApplyOutcome::RemovedMedia { .. }
            | ApplyOutcome::Saved
            | ApplyOutcome::Relinked { .. }
            | ApplyOutcome::LabeledMedia { .. }
            | ApplyOutcome::Exported { .. }
            | ApplyOutcome::SavedTemplate
            | ApplyOutcome::Edited(
                aria_commands::EditOutcome::Removed(_)
                | aria_commands::EditOutcome::CreatedMarker(_)
                | aria_commands::EditOutcome::UpdatedMarker(_)
                | aria_commands::EditOutcome::RemovedMarker(_)
                | aria_commands::EditOutcome::UpdatedAudioLayout
                | aria_commands::EditOutcome::UpdatedTimeDisplay
                | aria_commands::EditOutcome::UpdatedMaster
                | aria_commands::EditOutcome::UpdatedProject
                | aria_commands::EditOutcome::CreatedBin(_)
                | aria_commands::EditOutcome::UpdatedPool
                | aria_commands::EditOutcome::UpdatedSourcePatch,
            ) => {}
        }
        FilmstripInvalidation::Spans(tidy(self.before))
    }

    fn spans_in(&self, project: &aria_models::Project) -> Vec<(i64, i64)> {
        let mut spans = Vec::new();
        for clip in &self.clips {
            push_clip_span(project, *clip, &mut spans);
        }
        if let Some(media) = self.media {
            for track in project.timeline().tracks_ordered() {
                for clip in track.clips() {
                    if matches!(
                        &clip.content,
                        aria_models::ClipSource::Media { media: held, .. } if *held == media
                    ) {
                        spans.push((clip.timeline.start.value, clip.timeline.end_tick()));
                    }
                }
            }
        }
        spans
    }

    fn track_spans_in(&self, project: &aria_models::Project) -> Vec<(i64, i64)> {
        let mut spans = Vec::new();
        for track in &self.tracks {
            push_track_spans(project, *track, &mut spans);
        }
        spans
    }
}

/// Every clip on a picture lane, sorted, for [`moved_spans`] to compare across
/// an edit.
fn placements(project: &aria_models::Project) -> Vec<Placement> {
    let mut placed: Vec<Placement> = project
        .timeline()
        .tracks_ordered()
        .filter(|track| track.kind.is_visual())
        .flat_map(|track| {
            track.clips().map(move |clip| {
                (
                    clip.id,
                    track.id,
                    clip.timeline.start.value,
                    clip.timeline.end_tick(),
                    clip.source_range().map(|source| source.start.value),
                )
            })
        })
        .collect();
    placed.sort_unstable();
    placed
}

/// The span of every placement in one of `before` and `after` and not in the
/// other: a clip that left a span, reached one, or now plays it from another
/// source frame. Both lists are sorted.
fn moved_spans(before: &[Placement], after: &[Placement], spans: &mut Vec<(i64, i64)>) {
    for (one, other) in [(before, after), (after, before)] {
        spans.extend(
            one.iter()
                .filter(|placed| other.binary_search(placed).is_err())
                .map(|&(_, _, start, end, _)| (start, end)),
        );
    }
}

/// `spans` without empty spans or repeats, in order.
fn tidy(mut spans: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    spans.retain(|(start, end)| start < end);
    spans.sort_unstable();
    spans.dedup();
    spans
}

fn push_clip_span(
    project: &aria_models::Project,
    id: aria_models::ClipId,
    spans: &mut Vec<(i64, i64)>,
) {
    if let Some(clip) = project.clip(id) {
        spans.push((clip.timeline.start.value, clip.timeline.end_tick()));
    }
}

fn push_track_spans(
    project: &aria_models::Project,
    id: aria_models::TrackId,
    spans: &mut Vec<(i64, i64)>,
) {
    if let Some(track) = project.timeline().track(id) {
        spans.extend(
            track
                .clips()
                .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick())),
        );
    }
}

fn targets_touch_picture(
    project: &aria_models::Project,
    targets: &aria_commands::EditTargets,
) -> bool {
    if targets.clips.is_empty() && targets.tracks.is_empty() {
        return true;
    }
    let timeline = project.timeline();
    targets.tracks.iter().any(|&id| {
        timeline
            .track(id)
            .is_some_and(|track| track.kind.is_visual())
    }) || targets.clips.iter().any(|&id| {
        timeline
            .track_of(id)
            .and_then(|track| timeline.track(track))
            .is_some_and(|track| track.kind.is_visual())
    })
}

fn picture_neutral_edit(edit: &EditCommand) -> bool {
    match edit {
        EditCommand::ExtractAudio { .. }
        | EditCommand::SetReplaceable { .. }
        | EditCommand::SetTextEditable { .. }
        | EditCommand::SetClipPitch { .. }
        | EditCommand::SetClipAudio { .. }
        | EditCommand::SetClipDenoise { .. }
        | EditCommand::SetClipLabel { .. }
        | EditCommand::SetClipType { .. }
        | EditCommand::SetAudioRole { .. }
        | EditCommand::SetTrackName { .. }
        | EditCommand::SetTrackMuted { .. }
        | EditCommand::SetTrackLocked { .. }
        | EditCommand::SetTrackDuckSource { .. }
        | EditCommand::SetTrackTargeted { .. }
        | EditCommand::SetTrackSyncLock { .. }
        | EditCommand::SetTrackDisplay { .. }
        | EditCommand::SetTrackGain { .. }
        | EditCommand::SetTrackPan { .. }
        | EditCommand::SetTrackAutomation { .. }
        | EditCommand::SetTrackOutput { .. }
        | EditCommand::AddSend { .. }
        | EditCommand::SetSend { .. }
        | EditCommand::RemoveSend { .. }
        | EditCommand::LinkClips { .. }
        | EditCommand::UnlinkClips { .. }
        | EditCommand::GroupClips { .. }
        | EditCommand::UngroupClips { .. }
        | EditCommand::SetClipBeats { .. }
        | EditCommand::ClearBeats { .. }
        | EditCommand::SetAudioLayout { .. }
        | EditCommand::SetTimeDisplay { .. }
        | EditCommand::SetMasterMix { .. }
        | EditCommand::SetSourcePatch { .. } => true,
        EditCommand::SetParamKeyframe { param, .. }
        | EditCommand::RemoveParamKeyframe { param, .. }
        | EditCommand::SetParamConstant { param, .. }
        | EditCommand::SetParamDriver { param, .. }
        | EditCommand::ApplyEasingPreset { param, .. }
        | EditCommand::SetKeyframeEase { param, .. }
        | EditCommand::SetKeyframeRoving { param, .. } => {
            param.scope() == aria_models::ParamScope::Audio
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_invalidation_covers_every_picture_lane_of_the_same_kind() {
        let rate = aria_models::Rational::FPS_24;
        let mut project = aria_models::Project::new("cache", rate);
        let lower = project.add_track(aria_models::TrackKind::Text, "T1");
        let upper = project.add_track(aria_models::TrackKind::Text, "T2");
        let audio = project.add_track(aria_models::TrackKind::Audio, "A1");
        project
            .add_generated(
                lower,
                aria_models::Generator::text("Lower"),
                aria_models::TimeRange::at_rate(0, 24, rate),
            )
            .expect("lower title");
        project
            .add_generated(
                upper,
                aria_models::Generator::text("Upper"),
                aria_models::TimeRange::at_rate(48, 24, rate),
            )
            .expect("upper title");

        let visual = FilmstripImpact::capture(
            &Command::Edit(EditCommand::SetTrackSolo {
                track: upper,
                solo: true,
            }),
            &project,
        );
        let sound = FilmstripImpact::capture(
            &Command::Edit(EditCommand::SetTrackSolo {
                track: audio,
                solo: true,
            }),
            &project,
        );

        assert!(
            visual.picture,
            "soloing a picture lane changes the composite"
        );
        assert!(visual.before_tracks.contains(&(0, 24)));
        assert!(visual.before_tracks.contains(&(48, 72)));
        assert!(!sound.picture, "soloing an audio lane discarded filmstrips");
    }

    #[test]
    fn mixer_only_commands_have_no_picture_impact() {
        let project = aria_models::Project::new("cache", aria_models::Rational::FPS_24);
        let impact = FilmstripImpact::capture(
            &Command::Edit(EditCommand::SetAudioLayout {
                layout: aria_models::AudioLayout::Stereo,
            }),
            &project,
        );

        assert!(!impact.picture);
        assert!(!impact.full);
        assert!(impact.before.is_empty());
    }
}
