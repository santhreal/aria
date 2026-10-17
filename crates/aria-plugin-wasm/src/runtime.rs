//! Shared `wasmtime::Engine`, epoch ticker, and trap classification.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use wasmtime::{Config, Engine, Trap};

/// Resolution of the wall-clock epoch ticker while at least one WASM call runs.
pub(crate) const EPOCH_TICK: Duration = Duration::from_millis(1);

struct TickerState {
    active_calls: AtomicUsize,
    lock: Mutex<()>,
    cv: Condvar,
}

pub(crate) struct SharedEngine {
    engine: Engine,
    ticker: Arc<TickerState>,
}

pub(crate) struct CallGuard {
    ticker: Arc<TickerState>,
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.ticker.active_calls.fetch_sub(1, Ordering::AcqRel);
    }
}

static ENGINE: LazyLock<Result<SharedEngine, String>> = LazyLock::new(SharedEngine::init);

impl SharedEngine {
    fn init() -> Result<Self, String> {
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

        Ok(Self { engine, ticker })
    }

    pub(crate) fn get() -> Result<&'static Self, String> {
        ENGINE.as_ref().map_err(Clone::clone)
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
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
