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

/// A trim whose end falls at or before its start states both ends: the
/// seconds the script wrote, and the clip's own end or start where it wrote
/// only one. It read "end must be greater than start" when the script wrote
/// only a start past the clip's end.
#[test]
fn a_reversed_trim_states_both_ends() {
    let rows = [
        (
            "c.trim(start=5.0)",
            "the trim ends at 2 s, at or before its start at 5 s",
        ),
        (
            "c.trim(end=-1.0)",
            "the trim ends at -1 s, at or before its start at 0 s",
        ),
        (
            "c.trim(start=1.5, end=0.5)",
            "the trim ends at 0.5 s, at or before its start at 1.5 s",
        ),
        (
            "c.trim(start=1.0, end=1.0)",
            "the trim ends at 1 s, at or before its start at 1 s",
        ),
    ];
    for (code, expected) in rows {
        assert_eq!(run(code), Err(format!("ValueError: {expected}")), "{code}");
    }
    assert_eq!(
        run("c.trim(start=0.5, end=1.5)
result = (c.start, c.duration)"),
        Ok("(0.5, 1.0)".to_owned())
    );
}

/// A window that holds no frame is stated in the seconds the script wrote,
/// reversed apart from too short; both read `invalid source window` before.
/// A reversed window or a bound that is no time into the media fails where
/// the window is made, by `subclip` or by slicing, in one wording.
#[test]
fn an_empty_media_window_states_its_seconds() {
    let reversed = "ValueError: the media window ends at 0.5 s, before its start at 1.5 s";
    let rows = [
        ("v.add(m.subclip(1.5, 0.5), 3.0)", Err(reversed)),
        ("w = m.subclip(1.5, 0.5)", Err(reversed)),
        ("w = m[1.5:0.5]", Err(reversed)),
        (
            "w = m.subclip(float('nan'))",
            Err("ValueError: NaN is not a finite number of seconds"),
        ),
        (
            "w = m.subclip(0.5, float('inf'))",
            Err("ValueError: inf is not a finite number of seconds"),
        ),
        (
            "w = m.subclip(-1.0)",
            Err("ValueError: -1 s is before the start of the media"),
        ),
        (
            "result = m.subclip(1.5)",
            Ok("MediaSlice(1.500, end)".to_owned()),
        ),
        (
            "v.add(m.subclip(1.0, 1.0), 3.0)",
            Err("ValueError: the media window from 1 s to 1 s is shorter than one frame"),
        ),
        (
            "v.add(m.subclip(1.0, 1.0001), 3.0)",
            Err("ValueError: the media window from 1 s to 1.0001 s is shorter than one frame"),
        ),
        ("v.add(m.subclip(0.5, 1.5), 3.0)", Ok(String::new())),
    ];
    for (code, expected) in rows {
        assert_eq!(run(code), expected.map_err(str::to_owned), "{code}");
    }
}

/// WHY: a window past the end of its media read `source range is outside the
/// media bounds`, with no time and no file, and a start at or past the end
/// read `invalid source window`. Every way a script sets a window's end, a
/// `subclip` end, a slice end, and a `duration` on whole media or on a
/// `subclip`, states the end it asked for and where the media ends, as a
/// `MediaError`. Not caught: a trim past the end of the media, which the
/// model rejects without the times.
#[test]
fn a_window_past_the_media_states_where_the_media_ends() {
    let video = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4");
    let ends = |seconds: &str| {
        format!("the media window ends at {seconds} s, past the end of the media \"MEDIA\" at 3 s")
    };
    let starts = |seconds: &str| {
        format!("the media window starts at {seconds} s, and the media \"MEDIA\" ends at 3 s")
    };
    let rows = [
        ("a.add(n.subclip(1.0, 4.5), 0.0)", ends("4.5")),
        ("a.add(n[1.0:4.5], 0.0)", ends("4.5")),
        ("a.add(n, 0.0, 4.0)", ends("4")),
        ("a.add(n.subclip(2.0), 0.0, 1.5)", ends("3.5")),
        ("a.add(n.subclip(3.0), 0.0)", starts("3")),
        ("a.add(n.subclip(5.0), 0.0)", starts("5")),
        ("a.add(n.subclip(1.0, 3.0), 0.0)", "placed".to_owned()),
        ("a.add(n, 0.0, 3.0)", "placed".to_owned()),
    ];
    for (place, expected) in rows {
        let code = format!(
            "n = p.import_media({video:?})\n\
             a = p.add_track('video')\n\
             try:\n    {place}\n    result = 'placed'\n\
             except aria.MediaError as error:\n    result = str(error).replace(n.path, 'MEDIA')\n"
        );
        assert_eq!(run(&code), Ok(expected), "{place}");
    }
}
