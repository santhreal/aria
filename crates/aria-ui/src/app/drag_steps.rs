//! The edits an edge drag and a slide send, as steps.
//!
//! A trim, a ripple trim, a roll, a slide and a rate stretch each send one or
//! more of four edits, in order: a trim, a move, a shift of the rest of a lane
//! and a retime. The release sends them to the engine as one undo entry.
//! While the button is down the frame runs the same steps on the clips they
//! reach, with the model's own trim and retime arithmetic, on the project as
//! the drag found it: the drag draws what its release leaves.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Clip, ClipId, Project, Rational, RationalTime, TimeRange, TrackId};

use crate::app::AppState;
use crate::projection::Placement;
use crate::surfaces::timeline_gesture::{self, Drag, Gesture};
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
    /// Send `steps` as one undo entry. A refused step rolls back the steps
    /// before it.
    pub(crate) fn run_steps(&mut self, steps: &[Step]) -> Result<(), EngineError> {
        self.snap_guide = None;
        match steps {
            [] => Ok(()),
            [step] => self.run_step(*step),
            _ => self.one_step(|this| steps.iter().try_for_each(|step| this.run_step(*step))),
        }
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
    /// the clips an edge drag or a slide changes. The press reads the session
    /// and every move after it reads what the press read, so a move never
    /// waits for a preview decode.
    pub(crate) fn hold_drag(&mut self, gesture: Gesture, drag: Drag) {
        let project = self.drag_project();
        self.snap_guide = None;
        self.hold_room(gesture.room(), drag.clip, &project);
        self.trim_feedback = self.compute_trim_feedback(gesture, drag);
        self.hold_move(gesture, drag, &project);
        let Some(held) = timeline_gesture::held(self, gesture, &drag) else {
            self.held_edit = None;
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
        let lanes = self.project.timeline();
        let audio = lanes
            .track_of(clip)
            .and_then(|lane| lanes.track(lane))
            .is_some_and(|lane| lane.kind.is_audio());
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
}
