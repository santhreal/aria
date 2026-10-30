//! State machine and backward frame store for [`AheadDecoder`](super::AheadDecoder).

use core::cmp::Ordering;
use parking_lot::{Condvar, Mutex, MutexGuard};
use std::collections::VecDeque;

use aria_core::{DecodeError, FrameData, Rational, RationalTime, VideoDecoder, VideoFrame};

use super::{AHEAD_FRAMES, BEHIND_BYTES, BEHIND_SURFACES};
use crate::seek::{frame_covers, is_after, is_frame_before, seek_walk};

/// What a frame request accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ask {
    /// The first frame covering the target ([`VideoDecoder::frame_at`]).
    Exact,
    /// Any frame near it ([`VideoDecoder::frame_at_nearest`]).
    Nearest,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Want {
    pub(super) target: RationalTime,
    pub(super) ask: Ask,
}

impl Want {
    pub(super) fn is(self, other: Self) -> bool {
        self.ask == other.ask && self.target.compare(other.target) == Ordering::Equal
    }
}

/// The result of a request decoded on the pool, waiting for its caller.
pub(super) struct Answer {
    pub(super) want: Want,
    pub(super) result: Result<Option<VideoFrame>, DecodeError>,
}

/// Where the frames queued ahead start.
#[derive(Debug, Clone, Copy)]
pub(super) enum Reach {
    /// The queue holds whatever follows the last seek; no request can be
    /// answered from it.
    Unknown,
    /// Every frame at or after this time is queued or still to come.
    From(RationalTime),
    /// Every frame after this one is queued or still to come.
    After(RationalTime),
}

/// Frames decoded before the last frame a backward request returned, in
/// stream order with no gaps.
#[derive(Default)]
pub(super) struct Behind {
    pub(super) frames: VecDeque<VideoFrame>,
    pub(super) bytes: usize,
    pub(super) surfaces: usize,
}

/// CPU bytes and GPU surfaces a held frame occupies.
fn held(frame: &VideoFrame) -> (usize, usize) {
    match &frame.data {
        FrameData::Cpu(image) => (image.planes.iter().map(|plane| plane.data.len()).sum(), 0),
        FrameData::Gpu(_) => (0, 1),
    }
}

impl Behind {
    pub(super) fn push(&mut self, frame: VideoFrame) {
        let (bytes, surfaces) = held(&frame);
        self.bytes += bytes;
        self.surfaces += surfaces;
        self.frames.push_back(frame);
        while self.bytes > BEHIND_BYTES || self.surfaces > BEHIND_SURFACES {
            let Some(mut oldest) = self.frames.pop_front() else {
                break;
            };
            self.forget(&oldest);
            crate::recycle_frame_planes(&mut oldest);
        }
    }

    /// Prepend an earlier GOP walk before the frames currently held, dropping
    /// the oldest frames from the front when the combined store exceeds its
    /// byte or surface budget.
    pub(super) fn prepend(&mut self, mut older: Self) {
        if older.frames.is_empty() {
            return;
        }
        while self.bytes + older.bytes > BEHIND_BYTES
            || self.surfaces + older.surfaces > BEHIND_SURFACES
        {
            let Some(mut oldest) = older.frames.pop_front() else {
                return;
            };
            older.forget(&oldest);
            crate::recycle_frame_planes(&mut oldest);
        }
        older.bytes += self.bytes;
        older.surfaces += self.surfaces;
        older.frames.append(&mut self.frames);
        *self = older;
    }

    fn forget(&mut self, frame: &VideoFrame) {
        let (bytes, surfaces) = held(frame);
        self.bytes -= bytes;
        self.surfaces -= surfaces;
    }

    /// The index of the frame answering `target`: the first that covers it,
    /// unless that is the oldest held frame and lies past `target`, where an
    /// earlier frame no longer held may cover it.
    pub(super) fn index_for(&self, target: RationalTime, frame_rate: Rational) -> Option<usize> {
        let index = self
            .frames
            .iter()
            .position(|frame| frame_covers(frame.pts, target, frame_rate))?;
        (index > 0 || !is_after(self.frames[index].pts, target, frame_rate)).then_some(index)
    }

    /// Take the frame answering `target` and drop the frames after it, which
    /// a backward run has passed.
    pub(super) fn take(
        &mut self,
        target: RationalTime,
        frame_rate: Rational,
    ) -> Option<VideoFrame> {
        let index = self.index_for(target, frame_rate)?;
        while self.frames.len() > index + 1 {
            let mut passed = self.frames.pop_back()?;
            self.forget(&passed);
            crate::recycle_frame_planes(&mut passed);
        }
        let frame = self.frames.pop_back()?;
        self.forget(&frame);
        Some(frame)
    }

    pub(super) fn clear(&mut self) {
        for mut frame in self.frames.drain(..) {
            crate::recycle_frame_planes(&mut frame);
        }
        self.bytes = 0;
        self.surfaces = 0;
    }
}

/// One frame period before `floor` on `floor.rate`, or `None` at or before
/// the start of the stream.
pub(super) fn step_before(floor: RationalTime, frame_rate: Rational) -> Option<RationalTime> {
    if floor.value <= 0 || !floor.rate.is_valid() {
        return None;
    }
    let (rate_num, rate_den) = if frame_rate.is_valid() {
        (i128::from(frame_rate.num), i128::from(frame_rate.den))
    } else {
        (30, 1)
    };
    let num = rate_den * i128::from(floor.rate.num);
    let den = rate_num * i128::from(floor.rate.den);
    let step = ((num + den - 1) / den).max(1);
    let prev = (i128::from(floor.value) - step).max(0);
    let prev = i64::try_from(prev).ok()?;
    let target = RationalTime::new(prev, floor.rate);
    is_frame_before(target, floor, frame_rate).then_some(target)
}

/// One decode the wrapped decoder runs.
#[derive(Debug, Clone, Copy)]
pub(super) enum Op {
    Exact(RationalTime),
    Nearest(RationalTime),
    Back(RationalTime),
    BackFill {
        target: RationalTime,
        floor: RationalTime,
    },
    Next,
    Seek(RationalTime),
}

pub(super) struct Outcome {
    pub(super) result: Result<Option<VideoFrame>, DecodeError>,
    pub(super) behind: Behind,
}

/// Run `op` on the wrapped decoder. Called without the state lock.
pub(super) fn run(decoder: &mut dyn VideoDecoder, op: Op, frame_rate: Rational) -> Outcome {
    let mut behind = Behind::default();
    let result = match op {
        Op::Exact(target) => decoder.frame_at(target),
        Op::Nearest(target) => decoder.frame_at_nearest(target),
        Op::Next => decoder.next_frame(),
        Op::Seek(target) => decoder.seek(target).map(|()| None),
        // The same seek and walk as the wrapped decoder's own `frame_at`, so
        // an open-GOP sync sample backs off the same way, keeping the frames
        // the walk passes.
        Op::Back(target) => seek_walk(decoder, target, frame_rate, |frame| behind.push(frame)),
        Op::BackFill { target, floor } => {
            let found = seek_walk(decoder, target, frame_rate, |frame| {
                if is_frame_before(frame.pts, floor, frame_rate) {
                    behind.push(frame);
                }
            });
            match found {
                Ok(Some(frame)) => {
                    if is_frame_before(frame.pts, floor, frame_rate) {
                        behind.push(frame);
                    }
                    Ok(None)
                }
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            }
        }
    };
    Outcome { result, behind }
}

pub(super) struct State {
    /// `None` while a pool thread or the caller runs it.
    pub(super) decoder: Option<Box<dyn VideoDecoder>>,
    /// Frames decoded past `reach`, in stream order with no gaps.
    pub(super) ahead: VecDeque<VideoFrame>,
    pub(super) behind: Behind,
    /// Earliest PTS covered by the current backward run, for reverse look-ahead.
    pub(super) behind_floor: Option<RationalTime>,
    pub(super) reach: Reach,
    /// A request for the pool to decode.
    pub(super) want: Option<Want>,
    pub(super) answer: Option<Answer>,
    /// A pool job is queued or running.
    pub(super) queued: bool,
    /// The caller waits for the decoder: a fill stops after its frame.
    pub(super) stop: bool,
    /// The last decode walked backward or snapped near its target: nothing is
    /// decoded ahead until a decode moves forward.
    pub(super) parked: bool,
    /// The wrapped decoder reached the end of its stream after `ahead`.
    pub(super) ended: bool,
    /// The error the wrapped decoder returned after `ahead`.
    pub(super) failure: Option<DecodeError>,
    /// The wrapped decoder panicked and was dropped.
    pub(super) broken: bool,
    /// The [`AheadDecoder`](super::AheadDecoder) was dropped.
    pub(super) closed: bool,
}

impl State {
    pub(super) fn new(decoder: Box<dyn VideoDecoder>) -> Self {
        Self {
            decoder: Some(decoder),
            ahead: VecDeque::new(),
            behind: Behind::default(),
            behind_floor: None,
            reach: Reach::Unknown,
            want: None,
            answer: None,
            queued: false,
            stop: false,
            parked: false,
            ended: false,
            failure: None,
            broken: false,
            closed: false,
        }
    }

    /// Whether the frames queued ahead cover `target`.
    pub(super) fn reaches(&self, target: RationalTime, frame_rate: Rational) -> bool {
        match self.reach {
            Reach::Unknown => false,
            Reach::From(start) => target.compare(start) != Ordering::Less,
            Reach::After(last) => is_after(target, last, frame_rate),
        }
    }

    /// Take the queued frame answering `target`, dropping the frames before it.
    fn take_ahead(&mut self, target: RationalTime, frame_rate: Rational) -> Option<VideoFrame> {
        if !self.reaches(target, frame_rate) {
            return None;
        }
        while let Some(mut frame) = self.ahead.pop_front() {
            self.reach = Reach::After(frame.pts);
            if frame_covers(frame.pts, target, frame_rate) {
                return Some(frame);
            }
            crate::recycle_frame_planes(&mut frame);
        }
        None
    }

    /// Whether a frame held now answers `want`.
    pub(super) fn holds(&self, want: Want, frame_rate: Rational) -> bool {
        if self
            .answer
            .as_ref()
            .is_some_and(|answer| answer.want.is(want))
        {
            return true;
        }
        let ahead = self.reaches(want.target, frame_rate)
            && self
                .ahead
                .iter()
                .any(|frame| frame_covers(frame.pts, want.target, frame_rate));
        ahead
            || (want.ask == Ask::Exact && self.behind.index_for(want.target, frame_rate).is_some())
    }

    /// A frame held now that answers `want`.
    pub(super) fn take_held(
        &mut self,
        want: Want,
        frame_rate: Rational,
    ) -> Option<Result<Option<VideoFrame>, DecodeError>> {
        if let Some(answer) = self.answer.take()
            && answer.want.is(want)
        {
            return Some(answer.result);
        }
        if let Some(frame) = self.take_ahead(want.target, frame_rate) {
            self.behind.clear();
            self.behind_floor = None;
            self.parked = false;
            return Some(Ok(Some(frame)));
        }
        if want.ask == Ask::Exact
            && let Some(frame) = self.behind.take(want.target, frame_rate)
        {
            self.reach = Reach::After(frame.pts);
            return Some(Ok(Some(frame)));
        }
        None
    }

    /// The decode `want` needs: a walk back when it lies before the frames
    /// decoded so far, else the wrapped decoder's own request.
    pub(super) fn op_for(&self, want: Want, frame_rate: Rational) -> Op {
        match want.ask {
            Ask::Nearest => Op::Nearest(want.target),
            Ask::Exact => match self.reach {
                Reach::From(at) | Reach::After(at)
                    if is_frame_before(want.target, at, frame_rate) =>
                {
                    Op::Back(want.target)
                }
                Reach::Unknown | Reach::From(_) | Reach::After(_) => Op::Exact(want.target),
            },
        }
    }

    /// Drop what a repositioning decode makes stale.
    pub(super) fn reposition(&mut self) {
        for mut frame in self.ahead.drain(..) {
            crate::recycle_frame_planes(&mut frame);
        }
        self.behind.clear();
        self.behind_floor = None;
        self.discard_answer();
        self.parked = false;
        self.ended = false;
        self.failure = None;
    }

    /// Drop the answer held for a caller, returning its planes to the pool.
    pub(super) fn discard_answer(&mut self) {
        if let Some(Ok(Some(mut frame))) = self.answer.take().map(|answer| answer.result) {
            crate::recycle_frame_planes(&mut frame);
        }
    }

    /// Record a decode's outcome and return its result.
    pub(super) fn settle(
        &mut self,
        op: Op,
        outcome: Outcome,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        match op {
            Op::Next => match &outcome.result {
                Ok(Some(frame)) => self.reach = Reach::After(frame.pts),
                Ok(None) => self.ended = true,
                Err(_) => {}
            },
            // A seek lands on the keyframe at or before its target.
            Op::Seek(target) => self.reach = Reach::From(target),
            Op::Exact(_) | Op::Nearest(_) | Op::Back(_) => {
                let found_pts = outcome
                    .result
                    .as_ref()
                    .ok()
                    .and_then(|f| f.as_ref().map(|f| f.pts));
                self.reach = found_pts.map_or(Reach::Unknown, Reach::After);
                self.parked = matches!(op, Op::Back(_) | Op::Nearest(_));
                self.behind_floor = matches!(op, Op::Back(_))
                    .then(|| outcome.behind.frames.front().map(|f| f.pts).or(found_pts))
                    .flatten();
                self.behind = outcome.behind;
            }
            Op::BackFill { .. } => {}
        }
        outcome.result
    }

    /// Record a frame or backward GOP walk a fill decoded.
    pub(super) fn settle_fill(&mut self, op: Op, outcome: Outcome) {
        if let Op::BackFill { floor, .. } = op {
            if !self.parked || self.behind_floor != Some(floor) {
                return;
            }
            if outcome.result.is_err() || outcome.behind.frames.is_empty() {
                self.behind_floor = None;
                return;
            }
            let oldest = outcome.behind.frames.front().map(|frame| frame.pts);
            self.behind.prepend(outcome.behind);
            self.behind_floor = self.behind.frames.front().map(|f| f.pts).or(oldest);
            return;
        }
        match outcome.result {
            Ok(Some(frame)) => self.ahead.push_back(frame),
            Ok(None) => self.ended = true,
            Err(error) => self.failure = Some(error),
        }
    }

    /// Reverse look-ahead op when `behind` has room to pre-walk the preceding GOP.
    fn back_fill_op(&self, frame_rate: Rational) -> Option<Op> {
        if self.stop
            || !self.parked
            || self.ended
            || self.failure.is_some()
            || self.behind.frames.len() >= 2 * AHEAD_FRAMES
            || self.behind.bytes > BEHIND_BYTES / 2
            || self.behind.surfaces > BEHIND_SURFACES / 2
        {
            return None;
        }
        let floor = self.behind_floor?;
        let target = step_before(floor, frame_rate)?;
        Some(Op::BackFill { target, floor })
    }

    /// Next background fill operation for a pool job, if any.
    pub(super) fn fill_op(&self, frame_rate: Rational) -> Option<Op> {
        if self.stop || self.ended || self.failure.is_some() {
            return None;
        }
        if self.parked {
            self.back_fill_op(frame_rate)
        } else {
            (self.ahead.len() < AHEAD_FRAMES).then_some(Op::Next)
        }
    }

    /// Whether a pool job should decode ahead or pre-walk the preceding GOP.
    pub(super) fn fills(&self, frame_rate: Rational) -> bool {
        self.fill_op(frame_rate).is_some()
    }

    /// Whether an active forward fill is within two frames of `want`.
    pub(super) fn fills_toward(&self, want: Want, frame_rate: Rational) -> bool {
        if want.ask != Ask::Exact || self.parked || !self.fills(frame_rate) {
            return false;
        }
        let last = match self.ahead.back() {
            Some(frame) => frame.pts,
            None => match self.reach {
                Reach::After(at) => at,
                Reach::Unknown | Reach::From(_) => return false,
            },
        };
        if !is_after(want.target, last, frame_rate) {
            return false;
        }
        step_before(want.target, frame_rate)
            .and_then(|prev| step_before(prev, frame_rate))
            .is_none_or(|two_back| !is_frame_before(last, two_back, frame_rate))
    }
}

pub(super) struct Shared {
    pub(super) state: Mutex<State>,
    pub(super) changed: Condvar,
    pub(super) frame_rate: Rational,
}

impl Shared {
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock()
    }

    pub(super) fn wait(&self, state: &mut MutexGuard<'_, State>) {
        self.changed.wait(state);
    }

    pub(super) fn wait_for_ahead(&self, mut state: MutexGuard<'_, State>, min_frames: usize) {
        while state.ahead.len() < min_frames
            && state.queued
            && state.want.is_none()
            && !state.stop
            && !state.parked
            && !state.ended
            && state.failure.is_none()
            && !state.closed
            && !state.broken
        {
            self.wait(&mut state);
        }
    }

    pub(super) fn wait_for_behind(&self, mut state: MutexGuard<'_, State>) {
        while state.parked
            && state.behind.frames.is_empty()
            && state.queued
            && state.want.is_none()
            && !state.stop
            && !state.ended
            && state.failure.is_none()
            && !state.closed
            && !state.broken
        {
            self.wait(&mut state);
        }
    }
}

pub(super) fn broken() -> DecodeError {
    DecodeError::Decode("the video decoder panicked and was closed. Reopen the media.".into())
}

/// The wrapped decoder on loan to one thread. Dropped while that thread
/// unwinds, it drops the decoder and marks the [`AheadDecoder`](super::AheadDecoder)
/// broken, so no caller waits for a decoder that will not come back.
pub(super) struct Claim<'a> {
    pub(super) shared: &'a Shared,
    pub(super) decoder: Option<Box<dyn VideoDecoder>>,
}

impl<'a> Claim<'a> {
    /// Run `op` on the claimed decoder.
    pub(super) fn run(&mut self, op: Op, frame_rate: Rational) -> Outcome {
        match self.decoder.as_deref_mut() {
            Some(decoder) => run(decoder, op, frame_rate),
            None => Outcome {
                result: Err(broken()),
                behind: Behind::default(),
            },
        }
    }

    /// Return the decoder and lock the state.
    pub(super) fn release(mut self) -> MutexGuard<'a, State> {
        let mut state = self.shared.lock();
        state.decoder = self.decoder.take();
        state
    }

    /// Drop a decoder that panicked.
    pub(super) fn poison(mut self) -> MutexGuard<'a, State> {
        let decoder = self.decoder.take();
        drop(decoder);
        let mut state = self.shared.lock();
        state.broken = true;
        state
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        let Some(decoder) = self.decoder.take() else {
            return;
        };
        drop(decoder);
        self.shared.lock().broken = true;
        self.shared.changed.notify_all();
    }
}
