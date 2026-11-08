//! Contracts for the filmstrip cells an edit drops on lanes its payload does
//! not name.
//!
//! WHY: an edit dropped the cells of the clips and lanes it named, so a ripple
//! moved the clips of every other sync-locked lane and left their cells in
//! place: the strip of a lane that rode along kept the frames it showed before
//! the edit. A ripple addressed to a sound lane dropped no cell at all, though
//! every picture lane rode along. The class closed is a picture that changes
//! on a lane the payload does not name. The oracle reads what each picture lane
//! plays at each sampled tick before and after the edit, independently of the
//! spans the edit reports, and every tick where any lane plays another frame
//! must lose its cell. A cell past every clip must stay, so dropping every cell
//! does not pass.
//!
//! Not covered: an edit that changes the frame a clip opens on without moving
//! it, on a lane the payload does not name.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipSource, MediaId, RationalTime, TimeRange, TrackId, TrackKind};

use super::{has, land};
use crate::app::AppState;
use crate::app::fixtures::{asset, with_clip};

/// Each picture lane and its clips: the clip, its span, and the source frame
/// it opens on.
type Layout = Vec<(TrackId, Vec<(ClipId, i64, i64, Option<i64>)>)>;

fn layout(state: &AppState) -> Layout {
    let engine = state.engine().lock();
    engine
        .project()
        .timeline()
        .tracks_ordered()
        .filter(|lane| lane.kind.is_visual())
        .map(|lane| {
            let clips = lane
                .clips()
                .map(|clip| {
                    (
                        clip.id,
                        clip.timeline.start.value,
                        clip.timeline.end_tick(),
                        clip.source_range().map(|source| source.start.value),
                    )
                })
                .collect();
            (lane.id, clips)
        })
        .collect()
}

/// What each picture lane plays at `tick`: the clip covering it, where that
/// clip starts, and the source frame it opens on.
fn played(layout: &Layout, tick: i64) -> Vec<(TrackId, Option<(ClipId, i64, Option<i64>)>)> {
    layout
        .iter()
        .map(|(lane, clips)| {
            let covering = clips
                .iter()
                .find(|(_, start, end, _)| *start <= tick && tick < *end)
                .map(|&(clip, start, _, source)| (clip, start, source));
            (*lane, covering)
        })
        .collect()
}

/// A picture `a` at the start of the first lane and a copy of it two lengths
/// later there; a copy on a second picture lane a length after `a` ends; a
/// tone at the start of a sound lane. Every lane has its sync lock set.
struct Lanes {
    state: AppState,
    a: ClipId,
    pictures: TrackId,
    picture: MediaId,
    source: TimeRange,
    length: i64,
    tone: ClipId,
    sounds: TrackId,
    sound: MediaId,
    tone_source: TimeRange,
    tone_length: i64,
    rate: aria_models::Rational,
}

impl Lanes {
    fn at(&self, tick: i64) -> RationalTime {
        RationalTime::new(tick, self.rate)
    }
}

fn place(
    state: &mut AppState,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
) {
    state
        .apply_command(Command::Edit(EditCommand::AddClip {
            track,
            media,
            source,
            start: at,
        }))
        .expect("place a copy");
}

fn lanes() -> Lanes {
    let (mut state, a) = with_clip();
    let (pictures, picture, source, length, rate) = {
        let engine = state.engine().lock();
        let project = engine.project();
        let clip = project.clip(a).expect("a");
        let ClipSource::Media { media, .. } = &clip.content else {
            panic!("the fixture clip is not media");
        };
        (
            project.timeline().track_of(a).expect("a's lane"),
            *media,
            clip.source_range().expect("a media clip"),
            clip.timeline.end_tick() - clip.timeline.start.value,
            clip.timeline.start.rate,
        )
    };
    assert!(length >= 12, "a is {length} ticks, too short to sample");
    place(
        &mut state,
        pictures,
        picture,
        source,
        RationalTime::new(3 * length, rate),
    );
    let rider = state
        .add_lane_of_kind(TrackKind::Video, None)
        .expect("a second picture lane");
    place(
        &mut state,
        rider,
        picture,
        source,
        RationalTime::new(2 * length, rate),
    );
    let sound = state
        .import_media(asset("fixture-sine-48k-2s.wav"))
        .expect("import the tone");
    state
        .drop_media_on_timeline(sound, None, 0)
        .expect("place the tone");
    let (tone, sounds, tone_source, tone_length) = {
        let engine = state.engine().lock();
        let timeline = engine.project().timeline();
        assert!(
            timeline.tracks_ordered().all(|lane| lane.sync_lock),
            "every lane rides a ripple"
        );
        let lane = timeline
            .tracks_ordered()
            .find(|lane| lane.kind == TrackKind::Audio)
            .expect("a sound lane");
        let clip = lane.clips().next().expect("the tone");
        (
            clip.id,
            lane.id,
            clip.source_range().expect("a media clip"),
            clip.timeline.end_tick() - clip.timeline.start.value,
        )
    };
    state.sync_from_engine();
    Lanes {
        state,
        a,
        pictures,
        picture,
        source,
        length,
        tone,
        sounds,
        sound,
        tone_source,
        tone_length,
        rate,
    }
}

/// A rippling edit, addressed to the first picture lane or to the sound lane.
#[derive(Debug, Clone, Copy)]
enum Ripple {
    DeletePicture,
    InsertPicture,
    OpenGap,
    DeleteSound,
    InsertSound,
}

const RIPPLES: [Ripple; 5] = [
    Ripple::DeletePicture,
    Ripple::InsertPicture,
    Ripple::OpenGap,
    Ripple::DeleteSound,
    Ripple::InsertSound,
];

impl Ripple {
    fn command(self, lanes: &Lanes) -> Command {
        Command::Edit(match self {
            Self::DeletePicture => EditCommand::RippleDelete { clip: lanes.a },
            Self::InsertPicture => EditCommand::RippleInsert {
                track: lanes.pictures,
                media: lanes.picture,
                source: lanes.source,
                at: lanes.at(0),
            },
            Self::OpenGap => EditCommand::OpenGap {
                tracks: vec![lanes.pictures],
                at: lanes.at(0),
                duration: lanes.at(lanes.length),
            },
            Self::DeleteSound => EditCommand::RippleDelete { clip: lanes.tone },
            Self::InsertSound => EditCommand::RippleInsert {
                track: lanes.sounds,
                media: lanes.sound,
                source: lanes.tone_source,
                at: lanes.at(0),
            },
        })
    }
}

#[test]
fn a_ripple_drops_the_cells_of_every_picture_lane_it_moves() {
    for ripple in RIPPLES {
        let mut lanes = lanes();
        let horizon = 6 * lanes.length + lanes.tone_length;
        let stride = usize::try_from(lanes.length / 6).expect("a positive stride");
        let ticks: Vec<i64> = (0..horizon).step_by(stride).map(|tick| tick + 1).collect();
        let far = 4 * horizon;
        let before = layout(&lanes.state);
        for &tick in ticks.iter().chain([&far]) {
            land(&mut lanes.state, tick);
        }

        let command = ripple.command(&lanes);
        lanes
            .state
            .apply_command(command)
            .unwrap_or_else(|error| panic!("{ripple:?}: {error:?}"));

        let after = layout(&lanes.state);
        let changed: Vec<i64> = ticks
            .iter()
            .copied()
            .filter(|&tick| played(&before, tick) != played(&after, tick))
            .collect();
        assert!(!changed.is_empty(), "{ripple:?} moved no picture");
        let stale: Vec<i64> = changed
            .into_iter()
            .filter(|&tick| has(&lanes.state, tick))
            .collect();
        assert_eq!(
            stale,
            Vec::<i64>::new(),
            "{ripple:?}: cells kept at ticks where a picture lane plays another frame"
        );
        assert!(
            has(&lanes.state, far),
            "{ripple:?} dropped a cell past every clip"
        );
    }
}
