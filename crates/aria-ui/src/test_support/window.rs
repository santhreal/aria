//! The one way a test opens a window view, and the teardown that releases it.
//!
//! Compiled twice: into the library's tests as `crate::test_support::window`,
//! and into the `shell` integration test through `#[path]`. It therefore refers
//! to nothing in `aria_ui`.

/// A window filling the test screen over the view `build` returns, released
/// with everything in it when the test ends.
///
/// Every test window view in the crate is opened here; a census in
/// `test_support/window_teardown.rs` fails on any other call to
/// [`gpui::TestAppContext::add_window_view`].
pub(crate) fn window_view<V, F>(
    cx: &mut gpui::TestAppContext,
    build: F,
) -> (gpui::Entity<V>, &mut gpui::VisualTestContext)
where
    F: FnOnce(&mut gpui::Window, &mut gpui::Context<V>) -> V,
    V: 'static + gpui::Render,
{
    release_at_teardown(cx);
    cx.add_window_view(build)
}

/// Shuts the test's [`gpui::App`] down when the `#[gpui::test]` teardown drains
/// the dispatcher, once per app however many windows the test opens.
///
/// # Why
///
/// [`gpui::TestAppContext::add_window_view`] turns its [`gpui::VisualTestContext`]
/// into a `&'static mut` with `Box::into_raw` and registers the matching
/// `Box::from_raw` in `TestAppContext::on_quit`. The `#[gpui::test]` teardown
/// calls `App::quit`, which on the test platform does nothing, and never
/// `TestAppContext::quit`, so `on_quit` never runs and `App::shutdown` never
/// runs. Each window then kept its `App`, and a drawn shell its `AppState`,
/// engine, renderer and GPU device, preview worker thread and event bus thread,
/// until the test binary exited: the library suite at four test threads peaked
/// at 15338 MiB and 412 threads.
///
/// The guard lives in a timer task on the foreground executor. The teardown's
/// `dispatcher.drain_tasks()` drops that task after the test body and the
/// context teardowns, and the drop runs `TestAppContext::quit`. A test that is
/// already panicking skips the quit, so a failure is reported as itself rather
/// than as a second panic during unwinding.
pub(crate) fn release_at_teardown(cx: &gpui::TestAppContext) {
    let first = cx.update(|app| {
        if app.has_global::<ReleaseArmed>() {
            return false;
        }
        app.set_global(ReleaseArmed);
        true
    });
    if !first {
        return;
    }
    let guard = QuitOnDrain(cx.clone());
    cx.foreground_executor()
        .spawn(async move {
            loop {
                guard
                    .0
                    .executor()
                    .timer(std::time::Duration::from_secs(3600))
                    .await;
            }
        })
        .detach();
}

/// Marks an app whose teardown release is already armed.
struct ReleaseArmed;

impl gpui::Global for ReleaseArmed {}

struct QuitOnDrain(gpui::TestAppContext);

impl Drop for QuitOnDrain {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.0.quit();
        }
    }
}
