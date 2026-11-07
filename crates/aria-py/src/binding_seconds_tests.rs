//! WHY: closes the class of a script's seconds read against the wrong origin.
//! Every negative number of seconds was "before the start of the timeline",
//! so `c.fade_in = -1` and a keyframe at `-1` into a clip both read as a
//! timeline position, and a fade too long for its clip was stated in ticks.
//! Setting `c.volume` also cleared both fades, because the setter passed zero
//! fades to a model edit that writes the fades it is given.
//!
//! Each row is a script that fails, with the exact error it must raise: one row
//! per call site that converts seconds, so a site that passes the wrong
//! measure changes its message. Not caught: a call site added later with no
//! row here.

use std::ffi::CString;
use std::path::Path;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// A project with a two-second still `c` on a video track, its media `m`, and
/// the track `v`.
fn setup() -> String {
    let texture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    format!(
        "p = aria.Project('seconds', fps=30)\n\
         m = p.import_media({texture:?})\n\
         v = p.add_track('video')\n\
         c = v.add(m, 0.0, 2.0)\n"
    )
}

/// Runs `setup()` then `code` and returns what the script saw: `Ok` with the
/// value of `result`, or the error as `ValueError: …` or `AriaError: …`.
fn run(code: &str) -> Result<String, String> {
    Python::attach(|py| {
        let globals = PyDict::new(py);
        globals
            .set_item("aria", pyo3::wrap_pymodule!(crate::aria)(py))
            .expect("binding a module into a fresh dict cannot fail");
        let script = CString::new(format!("{}{code}\n", setup())).expect("no NUL in the script");
        match py.run(&script, Some(&globals), None) {
            Ok(()) => Ok(globals
                .get_item("result")
                .expect("reading a dict cannot fail")
                .map_or_else(String::new, |value| value.to_string())),
            Err(err) => {
                let kind = if err.is_instance_of::<PyValueError>(py) {
                    "ValueError"
                } else {
                    "AriaError"
                };
                Err(format!("{kind}: {}", err.value(py)))
            }
        }
    })
}

#[test]
fn a_negative_time_is_stated_against_its_own_origin() {
    let rows = [
        (
            "c.fade_in = -1.0",
            "ValueError: fade_in must be >= 0, got -1",
        ),
        (
            "c.fade_out = -1.0",
            "ValueError: fade_out must be >= 0, got -1",
        ),
        (
            "c.transition(aria.transitions()[0]['id'], -1.0)",
            "ValueError: duration must be >= 0, got -1",
        ),
        (
            "c.animate(opacity=[(-1.0, 0.0), (0.5, 1.0)])",
            "ValueError: -1 s is before the start of the clip",
        ),
        (
            "v.add(m.subclip(-1.0, 1.0), 3.0)",
            "ValueError: -1 s is before the start of the media",
        ),
        (
            "c.set_media(m, start=-1.0)",
            "ValueError: -1 s is before the start of the media",
        ),
        (
            "c.move(-1.0)",
            "ValueError: -1 s is before the start of the timeline",
        ),
        (
            "c.fade_in = 5.0",
            "AriaError: invalid parameter: fade_in (5.000 s) is longer than the clip (2.000 s)",
        ),
    ];
    for (code, expected) in rows {
        assert_eq!(run(code), Err(expected.to_owned()), "{code}");
    }
}

#[test]
fn setting_the_volume_keeps_both_fades() {
    assert_eq!(
        run("c.fade_in = 0.5\n\
             c.fade_out = 1.0\n\
             c.volume = 0.5\n\
             result = (c.fade_in, c.fade_out, c.volume)"),
        Ok("(0.5, 1.0, 0.5)".to_owned())
    );
}
