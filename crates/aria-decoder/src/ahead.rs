//! Decode-ahead over any [`VideoDecoder`].
//!
//! An [`AheadDecoder`] parks its decoder on a process-wide pool of decode
//! threads. After each frame it returns, a pool thread decodes up to
//! [`AHEAD_FRAMES`] frames past it, so sequential playback takes frames that
//! are already decoded and the caller waits only on a miss.
//! [`AheadDecoder::request`] runs the seek and walk of a miss on the pool
//! before the frame is needed, so the misses of several layers decode at once,
//! and [`open_ahead`] opens a decoder on the pool and positions it at its
//! first frame.
//!
//! A request that steps backward seeks once, walks the GOP forward from its
//! keyframe, and keeps the frames before the target, up to [`BEHIND_BYTES`] of
//! CPU frames or [`BEHIND_SURFACES`] GPU surfaces. The next earlier request
//! takes its frame from that store instead of seeking again.
//!
//! The frames returned are the frames the wrapped decoder returns: `frame_at`,
//! `frame_at_nearest`, `next_frame`, and `seek` answer as the wrapped decoder
//! would, and a miss goes to the wrapped decoder, whose roll-forward policy
//! chooses between rolling and seeking.

use core::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

use aria_core::{
    DecodeError, FrameData, Rational, RationalTime, SourceInfo, VideoDecoder, VideoFrame,
};

use crate::seek::{frame_covers, is_after, is_frame_before, seek_walk};

/// Frames an [`AheadDecoder`] holds decoded past the last frame it returned.
pub const AHEAD_FRAMES: usize = 8;

/// CPU frame bytes a backward request keeps before its target.
pub const BEHIND_BYTES: usize = 256 << 20;

/// GPU surfaces a backward request keeps before its target. Decoder surface
/// pools are bounded (the Windows shared NV12 pool holds 16), so a held
/// surface is counted, not its bytes.
pub const BEHIND_SURFACES: usize = AHEAD_FRAMES;

/// Name prefix of the pool's threads.
pub const DECODE_THREAD_PREFIX: &str = "aria-decode-";

type Job = Box<dyn FnOnce() + Send>;

/// The pool's job queue, or `None` when no decode thread could start; then
/// every decode runs on its caller.
fn pool() -> Option<&'static Sender<Job>> {
    static POOL: OnceLock<Option<Sender<Job>>> = OnceLock::new();
    POOL.get_or_init(|| {
        let (send, receive) = mpsc::channel::<Job>();
        let receive = Arc::new(Mutex::new(receive));
        let mut started = 0;
        for index in 0..pool_threads() {
            let jobs = Arc::clone(&receive);
            let spawned = std::thread::Builder::new()
                .name(format!("{DECODE_THREAD_PREFIX}{index}"))
                .spawn(move || work(&jobs));
            match spawned {
                Ok(_) => started += 1,
                Err(error) => tracing::warn!(%error, "a decode-ahead thread did not start"),
            }
        }
        (started > 0).then_some(send)
    })
    .as_ref()
}

/// Decode threads: a quarter of the host's parallelism, between two and four.
/// Hardware decoders do their own work on their own threads; these threads
/// pull, convert, and hold frames.
fn pool_threads() -> usize {
    std::thread::available_parallelism().map_or(2, |threads| (threads.get() / 4).clamp(2, 4))
}

fn work(jobs: &Mutex<Receiver<Job>>) {
    // Media Foundation readers are opened in the multithreaded apartment and
    // move between its threads.
    #[cfg(target_os = "windows")]
    // SAFETY: joins this thread to the MTA with no reserved pointer; a thread
    // already in an apartment reports the conflict, which changes nothing.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    loop {
        let job = jobs.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok(job) = job else {
            return;
        };
        job();
    }
}

/// Run `job` on the pool, or on this thread when the pool is gone.
fn run_on_pool(job: Job) {
    match pool() {
        Some(jobs) => {
            if let Err(mpsc::SendError(job)) = jobs.send(job) {
                job();
            }
        }
        None => job(),
    }
}

/// What a frame request accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// The first frame covering the target ([`VideoDecoder::frame_at`]).
    Exact,
    /// Any frame near it ([`VideoDecoder::frame_at_nearest`]).
    Nearest,
}

#[derive(Debug, Clone, Copy)]
struct Want {
    target: RationalTime,
    ask: Ask,
}

impl Want {
    fn is(self, other: Want) -> bool {
        self.ask == other.ask && self.target.compare(other.target) == Ordering::Equal
    }
}

/// The result of a request decoded on the pool, waiting for its caller.
struct Answer {
    want: Want,
    result: Result<Option<VideoFrame>, DecodeError>,
}

/// Where the frames queued ahead start.
#[derive(Debug, Clone, Copy)]
enum Reach {
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
struct Behind {
    frames: VecDeque<VideoFrame>,
    bytes: usize,
    surfaces: usize,
}

/// CPU bytes and GPU surfaces a held frame occupies.
fn held(frame: &VideoFrame) -> (usize, usize) {
    match &frame.data {
        FrameData::Cpu(image) => (image.planes.iter().map(|plane| plane.data.len()).sum(), 0),
        FrameData::Gpu(_) => (0, 1),
    }
}

impl Behind {
    fn push(&mut self, frame: VideoFrame) {
        let (bytes, surfaces) = held(&frame);
        self.bytes += bytes;
        self.surfaces += surfaces;
        self.frames.push_back(frame);
        while self.bytes > BEHIND_BYTES || self.surfaces > BEHIND_SURFACES {
            let Some(oldest) = self.frames.pop_front() else {
                break;
            };
            self.forget(&oldest);
        }
    }

    fn forget(&mut self, frame: &VideoFrame) {
        let (bytes, surfaces) = held(frame);
        self.bytes -= bytes;
        self.surfaces -= surfaces;
    }

    /// The index of the frame answering `target`: the first that covers it,
    /// unless that is the oldest held frame and lies past `target`, where an
    /// earlier frame no longer held may cover it.
    fn index_for(&self, target: RationalTime, frame_rate: Rational) -> Option<usize> {
        let index = self
            .frames
            .iter()
            .position(|frame| frame_covers(frame.pts, target, frame_rate))?;
        (index > 0 || !is_after(self.frames[index].pts, target, frame_rate)).then_some(index)
    }

    /// Take the frame answering `target` and drop the frames after it, which
    /// a backward run has passed.
    fn take(&mut self, target: RationalTime, frame_rate: Rational) -> Option<VideoFrame> {
        let index = self.index_for(target, frame_rate)?;
        while self.frames.len() > index + 1 {
            let passed = self.frames.pop_back()?;
            self.forget(&passed);
        }
        let frame = self.frames.pop_back()?;
        self.forget(&frame);
        Some(frame)
    }

    fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
        self.surfaces = 0;
    }
}

/// One decode the wrapped decoder runs.
#[derive(Debug, Clone, Copy)]
enum Op {
    Exact(RationalTime),
    Nearest(RationalTime),
    Back(RationalTime),
    Next,
    Seek(RationalTime),
}

struct Outcome {
    result: Result<Option<VideoFrame>, DecodeError>,
    behind: Behind,
}

/// Run `op` on the wrapped decoder. Called without the state lock.
fn run(decoder: &mut dyn VideoDecoder, op: Op, frame_rate: Rational) -> Outcome {
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
    };
    Outcome { result, behind }
}

struct State {
    /// `None` while a pool thread or the caller runs it.
    decoder: Option<Box<dyn VideoDecoder>>,
    /// Frames decoded past `reach`, in stream order with no gaps.
    ahead: VecDeque<VideoFrame>,
    behind: Behind,
    reach: Reach,
    /// A request for the pool to decode.
    want: Option<Want>,
    answer: Option<Answer>,
    /// A pool job is queued or running.
    queued: bool,
    /// The caller waits for the decoder: a fill stops after its frame.
    stop: bool,
    /// The last decode walked backward or snapped near its target: nothing is
    /// decoded ahead until a decode moves forward.
    parked: bool,
    /// The wrapped decoder reached the end of its stream after `ahead`.
    ended: bool,
    /// The error the wrapped decoder returned after `ahead`.
    failure: Option<DecodeError>,
    /// The wrapped decoder panicked and was dropped.
    broken: bool,
    /// The [`AheadDecoder`] was dropped.
    closed: bool,
}

impl State {
    fn new(decoder: Box<dyn VideoDecoder>) -> Self {
        Self {
            decoder: Some(decoder),
            ahead: VecDeque::new(),
            behind: Behind::default(),
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

    /// Whether the frames queued ahead decide `target`.
    fn reaches(&self, target: RationalTime, frame_rate: Rational) -> bool {
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
        while let Some(frame) = self.ahead.pop_front() {
            self.reach = Reach::After(frame.pts);
            if frame_covers(frame.pts, target, frame_rate) {
                return Some(frame);
            }
        }
        None
    }

    /// Whether a frame held now answers `want`.
    fn holds(&self, want: Want, frame_rate: Rational) -> bool {
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
    fn take_held(
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
            self.parked = false;
            return Some(Ok(Some(frame)));
        }
        if want.ask == Ask::Exact {
            return self
                .behind
                .take(want.target, frame_rate)
                .map(|frame| Ok(Some(frame)));
        }
        None
    }

    /// The decode `want` needs: a walk back when it lies before the frames
    /// decoded so far, else the wrapped decoder's own request.
    fn op_for(&self, want: Want, frame_rate: Rational) -> Op {
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
    fn reposition(&mut self) {
        self.ahead.clear();
        self.behind.clear();
        self.answer = None;
        self.parked = false;
        self.ended = false;
        self.failure = None;
    }

    /// Record a decode's outcome and return its result.
    fn settle(&mut self, op: Op, outcome: Outcome) -> Result<Option<VideoFrame>, DecodeError> {
        match op {
            Op::Next => match &outcome.result {
                Ok(Some(frame)) => self.reach = Reach::After(frame.pts),
                Ok(None) => self.ended = true,
                Err(_) => {}
            },
            // A seek lands on the keyframe at or before its target.
            Op::Seek(target) => self.reach = Reach::From(target),
            Op::Exact(_) | Op::Nearest(_) | Op::Back(_) => {
                self.reach = match &outcome.result {
                    Ok(Some(frame)) => Reach::After(frame.pts),
                    Ok(None) | Err(_) => Reach::Unknown,
                };
                self.parked = matches!(op, Op::Back(_) | Op::Nearest(_));
                self.behind = outcome.behind;
            }
        }
        outcome.result
    }

    /// Record a frame a fill decoded.
    fn settle_fill(&mut self, outcome: Outcome) {
        match outcome.result {
            Ok(Some(frame)) => self.ahead.push_back(frame),
            Ok(None) => self.ended = true,
            Err(error) => self.failure = Some(error),
        }
    }

    /// Whether a pool job should decode the next frame ahead.
    fn fills(&self) -> bool {
        !self.stop
            && !self.parked
            && !self.ended
            && self.failure.is_none()
            && self.ahead.len() < AHEAD_FRAMES
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    frame_rate: Rational,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Queue a pool job when there is a request to serve or a frame to decode
/// ahead and no job is queued.
fn schedule(shared: &Arc<Shared>, state: &mut State) {
    if state.queued || state.closed || state.broken || (state.want.is_none() && !state.fills()) {
        return;
    }
    state.queued = true;
    let shared = Arc::clone(shared);
    // A job queued while the state is locked runs after the lock is released;
    // only the no-pool fallback runs it here, and that needs the lock free.
    match pool() {
        Some(jobs) => {
            if jobs.send(Box::new(move || serve(&shared))).is_err() {
                state.queued = false;
            }
        }
        None => state.queued = false,
    }
}

/// The wrapped decoder on loan to one thread. Dropped while that thread
/// unwinds, it drops the decoder and marks the [`AheadDecoder`] broken, so no
/// caller waits for a decoder that will not come back.
struct Claim<'a> {
    shared: &'a Shared,
    decoder: Option<Box<dyn VideoDecoder>>,
}

impl<'a> Claim<'a> {
    /// Run `op` on the claimed decoder.
    fn run(&mut self, op: Op, frame_rate: Rational) -> Outcome {
        match self.decoder.as_deref_mut() {
            Some(decoder) => run(decoder, op, frame_rate),
            None => Outcome {
                result: Err(broken()),
                behind: Behind::default(),
            },
        }
    }

    /// Return the decoder and lock the state.
    fn release(mut self) -> MutexGuard<'a, State> {
        let mut state = self.shared.lock();
        state.decoder = self.decoder.take();
        state
    }

    /// Drop a decoder that panicked.
    fn poison(mut self) -> MutexGuard<'a, State> {
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

/// A pool job: serve the pending request, then decode ahead until the queue
/// is full, the caller wants the decoder, or the stream ends.
fn serve(shared: &Arc<Shared>) {
    let frame_rate = shared.frame_rate;
    let mut state = shared.lock();
    loop {
        if state.closed || state.broken {
            break;
        }
        let want = state.want.take();
        if let Some(want) = want {
            // A frame held now answers it; a stale answer gives way.
            if let Some(result) = state.take_held(want, frame_rate) {
                state.answer = Some(Answer { want, result });
                shared.changed.notify_all();
                continue;
            }
        } else if !state.fills() {
            break;
        }
        let Some(decoder) = state.decoder.take() else {
            // The caller holds the decoder; it schedules again when done.
            state.want = want;
            break;
        };
        let op = match want {
            Some(want) => {
                let op = state.op_for(want, frame_rate);
                state.reposition();
                op
            }
            None => Op::Next,
        };
        drop(state);
        let mut claim = Claim {
            shared,
            decoder: Some(decoder),
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| claim.run(op, frame_rate)));
        state = match outcome {
            Ok(outcome) => {
                let mut state = claim.release();
                match want {
                    Some(want) => {
                        let result = state.settle(op, outcome);
                        state.answer = Some(Answer { want, result });
                    }
                    None => state.settle_fill(outcome),
                }
                state
            }
            Err(_) => {
                tracing::error!("a video decoder panicked on a decode-ahead thread");
                claim.poison()
            }
        };
        shared.changed.notify_all();
    }
    state.queued = false;
    let orphan = if state.closed {
        state.decoder.take()
    } else {
        None
    };
    shared.changed.notify_all();
    drop(state);
    drop(orphan);
}

fn broken() -> DecodeError {
    DecodeError::Decode("the video decoder panicked and was closed. Reopen the media.".into())
}

/// A [`VideoDecoder`] that decodes ahead of its caller on the decode pool.
pub struct AheadDecoder {
    shared: Arc<Shared>,
    info: SourceInfo,
}

impl fmt::Debug for AheadDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.shared.lock();
        f.debug_struct("AheadDecoder")
            .field("info", &self.info)
            .field("ahead", &state.ahead.len())
            .field("behind", &state.behind.frames.len())
            .field("reach", &state.reach)
            .finish_non_exhaustive()
    }
}

impl AheadDecoder {
    /// Wrap `decoder`. Nothing is decoded until the first request.
    pub fn new(decoder: Box<dyn VideoDecoder>) -> Self {
        let info = decoder.info().clone();
        let frame_rate = info.frame_rate;
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::new(decoder)),
                changed: Condvar::new(),
                frame_rate,
            }),
            info,
        }
    }

    /// Start decoding the frame [`frame_at`](VideoDecoder::frame_at) (or, with
    /// `nearest`, [`frame_at_nearest`](VideoDecoder::frame_at_nearest)) will be
    /// asked for at `target`, on the pool, unless a frame held already answers
    /// it. The later call with the same target and kind takes that frame.
    pub fn request(&self, target: RationalTime, nearest: bool) {
        let want = Want {
            target,
            ask: if nearest { Ask::Nearest } else { Ask::Exact },
        };
        let mut state = self.shared.lock();
        if state.closed || state.broken || state.holds(want, self.shared.frame_rate) {
            return;
        }
        state.want = Some(want);
        state.stop = false;
        schedule(&self.shared, &mut state);
    }

    /// Drop every frame held ahead and behind, for a decoder no frame uses
    /// now. The next request decodes from the wrapped decoder's position.
    pub fn release(&self) {
        let (ahead, behind, answer) = {
            let mut state = self.shared.lock();
            state.stop = true;
            state.want = None;
            state.reach = Reach::Unknown;
            (
                std::mem::take(&mut state.ahead),
                std::mem::take(&mut state.behind),
                state.answer.take(),
            )
        };
        drop((ahead, behind, answer));
    }

    /// Frames decoded ahead of the last frame returned, held now.
    pub fn ahead(&self) -> usize {
        self.shared.lock().ahead.len()
    }

    /// Frames held from the last backward walk.
    pub fn behind(&self) -> usize {
        self.shared.lock().behind.frames.len()
    }

    /// Wait until no pool job runs for this decoder.
    pub fn idle(&self) {
        let mut state = self.shared.lock();
        while state.queued {
            state = self.shared.wait(state);
        }
    }

    /// Take the decoder from the state, waiting while a pool job runs it, unless
    /// `held` answers first.
    fn claim<T>(
        &self,
        mut held: impl FnMut(&mut State) -> Option<T>,
    ) -> Result<(MutexGuard<'_, State>, Box<dyn VideoDecoder>), T> {
        let mut state = self.shared.lock();
        loop {
            if let Some(found) = held(&mut state) {
                state.stop = false;
                schedule(&self.shared, &mut state);
                return Err(found);
            }
            if let Some(decoder) = state.decoder.take() {
                state.want = None;
                state.stop = false;
                return Ok((state, decoder));
            }
            // A fill stops after its frame; a request in flight is finished.
            if state.want.is_none() {
                state.stop = true;
            }
            state = self.shared.wait(state);
        }
    }

    /// Run `op` on this thread with the claimed decoder.
    fn decode(
        &self,
        mut state: MutexGuard<'_, State>,
        decoder: Box<dyn VideoDecoder>,
        op: Op,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        if !matches!(op, Op::Next) {
            state.reposition();
        }
        drop(state);
        let mut claim = Claim {
            shared: &self.shared,
            decoder: Some(decoder),
        };
        let outcome = claim.run(op, self.shared.frame_rate);
        let mut state = claim.release();
        let result = state.settle(op, outcome);
        schedule(&self.shared, &mut state);
        self.shared.changed.notify_all();
        result
    }

    fn get(&self, want: Want) -> Result<Option<VideoFrame>, DecodeError> {
        let frame_rate = self.shared.frame_rate;
        let claimed = self.claim(|state| {
            if let Some(result) = state.take_held(want, frame_rate) {
                return Some(result);
            }
            state.broken.then(|| Err(broken()))
        });
        match claimed {
            Err(result) => result,
            Ok((state, decoder)) => {
                let op = state.op_for(want, frame_rate);
                self.decode(state, decoder, op)
            }
        }
    }
}

impl VideoDecoder for AheadDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let claimed = self.claim(|state| state.broken.then(broken));
        match claimed {
            Err(error) => Err(error),
            Ok((state, decoder)) => self.decode(state, decoder, Op::Seek(target)).map(|_| ()),
        }
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let claimed = self.claim(|state| {
            if let Some(frame) = state.ahead.pop_front() {
                state.reach = Reach::After(frame.pts);
                state.answer = None;
                state.behind.clear();
                state.parked = false;
                return Some(Ok(Some(frame)));
            }
            if let Some(error) = state.failure.take() {
                return Some(Err(error));
            }
            if state.ended {
                return Some(Ok(None));
            }
            state.broken.then(|| Err(broken()))
        });
        match claimed {
            Err(result) => result,
            Ok((mut state, decoder)) => {
                state.answer = None;
                state.behind.clear();
                state.parked = false;
                self.decode(state, decoder, Op::Next)
            }
        }
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        self.get(Want {
            target,
            ask: Ask::Exact,
        })
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        self.get(Want {
            target,
            ask: Ask::Nearest,
        })
    }
}

impl Drop for AheadDecoder {
    fn drop(&mut self) {
        let held = {
            let mut state = self.shared.lock();
            state.closed = true;
            (
                state.decoder.take(),
                std::mem::take(&mut state.ahead),
                std::mem::take(&mut state.behind),
                state.answer.take(),
            )
        };
        self.shared.changed.notify_all();
        drop(held);
    }
}

/// A decoder opening on the decode pool.
pub struct OpeningDecoder {
    receive: Receiver<Result<AheadDecoder, DecodeError>>,
}

impl fmt::Debug for OpeningDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpeningDecoder").finish_non_exhaustive()
    }
}

impl OpeningDecoder {
    /// Wait for the open to finish.
    pub fn wait(self) -> Result<AheadDecoder, DecodeError> {
        self.receive.recv().unwrap_or_else(|_| {
            Err(DecodeError::Open(
                "the decoder open stopped before it finished. Try the media again.".into(),
            ))
        })
    }
}

/// Open a decoder with `open` on the decode pool and decode its frame at
/// `first` ([`AheadDecoder::request`]), so a clip about to come on screen has
/// its decoder open and its first frame decoded when it does. Without a pool,
/// the open runs on this thread.
pub fn open_ahead<F>(open: F, first: RationalTime, nearest: bool) -> OpeningDecoder
where
    F: FnOnce() -> Result<Box<dyn VideoDecoder>, DecodeError> + Send + 'static,
{
    let (send, receive) = mpsc::sync_channel(1);
    run_on_pool(Box::new(move || {
        let opened = catch_unwind(AssertUnwindSafe(open)).unwrap_or_else(|_| {
            Err(DecodeError::Open(
                "the video decoder panicked while opening. Try another file.".into(),
            ))
        });
        let decoder = opened.map(AheadDecoder::new);
        if let Ok(decoder) = &decoder {
            decoder.request(first, nearest);
        }
        let _ = send.send(decoder);
    }));
    OpeningDecoder { receive }
}

#[cfg(test)]
#[path = "ahead_tests.rs"]
mod tests;
