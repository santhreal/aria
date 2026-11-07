//! Shared `wasmtime::Engine`, epoch ticker, and trap classification.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Trap};

/// Resolution of the wall-clock epoch ticker while at least one WASM call runs.
pub(crate) const EPOCH_TICK: Duration = Duration::from_millis(1);

/// Distinct compiled components [`SharedEngine::compile`] keeps for reuse.
pub(crate) const COMPILED_KEPT: usize = 16;

struct TickerState {
    active_calls: AtomicUsize,
    lock: Mutex<()>,
    cv: Condvar,
}

pub(crate) struct SharedEngine {
    engine: Engine,
    ticker: Arc<TickerState>,
    /// Each kept component with the bytes it was compiled from, least
    /// recently used first.
    compiled: Mutex<Vec<(Box<[u8]>, Component)>>,
    /// How many components `compiled` holds at most.
    kept: usize,
}

pub(crate) struct CallGuard {
    ticker: Arc<TickerState>,
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.ticker.active_calls.fetch_sub(1, Ordering::AcqRel);
    }
}

static ENGINE: LazyLock<Result<SharedEngine, String>> =
    LazyLock::new(|| SharedEngine::init(COMPILED_KEPT));

impl SharedEngine {
    /// The engine, its epoch ticker, and a memo of at most `kept` compiled
    /// components.
    fn init(kept: usize) -> Result<Self, String> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        config.epoch_interruption(true);

        let engine = Engine::new(&config).map_err(|err| err.to_string())?;
        let ticker = Arc::new(TickerState {
            active_calls: AtomicUsize::new(0),
            lock: Mutex::new(()),
            cv: Condvar::new(),
        });

        let thread_engine = engine.clone();
        let thread_ticker = Arc::clone(&ticker);
        std::thread::Builder::new()
            .name("aria-wasm-epoch".to_owned())
            .spawn(move || epoch_loop(&thread_engine, &thread_ticker))
            .map_err(|err| format!("failed to spawn epoch ticker thread: {err}"))?;

        Ok(Self {
            engine,
            ticker,
            compiled: Mutex::new(Vec::with_capacity(kept)),
            kept,
        })
    }

    pub(crate) fn get() -> Result<&'static Self, String> {
        ENGINE.as_ref().map_err(Clone::clone)
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Compile `bytes` into a component, or return the component compiled
    /// from the same bytes before.
    ///
    /// A component's code depends on its bytes and the engine's configuration
    /// alone, and the engine is configured once, so the same bytes compile to
    /// the same code. A remount after a limits change, an enable after a
    /// disable, and a reload whose component file is unchanged reuse it. The
    /// [`COMPILED_KEPT`] most recently used components are kept; one past that
    /// compiles again.
    pub(crate) fn compile(&self, bytes: &[u8]) -> wasmtime::Result<Component> {
        {
            let mut compiled = self.compiled.lock();
            if let Some(at) = compiled.iter().position(|(kept, _)| **kept == *bytes) {
                let entry = compiled.remove(at);
                let component = entry.1.clone();
                compiled.push(entry);
                return Ok(component);
            }
        }
        // Compiled outside the lock, so packages mounting on other threads
        // compile at once.
        let component = Component::from_binary(&self.engine, bytes)?;
        let mut compiled = self.compiled.lock();
        if !compiled.iter().any(|(kept, _)| **kept == *bytes) {
            if compiled.len() == self.kept {
                compiled.remove(0);
            }
            compiled.push((bytes.into(), component.clone()));
        }
        Ok(component)
    }

    pub(crate) fn enter_call(&self) -> CallGuard {
        let prev = self.ticker.active_calls.fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            let _lock = self.ticker.lock.lock();
            self.ticker.cv.notify_all();
        }
        CallGuard {
            ticker: Arc::clone(&self.ticker),
        }
    }
}

fn epoch_loop(engine: &Engine, ticker: &TickerState) {
    loop {
        if ticker.active_calls.load(Ordering::Acquire) == 0 {
            let mut lock = ticker.lock.lock();
            while ticker.active_calls.load(Ordering::Acquire) == 0 {
                ticker.cv.wait(&mut lock);
            }
        }
        std::thread::sleep(EPOCH_TICK);
        if ticker.active_calls.load(Ordering::Acquire) > 0 {
            engine.increment_epoch();
        }
    }
}

/// Convert a wall-clock call deadline into epoch ticks of [`EPOCH_TICK`].
pub(crate) fn deadline_ticks(deadline: Duration) -> u64 {
    let ms = deadline.as_millis();
    let ticks = u64::try_from(ms).unwrap_or(u64::MAX);
    // Add 1 tick of slack so a call starting mid-tick gets at least `deadline`.
    ticks.saturating_add(1).max(2)
}

/// Format a `wasmtime::Error` from a trapped guest call with its specific fault
/// kind, followed by the guest's wasm backtrace when the call recorded one.
pub(crate) fn classify_trap(err: &wasmtime::Error) -> String {
    let Some(trap) = err.downcast_ref::<Trap>() else {
        return format!("wasm trap: {err:?}");
    };
    let kind = match *trap {
        Trap::OutOfFuel => "fuel exhausted".to_owned(),
        Trap::Interrupt => "call deadline exceeded".to_owned(),
        Trap::UnreachableCodeReached => {
            "unreachable code reached (guest panic or out of memory)".to_owned()
        }
        Trap::MemoryOutOfBounds => "memory out of bounds".to_owned(),
        Trap::StackOverflow => "stack overflow".to_owned(),
        other => format!("{other}"),
    };
    match err.downcast_ref::<wasmtime::WasmBacktrace>() {
        Some(trace) => format!("wasm trap: {kind}\n{trace}"),
        None => format!("wasm trap: {kind}"),
    }
}

/// The compiled-component memo behind [`SharedEngine::compile`].
///
/// Closes "a remount compiles a component it compiled before": a limits
/// change, an enable and a manifest-only reload each sent the same bytes
/// through Cranelift again. Holds that the same bytes return the one compile,
/// that other bytes compile anew, that the memo keeps no more than its bound
/// and drops the least recently used first, and that bytes that fail to
/// compile are not kept. Not caught: a compile kept across processes, which
/// this memo does not do.
#[cfg(test)]
mod tests {
    use super::SharedEngine;
    use wasmtime::component::Component;

    /// A component holding one core module with one empty function, and a
    /// custom section named `name`, so each name is another component.
    fn component(name: &str) -> Vec<u8> {
        const MODULE: [u8; 24] = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // core module header
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: one () -> ()
            0x03, 0x02, 0x01, 0x00, // function section: one of type 0
            0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // code section: an empty body
        ];
        assert!(name.len() < 64, "a one-byte section length");
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        bytes.extend([0x01, MODULE.len() as u8]);
        bytes.extend(MODULE);
        bytes.extend([0x00, 1 + name.len() as u8, name.len() as u8]);
        bytes.extend(name.as_bytes());
        bytes
    }

    fn engine(kept: usize) -> SharedEngine {
        SharedEngine::init(kept).expect("the engine starts")
    }

    fn compiled(engine: &SharedEngine, name: &str) -> Component {
        engine
            .compile(&component(name))
            .unwrap_or_else(|err| panic!("{name} compiles: {err}"))
    }

    /// Two components are one compile when their code is at one address.
    fn same(a: &Component, b: &Component) -> bool {
        a.image_range() == b.image_range()
    }

    #[test]
    fn the_same_bytes_compile_once_and_other_bytes_anew() {
        let engine = engine(4);
        let first = compiled(&engine, "a");
        assert!(!first.image_range().is_empty(), "the component holds code");
        assert!(
            same(&compiled(&engine, "a"), &first),
            "a was compiled again"
        );
        let other = compiled(&engine, "b");
        assert!(!same(&other, &first), "b was served a's code");
        assert!(same(&compiled(&engine, "a"), &first), "b displaced a");
        assert!(
            same(&compiled(&engine, "b"), &other),
            "b was compiled again"
        );
    }

    #[test]
    fn past_its_bound_the_memo_compiles_the_least_recently_used_again() {
        let engine = engine(2);
        let a = compiled(&engine, "a");
        let b = compiled(&engine, "b");
        // a is used again, so b is the least recently used when c arrives.
        assert!(same(&compiled(&engine, "a"), &a));
        let c = compiled(&engine, "c");
        assert_eq!(
            engine.compiled.lock().len(),
            2,
            "the memo grew past its bound"
        );
        assert!(same(&compiled(&engine, "a"), &a), "a was dropped before b");
        assert!(same(&compiled(&engine, "c"), &c), "c was dropped");
        assert!(
            !same(&compiled(&engine, "b"), &b),
            "b was kept past the bound"
        );
        assert_eq!(
            engine.compiled.lock().len(),
            2,
            "the memo grew past its bound"
        );
    }

    #[test]
    fn bytes_that_do_not_compile_are_not_kept() {
        let engine = engine(2);
        // Cut inside the last section, which states two bytes it no longer
        // has. Cutting the whole custom section off leaves a valid component.
        let mut broken = component("a");
        broken.truncate(broken.len() - 2);
        assert!(engine.compile(&broken).is_err(), "a cut component compiled");
        assert!(
            engine.compile(&broken).is_err(),
            "a cut component was served"
        );
        assert!(
            engine.compiled.lock().is_empty(),
            "a failed compile was kept"
        );
        let a = compiled(&engine, "a");
        assert!(same(&compiled(&engine, "a"), &a));
    }
}
