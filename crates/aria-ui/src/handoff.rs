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
//! Three shapes satisfy both, and this module is the only place any is
//! written:
//!
//! * [`on_a_thread`] runs work that **finishes** on a thread of its own and
//!   waits for that thread from a background task. The wait is as long as the
//!   work and no longer, and the thread means a codec that panics costs the
//!   frame rather than the window.
//! * [`reply`] waits for the answer to a job handed to a long-lived worker
//!   thread, such as the preview worker. The worker answers every job it takes
//!   or drops the reply, so the wait is as long as the queue ahead of it.
//! * [`wakes`] is a queue for an event that **may never come** — a job that is
//!   cancelled, a dialog nobody opens. Writing to it never blocks, from any
//!   thread. A reader on the window's thread awaits it directly. A reader
//!   waiting on another thread's writer sleeps until that writer sends in the
//!   shipped editor, and checks the queue on the scheduler's own timer under
//!   test, so no foreign thread ever wakes a test task.

use std::time::Duration;

use gpui::{BackgroundExecutor, Task};

/// How often the test scheduler checks for a wake from another thread.
///
/// A frame at 60 fps, so a job's progress and an assistant's next word land in
/// the frame after they were produced. The shipped editor does not check: its
/// reader sleeps until a writer sends.
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

/// Await the answer a worker thread sends on `answer`.
///
/// For a job handed to a thread that outlives it. The worker sends one value
/// or drops the sender; a dropped sender, from a job that panicked or a worker
/// that stopped, resolves to `None`. The wait blocks the pool thread it runs
/// on, for the same reason [`on_a_thread`] does: the worker never wakes a task.
pub fn reply<T: Send + 'static>(
    executor: &BackgroundExecutor,
    answer: async_channel::Receiver<T>,
) -> Task<Option<T>> {
    executor.spawn(async move { answer.recv_blocking().ok() })
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

    /// The next wake from a writer on another thread, or `None` once every
    /// writer is gone.
    ///
    /// On the platform executor the reader sleeps until a writer sends or the
    /// last writer is dropped, so a job that never reports and a window at rest
    /// cost no wakes at all.
    ///
    /// The test scheduler rejects a task woken from a thread it does not own,
    /// so under test the queue is checked on the scheduler's own timer every
    /// [`CHECK`] instead: the writer only drops a value into the queue, and the
    /// scheduler finds it.
    pub async fn next_from_a_thread(&self, executor: &BackgroundExecutor) -> Option<T> {
        if executor.dispatcher().as_test().is_none() {
            return self.0.recv().await.ok();
        }
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

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
    async fn a_worker_reply_reaches_the_waiter(cx: &mut gpui::TestAppContext) {
        let executor = cx.executor();
        executor.allow_parking();
        let (send, answer) = async_channel::bounded(1);
        let worker = std::thread::spawn(move || send.try_send(5).is_ok());
        assert_eq!(reply(&executor, answer).await, Some(5));
        assert!(worker.join().is_ok_and(|sent| sent));
    }

    /// A worker that drops a job without answering, because it panicked or
    /// stopped, ends the wait instead of holding the pool thread forever.
    #[gpui::test]
    async fn a_dropped_reply_ends_the_wait(cx: &mut gpui::TestAppContext) {
        let executor = cx.executor();
        executor.allow_parking();
        let (send, answer) = async_channel::bounded::<u8>(1);
        std::thread::spawn(move || drop(send));
        assert_eq!(reply(&executor, answer).await, None);
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

    /// A platform dispatcher that runs each runnable on a thread of its own
    /// and counts the timers it is asked to arm.
    #[derive(Default)]
    struct Threads {
        timers: AtomicUsize,
    }

    impl gpui::PlatformDispatcher for Threads {
        fn is_main_thread(&self) -> bool {
            false
        }

        fn dispatch(&self, runnable: gpui::RunnableVariant, _: gpui::Priority) {
            std::thread::spawn(move || runnable.run());
        }

        fn dispatch_on_main_thread(
            &self,
            runnable: gpui::RunnableVariant,
            priority: gpui::Priority,
        ) {
            self.dispatch(runnable, priority);
        }

        fn dispatch_after(&self, duration: Duration, runnable: gpui::RunnableVariant) {
            self.timers.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                runnable.run()
            });
        }

        fn spawn_realtime(&self, work: Box<dyn FnOnce() + Send>) {
            std::thread::spawn(work);
        }
    }

    /// What a reader on the platform executor returned, and how many timers
    /// the executor armed while it waited for `writer`.
    fn platform_wait(
        writer: impl FnOnce(Wake<u8>) + Send + 'static,
    ) -> (Result<Option<u8>, mpsc::RecvTimeoutError>, usize) {
        let threads = Arc::new(Threads::default());
        let executor = BackgroundExecutor::new(threads.clone());
        let (wake, wakes) = wakes::<u8>();
        let (done, result) = mpsc::channel();
        let reader = executor.clone();
        executor
            .spawn(async move {
                let _ = done.send(wakes.next_from_a_thread(&reader).await);
            })
            .detach();
        std::thread::spawn(move || {
            // Long enough for a reader that checked on a timer to arm several.
            std::thread::sleep(CHECK * 6);
            writer(wake);
        });
        let returned = result.recv_timeout(Duration::from_secs(10));
        (returned, threads.timers.load(Ordering::SeqCst))
    }

    /// The shipped editor's reader sleeps until the writer sends. A timer
    /// armed per check is what kept an idle window busy sixty times a second
    /// for every job, plugin and transport watcher.
    ///
    /// Not covered: the test scheduler's own path, which keeps its timer and
    /// is covered by `a_wake_from_a_thread_is_found_on_the_timer`.
    #[test]
    fn a_platform_reader_sleeps_until_the_writer_sends() {
        let (returned, timers) = platform_wait(|wake| {
            wake.wake(7);
        });
        assert_eq!(
            returned,
            Ok(Some(7)),
            "the reader missed the wake or never returned"
        );
        assert_eq!(
            timers, 0,
            "the reader armed {timers} timers while it waited"
        );
    }

    /// A writer that goes away ends a platform wait at once, without a timer
    /// finding the closed queue.
    #[test]
    fn a_platform_reader_ends_when_every_writer_is_gone() {
        let (returned, timers) = platform_wait(drop::<Wake<u8>>);
        assert_eq!(returned, Ok(None), "the reader outlived its last writer");
        assert_eq!(
            timers, 0,
            "the reader armed {timers} timers while it waited"
        );
    }
}
