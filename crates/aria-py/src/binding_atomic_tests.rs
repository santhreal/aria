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
//! The same rows hold every refusal to the words a script reads: no track,
//! clip or media id, since a script holds none of them.
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
    ("Clip.add_effect", "a.add_effect('gaussian_blur')"),
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
    ("Clip.move", "c.move(0.0, track=gone_track)"),
    ("Clip.move", "c.move(9.0, track=sticker)"),
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
    ("Clip.set_speed", "c.set_speed(0.5)"),
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
    ("Clip.trim", "c.trim(0.0, 3.0)"),
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
    ("Track.add", "sound.add(aria.Solid((0, 0, 0)), 5.0, 1.0)"),
    ("Track.append", "t.append(aria.Solid((0, 0, 0)))"),
    ("Track.clip_at", "t.clip_at(float('nan'))"),
    ("Track.remove", "gone_track.remove()"),
    ("Track.add", "gone_track.add(m, 0.0, 1.0)"),
];

/// The project every row starts from: a still clip with an opacity ramp and
/// a blur beside a second still, a shape, a text, a tone, a marker, and one
/// handle of each kind whose target is already gone.
fn setup() -> String {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let texture = fixtures.join("texture.png");
    let tone = fixtures.join("fixture-sine-48k-2s.wav");
    format!(
        "p = aria.Project('atomic', fps=30)\n\
         m = p.import_media({texture:?})\n\
         sound = p.add_track('audio')\n\
         a = sound.add(p.import_media({tone:?}), 0.0, 1.0)\n\
         t = p.add_track('video')\n\
         c = t.add(m, 0.0, 2.0)\n\
         c2 = t.add(m, 2.0, 2.0)\n\
         c.animate(opacity=[(0.0, 0.0), (1.0, 1.0)])\n\
         fx = c.add_effect('gaussian_blur')\n\
         sticker = p.add_track('sticker')\n\
         s = sticker.add(aria.Rect(width=100, height=100), 0.0, 2.0)\n\
         x = p.add_track('text').add(aria.Text('hello'), 0.0, 2.0)\n\
         p.add_marker(1.0, name='one')\n\
         gone = t.add(m, 6.0, 1.0)\n\
         gone.delete()\n\
         gone_marker = p.add_marker(3.0)\n\
         gone_marker.delete()\n\
         gone_track = p.add_track('video')\n\
         gone_track.remove()\n",
        texture = texture.display().to_string(),
        tone = tone.display().to_string(),
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

/// WHY: closes the class of a script refusal that states a track, clip or
/// media by its internal id, as `clip overlaps an existing clip on track
/// #1382` did: a script holds no id, so it cannot tell which of its tracks the
/// number means. Every failing call above is held to it, so a new method's row
/// is too. Not caught: a refusal no row provokes.
#[test]
fn a_refusal_states_no_internal_id() {
    Python::attach(|py| {
        let setup = setup();
        let mut wrong = Vec::new();
        for &(method, call) in FAILING {
            let (globals, outcome) = run(py, &setup);
            outcome.unwrap_or_else(|err| panic!("the setup failed: {err}"));
            let code = CString::new(call).expect("test source holds no NUL");
            if let Err(err) = py.run(&code, Some(&globals), None) {
                let message = err.to_string();
                let states_an_id = message
                    .as_bytes()
                    .windows(2)
                    .any(|pair| pair[0] == b'#' && pair[1].is_ascii_digit());
                if states_an_id {
                    wrong.push(format!("{method}: `{call}` reported {message:?}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    });
}

/// WHY: the track a refusal states must be the track the call addressed: its
/// kind as `add_track` spells it, its name, and an index into `Project.tracks`
/// that yields it; a media must read as the file `Media.path` reports. Not
/// caught: a refusal about a track no call below addresses.
#[test]
fn a_refusal_states_the_track_as_the_script_reaches_it() {
    let texture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    let code = format!(
        "p = aria.Project('words', fps=30)\n\
         m = p.import_media({texture:?})\n\
         broll = p.add_track('video', name='B-roll')\n\
         broll.add(m, 0.0, 2.0)\n\
         title = p.add_track('text')\n\
         words = []\n\
         for call in (lambda: broll.add(m, 1.0, 1.0), lambda: title.add(m, 0.0, 1.0), lambda: p.remove_media(m)):\n\
         \x20   try:\n\
         \x20       call()\n\
         \x20   except aria.AriaError as err:\n\
         \x20       words.append(f'{{type(err).__name__}}: {{err}}')\n\
         at = [(t.kind, t.name) for t in p.tracks]\n\
         path = m.path\n",
        texture = texture.display().to_string(),
    );
    Python::attach(|py| {
        let (globals, outcome) = run(py, &code);
        outcome.unwrap_or_else(|err| panic!("the script failed: {err}"));
        let read = |name: &str| {
            globals
                .get_item(name)
                .expect("reading a dict cannot fail")
                .unwrap_or_else(|| panic!("the script binds {name}"))
        };
        let words: Vec<String> = read("words").extract().expect("words are strings");
        let at: Vec<(String, String)> = read("at").extract().expect("tracks are pairs");
        let path: String = read("path").extract().expect("a path is a string");
        let index = |kind: &str, name: &str| {
            at.iter()
                .position(|(k, n)| k == kind && n == name)
                .unwrap_or_else(|| panic!("no {kind} track {name:?} in {at:?}"))
        };
        let (broll, title) = (index("video", "B-roll"), index("text", ""));
        assert_eq!(
            words,
            [
                format!(
                    "OverlapError: clip overlaps an existing clip on the video track \"B-roll\" at tracks[{broll}]"
                ),
                format!("TrackKindError: the text track at tracks[{title}] cannot hold this clip"),
                format!(
                    "MediaError: the media {path:?} is still used by one or more clips; delete them first"
                ),
            ]
        );
    });
}

/// WHY: a repr states a clip the way a script reaches it, as a refusal does:
/// `Clip(..., id=1382)` and `Effect(..., clip=1382, ...)` printed an internal
/// id no call takes. Not caught: a repr of a class with no row below.
#[test]
fn a_repr_states_the_track_as_the_script_reaches_it() {
    let code = format!(
        "{}shown = [repr(c), repr(fx), repr(a)]\nat = [t.kind for t in p.tracks]\n",
        setup()
    );
    Python::attach(|py| {
        let (globals, outcome) = run(py, &code);
        outcome.unwrap_or_else(|err| panic!("the script failed: {err}"));
        let read = |name: &str| {
            globals
                .get_item(name)
                .expect("reading a dict cannot fail")
                .unwrap_or_else(|| panic!("the script binds {name}"))
        };
        let shown: Vec<String> = read("shown").extract().expect("reprs are strings");
        let at: Vec<String> = read("at").extract().expect("kinds are strings");
        let index = |kind: &str| {
            at.iter()
                .position(|k| k == kind)
                .unwrap_or_else(|| panic!("no {kind} track in {at:?}"))
        };
        let (video, audio) = (index("video"), index("audio"));
        assert_eq!(
            shown,
            [
                format!("Clip(start=0.000s, duration=2.000s, track=tracks[{video}])"),
                format!(
                    "Effect(\"gaussian_blur\", index=0, clip=Clip(start=0.000s, duration=2.000s, track=tracks[{video}]))"
                ),
                format!("Clip(start=0.000s, duration=1.000s, track=tracks[{audio}])"),
            ]
        );
    });
}
