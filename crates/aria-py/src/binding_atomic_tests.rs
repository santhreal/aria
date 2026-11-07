//! WHY: closes the class of a script call that fails after it has written part
//! of its edit. `animate` with two properties set the first and then failed on
//! the second, `add_effect` with a bad parameter kept the effect, and a
//! transition too long for its neighbor stayed at its default length, so a
//! script that caught the error went on with a project it never asked for.
//!
//! Every public method of every class that edits a project has rows below,
//! each a call that fails, and the project must save the same bytes after the
//! call as before it. The methods are read off the module at run time, so a
//! method or a class with no row is red until one is written.
//!
//! What this does not catch: a call that fails only on an input no row tries,
//! and a property setter, which writes one model step and is not listed.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::path::Path;

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Classes whose methods never edit a project, each with the reason.
const EXEMPT: &[(&str, &str)] = &[(
    "Template",
    "reads and writes its own document; `fill` builds a new project",
)];

/// Calls that fail, by the method they call. Several rows may name one method.
const FAILING: &[(&str, &str)] = &[
    (
        "Clip.add_effect",
        "c.add_effect('gaussian_blur', radius=8.0, bogus=1.0)",
    ),
    (
        "Clip.animate",
        "c.animate(scale=[(0.0, 1.0), (1.0, 2.0)], opacity=[(0.0, 0.0), (99.0, 1.0)])",
    ),
    (
        "Clip.clear_animation",
        "c.clear_animation('opacity', 'bogus')",
    ),
    ("Clip.crop", "c.crop(x=0.6, w=0.6)"),
    ("Clip.delete", "gone.delete()"),
    ("Clip.link_with", "c.link_with([c])"),
    ("Clip.move", "c.move(1.0)"),
    ("Clip.remove_effect", "c2.remove_effect(fx)"),
    (
        "Clip.remove_keyframe",
        "c.remove_keyframe('opacity', at=0.5)",
    ),
    ("Clip.remove_transition", "c2.remove_transition()"),
    ("Clip.ripple_delete", "gone.ripple_delete()"),
    ("Clip.set_animation", "c.set_animation('in', 'bogus')"),
    ("Clip.set_media", "c.set_media(m, start=-1.0)"),
    ("Clip.set_speed", "c.set_speed(0.0)"),
    (
        "Clip.set_speed_curve",
        "c.set_speed_curve([(0.0, 1.0), (1.0, 0.0)])",
    ),
    ("Clip.set_style", "s.set_style(color=(0, 255, 0), bogus=1)"),
    ("Clip.set_style", "x.set_style(size=40, bogus=1)"),
    ("Clip.split", "c.split(0.0)"),
    ("Clip.transform_at", "c.transform_at(float('nan'))"),
    ("Clip.transition", "c.transition('crossfade', 99.0)"),
    ("Clip.trim", "c.trim(1.0, 0.5)"),
    ("Clip.unlink", "c.unlink()"),
    (
        "Effect.animate",
        "fx.animate(radius=[(0.0, 1.0), (1.0, 2.0)], bogus=[(0.0, 1.0)])",
    ),
    ("Marker.delete", "gone_marker.delete()"),
    ("Media.subclip", "t.add(m.subclip(float('nan')), 10.0)"),
    ("Project.add_marker", "p.add_marker(float('nan'))"),
    ("Project.add_track", "p.add_track('bogus')"),
    ("Project.export", "p.export('/nonexistent-dir/x.mp4')"),
    ("Project.get_frame", "p.get_frame(-1.0)"),
    ("Project.import_media", "p.import_media('/nonexistent.png')"),
    ("Project.load", "aria.Project.load('/nonexistent.aria')"),
    ("Project.load_font", "p.load_font('/nonexistent.ttf')"),
    ("Project.remove_media", "p.remove_media(m)"),
    ("Project.save", "p.save('/nonexistent-dir/x.aria')"),
    (
        "Project.save_template",
        "p.save_template('/nonexistent-dir/x.ariatemplate', 'x')",
    ),
    ("Project.track", "p.track('nope')"),
    ("Track.add", "t.add(m, 1.0)"),
    ("Track.append", "t.append(aria.Solid((0, 0, 0)))"),
    ("Track.clip_at", "t.clip_at(float('nan'))"),
    ("Track.remove", "gone_track.remove()"),
    ("Track.add", "gone_track.add(m, 0.0, 1.0)"),
];

/// The project every row starts from: a still clip with an opacity ramp and
/// a blur beside a second still, a shape, a text, a marker, and one handle of
/// each kind whose target is already gone.
fn setup() -> String {
    let texture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    format!(
        "p = aria.Project('atomic', fps=30)\n\
         m = p.import_media({texture:?})\n\
         t = p.add_track('video')\n\
         c = t.add(m, 0.0, 2.0)\n\
         c2 = t.add(m, 2.0, 2.0)\n\
         c.animate(opacity=[(0.0, 0.0), (1.0, 1.0)])\n\
         fx = c.add_effect('gaussian_blur')\n\
         s = p.add_track('sticker').add(aria.Rect(width=100, height=100), 0.0, 2.0)\n\
         x = p.add_track('text').add(aria.Text('hello'), 0.0, 2.0)\n\
         p.add_marker(1.0, name='one')\n\
         gone = t.add(m, 6.0, 1.0)\n\
         gone.delete()\n\
         gone_marker = p.add_marker(3.0)\n\
         gone_marker.delete()\n\
         gone_track = p.add_track('video')\n\
         gone_track.remove()\n",
        texture = texture.display().to_string(),
    )
}

/// Run `code` against a fresh module and globals.
fn run<'py>(py: Python<'py>, code: &str) -> (Bound<'py, PyDict>, PyResult<()>) {
    let globals = PyDict::new(py);
    globals
        .set_item("aria", pyo3::wrap_pymodule!(crate::aria)(py))
        .expect("binding a module into a fresh dict cannot fail");
    let code = CString::new(code).expect("test source holds no NUL");
    let outcome = py.run(&code, Some(&globals), None);
    (globals, outcome)
}

/// The project `p` as it saves.
fn saved(globals: &Bound<'_, PyDict>) -> String {
    let project = globals
        .get_item("p")
        .expect("reading a dict cannot fail")
        .expect("the setup binds p");
    let project = project
        .extract::<PyRef<crate::project::Project>>()
        .expect("p is a project");
    project.model().save_to_string().expect("the project saves")
}

/// Every public method of every class the module exports, as `Class.method`.
fn methods(py: Python<'_>) -> BTreeSet<String> {
    let (globals, outcome) = run(
        py,
        "names = {f'{n}.{a}' for n, v in vars(aria).items()\n\
         \x20   if isinstance(v, type) and not issubclass(v, BaseException)\n\
         \x20   for a in dir(v) if not a.startswith('_') and callable(getattr(v, a))}\n",
    );
    outcome.expect("listing the module cannot fail");
    globals
        .get_item("names")
        .expect("reading a dict cannot fail")
        .expect("the listing binds names")
        .extract()
        .expect("names are strings")
}

#[test]
fn every_method_has_a_failing_call() {
    Python::attach(|py| {
        let listed: BTreeSet<&str> = FAILING.iter().map(|(method, _)| *method).collect();
        let exempt = |method: &str| {
            EXEMPT
                .iter()
                .any(|(class, _)| method.split('.').next() == Some(*class))
        };
        let found = methods(py);
        let missing: Vec<&String> = found
            .iter()
            .filter(|method| !exempt(method.as_str()) && !listed.contains(method.as_str()))
            .collect();
        let stale: Vec<&&str> = listed
            .iter()
            .filter(|method| !found.contains(**method))
            .collect();
        assert!(
            missing.is_empty() && stale.is_empty(),
            "methods with no failing call: {missing:?}; rows naming no method: {stale:?}"
        );
    });
}

#[test]
fn a_failed_call_leaves_the_project_as_it_was() {
    Python::attach(|py| {
        let setup = setup();
        let mut wrong = Vec::new();
        for &(method, call) in FAILING {
            let (globals, outcome) = run(py, &setup);
            outcome.unwrap_or_else(|err| panic!("the setup failed: {err}"));
            let before = saved(&globals);
            let code = CString::new(call).expect("test source holds no NUL");
            match py.run(&code, Some(&globals), None) {
                Ok(()) => wrong.push(format!("{method}: `{call}` was accepted")),
                Err(err) if saved(&globals) != before => wrong.push(format!(
                    "{method}: `{call}` failed ({err}) and changed the project"
                )),
                Err(_) => {}
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    });
}
