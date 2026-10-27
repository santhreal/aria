//! The session held on another thread, as a preview decode holds it.
//!
//! The preview worker holds the engine for the length of every frame it
//! decodes. A test that runs some work with the engine held this way finds out
//! whether the work reads the session: work that does waits for the holder,
//! whose own wait runs out first, so the test ends either way.

use std::sync::{Arc, mpsc};
use std::time::Duration;

use aria_engine::Engine;
use parking_lot::Mutex;

/// How long the holder keeps the session before it lets go on its own.
const HOLD_BOUND: Duration = Duration::from_secs(5);

/// Run `work` with `engine` held on another thread. `true` when `work` waited
/// for the session: the holder's wait of [`HOLD_BOUND`] ran out before `work`
/// finished.
pub(crate) fn while_held(engine: &Arc<Mutex<Engine>>, work: impl FnOnce()) -> bool {
    let engine = Arc::clone(engine);
    let (taken, is_taken) = mpsc::channel::<()>();
    let (release, held) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _session = engine.lock();
        taken.send(()).expect("the test waits for the hold");
        held.recv_timeout(HOLD_BOUND).is_err()
    });
    is_taken
        .recv_timeout(HOLD_BOUND)
        .expect("the session is held");
    work();
    // A holder whose wait ran out has gone, and the send finds no one.
    let _ = release.send(());
    holder.join().expect("the holder")
}
