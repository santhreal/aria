//! The edits an edge drag and a slide send, as steps, and what each clip
//! drag that changes clips in place draws while the button is down.
//!
//! A trim, a ripple trim, a roll, a slide and a rate stretch each send one or
//! more of four edits, in order: a trim, a move, a shift of the rest of a lane
//! and a retime. The release sends them to the engine as one undo entry.
//! While the button is down the frame runs the same steps on the clips they
//! reach, with the model's own trim and retime arithmetic, on the project as
//! the drag found it: the drag draws what its release leaves.
//!
//! A transition at a cut the steps move is judged once, by where the steps
//! leave its clips ([`kept`]): it stays while they meet, shortened to the
//! longest window they hold, and goes once they part. The engine judges each
//! edit alone, and an edit that opens a cut drops its transition, so a roll
//! or a slide, whose first trim opens the cut its last one closes, would drop
//! the transition it carries.
//!
//! A slip, a transition window drag and a fade drag each send one
//! [`Setting`]. The held frame draws the clips it changes with it: a slip's
//! clips playing their slipped takes, through the model's own slip, and a
//! transition's window or a fade at the length the release sets.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Clip, ClipId, Project, Rational, RationalTime, TimeRange, TrackId, Transition};

use crate::app::AppState;
use crate::projection::Placement;
use crate::surfaces::timeline_gesture::{self, Drag, Gesture, Setting};
use crate::types::{HeldCard, HeldEdge, HeldEdit, LaneShift, TakeSpan, UiClip};

/// One edit of a drag's release, in timeline ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Trim `clip` to `start..end`. With linked selection on, every clip
    /// linked to it takes the same edge deltas.
    Trim { clip: ClipId, start: i64, end: i64 },
    /// Move `clip` to `start` on its lane. Every clip linked or grouped with
    /// it moves by the same distance, stopped at the sequence start; with
    /// linked selection off only the clips grouped with it do.
    Move { clip: ClipId, start: i64 },
    /// Move every clip on `lane` that starts at or after `from` by `by`.
    Shift { lane: TrackId, from: i64, by: i64 },
    /// Play `clip`'s take over `duration` ticks from where it starts. Every
    /// clip linked to it takes the same speed.
    Retime { clip: ClipId, duration: i64 },
}

/// What a drag sends on release, and the edge it holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    pub steps: Vec<Step>,
    pub edge: Option<HeldEdge>,
}

impl AppState {
    /// Send `steps` as one undo entry, and settle each transition at a cut
    /// they move ([`kept`]). A refused step rolls back the steps before it.
    /// The entry is named `name`, or after its first edit without one.
    pub(crate) fn run_steps(
        &mut self,
        name: Option<&'static str>,
        steps: &[Step],
    ) -> Result<(), EngineError> {
        self.snap_guide = None;
        if steps.is_empty() {
            return Ok(());
        }
        let cuts = Cut::all(self.engine.lock().project());
        match steps {
            [step] if name.is_none() && cuts.is_empty() => self.run_step(*step),
            _ => self.one_step_named(
                |_| name,
                |this| {
                    steps.iter().try_for_each(|step| this.run_step(*step))?;
                    this.settle_cuts(&cuts)
                },
            ),
        }
    }

    /// Keep, shorten or drop each transition in `cuts` whose clips the steps
    /// moved, as [`kept`] reads where they left them.
    fn settle_cuts(&mut self, cuts: &[Cut]) -> Result<(), EngineError> {
        for cut in cuts {
            let clip = cut.transition.left;
            let (now, held) = {
                let engine = self.engine.lock();
                let project = engine.project();
                let span = |id: ClipId| project.clip(id).map(|clip| clip.timeline);
                (
                    span(clip).zip(span(cut.transition.right)),
                    project.transition_at(clip).map(|held| held.duration),
                )
            };
            if now == Some((cut.left, cut.right)) {
                continue;
            }
            let keep = now.and_then(|(left, right)| kept(&cut.transition, left, right));
            let edit = match (keep, held) {
                (Some(duration), None) => {
                    self.apply_command(Command::Edit(EditCommand::AddTransition {
                        clip,
                        transition_id: cut.transition.transition_id.clone(),
                    }))?;
                    EditCommand::SetTransitionState {
                        clip,
                        transition: Transition {
                            duration,
                            ..cut.transition.clone()
                        },
                    }
                }
                (Some(duration), Some(held)) if held != duration => {
                    EditCommand::SetTransition { clip, duration }
                }
                (None, Some(_)) => EditCommand::RemoveTransition { clip },
                _ => continue,
            };
            self.apply_command(Command::Edit(edit))?;
        }
        Ok(())
    }

    fn run_step(&mut self, step: Step) -> Result<(), EngineError> {
        let rate = self.playhead.rate;
        let edit = match step {
            Step::Trim { clip, start, end } => {
                self.trim_command(clip, TimeRange::at_rate(start, end - start, rate))
            }
            Step::Move { clip, start } => {
                let track = self.track_of(clip).ok_or_else(|| {
                    EngineError::Refused("That clip is no longer on the timeline".into())
                })?;
                self.move_command(clip, track, RationalTime::new(start, rate))
            }
            Step::Shift { lane, from, by } => EditCommand::ShiftClips {
                track: lane,
                from: RationalTime::new(from, rate),
                delta: RationalTime::new(by, rate),
            },
            Step::Retime { clip, duration } => {
                let engine = self.engine.lock();
                let held = engine.project().require_clip(clip)?;
                EditCommand::SetClipSpeed {
                    clip,
                    speed: held.speed_for(duration, rate)?,
                    reversed: held.reversed,
                }
            }
        };
        self.apply_command(Command::Edit(edit))?;
        Ok(())
    }

    /// Hold what the drag in flight draws while the button is down: its
    /// room, its readout, the magnet it lands on, the clips a move takes and
    /// the clips an edge drag, a slide, a slip, a transition window drag or a
    /// fade drag changes. The press reads the session and every move after it
    /// reads what the press read, so a move never waits for a preview decode.
    pub(crate) fn hold_drag(&mut self, gesture: Gesture, drag: Drag) {
        let project = self.drag_project();
        self.snap_guide = None;
        self.hold_room(gesture.room(), drag.clip, &project);
        self.trim_feedback = self.compute_trim_feedback(&project, gesture, drag);
        self.hold_move(gesture, drag, &project);
        let Some(held) = timeline_gesture::held(self, gesture, &drag) else {
            self.held_edit = timeline_gesture::setting(self, &project, gesture, &drag)
                .and_then(|set| settled(self, &project, drag.clip, set))
                .map(Arc::new);
            return;
        };
        self.snap_guide = held.guide;
        let plan = timeline_gesture::plan(self, gesture, &drag, held);
        self.held_edit = Some(Arc::new(preview(self, &project, &plan)));
    }

    /// Drop what the drag in flight holds. The release drops it before it
    /// applies, so the engine never copies the project for a snapshot the
    /// drag still holds.
    pub(crate) fn end_drag(&mut self) {
        self.snap_guide = None;
        self.trim_feedback = None;
        self.held_move = None;
        self.held_room = None;
        self.held_edit = None;
        self.drag_project = None;
    }

    /// Whether a move of `clip` may take `track`: the lane's kind holds the
    /// clip, read off the project the drag found. A move carried over a lane
    /// that refuses the clip keeps the lane it had, so its release never sends
    /// an edit the engine refuses.
    pub(crate) fn drag_lands_on(&mut self, clip: ClipId, track: TrackId) -> bool {
        let project = self.drag_project();
        let timeline = project.timeline();
        timeline
            .clip(clip)
            .zip(timeline.track(track))
            .is_some_and(|(clip, track)| track.kind.accepts_clip(clip))
    }

    /// The project as the drag in flight found it, read at its press: reading
    /// it locks the engine.
    fn drag_project(&mut self) -> Arc<Project> {
        match &self.drag_project {
            Some((revision, project)) if *revision == self.revision => Arc::clone(project),
            Some(_) | None => {
                let project = self.engine.lock().snapshot();
                self.drag_project = Some((self.revision, Arc::clone(&project)));
                project
            }
        }
    }
}

/// The clips `plan` changes, as its release leaves them, built from the
/// store's cards and from `project`, the project as the drag found it.
fn preview(state: &AppState, project: &Project, plan: &Plan) -> HeldEdit {
    let mut build = Build {
        state,
        project,
        rate: project.timeline().frame_rate,
        reached: Vec::new(),
        shifts: Vec::new(),
    };
    for &step in &plan.steps {
        build.step(step);
    }
    build.settle_cuts();
    let Build {
        reached,
        shifts,
        rate,
        ..
    } = build;
    HeldEdit {
        cards: reached
            .into_iter()
            .map(|reached| {
                let mut card = reached.ui;
                Placement::of(&reached.clip, rate, reached.audio).write(&mut card);
                HeldCard {
                    clip: card,
                    was: reached.was,
                }
            })
            .collect(),
        shifts,
        edge: plan.edge,
    }
}

/// The clips `set` changes on `clip`, as its release leaves them, built from
/// the store's cards and from `project`, the project as the drag found it.
/// `None` when it changes no card: a transition drag at a junction with no
/// transition, or a slip the model refuses for any clip it reaches, which the
/// engine refuses whole.
fn settled(state: &AppState, project: &Project, clip: ClipId, set: Setting) -> Option<HeldEdit> {
    let rate = project.timeline().frame_rate;
    let card = |id: ClipId| {
        let ui = state.ui_clip_ref(id)?;
        Some((ui.clone(), TakeSpan::of(ui)))
    };
    let cards: Vec<HeldCard> = match set {
        // Each clip the slip reaches, slipped by its own share in the ticks
        // of its own take, as the engine slips it.
        Setting::Slip(by) => project
            .slip_reach(clip, by)
            .ok()?
            .into_iter()
            .map(|(member, by)| {
                let mut held = project.clip(member)?.clone();
                let window = project.slip_source(&held, by).ok()?;
                let timeline = held.timeline;
                held.trim_to(timeline, Some(window));
                let (mut ui, was) = card(member)?;
                Placement::of(&held, rate, on_audio_lane(project, member)).write(&mut ui);
                Some(HeldCard { clip: ui, was })
            })
            .collect::<Option<_>>()?,
        Setting::Transition(ticks) => {
            let (mut ui, was) = card(clip)?;
            ui.transition.as_mut()?.duration_ticks = ticks;
            vec![HeldCard { clip: ui, was }]
        }
        Setting::Fade { left, ticks } => {
            let (mut ui, was) = card(clip)?;
            if left {
                ui.fade_in_ticks = ticks;
            } else {
                ui.fade_out_ticks = ticks;
            }
            vec![HeldCard { clip: ui, was }]
        }
    };
    (!cards.is_empty()).then_some(HeldEdit {
        cards,
        shifts: Vec::new(),
        edge: None,
    })
}

/// Whether `clip` is on a sound lane of `project`.
fn on_audio_lane(project: &Project, clip: ClipId) -> bool {
    let lanes = project.timeline();
    lanes
        .track_of(clip)
        .and_then(|lane| lanes.track(lane))
        .is_some_and(|lane| lane.kind.is_audio())
}

/// A transition at a cut, with the spans its clips held when the steps
/// began.
struct Cut {
    track: TrackId,
    transition: Transition,
    left: TimeRange,
    right: TimeRange,
}

impl Cut {
    /// Every transition in `project`, with the spans of its clips.
    fn all(project: &Project) -> Vec<Self> {
        project
            .timeline()
            .tracks_ordered()
            .flat_map(|track| {
                track.transitions().iter().filter_map(move |transition| {
                    Some(Self {
                        track: track.id,
                        left: track.clip(transition.left)?.timeline,
                        right: track.clip(transition.right)?.timeline,
                        transition: transition.clone(),
                    })
                })
            })
            .collect()
    }
}

/// The window `transition` keeps once its clips span `left` and `right`:
/// its own while they meet, shortened to the longest window they hold
/// ([`aria_models::TransitionAlignment::junction_limit`]), and none once
/// they part.
fn kept(transition: &Transition, left: TimeRange, right: TimeRange) -> Option<i64> {
    (left.end_tick() == right.start.value).then(|| {
        let limit = transition
            .alignment
            .junction_limit(left.duration.value, right.duration.value);
        transition.duration.min(limit)
    })
}

/// A clip a step reaches: the model clip as the steps so far leave it, the
/// card the store draws for it, and where it played its take before.
struct Reached {
    clip: Clip,
    ui: UiClip,
    was: TakeSpan,
    audio: bool,
}

impl Reached {
    fn span(&self) -> (i64, i64) {
        (
            self.clip.timeline.start.value,
            self.clip.timeline.end_tick(),
        )
    }

    fn shift(&mut self, by: i64, rate: Rational) {
        let (start, end) = self.span();
        self.clip.timeline = TimeRange::at_rate(start + by, end - start, rate);
    }
}

struct Build<'a> {
    state: &'a AppState,
    project: &'a Project,
    rate: Rational,
    reached: Vec<Reached>,
    shifts: Vec<LaneShift>,
}

impl Build<'_> {
    fn step(&mut self, step: Step) {
        match step {
            Step::Trim { clip, start, end } => {
                let Some(at) = self.reach(clip) else {
                    return;
                };
                let (from, to) = self.reached[at].span();
                let (head, tail) = (start - from, end - to);
                let members = if self.state.linked_selection {
                    self.linked(clip)
                } else {
                    vec![clip]
                };
                for member in members {
                    let Some(at) = self.reach(member) else {
                        continue;
                    };
                    let (from, to) = self.reached[at].span();
                    let range =
                        TimeRange::at_rate(from + head, (to + tail) - (from + head), self.rate);
                    let reached = &mut self.reached[at];
                    // A trim the take refuses is one the room holds the drag
                    // short of, so the card keeps its take rather than drawing
                    // a guess.
                    if let Ok(source) = self.project.trim_source(&reached.clip, range) {
                        reached.clip.trim_to(range, source);
                    }
                }
            }
            Step::Move { clip, start } => {
                let Some(at) = self.reach(clip) else {
                    return;
                };
                let by = start - self.reached[at].span().0;
                let members = if self.state.linked_selection {
                    self.project.move_companions(clip)
                } else {
                    self.project.group_companions(clip)
                };
                for member in members {
                    let Some(at) = self.reach(member) else {
                        continue;
                    };
                    let from = self.reached[at].span().0;
                    let to = if member == clip {
                        start
                    } else {
                        (from + by).max(0)
                    };
                    self.reached[at].shift(to - from, self.rate);
                }
            }
            Step::Shift { lane, from, by } => {
                let lane = lane.raw();
                for reached in &mut self.reached {
                    if reached.ui.track_id == lane && reached.span().0 >= from {
                        reached.shift(by, self.rate);
                    }
                }
                self.shifts.push(LaneShift { lane, from, by });
            }
            Step::Retime { clip, duration } => {
                let Some(at) = self.reach(clip) else {
                    return;
                };
                let held = &self.reached[at].clip;
                let Ok(speed) = held.speed_for(duration, self.rate) else {
                    return;
                };
                let reversed = held.reversed;
                for member in self.linked(clip) {
                    if let Some(at) = self.reach(member) {
                        // A retime the take refuses keeps the card as it is.
                        let _ = self.reached[at].clip.retime(speed, reversed, self.rate);
                    }
                }
            }
        }
    }

    /// Where `clip` is among the reached clips, added from the store and the
    /// project on first reach with the shifts so far applied.
    fn reach(&mut self, clip: ClipId) -> Option<usize> {
        if let Some(at) = self
            .reached
            .iter()
            .position(|reached| reached.clip.id == clip)
        {
            return Some(at);
        }
        let ui = self.state.ui_clip_ref(clip)?.clone();
        let audio = on_audio_lane(self.project, clip);
        let mut reached = Reached {
            clip: self.project.clip(clip)?.clone(),
            was: TakeSpan::of(&ui),
            ui,
            audio,
        };
        let by = LaneShift::total(&self.shifts, reached.ui.track_id, reached.span().0);
        reached.shift(by, self.rate);
        self.reached.push(reached);
        Some(self.reached.len() - 1)
    }

    /// Every clip linked to `clip`, or `clip` alone.
    fn linked(&self, clip: ClipId) -> Vec<ClipId> {
        match self.project.clip(clip).and_then(|held| held.link) {
            Some(link) => self.project.linked_clips(link),
            None => vec![clip],
        }
    }

    /// Draw each transition at a cut the steps moved as the release leaves
    /// it ([`kept`]): shortened to the window its clips hold, or gone once
    /// they part.
    fn settle_cuts(&mut self) {
        for cut in Cut::all(self.project) {
            let left = self.span_after(cut.transition.left, cut.track, cut.left);
            let right = self.span_after(cut.transition.right, cut.track, cut.right);
            if (left, right) == (cut.left, cut.right) {
                continue;
            }
            let keep = kept(&cut.transition, left, right);
            let Some(at) = self.reach(cut.transition.left) else {
                continue;
            };
            let card = &mut self.reached[at].ui;
            match (keep, card.transition.as_mut()) {
                (Some(duration), Some(drawn)) => drawn.duration_ticks = duration,
                (Some(_), None) => {}
                (None, _) => card.transition = None,
            }
        }
    }

    /// Where the steps leave `clip`, which spanned `found` on `track` when
    /// the drag found it.
    fn span_after(&self, clip: ClipId, track: TrackId, found: TimeRange) -> TimeRange {
        match self.reached.iter().find(|reached| reached.clip.id == clip) {
            Some(reached) => reached.clip.timeline,
            None => {
                let by = LaneShift::total(&self.shifts, track.raw(), found.start.value);
                TimeRange::at_rate(found.start.value + by, found.duration.value, self.rate)
            }
        }
    }
}
