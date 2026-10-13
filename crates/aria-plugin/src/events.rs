//! Typed events: notifications posted to a delivery thread, and waterfall and
//! serial dispatch on the caller's thread.
//!
//! A notification ([`Event`]) is posted, not called. [`EventBus::post`] queues
//! it and returns; one delivery thread per bus runs the handlers in post order,
//! so a slow handler delays later notifications and never the thread that
//! posted. A handler registered with [`EventBus::subscribe_main`] runs on the
//! thread that calls [`EventBus::run_main`] instead, which in the editor is the
//! UI thread.
//!
//! Waterfall and serial events return a result to the caller, so their handlers
//! run on the caller's thread, in registration order.

use std::any::{Any, TypeId};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, RwLock};
use rustc_hash::FxHashMap;

use crate::registration::Registration;

/// Marker for notifications.
pub trait Event: Send + Sync + 'static {}

/// Marker for sequential short-circuit events.
pub trait WaterfallEvent: Send + Sync + 'static {
    /// Continue the chain, or stop.
    fn should_stop(&self) -> bool;
}

/// Marker for first-non-None-wins events.
pub trait SerialEvent: Send + Sync + 'static {}

/// Result of a waterfall dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterfallResult {
    /// Every handler ran.
    Completed,
    /// A handler set the stop flag.
    Stopped,
}

/// Where a notification handler runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    /// The bus's delivery thread.
    Delivery,
    /// The thread that calls [`EventBus::run_main`].
    Main,
}

/// One registered handler. `live` is cleared when its registration drops, so a
/// notification already queued for it is skipped.
struct Handler {
    call: Box<dyn Any + Send + Sync>,
    lane: Lane,
    live: AtomicBool,
}

struct HandlerEntry {
    id: usize,
    handler: Arc<Handler>,
}

type Delivery = Box<dyn FnOnce() + Send>;

/// The delivery thread. Dropping it closes the queue, and the thread exits
/// once it has delivered what was already posted.
struct Dispatcher {
    jobs: mpsc::Sender<(u64, Delivery)>,
    thread: ThreadId,
}

impl Dispatcher {
    fn spawn(delivered: Arc<Delivered>) -> std::io::Result<Self> {
        let (jobs, queue) = mpsc::channel::<(u64, Delivery)>();
        let handle = std::thread::Builder::new()
            .name("aria-events".into())
            .spawn(move || {
                for (sequence, delivery) in queue {
                    delivery();
                    delivered.reach(sequence);
                }
            })?;
        Ok(Self {
            jobs,
            thread: handle.thread().id(),
        })
    }
}

/// How many notifications were posted and how many the delivery thread has
/// finished, for [`EventBus::flush`].
#[derive(Default)]
struct Delivered {
    counts: Mutex<(u64, u64)>,
    changed: Condvar,
}

impl Delivered {
    fn reach(&self, sequence: u64) {
        self.counts.lock().1 = sequence;
        self.changed.notify_all();
    }
}

/// Handler calls waiting for [`EventBus::run_main`].
#[derive(Default)]
struct MainLane {
    queue: Mutex<VecDeque<Delivery>>,
    waker: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl MainLane {
    fn push(&self, delivery: Delivery) {
        let first = {
            let mut queue = self.queue.lock();
            queue.push_back(delivery);
            queue.len() == 1
        };
        if first {
            self.wake();
        }
    }

    fn wake(&self) {
        let waker = self.waker.read().clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

/// Run one handler for `event`, unless its registration has dropped. A panic is
/// logged and contained, so one handler cannot stop delivery to the rest.
fn call<E: Event>(handler: &Handler, event: &E) {
    if !handler.live.load(Ordering::Acquire) {
        return;
    }
    let Some(f) = handler.call.downcast_ref::<Box<dyn Fn(&E) + Send + Sync>>() else {
        return;
    };
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| f(event))) {
        tracing::error!(
            event = std::any::type_name::<E>(),
            panic = %panic_message(payload.as_ref()),
            "event handler panicked"
        );
    }
}

/// The text of a panic payload.
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no message".to_string()
    }
}

/// The event bus. Shared via `Arc` so `Registration` guards can hold a `Weak`.
pub struct EventBus {
    handlers: RwLock<FxHashMap<TypeId, Vec<HandlerEntry>>>,
    next_id: AtomicUsize,
    /// Started by the first notification that has a handler.
    dispatcher: Mutex<Option<Dispatcher>>,
    delivered: Arc<Delivered>,
    main: Arc<MainLane>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(FxHashMap::default()),
            next_id: AtomicUsize::new(0),
            dispatcher: Mutex::new(None),
            delivered: Arc::default(),
            main: Arc::default(),
        }
    }

    /// Whether any handler of any kind is registered for `E`. Lets an emitter
    /// skip building an event nobody reads.
    pub fn has_handlers<E: 'static>(&self) -> bool {
        self.handlers
            .read()
            .get(&TypeId::of::<E>())
            .is_some_and(|entries| !entries.is_empty())
    }

    fn insert(
        self: &Arc<Self>,
        type_id: TypeId,
        call: Box<dyn Any + Send + Sync>,
        lane: Lane,
    ) -> Registration {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.handlers
            .write()
            .entry(type_id)
            .or_default()
            .push(HandlerEntry {
                id,
                handler: Arc::new(Handler {
                    call,
                    lane,
                    live: AtomicBool::new(true),
                }),
            });
        Registration::handler(type_id, id, self)
    }

    /// The handlers registered for `type_id`, in registration order.
    fn targets(&self, type_id: TypeId) -> Vec<Arc<Handler>> {
        self.handlers
            .read()
            .get(&type_id)
            .map(|entries| entries.iter().map(|e| Arc::clone(&e.handler)).collect())
            .unwrap_or_default()
    }

    /// Subscribe to a notification on the delivery thread. Returns a Drop guard.
    pub fn subscribe<E, F>(self: &Arc<Self>, handler: F) -> Registration
    where
        E: Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let boxed: Box<dyn Fn(&E) + Send + Sync> = Box::new(handler);
        self.insert(TypeId::of::<E>(), Box::new(boxed), Lane::Delivery)
    }

    /// Subscribe to a notification on the thread that calls
    /// [`run_main`](Self::run_main). Returns a Drop guard.
    pub fn subscribe_main<E, F>(self: &Arc<Self>, handler: F) -> Registration
    where
        E: Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let boxed: Box<dyn Fn(&E) + Send + Sync> = Box::new(handler);
        self.insert(TypeId::of::<E>(), Box::new(boxed), Lane::Main)
    }

    /// Queue `event` for every handler registered now, and return.
    ///
    /// Notifications are delivered in post order. A handler registered after
    /// this call does not receive this event; one whose registration drops
    /// before delivery does not either. With no handler registered this does
    /// nothing.
    pub fn post<E: Event>(&self, event: E) {
        let targets = self.targets(TypeId::of::<E>());
        if targets.is_empty() {
            return;
        }
        let event = Arc::new(event);
        let main = Arc::clone(&self.main);
        self.enqueue(Box::new(move || {
            for handler in targets {
                match handler.lane {
                    Lane::Delivery => call(&handler, &*event),
                    Lane::Main => {
                        let event = Arc::clone(&event);
                        main.push(Box::new(move || call(&handler, &*event)));
                    }
                }
            }
        }));
    }

    fn enqueue(&self, delivery: Delivery) {
        let mut dispatcher = self.dispatcher.lock();
        let running = match dispatcher.as_mut() {
            Some(running) => running,
            None => match Dispatcher::spawn(Arc::clone(&self.delivered)) {
                Ok(spawned) => dispatcher.insert(spawned),
                Err(err) => {
                    drop(dispatcher);
                    tracing::error!(
                        error = %err,
                        "the event delivery thread did not start; delivering on the posting thread"
                    );
                    delivery();
                    return;
                }
            },
        };
        // Numbered under the dispatcher lock, so numbers reach the thread in
        // order and `flush` can wait for one.
        let sequence = {
            let mut counts = self.delivered.counts.lock();
            counts.0 += 1;
            counts.0
        };
        if let Err(mpsc::SendError((sequence, delivery))) = running.jobs.send((sequence, delivery))
        {
            // The thread is gone. Deliver here; the next post starts another.
            *dispatcher = None;
            drop(dispatcher);
            delivery();
            self.delivered.reach(sequence);
        }
    }

    /// Wait until every notification posted before this call has run on the
    /// delivery thread and been queued for the main thread.
    ///
    /// Returns `false` when `within` elapses first, and at once when called
    /// from the delivery thread, which cannot wait for itself.
    pub fn flush(&self, within: Duration) -> bool {
        let me = std::thread::current().id();
        if self
            .dispatcher
            .lock()
            .as_ref()
            .is_some_and(|dispatcher| dispatcher.thread == me)
        {
            return false;
        }
        let deadline = Instant::now() + within;
        let mut counts = self.delivered.counts.lock();
        let target = counts.0;
        while counts.1 < target {
            if self
                .delivered
                .changed
                .wait_until(&mut counts, deadline)
                .timed_out()
            {
                return counts.1 >= target;
            }
        }
        true
    }

    /// Run every main-lane handler call queued so far, on this thread, in post
    /// order. Returns how many ran.
    pub fn run_main(&self) -> usize {
        let batch = std::mem::take(&mut *self.main.queue.lock());
        let count = batch.len();
        for delivery in batch {
            delivery();
        }
        count
    }

    /// Set what the delivery thread calls when main-lane work arrives and none
    /// was waiting. The host's waker schedules a [`run_main`](Self::run_main)
    /// on its main thread. Calls the waker at once if work is already waiting.
    pub fn set_main_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        *self.main.waker.write() = Some(Arc::new(waker));
        if !self.main.queue.lock().is_empty() {
            self.main.wake();
        }
    }

    /// Sequential dispatch. A handler that sets the stop flag ends the chain.
    pub fn waterfall<E, F>(self: &Arc<Self>, handler: F) -> Registration
    where
        E: WaterfallEvent,
        F: Fn(&mut E) + Send + Sync + 'static,
    {
        let boxed: Box<dyn Fn(&mut E) + Send + Sync> = Box::new(handler);
        self.insert(TypeId::of::<E>(), Box::new(boxed), Lane::Delivery)
    }

    /// Run waterfall handlers on this thread. Returns
    /// [`WaterfallResult::Stopped`] if a handler set the stop flag.
    pub fn emit_waterfall<E: WaterfallEvent>(&self, event: &mut E) -> WaterfallResult {
        for handler in self.targets(TypeId::of::<E>()) {
            if !handler.live.load(Ordering::Acquire) {
                continue;
            }
            if let Some(f) = handler
                .call
                .downcast_ref::<Box<dyn Fn(&mut E) + Send + Sync>>()
            {
                f(event);
                if event.should_stop() {
                    return WaterfallResult::Stopped;
                }
            }
        }
        WaterfallResult::Completed
    }

    /// Subscribe to a serial (first-non-None-wins) event.
    pub fn on_serial<E, R, F>(self: &Arc<Self>, handler: F) -> Registration
    where
        E: SerialEvent,
        R: Send + Sync + 'static,
        F: Fn(&E) -> Option<R> + Send + Sync + 'static,
    {
        let boxed: Box<dyn Fn(&E) -> Option<R> + Send + Sync> = Box::new(handler);
        self.insert(TypeId::of::<E>(), Box::new(boxed), Lane::Delivery)
    }

    /// First non-None wins, on this thread. Remaining handlers are not called.
    pub fn serial<E, R>(&self, event: &E) -> Option<R>
    where
        E: SerialEvent,
        R: Send + Sync + 'static,
    {
        for handler in self.targets(TypeId::of::<E>()) {
            if !handler.live.load(Ordering::Acquire) {
                continue;
            }
            if let Some(f) = handler
                .call
                .downcast_ref::<Box<dyn Fn(&E) -> Option<R> + Send + Sync>>()
                && let Some(value) = f(event)
            {
                return Some(value);
            }
        }
        None
    }

    pub(crate) fn remove_handler(&self, type_id: TypeId, handler_id: usize) {
        if let Some(entries) = self.handlers.write().get_mut(&type_id) {
            entries.retain(|entry| {
                let keep = entry.id != handler_id;
                if !keep {
                    entry.handler.live.store(false, Ordering::Release);
                }
                keep
            });
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
