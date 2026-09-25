//! Waiting for work that is not on the thread that draws.
//!
//! gpui's background executor is a thread pool in the shipped editor and a
//! single deterministic scheduler under test, and both facts constrain a wait:
//!
//! * A blocking wait inside a task holds the thread the task is running on. In
//!   the test scheduler that thread is the one ticking the run loop, so a wait
//!   for something that never arrives stops the whole application rather than
//!   costing a frame. Building a shell window used to hang forever because the
//!   job watcher blocked on a job that never ran.
//! * A task woken from a thread the scheduler does not own is non-determinism
//!   the test scheduler refuses outright, so a raw thread must never be the
//!   thing that wakes a task.
//!
//! Two shapes satisfy both, and this module is the only place either is
//! written:
//!
//! * [`on_a_thread`] runs work that **finishes** on a thread of its own and
//!   waits for that thread from a background task. The wait is as long as the
//!   work and no longer, and the thread means a codec that panics costs the
//!   frame rather than the window.
//! * [`wakes`] is a queue for an event that **may never come** — a job that is
//!   cancelled, a dialog nobody opens. Writing to it never blocks, from any
//!   thread. A reader on the window's thread awaits it directly; a reader
//!   waiting on another thread's writer checks it on the scheduler's own timer,
//!   so no foreign thread ever wakes a task.

use std::time::Duration;

use gpui::{BackgroundExecutor, Task};

/// How often a wake from another thread is checked for.
///
/// A frame at 60 fps, so a job's progress and an assistant's next word land in
/// the frame after they were produced. The check is one atomic read of an empty
/// queue, which is why it can afford to be this frequent.
pub const CHECK: Duration = Duration::from_millis(16);

/// Run `work` on its own thread and await what it returns.
///
/// For work that finishes: a decode, a render, a request with a timeout. The
/// wait is bounded by the work itself, which is what makes it safe to hold a
/// pool thread — the alternative, running the work as a task, hands a panicking
/// codec the whole executor.
///
/// The thread is named, so `/proc/<pid>/task/*/comm` says what a hung run is
/// waiting for. A panic inside `work` is not a panic here: the thread dies and
/// the caller gets `None`.
pub fn on_a_thread<T: Send + 'static>(
    executor: &BackgroundExecutor,
    label: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Task<Option<T>> {
    let started = std::thread::Builder::new()
        .name(label.to_owned())
        .spawn(work);
    executor.spawn(async move {
        // A thread that could not be started, and a thread that panicked, are
        // the same answer: there is no result.
        started.ok()?.join().ok()
    })
}

/// A queue of wakes from any thread to the one that draws.
///
/// Unbounded because a writer is a job thread or a subscription callback that
/// must not be made to wait by a window that is busy drawing.
pub fn wakes<T>() -> (Wake<T>, Wakes<T>) {
    let (to_window, from_work) = async_channel::unbounded();
    (Wake(to_window), Wakes(from_work))
}

/// The writing half of [`wakes`]. Never blocks, from any thread.
pub struct Wake<T>(async_channel::Sender<T>);

impl<T> Wake<T> {
    /// Hand `value` to the reader. `false` means the reader is gone, which is
    /// shutdown rather than an error.
    pub fn wake(&self, value: T) -> bool {
        self.0.try_send(value).is_ok()
    }
}

impl<T> Clone for Wake<T> {
    fn clone(&self) -> Self {
        Self(async_channel::Sender::clone(&self.0))
    }
}

impl<T> std::fmt::Debug for Wake<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wake({} waiting)", self.0.len())
    }
}

/// The reading half of [`wakes`], read by a task on the window's thread.
pub struct Wakes<T>(async_channel::Receiver<T>);

impl<T> Wakes<T> {
    /// The next wake, or `None` once every writer is gone.
    ///
    /// For a queue written on the window's own thread — a command asking for a
    /// dialog — where the wake and the reader share a scheduler. A queue a
    /// *thread* writes to must use [`Self::next_from_a_thread`] instead.
    pub async fn next(&self) -> Option<T> {
        self.0.recv().await.ok()
    }

    /// The next wake from a writer on another thread, checked on the
    /// scheduler's own timer.
    ///
    /// The timer is what keeps the wait deterministic: the writer only drops a
    /// value into the queue, and the scheduler finds it. Nothing blocks and no
    /// foreign thread ever wakes a task, so a job that is never started, or
    /// cancelled before it reports, costs one empty check per frame instead of
    /// a thread.
    pub async fn next_from_a_thread(&self, executor: &BackgroundExecutor) -> Option<T> {
        loop {
            if let Some(value) = self.try_next() {
                return Some(value);
            }
            // Empty and closed: every writer is gone without reporting, which
            // is a job that panicked or a session shutting down.
            if self.0.is_closed() {
                return None;
            }
            executor.timer(CHECK).await;
        }
    }

    /// A wake that is already here, without waiting for one that is not.
    pub fn try_next(&self) -> Option<T> {
        self.0.try_recv().ok()
    }
}

impl<T> Clone for Wakes<T> {
    fn clone(&self) -> Self {
        Self(async_channel::Receiver::clone(&self.0))
    }
}

impl<T> std::fmt::Debug for Wakes<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wakes({} waiting)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    async fn a_thread_hands_its_result_back(cx: &mut gpui::TestAppContext) {
        let executor = cx.executor();
        executor.allow_parking();
        let waited = on_a_thread(&executor, "test-result", || 6 * 7);
        assert_eq!(waited.await, Some(42));
    }

    #[gpui::test]
    async fn a_panic_costs_the_result_and_nothing_else(cx: &mut gpui::TestAppContext) {
        let executor = cx.executor();
        executor.allow_parking();
        let waited = on_a_thread(&executor, "test-panic", || -> u8 {
            panic!("the codec fell over");
        });
        assert_eq!(waited.await, None, "a panicking thread reported a result");
        // The caller is still running, which is the point of the thread.
        let after = on_a_thread(&executor, "test-after", || 1);
        assert_eq!(after.await, Some(1));
    }

    #[gpui::test]
    async fn a_wake_reaches_the_reader_in_order(_cx: &mut gpui::TestAppContext) {
        let (wake, wakes) = wakes();
        assert!(wake.wake("first"));
        assert!(wake.wake("second"));
        assert_eq!(wakes.next().await, Some("first"));
        assert_eq!(wakes.next().await, Some("second"));
    }

    #[test]
    fn an_empty_queue_answers_at_once() {
        let (wake, wakes) = wakes::<u8>();
        assert_eq!(wakes.try_next(), None, "an empty queue produced a wake");
        assert!(wake.wake(3));
        assert_eq!(wakes.try_next(), Some(3));
    }

    #[test]
    fn a_reader_that_is_gone_refuses_the_wake() {
        let (wake, wakes) = wakes();
        drop(wakes);
        assert!(!wake.wake(1), "a wake with nobody to read it was accepted");
    }

    #[gpui::test]
    async fn wakes_end_when_every_writer_is_gone(_cx: &mut gpui::TestAppContext) {
        let (wake, wakes) = wakes::<u8>();
        let second = Wake::clone(&wake);
        drop(wake);
        drop(second);
        assert_eq!(
            wakes.next().await,
            None,
            "the reader kept waiting for a writer that no longer exists"
        );
    }

    /// A wake written by a thread is read without that thread waking anything:
    /// the scheduler's timer finds it. This is the shape every job report uses.
    #[gpui::test]
    async fn a_wake_from_a_thread_is_found_on_the_timer(cx: &mut gpui::TestAppContext) {
        let executor = cx.executor();
        executor.allow_parking();
        let (wake, wakes) = wakes();
        std::thread::spawn(move || {
            wake.wake(9);
        });
        assert_eq!(wakes.next_from_a_thread(&executor).await, Some(9));
    }

    /// A writer that goes away without reporting ends the wait, so a job that
    /// panicked does not leave the panel waiting for a frame that never comes.
    #[gpui::test]
    async fn a_thread_that_reports_nothing_ends_the_wait(cx: &mut gpui::TestAppContext) {
        let executor = cx.executor();
        executor.allow_parking();
        let (wake, wakes) = wakes::<u8>();
        std::thread::spawn(move || {
            drop(wake);
        });
        assert_eq!(wakes.next_from_a_thread(&executor).await, None);
    }
}
