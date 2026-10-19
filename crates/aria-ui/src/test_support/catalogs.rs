//! The process-wide catalogs in the library's tests: a test that adds and
//! removes an entry runs in a process of its own, and the shared fixture fails
//! once an entry has left a catalog in the shared process.
//!
//! # Why
//!
//! The effect, transition, audio effect and clip type catalogs are process
//! wide, and the library's tests read them from many threads at once. A test
//! that registered an effect and dropped the registration removed that effect
//! while other tests were reading: the rack census staged an Add for every
//! effect the browse list offered, the registration dropped between the staging
//! and the press, the press was refused as an unknown effect, and the census
//! every operator surface test reads was poisoned with it. Any test that lists a
//! catalog and then acts on what it listed was exposed the same way.
//!
//! [`own_process`] runs such a test's body in a child process of the test
//! binary, alone. [`assert_catalogs_only_grew`] holds the rule for the shared
//! process: a catalog there only grows. The shared fixture
//! (`premiere_tests::with_media`) calls it, so a test added tomorrow that drops
//! a registration in the shared process turns every test that builds the
//! fixture after it red, with a message naming the catalog.
//!
//! Not caught: a removal after the last fixture the run builds, and a reader
//! that lists a catalog while a permanent entry is added (the overlay census
//! lists its clip type for the life of the binary). A permanent entry never
//! stops resolving, so nothing that lists it can press an id that is gone.

use std::process::Command;

use aria_models::Catalog;

/// Set on a child [`own_process`] starts, to the name of the test it runs.
const OWN_PROCESS: &str = "ARIA_UI_OWN_PROCESS";

/// Whether this process is a child [`own_process`] started.
fn in_own_process() -> bool {
    std::env::var_os(OWN_PROCESS).is_some()
}

/// Runs `body` in a child process of this test binary that runs the calling
/// test and nothing else, and fails with the child's output unless that test
/// ran there and passed.
///
/// For a test that registers a catalog entry and drops it: see the module
/// documentation. The test is named by the calling thread, which libtest names
/// after the test it runs.
pub(crate) fn own_process(body: impl FnOnce()) {
    if in_own_process() {
        body();
        return;
    }
    let thread = std::thread::current();
    let test = thread
        .name()
        .filter(|name| *name != "main")
        .expect("own_process runs on a test's thread, which libtest names after the test");
    alone(test);
}

/// Runs this test binary on `test` alone, as the child [`own_process`] starts,
/// and fails with the child's output unless `test` ran there and passed.
fn alone(test: &str) {
    let exe = std::env::current_exe().expect("the test binary has a path");
    let out = Command::new(exe)
        .args(["--exact", test, "--test-threads=1"])
        .env(OWN_PROCESS, test)
        .output()
        .expect("the test binary starts again as the child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{test} did not pass in its own process ({}):\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
}

/// Fails, naming every catalog at fault, when an entry has left a catalog or
/// changed in it since this process started.
///
/// Every registration, removal and change moves a catalog's revision by one,
/// so a catalog that only grew has a revision equal to the entries registered
/// on top of the shipped ones.
pub(crate) fn assert_catalogs_only_grew() {
    let at_fault: Vec<String> = [
        changed(
            "effect",
            aria_models::effect_catalog(),
            aria_models::shipped_effect_catalog().len(),
        ),
        changed(
            "transition",
            aria_models::transition_catalog(),
            aria_models::shipped_transition_catalog().len(),
        ),
        changed(
            "audio effect",
            aria_models::audio_effect_catalog(),
            aria_models::shipped_audio_effect_catalog().len(),
        ),
        // No clip type ships.
        changed("clip type", aria_models::clip_type_catalog(), 0),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(
        at_fault.is_empty(),
        "an entry left or changed in a catalog every library test reads: {}. A test that \
         drops a registration runs its body through test_support::catalogs::own_process",
        at_fault.join(", ")
    );
}

/// `name`, its revision and its registered entries, when `catalog` has moved by
/// anything but an addition.
fn changed<T>(name: &str, catalog: Catalog<T>, shipped: usize) -> Option<String> {
    let registered = catalog.len().saturating_sub(shipped);
    (catalog.revision() != registered as u64).then(|| {
        format!(
            "catalog `{name}` at revision {} with {registered} registered",
            catalog.revision()
        )
    })
}

/// The text a caught panic carries.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_default()
}

#[test]
fn a_body_run_in_its_own_process_changes_no_catalog_here() {
    const ID: &str = "test.ui.own_process";
    let before = aria_models::effect_catalog().revision();
    own_process(|| {
        let registered = aria_models::register_effect(aria_models::EffectSpec {
            id: ID.into(),
            label: "Own Process".into(),
            ..Default::default()
        })
        .expect("the effect registers");
        assert!(aria_models::effect_spec(ID).is_some());
        drop(registered);
    });
    if !in_own_process() {
        assert_eq!(
            aria_models::effect_catalog().revision(),
            before,
            "the body registered in the process every other test reads"
        );
    }
}

#[test]
fn a_body_that_fails_in_its_own_process_fails_its_test() {
    const MARK: &str = "the body failed on purpose";
    if in_own_process() {
        own_process(|| panic!("{MARK}"));
        return;
    }
    let failed = std::panic::catch_unwind(|| own_process(|| panic!("{MARK}")))
        .expect_err("a body that failed in its own process passed");
    let text = panic_text(failed.as_ref());
    assert!(text.contains(MARK), "{text}");
}

#[test]
fn a_child_that_runs_no_test_fails() {
    let failed = std::panic::catch_unwind(|| alone("test_support::catalogs::no_such_test"))
        .expect_err("a child that ran no test was taken for a pass");
    let text = panic_text(failed.as_ref());
    assert!(text.contains("0 passed"), "{text}");
}

/// Each catalog is checked: a change other than an addition is named for the
/// catalog it happened in and no other, and the shared fixture fails after it.
#[test]
fn every_catalog_change_but_an_addition_is_seen() {
    use aria_models::{AudioEffectSpec, ClipTypeSpec, EffectSpec, MarkerColor, TransitionSpec};

    own_process(|| {
        let seen = || {
            let failed = std::panic::catch_unwind(assert_catalogs_only_grew)
                .expect_err("a catalog that did more than grow passed");
            panic_text(failed.as_ref())
        };
        assert_catalogs_only_grew();
        let effect = aria_models::register_effect(EffectSpec {
            id: "test.ui.grew.effect".into(),
            label: "Grew Effect".into(),
            ..Default::default()
        })
        .expect("the effect registers")
        .0;
        let transition = aria_models::register_transition(TransitionSpec {
            id: "test.ui.grew.transition".into(),
            label: "Grew Transition".into(),
            ..Default::default()
        })
        .expect("the transition registers")
        .0;
        let audio = aria_models::register_audio_effect(AudioEffectSpec {
            id: "test.ui.grew.audio".into(),
            label: "Grew Audio".into(),
            category: String::new(),
            params: Vec::new(),
            latency: 0,
            sidechain: false,
        })
        .expect("the audio effect registers")
        .0;
        let clip_type = aria_models::register_clip_type(ClipTypeSpec::new(
            "test.ui.grew.clip_type",
            "Grew Clip Type",
            "color_overlay",
            MarkerColor::Blue,
        ))
        .expect("the clip type registers")
        .0;
        assert_catalogs_only_grew();

        aria_models::set_registered_effect_category("test.ui.grew.effect", "Blur")
            .expect("the category changes");
        let text = seen();
        assert!(text.contains("`effect`"), "{text}");
        assert!(!text.contains("`transition`"), "{text}");

        for (name, registration) in [
            ("transition", transition),
            ("audio effect", audio),
            ("clip type", clip_type),
        ] {
            drop(registration);
            let text = seen();
            assert!(text.contains(&format!("`{name}`")), "{name}: {text}");
        }

        let failed = std::panic::catch_unwind(crate::app::premiere_tests::with_clip)
            .map(drop)
            .expect_err("the shared fixture built a state after entries left the catalogs");
        let text = panic_text(failed.as_ref());
        assert!(text.contains("`effect`"), "{text}");
        drop(effect);
    });
}
