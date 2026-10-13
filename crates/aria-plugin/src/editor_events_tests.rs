//! The editor view's contract: one notification per part of the view that
//! changed since the last drawn frame, none for the first frame or a frame
//! that changed nothing, and playback events only on a transition or a speed
//! change while playing. A context menu collects rows from every handler, in
//! registration order.
//!
//! Not caught here: the editor calling `publish` once per frame; the editor's
//! suite drives the window for that.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use aria_core::{Rational, RationalTime};
use aria_models::{ClipId, Project};
use parking_lot::Mutex;

use super::{
    ContextMenuRequested, EditorView, InOutChanged, MenuTarget, PlaybackStarted, PlaybackStopped,
    PlayheadMoved, SelectionChanged, ViewFrame, ViewState,
};
use crate::Registration;
use crate::events::{Event, EventBus};

const BOUND: Duration = Duration::from_secs(10);

const RATE: Rational = Rational::new(24, 1);

const A: ClipId = ClipId::from_raw(1);
const B: ClipId = ClipId::from_raw(2);

fn at(frame: i64) -> RationalTime {
    RationalTime::new(frame, RATE)
}

#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Selection(Vec<ClipId>),
    Playhead(RationalTime),
    InOut(Option<RationalTime>, Option<RationalTime>),
    Started(RationalTime, i32),
    Stopped(RationalTime),
}

struct Recorder {
    seen: Arc<Mutex<Vec<Seen>>>,
    bus: Arc<EventBus>,
    _registrations: Vec<Registration>,
}

/// Record each `E` delivered on `bus` into `seen` as `to_seen` describes it.
fn record<E: Event>(
    bus: &Arc<EventBus>,
    seen: &Arc<Mutex<Vec<Seen>>>,
    to_seen: fn(&E) -> Seen,
) -> Registration {
    let seen = Arc::clone(seen);
    bus.subscribe::<E, _>(move |event| seen.lock().push(to_seen(event)))
}

impl Recorder {
    fn new(bus: &Arc<EventBus>) -> Self {
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let registrations = vec![
            record::<SelectionChanged>(bus, &seen, |e| Seen::Selection(e.clips.clone())),
            record::<PlayheadMoved>(bus, &seen, |e| Seen::Playhead(e.at)),
            record::<InOutChanged>(bus, &seen, |e| Seen::InOut(e.mark_in, e.mark_out)),
            record::<PlaybackStarted>(bus, &seen, |e| Seen::Started(e.at, e.speed)),
            record::<PlaybackStopped>(bus, &seen, |e| Seen::Stopped(e.at)),
        ];
        Self {
            seen,
            bus: Arc::clone(bus),
            _registrations: registrations,
        }
    }

    /// Everything delivered since the last call.
    fn take(&self) -> Vec<Seen> {
        assert!(self.bus.flush(BOUND), "delivery must finish within the bound");
        std::mem::take(&mut *self.seen.lock())
    }
}

fn stopped_at_zero(selection: &[ClipId]) -> ViewState {
    ViewState {
        selection: selection.to_vec(),
        playhead: at(0),
        mark_in: None,
        mark_out: None,
        playing: false,
        speed: 0,
    }
}

fn frame(state: &ViewState) -> ViewFrame<'_> {
    ViewFrame {
        selection: &state.selection,
        playhead: state.playhead,
        mark_in: state.mark_in,
        mark_out: state.mark_out,
        playing: state.playing,
        speed: state.speed,
    }
}

struct Step {
    name: &'static str,
    change: fn(&mut ViewState),
    expected: Vec<Seen>,
}

fn step(name: &'static str, change: fn(&mut ViewState), expected: Vec<Seen>) -> Step {
    Step {
        name,
        change,
        expected,
    }
}

#[test]
fn each_changed_part_of_the_view_posts_its_own_notification() {
    let bus = Arc::new(EventBus::new());
    let recorder = Recorder::new(&bus);
    let view = EditorView::new();
    assert_eq!(view.current(), None, "nothing is recorded before the first frame");

    let mut current = stopped_at_zero(&[]);
    view.publish(frame(&current), &bus);
    assert!(recorder.take().is_empty(), "the first frame is recorded, not announced");
    assert_eq!(view.current().as_ref(), Some(&current));

    let steps = [
        step("unchanged", |_| {}, vec![]),
        step("select", |s| s.selection = vec![A], vec![Seen::Selection(vec![A])]),
        step(
            "extend the selection",
            |s| s.selection.push(B),
            vec![Seen::Selection(vec![A, B])],
        ),
        step(
            "reorder the selection",
            |s| s.selection.reverse(),
            vec![Seen::Selection(vec![B, A])],
        ),
        step("seek", |s| s.playhead = at(10), vec![Seen::Playhead(at(10))]),
        step(
            "mark in",
            |s| s.mark_in = Some(at(5)),
            vec![Seen::InOut(Some(at(5)), None)],
        ),
        step(
            "mark out",
            |s| s.mark_out = Some(at(20)),
            vec![Seen::InOut(Some(at(5)), Some(at(20)))],
        ),
        step("speed while stopped", |s| s.speed = 1, vec![]),
        step("play", |s| s.playing = true, vec![Seen::Started(at(10), 1)]),
        step(
            "advance while playing",
            |s| s.playhead = at(11),
            vec![Seen::Playhead(at(11))],
        ),
        step(
            "speed up while playing",
            |s| s.speed = 2,
            vec![Seen::Started(at(11), 2)],
        ),
        step(
            "reverse while playing",
            |s| s.speed = -1,
            vec![Seen::Started(at(11), -1)],
        ),
        step(
            "stop",
            |s| {
                s.playing = false;
                s.speed = 0;
            },
            vec![Seen::Stopped(at(11))],
        ),
        step(
            "everything at once",
            |s| {
                s.selection.clear();
                s.playhead = at(0);
                s.mark_in = None;
                s.mark_out = None;
                s.playing = true;
                s.speed = 1;
            },
            vec![
                Seen::Selection(vec![]),
                Seen::Playhead(at(0)),
                Seen::InOut(None, None),
                Seen::Started(at(0), 1),
            ],
        ),
        step(
            "stop and seek",
            |s| {
                s.playing = false;
                s.playhead = at(3);
            },
            vec![Seen::Playhead(at(3)), Seen::Stopped(at(3))],
        ),
        step("unchanged again", |_| {}, vec![]),
    ];
    for Step {
        name,
        change,
        expected,
    } in steps
    {
        change(&mut current);
        view.publish(frame(&current), &bus);
        assert_eq!(recorder.take(), expected, "{name}");
        assert_eq!(
            view.current().as_ref(),
            Some(&current),
            "{name}: the view reads the last frame"
        );
    }
}

#[test]
fn a_view_nobody_listens_to_still_records_each_frame() {
    let bus = Arc::new(EventBus::new());
    let view = EditorView::new();
    let mut current = stopped_at_zero(&[A]);
    view.publish(frame(&current), &bus);
    current.playhead = at(42);
    current.playing = true;
    view.publish(frame(&current), &bus);
    assert_eq!(view.current(), Some(current));
}

#[test]
fn a_context_menu_collects_rows_from_every_handler_in_registration_order() {
    let bus = Arc::new(EventBus::new());
    let ran = Arc::new(AtomicUsize::new(0));
    let seen_targets: Arc<Mutex<Vec<MenuTarget>>> = Arc::default();
    let _first = {
        let ran = Arc::clone(&ran);
        let seen = Arc::clone(&seen_targets);
        bus.waterfall::<ContextMenuRequested, _>(move |menu| {
            seen.lock().push(menu.target.clone());
            let ran = Arc::clone(&ran);
            menu.add("First", move || {
                ran.fetch_add(1, Ordering::SeqCst);
            });
        })
    };
    let _silent = {
        let seen = Arc::clone(&seen_targets);
        bus.waterfall::<ContextMenuRequested, _>(move |menu| seen.lock().push(menu.target.clone()))
    };
    let _last = {
        let ran = Arc::clone(&ran);
        bus.waterfall::<ContextMenuRequested, _>(move |menu| {
            menu.add("Second", || {});
            let ran = Arc::clone(&ran);
            menu.add("Third", move || {
                ran.fetch_add(10, Ordering::SeqCst);
            });
        })
    };

    let target = MenuTarget::Viewer;
    let project = Arc::new(Project::new("menu", RATE));
    let mut menu = ContextMenuRequested::new(target.clone(), project);
    bus.emit_waterfall(&mut menu);

    let labels: Vec<&str> = menu.entries.iter().map(|entry| entry.label.as_str()).collect();
    assert_eq!(labels, ["First", "Second", "Third"]);
    assert_eq!(
        *seen_targets.lock(),
        [target.clone(), target],
        "every handler sees the target"
    );
    assert_eq!(ran.load(Ordering::SeqCst), 0, "opening the menu runs no row");
    (menu.entries[2].run)();
    (menu.entries[0].run)();
    assert_eq!(ran.load(Ordering::SeqCst), 11, "each row runs its own action");
}
