//! Contracts for filmstrip cells across a rolled-back edit.
//!
//! The class closed here has two halves. A rollback drops only the cells the
//! edits it reverted changed: a value scrub, a clip line drag, and a wheel
//! drag on one clip keep every other clip's cells through each pointer move,
//! which rolls back the move before it, and through Escape; a one-step edit
//! that fails keeps them too. And a change the store did not follow drops
//! every cell at the next edit, instead of keeping cells rendered against a
//! revision the store skipped: another origin's edit, a group dropped without
//! `roll_back`, a journal that does not link back to where the group opened.
//!
//! Not covered: undo and redo, and the assistant's groups, which edit outside
//! `apply_command` and drop every cell by the fallback the last half pins.

use std::rc::Rc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, ClipParam, ClipSource, ParamValue};
use aria_plugin::{Origin, Reverted};

use super::{FilmstripStore, JOURNAL, test_cell as cell};
use crate::app::AppState;
use crate::app::clip_line::LinePress;
use crate::app::grade_controls::WheelControl;
use crate::app::premiere_tests::with_clip;
use crate::app::value_scrub::{ScrubTarget, ValueKind};
use crate::motion::MotionKey;

/// Two pictures on the timeline: `a`, selected, and `b` a second after it,
/// with one tick inside each.
struct Pictures {
    state: AppState,
    a: ClipId,
    b: ClipId,
    in_a: i64,
    in_b: i64,
}

fn bounds(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state.ui_clip_ref(clip).expect("the clip is drawn");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}

fn pictures() -> Pictures {
    let (mut state, a) = with_clip();
    let media = match &state.engine().lock().project().clip(a).expect("a").content {
        ClipSource::Media { media, .. } => *media,
        _ => panic!("the fixture clip is not media"),
    };
    let (a_start, a_end) = bounds(&state, a);
    state
        .drop_media_on_timeline(media, None, a_end + 24)
        .expect("place b");
    let b = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|&clip| clip != a)
        .expect("b is on the timeline");
    state.select_clip(a);
    let (b_start, _) = bounds(&state, b);
    assert!(
        a_start + 1 < a_end && a_end < b_start,
        "a ends before b starts"
    );
    Pictures {
        state,
        a,
        b,
        in_a: a_start + 1,
        in_b: b_start + 1,
    }
}

/// Land a cell for `tick` rendered at the projected revision, as the
/// timeline's draw does.
fn land(state: &mut AppState, tick: i64) {
    let revision = state.revision;
    state.filmstrips.start(tick);
    state.filmstrips.finish(revision, tick, Some(cell()));
    assert!(
        state.filmstrips.get(tick).is_some(),
        "a cell rendered at the projected revision {revision} was rejected"
    );
}

fn has(state: &AppState, tick: i64) -> bool {
    state.filmstrips.get(tick).is_some()
}

fn opacity(clip: ClipId, value: f32) -> Command {
    Command::Edit(EditCommand::SetParamConstant {
        clip,
        param: ClipParam::Opacity,
        value: ParamValue::Scalar(value),
    })
}

/// A gesture that writes to the selected clip in an edit group and rolls
/// the group back on every move after the first and on Escape.
#[derive(Debug, Clone, Copy)]
enum Gesture {
    ValueScrub,
    ClipLine,
    Wheel,
}

const GESTURES: [Gesture; 3] = [Gesture::ValueScrub, Gesture::ClipLine, Gesture::Wheel];

/// The clip line's band, and the wheel's frame and center.
const BAND: f32 = 40.0;
const FRAME: [f32; 4] = [400.0, 300.0, 72.0, 72.0];
const CENTER: [f32; 2] = [436.0, 336.0];

impl Gesture {
    fn press(self, pictures: &mut Pictures) {
        let state = &mut pictures.state;
        match self {
            Self::ValueScrub => {
                let drawn = state
                    .param_scalar(ClipParam::Opacity, None)
                    .expect("opacity");
                state.press_value(ScrubTarget {
                    key: MotionKey::new("filmstrip-rollback", "opacity"),
                    value: drawn,
                    unit: 0.01,
                    kind: ValueKind::Number { decimals: 2 },
                    write: Rc::new(move |state: &mut AppState, delta| {
                        state.set_param_scalar(ClipParam::Opacity, None, drawn + delta)
                    }),
                });
            }
            Self::ClipLine => {
                assert!(state.press_clip_line(pictures.a, LinePress::Line(pictures.in_a), BAND))
            }
            Self::Wheel => assert!(state.press_wheel(WheelControl::Lift, FRAME, CENTER)),
        }
    }

    /// Move the held pointer to its `step`th place. Every place writes a
    /// different value.
    fn drag(self, state: &mut AppState, step: u8) {
        let step = f32::from(step);
        let moved = match self {
            Self::ValueScrub => state.drag_value(-10.0, false, false),
            Self::ClipLine => state.drag_clip_line(5.0 * step),
            Self::Wheel => state.drag_wheel([CENTER[0] + 6.0 * step, CENTER[1]]),
        };
        assert!(moved, "{self:?}: move {step} wrote nothing");
    }

    fn cancel(self, state: &mut AppState) {
        let held = match self {
            Self::ValueScrub => state.cancel_value_drag(),
            Self::ClipLine => state.cancel_clip_line(),
            Self::Wheel => state.cancel_wheel_drag(),
        };
        assert!(held, "{self:?}: Escape found nothing held");
    }
}

#[test]
fn a_gesture_keeps_the_cells_of_every_clip_it_did_not_edit() {
    for gesture in GESTURES {
        let mut pictures = pictures();
        let (in_a, in_b) = (pictures.in_a, pictures.in_b);
        land(&mut pictures.state, in_a);
        land(&mut pictures.state, in_b);
        gesture.press(&mut pictures);
        let state = &mut pictures.state;
        for step in 1..=3 {
            gesture.drag(state, step);
            assert!(
                !has(state, in_a),
                "{gesture:?}: move {step} kept a cell of the value it replaced"
            );
            assert!(
                has(state, in_b),
                "{gesture:?}: move {step} dropped a cell of a clip it did not edit"
            );
            // The timeline draws the moved value.
            land(state, in_a);
        }
        gesture.cancel(state);
        assert!(
            !has(state, in_a),
            "{gesture:?}: Escape kept a cell of the value it put back"
        );
        assert!(
            has(state, in_b),
            "{gesture:?}: Escape dropped a cell of a clip it did not edit"
        );
        state
            .set_param_scalar(ClipParam::Opacity, None, 0.3)
            .expect("an edit after Escape");
        assert!(
            has(state, in_b),
            "{gesture:?}: the edit after Escape dropped a cell of a clip it did not edit"
        );
    }
}

#[test]
fn a_failed_one_step_edit_keeps_the_cells_of_every_clip_it_did_not_edit() {
    let Pictures {
        mut state,
        in_a,
        in_b,
        ..
    } = pictures();
    land(&mut state, in_b);
    let failed = state.one_step(|state| {
        state.set_param_scalar(ClipParam::Opacity, None, 0.4)?;
        // A cell drawn while the edit was open.
        land(state, in_a);
        Err::<(), _>(EngineError::Refused("the second write is refused".into()))
    });
    assert!(failed.is_err());
    assert_eq!(state.param_scalar(ClipParam::Opacity, None), Some(1.0));
    assert!(
        !has(&state, in_a),
        "a cell of the value the failed edit put back was kept"
    );
    assert!(
        has(&state, in_b),
        "a failed edit on one clip dropped a cell of another"
    );
}

/// A change made outside `apply_command` and `roll_back`.
#[derive(Debug, Clone, Copy)]
enum Unfollowed {
    /// Another origin's edit through the project host.
    AnotherOrigin,
    /// A group dropped without `roll_back`, as a panic unwinding through a
    /// gesture drops it.
    DroppedGroup,
}

#[test]
fn a_change_the_store_did_not_follow_drops_every_cell_at_the_next_edit() {
    for unfollowed in [Unfollowed::AnotherOrigin, Unfollowed::DroppedGroup] {
        let mut pictures = pictures();
        let Pictures {
            state, a, b, in_a, ..
        } = &mut pictures;
        land(state, *in_a);
        match unfollowed {
            Unfollowed::AnotherOrigin => {
                state
                    .host()
                    .apply(&Origin::Assistant, opacity(*a, 0.2))
                    .expect("another origin's edit");
            }
            Unfollowed::DroppedGroup => {
                let group = state.host().group(&Origin::Editor).expect("open");
                state
                    .apply_command(opacity(*a, 0.2))
                    .expect("an edit in the group");
                land(state, *in_a);
                drop(group);
            }
        }
        // The next edit lands before anything projects the change.
        state.apply_command(opacity(*b, 0.5)).expect("edit b");
        assert!(
            !has(state, *in_a),
            "{unfollowed:?}: a cell rendered before the change survived the next edit"
        );
    }
}

/// A store at revision 4 with a cell at each of `ticks`.
fn store_with(ticks: &[i64]) -> FilmstripStore {
    let mut store = FilmstripStore::default();
    store.sync_revision(4);
    for &tick in ticks {
        store.start(tick);
        store.finish(4, tick, Some(cell()));
    }
    store
}

fn kept(store: &FilmstripStore, ticks: &[i64]) -> Vec<i64> {
    ticks
        .iter()
        .copied()
        .filter(|&tick| store.get(tick).is_some())
        .collect()
}

#[test]
fn an_edit_from_a_revision_the_store_skipped_drops_every_cell() {
    let ticks = [8, 24, 40];
    let mut store = store_with(&ticks);
    store.advance(5, 6, vec![(16, 32)]);
    assert_eq!(kept(&store, &ticks), [] as [i64; 0]);
    assert!(store.wants(8), "a dropped cell is rendered again");

    let mut store = store_with(&ticks);
    store.advance(4, 5, vec![(16, 32)]);
    assert_eq!(kept(&store, &ticks), [8, 40], "a followed edit");
}

#[test]
fn a_savepoint_and_its_group_revert_what_each_one_spanned() {
    let ticks = [8, 24, 40, 56, 72];
    let mut store = store_with(&ticks);
    // The group opens at 4 and edits around 24.
    store.advance(4, 5, vec![(16, 32)]);
    // A savepoint opens at 5, edits around 40, and rolls back to 7.
    store.advance(5, 6, vec![(32, 48)]);
    for tick in [24, 40] {
        store.start(tick);
        store.finish(6, tick, Some(cell()));
    }
    store.revert(Reverted {
        opened: 5,
        found: 6,
        left: 7,
    });
    assert_eq!(kept(&store, &ticks), [8, 24, 56, 72], "the savepoint");
    // The group edits around 56, then rolls back to 9.
    store.advance(7, 8, vec![(48, 64)]);
    store.revert(Reverted {
        opened: 4,
        found: 8,
        left: 9,
    });
    assert_eq!(kept(&store, &ticks), [8, 72], "the group");
    store.start(40);
    store.finish(9, 40, Some(cell()));
    assert!(
        store.get(40).is_some(),
        "a cell rendered after the rollback"
    );
}

#[test]
fn a_rollback_the_journal_does_not_link_drops_every_cell() {
    let ticks = [8, 24, 40];

    // A change the store did not follow sits between the group's edits.
    let mut store = store_with(&ticks);
    store.advance(4, 5, vec![(16, 32)]);
    store.sync_revision(6);
    for &tick in &ticks {
        store.start(tick);
        store.finish(6, tick, Some(cell()));
    }
    store.advance(6, 7, vec![(32, 48)]);
    store.revert(Reverted {
        opened: 4,
        found: 7,
        left: 8,
    });
    assert_eq!(kept(&store, &ticks), [] as [i64; 0], "a broken link");

    // The rollback found a revision the store is not at.
    let mut store = store_with(&ticks);
    store.advance(4, 5, vec![(16, 32)]);
    store.revert(Reverted {
        opened: 4,
        found: 6,
        left: 7,
    });
    assert_eq!(kept(&store, &ticks), [] as [i64; 0], "another revision");

    // A rollback that reverted nothing keeps everything, the render in
    // flight included.
    let mut store = store_with(&ticks);
    store.start(56);
    store.revert(Reverted {
        opened: 4,
        found: 4,
        left: 4,
    });
    assert_eq!(kept(&store, &ticks), ticks, "an empty rollback");
    assert!(
        !store.wants(72),
        "an empty rollback abandoned the render in flight"
    );
}

#[test]
fn a_rollback_reaching_past_the_journal_drops_every_cell() {
    let far = 1_000_000;
    for (edits, survives) in [(JOURNAL, true), (JOURNAL + 1, false)] {
        let mut store = store_with(&[far]);
        let edits = u64::try_from(edits).expect("a small count");
        for edit in 0..edits {
            let tick = i64::try_from(edit).expect("a small tick");
            store.advance(4 + edit, 5 + edit, vec![(tick, tick + 1)]);
        }
        assert!(store.journal.len() <= JOURNAL, "the journal is bounded");
        store.revert(Reverted {
            opened: 4,
            found: 4 + edits,
            left: 5 + edits,
        });
        assert_eq!(
            store.get(far).is_some(),
            survives,
            "{edits} edits: a cell outside every edit"
        );
    }
}
