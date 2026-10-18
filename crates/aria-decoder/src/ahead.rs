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
//! CPU frames or [`BEHIND_SURFACES`] GPU surfaces. While stepping backward,
//! a pool thread pre-walks the preceding GOP before the held backward store
//! drains, so reverse playback crosses GOP boundaries from held frames.
//!
//! The frames returned are the frames the wrapped decoder returns: `frame_at`,
//! `frame_at_nearest`, `next_frame`, and `seek` answer as the wrapped decoder
//! would, and a miss goes to the wrapped decoder, whose roll-forward policy
//! chooses between rolling and seeking.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError};

use aria_core::{DecodeError, Rational, RationalTime, SourceInfo, VideoDecoder, VideoFrame};

#[path = "ahead_state.rs"]
mod state;
use state::{Answer, Ask, Claim, Op, Reach, Shared, State, Want, broken};

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

enum Job {
    Serve(Arc<Shared>),
    Open(Box<dyn FnOnce() + Send>),
}

impl Job {
    fn run(self) {
        match self {
            Self::Serve(shared) => serve(&shared),
            Self::Open(open) => open(),
        }
    }
}

/// The pool's job queue, or `None` when no decode thread could start; then
/// every decode runs on its caller.
fn pool() -> Option<&'static Sender<Job>> {
    static POOL: LazyLock<Option<Sender<Job>>> = LazyLock::new(|| {
        let (send, receive) = mpsc::channel::<Job>();
        let receive = Arc::new(Mutex::new(receive));
        let mut started = 0;
        for index in 0..pool_threads() {
            let jobs = Arc::clone(&receive);
            match std::thread::Builder::new()
                .name(format!("{DECODE_THREAD_PREFIX}{index}"))
                .spawn(move || work(&jobs))
            {
                Ok(_) => started += 1,
                Err(error) => tracing::warn!(%error, "a decode-ahead thread did not start"),
            }
        }
        (started > 0).then_some(send)
    });
    POOL.as_ref()
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
        job.run();
    }
}

/// Run `job` on the pool, or on this thread when the pool is gone.
fn run_on_pool(job: Job) {
    match pool() {
        Some(jobs) => {
            if let Err(mpsc::SendError(job)) = jobs.send(job) {
                job.run();
            }
        }
        None => job.run(),
    }
}

/// Queue a pool job when there is a request to serve or a frame to decode
/// ahead and no job is queued.
fn schedule(shared: &Arc<Shared>, state: &mut State, frame_rate: Rational) {
    if state.queued
        || state.closed
        || state.broken
        || (state.want.is_none() && !state.fills(frame_rate))
    {
        return;
    }
    state.queued = true;
    // A job queued while the state is locked runs after the lock is released;
    // only the no-pool fallback runs it here, and that needs the lock free.
    match pool() {
        Some(jobs) => {
            if jobs.send(Job::Serve(Arc::clone(shared))).is_err() {
                state.queued = false;
            }
        }
        None => state.queued = false,
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
        }
        let op = match want {
            Some(want) => {
                let op = state.op_for(want, frame_rate);
                state.reposition();
                op
            }
            None => match state.fill_op(frame_rate) {
                Some(op) => op,
                None => break,
            },
        };
        let Some(decoder) = state.decoder.take() else {
            // The caller holds the decoder; it schedules again when done.
            state.want = want;
            break;
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
                    None => state.settle_fill(op, outcome),
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
    let orphan = state.closed.then(|| state.decoder.take()).flatten();
    shared.changed.notify_all();
    drop(state);
    drop(orphan);
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
        let frame_rate = self.shared.frame_rate;
        let ask = if nearest { Ask::Nearest } else { Ask::Exact };
        let want = Want { target, ask };
        let mut state = self.shared.lock();
        if state.closed
            || state.broken
            || state.holds(want, frame_rate)
            || (state.queued && state.want.is_none() && state.fills_toward(want, frame_rate))
        {
            return;
        }
        state.want = Some(want);
        state.stop = false;
        schedule(&self.shared, &mut state, frame_rate);
    }

    /// Drop every frame held ahead and behind, for a decoder no frame uses
    /// now. The next request decodes from the wrapped decoder's position.
    pub fn release(&self) {
        let (ahead, behind, answer) = {
            let mut state = self.shared.lock();
            state.stop = true;
            state.want = None;
            state.behind_floor = None;
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
        mut stop_fill: impl FnMut(&State) -> bool,
    ) -> Result<(MutexGuard<'_, State>, Box<dyn VideoDecoder>), T> {
        let frame_rate = self.shared.frame_rate;
        let mut state = self.shared.lock();
        loop {
            if let Some(found) = held(&mut state) {
                state.stop = false;
                schedule(&self.shared, &mut state, frame_rate);
                self.shared.wait_for_behind(state);
                return Err(found);
            }
            if (!state.queued || stop_fill(&state))
                && let Some(decoder) = state.decoder.take()
            {
                state.want = None;
                state.stop = false;
                return Ok((state, decoder));
            }
            // A fill stops after its frame unless it is already decoding toward
            // the caller's target; a request in flight is finished.
            if state.want.is_none() && stop_fill(&state) {
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
        let frame_rate = self.shared.frame_rate;
        if !matches!(op, Op::Next) {
            state.reposition();
        }
        drop(state);
        let mut claim = Claim {
            shared: &self.shared,
            decoder: Some(decoder),
        };
        let outcome = claim.run(op, frame_rate);
        let mut state = claim.release();
        let result = state.settle(op, outcome);
        schedule(&self.shared, &mut state, frame_rate);
        match op {
            Op::Exact(_) if result.as_ref().is_ok_and(Option::is_some) => {
                self.shared.wait_for_ahead(state, AHEAD_FRAMES / 2);
            }
            Op::Back(_) if result.as_ref().is_ok_and(Option::is_some) => {
                self.shared.wait_for_behind(state);
            }
            _ => drop(state),
        }
        self.shared.changed.notify_all();
        result
    }

    fn get(&self, want: Want) -> Result<Option<VideoFrame>, DecodeError> {
        let frame_rate = self.shared.frame_rate;
        let claimed = self.claim(
            |state| {
                if let Some(result) = state.take_held(want, frame_rate) {
                    return Some(result);
                }
                state.broken.then(|| Err(broken()))
            },
            |state| !state.fills_toward(want, frame_rate),
        );
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
        let claimed = self.claim(|state| state.broken.then(broken), |_| true);
        match claimed {
            Err(error) => Err(error),
            Ok((state, decoder)) => self.decode(state, decoder, Op::Seek(target)).map(|_| ()),
        }
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let claimed = self.claim(
            |state| {
                if let Some(frame) = state.ahead.pop_front() {
                    state.reach = Reach::After(frame.pts);
                    state.answer = None;
                    state.behind.clear();
                    state.behind_floor = None;
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
            },
            |_| true,
        );
        match claimed {
            Err(result) => result,
            Ok((mut state, decoder)) => {
                state.answer = None;
                state.behind.clear();
                state.behind_floor = None;
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
    run_on_pool(Job::Open(Box::new(move || {
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
    })));
    OpeningDecoder { receive }
}

#[cfg(test)]
#[path = "ahead_reverse_tests.rs"]
mod reverse_tests;
#[cfg(test)]
#[path = "ahead_tests.rs"]
mod tests;
