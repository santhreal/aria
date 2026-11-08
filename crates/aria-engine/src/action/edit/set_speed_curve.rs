use aria_models::{ClipId, ModelError, Param};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set (or clear) a media clip's playback-rate ramp (CapCut speed curves,
/// M2). The model validates the curve, re-derives the timeline duration from
/// the curve's average, and checks neighbor overlap. The inverse is a
/// full-clip restore — the curve and the re-derived timeline range roll back
/// in one shot, like the constant-speed and param edits.
pub(crate) fn set_speed_curve(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    curve: Option<Param<f32>>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    let ids = ctx.project.linked_followers(clip);
    let max_duration = ids
        .iter()
        .filter_map(|&id| ctx.project.clip_room_after(id))
        .min();
    ctx.project
        .set_clip_speed_curve_bounded(clip, curve, max_duration)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

#[cfg(test)]
mod tests {
    use aria_commands::{Command, EditCommand};
    use aria_models::{
        MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind, speed_preset,
    };

    use crate::{Engine, EngineConfig};

    const RATE: Rational = Rational::new(24, 1);

    #[test]
    fn lengthening_ramp_and_slowdown_clamp_to_track_and_linked_lane_neighbors_and_undo() {
        let mut project = Project::new("bounded_speed", RATE);
        let media = project.add_media(MediaSource::new("/m.mp4", 1920, 1080, RATE, 600, true));
        let v1 = project.add_track(TrackKind::Video, "V1");
        let a1 = project.add_track(TrackKind::Audio, "A1");
        let c_v1 = project
            .add_clip(
                v1,
                media,
                TimeRange::at_rate(0, 100, RATE),
                RationalTime::new(0, RATE),
            )
            .expect("v1 clip");
        let neighbor_v1 = project
            .add_clip(
                v1,
                media,
                TimeRange::at_rate(100, 100, RATE),
                RationalTime::new(108, RATE),
            )
            .expect("v1 neighbor at 108");
        let c_a1 = project
            .add_clip(
                a1,
                media,
                TimeRange::at_rate(0, 100, RATE),
                RationalTime::new(0, RATE),
            )
            .expect("a1 clip");
        let neighbor_a1 = project
            .add_clip(
                a1,
                media,
                TimeRange::at_rate(100, 100, RATE),
                RationalTime::new(104, RATE),
            )
            .expect("a1 tighter neighbor at 104");
        project.link_clips(&[c_v1, c_a1]).expect("link v1 and a1");

        let mut engine = Engine::with_project(EngineConfig::default(), project).expect("engine");
        let hero = speed_preset("hero").expect("hero");
        engine
            .apply(Command::Edit(EditCommand::SetSpeedCurve {
                clip: c_v1,
                curve: Some(hero),
            }))
            .expect("hero clamps to tighter linked neighbor at 104");
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").timeline,
            TimeRange::at_rate(0, 104, RATE)
        );
        assert_eq!(
            engine.project().clip(neighbor_v1).expect("nv1").timeline,
            TimeRange::at_rate(108, 100, RATE)
        );
        assert_eq!(
            engine.project().clip(neighbor_a1).expect("na1").timeline,
            TimeRange::at_rate(104, 100, RATE)
        );
        assert!(engine.undo());
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").timeline,
            TimeRange::at_rate(0, 100, RATE)
        );
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").source_range(),
            Some(TimeRange::at_rate(0, 100, RATE))
        );

        // Constant slowdown (0.5x -> 200 ticks uncapped) clamps both linked
        // clips to 104 ticks and restores both on undo.
        engine
            .apply(Command::Edit(EditCommand::SetClipSpeed {
                clip: c_v1,
                speed: Rational::new(1, 2),
                reversed: false,
            }))
            .expect("0.5x clamps both linked clips to 104");
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").timeline,
            TimeRange::at_rate(0, 104, RATE)
        );
        assert_eq!(
            engine.project().clip(c_a1).expect("a1").timeline,
            TimeRange::at_rate(0, 104, RATE)
        );
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").source_range(),
            Some(TimeRange::at_rate(0, 52, RATE))
        );
        assert!(engine.undo());
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").timeline,
            TimeRange::at_rate(0, 100, RATE)
        );
        assert_eq!(
            engine.project().clip(c_a1).expect("a1").timeline,
            TimeRange::at_rate(0, 100, RATE)
        );
        assert_eq!(
            engine.project().clip(c_v1).expect("v1").source_range(),
            Some(TimeRange::at_rate(0, 100, RATE))
        );
    }
}
