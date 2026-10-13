//! The bus's delivery contract: a notification reaches every handler
//! registered when it was posted, in post order, off the posting thread unless
//! the handler asked for the main lane; a slow or panicking handler delays or
//! loses only its own call; a dropped registration receives nothing still
//! queued; and `flush` is bounded.
//!
//! Not caught here: ordering between two threads that post at once, which the
//! bus does not promise.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// Long enough that only a stuck delivery misses it.
const BOUND: Duration = Duration::from_secs(10);

struct Ping;
impl Event for Ping {}

struct Numbered(u32);
impl Event for Numbered {}

struct Chain {
    value: i32,
    stop: bool,
}
impl WaterfallEvent for Chain {
    fn should_stop(&self) -> bool {
        self.stop
    }
}

struct Ask;
impl SerialEvent for Ask {}

fn bus() -> Arc<EventBus> {
    Arc::new(EventBus::new())
}

#[test]
fn every_handler_receives_every_notification_in_post_order() {
    let bus = bus();
    let seen: Arc<Mutex<Vec<(char, u32)>>> = Arc::default();
    let registrations: Vec<Registration> = ['a', 'b']
        .into_iter()
        .map(|name| {
            let seen = Arc::clone(&seen);
            bus.subscribe::<Numbered, _>(move |event| seen.lock().push((name, event.0)))
        })
        .collect();
    for n in 0..200 {
        bus.post(Numbered(n));
    }
    assert!(bus.flush(BOUND), "delivery must finish within the bound");
    let expected: Vec<(char, u32)> = (0..200).flat_map(|n| [('a', n), ('b', n)]).collect();
    assert_eq!(*seen.lock(), expected);
    drop(registrations);
}

#[test]
fn post_returns_while_a_handler_is_still_running_and_flush_is_bounded() {
    let bus = bus();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let ran = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&ran);
    let _slow = bus.subscribe::<Ping, _>(move |_| {
        let _ = gate.lock().recv_timeout(BOUND);
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let posted = Instant::now();
    bus.post(Ping);
    assert!(
        posted.elapsed() < Duration::from_secs(1),
        "post must not wait for the handler"
    );
    let waited = Instant::now();
    assert!(
        !bus.flush(Duration::from_millis(50)),
        "flush reports a delivery that has not finished"
    );
    let waited = waited.elapsed();
    assert!(
        waited >= Duration::from_millis(50) && waited < Duration::from_secs(2),
        "flush must give up at its bound, waited {waited:?}"
    );
    assert_eq!(ran.load(Ordering::SeqCst), 0);

    release.send(()).expect("the handler is waiting");
    assert!(bus.flush(BOUND));
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

#[test]
fn a_panicking_handler_loses_only_its_own_call() {
    let bus = bus();
    let _panics = bus.subscribe::<Ping, _>(|_| panic!("a broken plugin"));
    let count = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&count);
    let _counts = bus.subscribe::<Ping, _>(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    for _ in 0..3 {
        bus.post(Ping);
    }
    assert!(
        bus.flush(BOUND),
        "a panic must not stop the delivery thread"
    );
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn a_dropped_registration_receives_nothing_still_queued() {
    let bus = bus();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    // Registered first, so it holds the delivery thread while the second is
    // still queued behind it.
    let _first = bus.subscribe::<Ping, _>(move |_| {
        let _ = gate.lock().recv_timeout(BOUND);
    });
    let count = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&count);
    let second = bus.subscribe::<Ping, _>(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    bus.post(Ping);
    bus.post(Ping);
    drop(second);
    release.send(()).expect("the first handler is waiting");
    release.send(()).expect("the first handler waits again");
    assert!(bus.flush(BOUND));
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "a handler whose registration dropped must not run"
    );
}

#[test]
fn a_handler_registered_after_a_post_does_not_receive_it() {
    let bus = bus();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let _first = bus.subscribe::<Ping, _>(move |_| {
        let _ = gate.lock().recv_timeout(BOUND);
    });
    bus.post(Ping);
    let count = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&count);
    let _late = bus.subscribe::<Ping, _>(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    release.send(()).expect("the first handler is waiting");
    assert!(bus.flush(BOUND));
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn delivery_runs_off_the_posting_thread_and_main_lane_handlers_wait_for_run_main() {
    let bus = bus();
    let poster = std::thread::current().id();
    let delivery_threads: Arc<Mutex<Vec<ThreadId>>> = Arc::default();
    let sink = Arc::clone(&delivery_threads);
    let _off = bus.subscribe::<Numbered, _>(move |_| sink.lock().push(std::thread::current().id()));
    let main_seen: Arc<Mutex<Vec<(u32, ThreadId)>>> = Arc::default();
    let sink = Arc::clone(&main_seen);
    let _main = bus.subscribe_main::<Numbered, _>(move |event| {
        sink.lock().push((event.0, std::thread::current().id()));
    });
    let wakes = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&wakes);
    bus.set_main_waker(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    for n in 0..3 {
        bus.post(Numbered(n));
    }
    assert!(bus.flush(BOUND));
    assert!(
        delivery_threads
            .lock()
            .iter()
            .all(|thread| *thread != poster),
        "a notification handler must not run on the posting thread"
    );
    assert!(
        main_seen.lock().is_empty(),
        "a main-lane handler runs only inside run_main"
    );
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        1,
        "one wake per batch of main-lane work"
    );

    assert_eq!(bus.run_main(), 3);
    let me = std::thread::current().id();
    assert_eq!(*main_seen.lock(), [(0, me), (1, me), (2, me)]);
    assert_eq!(bus.run_main(), 0, "run_main drains the lane");

    bus.post(Numbered(3));
    assert!(bus.flush(BOUND));
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "an empty lane wakes again");
}

#[test]
fn a_waker_set_after_work_arrived_is_called_at_once() {
    let bus = bus();
    let _main = bus.subscribe_main::<Ping, _>(|_| {});
    bus.post(Ping);
    assert!(bus.flush(BOUND));
    let wakes = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&wakes);
    bus.set_main_waker(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert_eq!(bus.run_main(), 1);
}

#[test]
fn flush_on_the_delivery_thread_returns_instead_of_waiting_for_itself() {
    let bus = bus();
    let answer: Arc<Mutex<Option<(bool, Duration)>>> = Arc::default();
    let sink = Arc::clone(&answer);
    let weak = Arc::downgrade(&bus);
    let _flushes = bus.subscribe::<Ping, _>(move |_| {
        let Some(bus) = weak.upgrade() else { return };
        let at = Instant::now();
        let flushed = bus.flush(BOUND);
        *sink.lock() = Some((flushed, at.elapsed()));
    });
    bus.post(Ping);
    assert!(bus.flush(BOUND));
    let (flushed, took) = answer.lock().expect("the handler ran");
    assert!(!flushed, "the delivery thread cannot have delivered itself");
    assert!(
        took < Duration::from_secs(1),
        "and must not wait trying, took {took:?}"
    );
}

#[test]
fn a_notification_nobody_handles_starts_no_thread() {
    let bus = bus();
    bus.post(Ping);
    assert!(bus.dispatcher.lock().is_none());
    assert!(bus.flush(Duration::ZERO), "nothing was posted to wait for");
}

#[test]
fn waterfall_short_circuits() {
    let bus = bus();
    let called = Arc::new(AtomicU32::new(0));
    let _r1 = bus.waterfall::<Chain, _>(|e| e.value += 1);
    let _r2 = bus.waterfall::<Chain, _>(|e| {
        e.value += 10;
        e.stop = true;
    });
    let c = called.clone();
    let _r3 = bus.waterfall::<Chain, _>(move |_| {
        c.fetch_add(1, Ordering::Relaxed);
    });
    let mut event = Chain {
        value: 0,
        stop: false,
    };
    let result = bus.emit_waterfall(&mut event);
    assert_eq!(result, WaterfallResult::Stopped);
    assert_eq!(event.value, 11);
    assert_eq!(called.load(Ordering::Relaxed), 0);
}

#[test]
fn serial_first_wins() {
    let bus = bus();
    let called = Arc::new(AtomicU32::new(0));
    let _r1 = bus.on_serial::<Ask, &'static str, _>(|_| Some("first"));
    let c = called.clone();
    let _r2 = bus.on_serial::<Ask, &'static str, _>(move |_| {
        c.fetch_add(1, Ordering::Relaxed);
        Some("second")
    });
    let _r3 = bus.on_serial::<Ask, &'static str, _>(|_| Some("third"));
    assert_eq!(bus.serial::<Ask, &'static str>(&Ask), Some("first"));
    assert_eq!(called.load(Ordering::Relaxed), 0);
}

#[test]
fn subscription_drop_unsubscribes() {
    let bus = bus();
    let counter = Arc::new(AtomicU32::new(0));
    {
        let c = counter.clone();
        let _reg = bus.subscribe::<Ping, _>(move |_| {
            c.fetch_add(1, Ordering::Relaxed);
        });
        bus.post(Ping);
        assert!(bus.flush(BOUND));
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }
    bus.post(Ping);
    assert!(bus.flush(BOUND));
    assert_eq!(counter.load(Ordering::Relaxed), 1);
    assert!(!bus.has_handlers::<Ping>());
}

#[test]
fn subscribe_inside_handler_does_not_deadlock() {
    let bus = bus();
    let counter = Arc::new(AtomicU32::new(0));
    let retained = Arc::new(Mutex::new(None));
    let bus_clone = Arc::downgrade(&bus);
    let counter_clone = counter.clone();
    let retained_clone = Arc::clone(&retained);
    let _r1 = bus.subscribe::<Ping, _>(move |_| {
        counter_clone.fetch_add(1, Ordering::Relaxed);
        let inner_counter = counter_clone.clone();
        let Some(bus) = bus_clone.upgrade() else {
            return;
        };
        let mut retained = retained_clone.lock();
        if retained.is_none() {
            *retained = Some(bus.subscribe::<Ping, _>(move |_| {
                inner_counter.fetch_add(10, Ordering::Relaxed);
            }));
        }
    });
    bus.post(Ping);
    assert!(bus.flush(BOUND));
    assert_eq!(counter.load(Ordering::Relaxed), 1);
    bus.post(Ping);
    assert!(bus.flush(BOUND));
    assert_eq!(counter.load(Ordering::Relaxed), 12);
}
