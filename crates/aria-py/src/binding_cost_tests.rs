//! What the binding costs a script, measured against a ceiling.
//!
//! The crate's cost on the path a script takes was unmeasured: nothing said
//! whether placing a clip or setting a transform from Python was a few
//! microseconds of conversion over the engine call or a hidden project copy per
//! statement. These cases run the real module, in the interpreter, on a project
//! large enough for a per-statement copy to show, and report the per-call cost.
//!
//! The ceilings are set for a tenfold regression, not for the numbers below: a
//! machine several times slower than the one they were recorded on still passes,
//! while a call that starts copying the project does not. The measured figures
//! are printed, so `cargo test -- --nocapture` reports them rather than only
//! asserting them.

use std::ffi::CString;
use std::time::Instant;

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Clips placed before the transform measurement, so a per-statement copy of
/// the project would cost 400 clips' worth of work per call.
const CLIPS: usize = 400;
/// Transform assignments measured. Enough that interpreter startup and the
/// module import are a rounding error on the per-call figure.
const ASSIGNMENTS: usize = 2_000;

/// Microseconds per call, above which the call is doing more than converting
/// arguments and applying one edit.
const CEILING_US: f64 = 200.0;

/// Runs `code` in a fresh set of globals with `aria` bound, the way a script
/// sees the module, and returns how long it took.
fn timed(globals: &Bound<'_, PyDict>, py: Python<'_>, code: &str) -> std::time::Duration {
    let code = CString::new(code).expect("test source holds no NUL");
    let start = Instant::now();
    py.run(code.as_c_str(), Some(globals), None)
        .unwrap_or_else(|err| {
            err.print(py);
            panic!("the script failed: {err}")
        });
    start.elapsed()
}

fn per_call_us(elapsed: std::time::Duration, calls: usize) -> f64 {
    elapsed.as_secs_f64() * 1e6 / calls as f64
}

#[test]
fn placing_a_clip_and_setting_a_transform_cost_the_edit_not_the_project() {
    Python::attach(|py| {
        let module = pyo3::wrap_pymodule!(crate::aria)(py);
        let globals = PyDict::new(py);
        globals
            .set_item("aria", module)
            .expect("binding a module into a fresh dict cannot fail");

        // Warm the interpreter's caches on the same statements, so the measured
        // run pays for the calls and not for the first lookup of each name.
        timed(
            &globals,
            py,
            "\
warm = aria.Project('warm', fps=30)
warm_track = warm.add_track('sticker')
for i in range(20):
    warm_track.add(aria.Solid((40, 80, 120)), i * 0.5, 0.4).position = (0.01, -0.01)
",
        );

        let place = timed(
            &globals,
            py,
            &format!(
                "\
project = aria.Project('cost', fps=30)
track = project.add_track('sticker')
clips = [track.add(aria.Solid((40, 80, 120)), i * 0.5, 0.4) for i in range({CLIPS})]
"
            ),
        );
        let place_us = per_call_us(place, CLIPS);

        let assign = timed(
            &globals,
            py,
            &format!(
                "\
clip = clips[0]
for i in range({ASSIGNMENTS}):
    offset = (i % 100) * 0.001
    clip.position = (offset, -offset)
"
            ),
        );
        let assign_us = per_call_us(assign, ASSIGNMENTS);

        println!(
            "aria-py: place a clip {place_us:.2} us/call ({CLIPS} clips), \
             set a transform {assign_us:.2} us/call ({ASSIGNMENTS} assignments \
             on a {CLIPS}-clip project)"
        );
        assert!(
            place_us < CEILING_US,
            "placing a clip costs {place_us:.2} us/call, over the {CEILING_US} us ceiling"
        );
        assert!(
            assign_us < CEILING_US,
            "setting a transform costs {assign_us:.2} us/call on a {CLIPS}-clip \
             project, over the {CEILING_US} us ceiling"
        );

        // The clips were placed, so the ceiling was measured against real work
        // and not against a refusal that returned early.
        let count = globals
            .get_item("clips")
            .expect("a bound name")
            .expect("clips")
            .len()
            .expect("a list has a length");
        assert_eq!(count, CLIPS);
    });
}

/// The cost of one call must not grow with the project: a script that places a
/// thousand clips and then adjusts one of them is the ordinary shape of a
/// generated edit, and a per-statement copy is what would make it quadratic.
#[test]
fn a_transform_costs_the_same_on_a_large_project_as_on_a_small_one() {
    Python::attach(|py| {
        let module = pyo3::wrap_pymodule!(crate::aria)(py);
        let globals = PyDict::new(py);
        globals
            .set_item("aria", module)
            .expect("binding a module into a fresh dict cannot fail");

        let build = |clips: usize| {
            format!(
                "\
project = aria.Project('scale', fps=30)
track = project.add_track('sticker')
clips = [track.add(aria.Solid((40, 80, 120)), i * 0.5, 0.4) for i in range({clips})]
clip = clips[0]
for i in range(200):
    clip.position = ((i % 100) * 0.001, 0.0)
"
            )
        };
        let measure = format!(
            "\
for i in range({ASSIGNMENTS}):
    offset = (i % 100) * 0.001
    clip.position = (offset, -offset)
"
        );

        timed(&globals, py, &build(1));
        let small = per_call_us(timed(&globals, py, &measure), ASSIGNMENTS);
        timed(&globals, py, &build(CLIPS));
        let large = per_call_us(timed(&globals, py, &measure), ASSIGNMENTS);

        println!(
            "aria-py: set a transform {small:.2} us/call on 1 clip, \
             {large:.2} us/call on {CLIPS} clips"
        );
        // A generous factor: the two differ by a clip lookup, and the box is
        // shared. A copy of the project would be three orders of magnitude.
        assert!(
            large < small * 10.0 + CEILING_US,
            "a transform costs {large:.2} us/call on {CLIPS} clips against \
             {small:.2} us/call on one, so the call is paying for the project"
        );
    });
}
