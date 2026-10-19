//! A test's window, and everything drawn in it, ends with the test.
//!
//! The class: a test window whose context gpui parks in
//! `TestAppContext::on_quit`, which the `#[gpui::test]` teardown never drains.
//! Every such window kept its `App` alive until the test binary exited, and a
//! drawn shell kept its engine, GPU device and worker threads with it. The
//! first rule below holds the release itself to a run of the real teardown;
//! the second holds every test window in the crate, the `shell` integration
//! test included, to the one function that arms it.
//!
//! Not covered: a test that builds its own `TestAppContext` outside
//! `#[gpui::test]`, and `gpui::TestApp`, which shuts its app down on its own.

use std::path::Path;

use super::collect_rs;

/// **A drawn shell and its app are released when the test ends.**
///
/// Runs a test through `gpui::run_test` with the teardown `#[gpui::test]`
/// generates for a `&mut TestAppContext` argument, step for step, then reads
/// two weak handles taken during the test: one to the test's `App`, and one
/// to the export queue only the shell's `AppState` holds. Without the release
/// both stayed alive, so this fails on the leak itself rather than on the
/// memory it cost.
///
/// Not covered: the guard's panicking branch, which skips the quit so a failed
/// test unwinds once. A test cannot fail inside this harness and then read its
/// own teardown.
#[test]
fn a_drawn_shell_and_its_app_are_released_when_the_test_ends() {
    gpui::run_test(
        1,
        &[],
        0,
        &mut |dispatcher, _seed| {
            let mut cx = gpui::TestAppContext::build(
                dispatcher.clone(),
                Some("a_drawn_shell_and_its_app_are_released_when_the_test_ends"),
            );
            let _entity_refcounts = cx.app.borrow().ref_counts_drop_handle();
            let app = std::rc::Rc::downgrade(&cx.app);
            let queue = {
                let (state, visual) = super::drawn_shell(&mut cx, "window-teardown");
                visual.read(|app| std::sync::Arc::downgrade(&state.read(app).export_queue))
            };
            assert!(
                queue.upgrade().is_some() && app.upgrade().is_some(),
                "the witnesses died before the teardown, so this test proves nothing"
            );

            // The `#[gpui::test]` teardown, in its order.
            cx.run_until_parked();
            cx.update(|cx| {
                cx.background_executor().forbid_parking();
                cx.quit();
            });
            cx.run_until_parked();
            drop(cx);
            dispatcher.drain_tasks();

            assert!(
                queue.upgrade().is_none(),
                "the shell's AppState outlived its test, holding its engine, GPU device \
                 and worker threads until the test binary exits"
            );
            assert!(
                app.upgrade().is_none(),
                "the test's App outlived its test: a window context is still parked in \
                 TestAppContext::on_quit"
            );
        },
        None,
    );
}

/// **Every test window view in the crate is opened through `window_view`.**
///
/// `add_window_view` and `add_empty_window` park their context in
/// `TestAppContext::on_quit`, and so does `VisualTestContext::into_mut`. Only
/// `test_support::window` pairs that with the teardown release, so it is the
/// one file allowed to make those calls. Read from the sources, crate-root test
/// targets included, and fails by default: a new test that opens a window any
/// other way turns this red on the run that adds it.
#[test]
fn every_test_window_view_is_opened_through_window_view() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs(&root.join("src"), &mut files);
    // Crate-root files are the `[[test]]` and `[[bench]]` targets wired through
    // `path` in `Cargo.toml`.
    for entry in std::fs::read_dir(root)
        .unwrap_or_else(|err| panic!("read {root:?}: {err}"))
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    assert!(
        files.len() > 200,
        "found {} files under {root:?}, which cannot be this crate: the rule just went vacuous",
        files.len()
    );
    assert!(
        files.iter().any(|file| file.ends_with("tests_shell.rs")),
        "the shell integration test is not among the scanned files"
    );

    // Built at run time so this file's own source does not match.
    let parking: Vec<String> = ["add_window_view", "add_empty_window"]
        .iter()
        .map(|name| format!(".{name}("))
        .collect();
    let into_mut = format!(".{}()", "into_mut");
    let owner = root.join("src/test_support/window.rs");
    let mut owned = None;
    let mut strays = Vec::new();
    for file in &files {
        let source =
            std::fs::read_to_string(file).unwrap_or_else(|err| panic!("read {file:?}: {err}"));
        // Comments dropped and whitespace squashed, so a call a formatter
        // splits over lines is still the same call.
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .flat_map(str::chars)
            .filter(|ch| !ch.is_whitespace())
            .collect();
        let mut calls: usize = parking.iter().map(|call| code.matches(call).count()).sum();
        if code.contains("VisualTestContext") {
            calls += code.matches(&into_mut).count();
        }
        if *file == owner {
            owned = Some(calls);
        } else if calls > 0 {
            let shown = file.strip_prefix(root).unwrap_or(file);
            strays.push(format!("{}: {calls}", shown.display()));
        }
    }
    assert_eq!(
        owned,
        Some(1),
        "test_support/window.rs must open the one window view the crate's tests use"
    );
    assert!(
        strays.is_empty(),
        "these files open a test window whose context is never released; open it \
         through test_support::window_view instead: {strays:?}"
    );
}
