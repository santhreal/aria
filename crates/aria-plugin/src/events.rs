//! Typed event system with emit, waterfall, and serial dispatch.

use std::any::{Any, TypeId};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;

use crate::registration::Registration;

/// Marker for fire-and-forget events.
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

struct HandlerEntry {
    id: usize,
    handler: Arc<dyn Any + Send + Sync>,
}

/// The event bus. Shared via `Arc` so `Registration` guards can hold a `Weak`.
pub struct EventBus {
    handlers: RwLock<FxHashMap<TypeId, Vec<HandlerEntry>>>,
    next_id: Mutex<usize>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(FxHashMap::default()),
            next_id: Mutex::new(0),
        }
    }

    fn next_id(&self) -> usize {
        let mut counter = self.next_id.lock();
        let id = *counter;
        *counter += 1;
        id
    }

    /// Whether any handler of any kind is registered for `E`. Lets an emitter
    /// skip building an event nobody reads.
    pub fn has_handlers<E: 'static>(&self) -> bool {
        self.handlers
            .read()
            .get(&TypeId::of::<E>())
            .is_some_and(|entries| !entries.is_empty())
    }

    /// Subscribe to an emit event. Returns a Drop guard.
    pub fn subscribe<E, F>(self: &Arc<Self>, handler: F) -> Registration
    where
        E: Event + Send + Sync,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();
        let id = self.next_id();
        let boxed: Box<dyn Fn(&E) + Send + Sync> = Box::new(handler);
        self.handlers
            .write()
            .entry(type_id)
            .or_default()
            .push(HandlerEntry {
                id,
                handler: Arc::new(boxed),
            });
        Registration::handler(type_id, id, self)
    }

    /// Fire-and-forget: every handler runs in registration order.
    pub fn emit<E: Event + Send + Sync>(&self, event: E) {
        let type_id = TypeId::of::<E>();
        let entries: Vec<Arc<dyn Any + Send + Sync>> = {
            let handlers = self.handlers.read();
            let Some(entries) = handlers.get(&type_id) else {
                return;
            };
            entries.iter().map(|e| Arc::clone(&e.handler)).collect()
        };
        for entry in entries {
            if let Some(f) = entry.downcast_ref::<Box<dyn Fn(&E) + Send + Sync>>() {
                f(&event);
            }
        }
    }

    /// Sequential dispatch. A handler that sets the stop flag ends the chain.
    pub fn waterfall<E, F>(self: &Arc<Self>, handler: F) -> Registration
    where
        E: WaterfallEvent + Send + Sync,
        F: Fn(&mut E) + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();
        let id = self.next_id();
        let boxed: Box<dyn Fn(&mut E) + Send + Sync> = Box::new(handler);
        self.handlers
            .write()
            .entry(type_id)
            .or_default()
            .push(HandlerEntry {
                id,
                handler: Arc::new(boxed),
            });
        Registration::handler(type_id, id, self)
    }

    /// Run waterfall handlers. Returns [`WaterfallResult::Stopped`] if a
    /// handler set the stop flag.
    pub fn emit_waterfall<E: WaterfallEvent + Send + Sync>(
        &self,
        event: &mut E,
    ) -> WaterfallResult {
        let type_id = TypeId::of::<E>();
        let entries: Vec<Arc<dyn Any + Send + Sync>> = {
            let handlers = self.handlers.read();
            let Some(entries) = handlers.get(&type_id) else {
                return WaterfallResult::Completed;
            };
            entries.iter().map(|e| Arc::clone(&e.handler)).collect()
        };
        for entry in entries {
            if let Some(f) = entry.downcast_ref::<Box<dyn Fn(&mut E) + Send + Sync>>() {
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
        E: SerialEvent + Send + Sync,
        R: Send + Sync + 'static,
        F: Fn(&E) -> Option<R> + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();
        let id = self.next_id();
        let boxed: Box<dyn Fn(&E) -> Option<R> + Send + Sync> = Box::new(handler);
        self.handlers
            .write()
            .entry(type_id)
            .or_default()
            .push(HandlerEntry {
                id,
                handler: Arc::new(boxed),
            });
        Registration::handler(type_id, id, self)
    }

    /// First non-None wins. Remaining handlers are not called.
    pub fn serial<E, R>(&self, event: &E) -> Option<R>
    where
        E: SerialEvent + Send + Sync,
        R: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();
        let entries: Vec<Arc<dyn Any + Send + Sync>> = {
            let handlers = self.handlers.read();
            let Some(entries) = handlers.get(&type_id) else {
                return None;
            };
            entries.iter().map(|e| Arc::clone(&e.handler)).collect()
        };
        for entry in entries {
            if let Some(f) = entry.downcast_ref::<Box<dyn Fn(&E) -> Option<R> + Send + Sync>>() {
                if let Some(value) = f(event) {
                    return Some(value);
                }
            }
        }
        None
    }

    pub(crate) fn remove_handler(&self, type_id: TypeId, handler_id: usize) {
        if let Some(entries) = self.handlers.write().get_mut(&type_id) {
            entries.retain(|entry| entry.id != handler_id);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Ping;
    impl Event for Ping {}

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

    #[test]
    fn emit_all_handlers_run() {
        let bus = Arc::new(EventBus::new());
        let counter = Arc::new(AtomicU32::new(0));
        let a = counter.clone();
        let _r1 = bus.subscribe::<Ping, _>(move |_| {
            a.fetch_add(1, Ordering::Relaxed);
        });
        let b = counter.clone();
        let _r2 = bus.subscribe::<Ping, _>(move |_| {
            b.fetch_add(10, Ordering::Relaxed);
        });
        let c = counter.clone();
        let _r3 = bus.subscribe::<Ping, _>(move |_| {
            c.fetch_add(100, Ordering::Relaxed);
        });
        bus.emit(Ping);
        assert_eq!(counter.load(Ordering::Relaxed), 111);
    }

    #[test]
    fn waterfall_short_circuits() {
        let bus = Arc::new(EventBus::new());
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
        let bus = Arc::new(EventBus::new());
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
        let bus = Arc::new(EventBus::new());
        let counter = Arc::new(AtomicU32::new(0));
        {
            let c = counter.clone();
            let _reg = bus.subscribe::<Ping, _>(move |_| {
                c.fetch_add(1, Ordering::Relaxed);
            });
            bus.emit(Ping);
            assert_eq!(counter.load(Ordering::Relaxed), 1);
        }
        bus.emit(Ping);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn subscribe_inside_handler_does_not_deadlock() {
        let bus = Arc::new(EventBus::new());
        let counter = Arc::new(AtomicU32::new(0));
        let retained = Arc::new(parking_lot::Mutex::new(None));
        let bus_clone = bus.clone();
        let counter_clone = counter.clone();
        let retained_clone = Arc::clone(&retained);
        let _r1 = bus.subscribe::<Ping, _>(move |_| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            let inner_counter = counter_clone.clone();
            *retained_clone.lock() = Some(bus_clone.subscribe::<Ping, _>(move |_| {
                inner_counter.fetch_add(10, Ordering::Relaxed);
            }));
        });
        bus.emit(Ping);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
        bus.emit(Ping);
        assert_eq!(counter.load(Ordering::Relaxed), 12);
    }
}
