//! What a script can actually do with the bindings.
//!
//! The rest of this crate was covered only by `convert.rs`'s arithmetic, so
//! every defect in the surface a script touches — a descriptor that reaches the
//! wrong generator, a handle to a deleted track, an error that arrives as a
//! panic — was invisible to the suite. These tests drive the real module: they
//! build it exactly as `import aria` does, then run Python against it.
//!
//! They need an interpreter to link against, which the shipped extension module
//! does not have, so linking one is the crate's default feature and
//! `cargo test --all-targets` runs them with no flags. A wheel build is the
//! build that turns it off.

use std::ffi::CStr;

use aria_models::{Generator, Param, Shape};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::content::{Arrow, Ellipse, Heart, Line, Polygon, Rect, Star};

/// Run `f` with the `aria` module bound in a fresh set of globals.
///
/// The module is built by the same wrapper CPython calls, so a class missing
/// from `#[pymodule]` fails here the way it would fail in a script.
fn with_aria<R>(f: impl for<'py> FnOnce(Python<'py>, &Bound<'py, PyDict>) -> PyResult<R>) -> R {
    Python::attach(|py| {
        let module = pyo3::wrap_pymodule!(crate::aria)(py);
        let globals = PyDict::new(py);
        globals
            .set_item("aria", module)
            .expect("binding a module into a fresh dict cannot fail");
        f(py, &globals).unwrap_or_else(|err| {
            err.print(py);
            panic!("the script failed: {err}")
        })
    })
}

/// The error a script would see from `code`, which must not succeed.
fn refused(code: &CStr) -> String {
    with_aria(|py, globals| {
        let outcome = py.run(code, Some(globals), None);
        Ok(match outcome {
            Ok(()) => panic!("the script was accepted, so nothing refused it"),
            Err(err) => {
                let kind = if err.is_instance_of::<PyValueError>(py) {
                    "ValueError"
                } else {
                    "error"
                };
                format!("{kind}: {err}")
            }
        })
    })
}

/// A descriptor a script can build for `shape`, or the reason there is none.
///
/// Exhaustive on [`Shape`] on purpose: a shape added to the engine that no
/// descriptor reaches is a feature a script cannot ask for, and the only way to
/// notice is to stop compiling. Adding a variant fails this match, and the
/// choice recorded here — a descriptor or a reason — is the decision that
/// unblocks it.
fn descriptor_for(py: Python<'_>, shape: &Shape) -> PyResult<Option<Py<PyAny>>> {
    let stroke = None;
    let obj = match shape {
        Shape::Rectangle => Bound::new(
            py,
            Rect {
                width: 320.0,
                height: 180.0,
                rgba: [1, 2, 3, 4],
                corner_radius: 8.0,
                stroke,
            },
        )?
        .into_any(),
        Shape::Ellipse => Bound::new(
            py,
            Ellipse {
                width: 320.0,
                height: 180.0,
                rgba: [1, 2, 3, 4],
                corner_radius: 0.0,
                stroke,
            },
        )?
        .into_any(),
        Shape::Polygon { sides } => Bound::new(
            py,
            Polygon {
                sides: *sides,
                width: 320.0,
                height: 180.0,
                rgba: [1, 2, 3, 4],
                corner_radius: 0.0,
                stroke,
            },
        )?
        .into_any(),
        Shape::Star {
            points,
            inner_ratio,
        } => Bound::new(
            py,
            Star {
                points: *points,
                inner_ratio: match inner_ratio {
                    Param::Constant(value) => *value,
                    animated => panic!("an animated inner_ratio is not a descriptor: {animated:?}"),
                },
                width: 320.0,
                height: 180.0,
                rgba: [1, 2, 3, 4],
                stroke,
            },
        )?
        .into_any(),
        Shape::Line => Bound::new(
            py,
            Line {
                length: 320.0,
                thickness: 180.0,
                rgba: [1, 2, 3, 4],
                stroke,
            },
        )?
        .into_any(),
        Shape::Arrow => Bound::new(
            py,
            Arrow {
                width: 320.0,
                height: 180.0,
                rgba: [1, 2, 3, 4],
                corner_radius: 0.0,
                stroke,
            },
        )?
        .into_any(),
        Shape::Heart => Bound::new(
            py,
            Heart {
                width: 320.0,
                height: 180.0,
                rgba: [1, 2, 3, 4],
                corner_radius: 0.0,
                stroke,
            },
        )?
        .into_any(),
        // No descriptor, and that is the decision: a pen path is drawn with the
        // pen tool, point by point, and a script that wants one loads a project
        // that has it rather than typing coordinates into a constructor.
        Shape::Path(_) => return Ok(None),
    };
    Ok(Some(obj.unbind()))
}

/// Every shape the engine draws, with the geometry a descriptor carries.
fn every_shape() -> Vec<Shape> {
    vec![
        Shape::Rectangle,
        Shape::Ellipse,
        Shape::Polygon { sides: 5 },
        Shape::Star {
            points: 6,
            inner_ratio: Param::Constant(0.375),
        },
        Shape::Line,
        Shape::Arrow,
        Shape::Heart,
        Shape::Path(aria_models::ShapePath {
            points: Vec::new(),
            closed: true,
        }),
    ]
}

#[test]
fn every_shape_a_script_can_name_reaches_the_shape_it_named() {
    with_aria(|py, _| {
        for shape in every_shape() {
            let Some(descriptor) = descriptor_for(py, &shape)? else {
                continue;
            };
            let generator = crate::content::generator_from(descriptor.bind(py))?;
            match generator {
                Generator::Shape { shape: reached, .. } => assert_eq!(
                    reached, shape,
                    "the descriptor for {shape:?} reached {reached:?} instead"
                ),
                other => panic!("the descriptor for {shape:?} is not a shape at all: {other:?}"),
            }
        }
        Ok(())
    });
}

#[test]
fn a_descriptor_carries_the_size_and_colour_it_was_given() {
    with_aria(|py, _| {
        let descriptor = Bound::new(
            py,
            Rect {
                width: 640.0,
                height: 360.0,
                rgba: [10, 20, 30, 40],
                corner_radius: 12.0,
                stroke: None,
            },
        )?
        .into_any();
        match crate::content::generator_from(&descriptor)? {
            Generator::Shape {
                width,
                height,
                rgba,
                corner_radius,
                stroke,
                ..
            } => {
                assert_eq!(width, Param::Constant(640.0));
                assert_eq!(height, Param::Constant(360.0));
                assert_eq!(rgba, Param::Constant([10, 20, 30, 40]));
                assert_eq!(corner_radius, Param::Constant(12.0));
                assert!(stroke.is_none(), "a descriptor without a stroke grew one");
            }
            other => panic!("a Rect reached {other:?}"),
        }
        Ok(())
    });
}

#[test]
fn a_scripted_edit_places_a_clip_where_it_asked_for_one() {
    let (start, end, duration, clips, project_duration) = with_aria(|py, globals| {
        py.run(
            c"p = aria.Project('scripted', fps = 30)
t = p.add_track('sticker')
c = t.add(aria.Rect(width = 320, height = 180), 1.0, 2.0)
",
            Some(globals),
            None,
        )?;
        Ok((
            py.eval(c"c.start", Some(globals), None)?.extract::<f64>()?,
            py.eval(c"c.end", Some(globals), None)?.extract::<f64>()?,
            py.eval(c"c.duration", Some(globals), None)?
                .extract::<f64>()?,
            py.eval(c"len(t)", Some(globals), None)?
                .extract::<usize>()?,
            py.eval(c"p.duration", Some(globals), None)?
                .extract::<f64>()?,
        ))
    });
    assert_eq!(start, 1.0);
    assert_eq!(duration, 2.0);
    assert_eq!(end, 3.0);
    assert_eq!(clips, 1, "one add put {clips} clips on the track");
    assert_eq!(project_duration, 3.0);
}

#[test]
fn a_scripted_split_leaves_two_clips_that_meet() {
    let (first_end, second_start, clips) = with_aria(|py, globals| {
        py.run(
            c"p = aria.Project('scripted', fps = 30)
t = p.add_track('sticker')
c = t.add(aria.Ellipse(), 0.0, 4.0)
second = c.split(1.5)
",
            Some(globals),
            None,
        )?;
        Ok((
            py.eval(c"c.end", Some(globals), None)?.extract::<f64>()?,
            py.eval(c"second.start", Some(globals), None)?
                .extract::<f64>()?,
            py.eval(c"len(t)", Some(globals), None)?
                .extract::<usize>()?,
        ))
    });
    assert_eq!(clips, 2, "a split left {clips} clips");
    assert_eq!(first_end, 1.5);
    assert_eq!(
        second_start, first_end,
        "the split left a gap between the halves"
    );
}

#[test]
fn a_handle_to_a_removed_track_says_so_instead_of_panicking() {
    let message = refused(
        c"p = aria.Project('scripted', fps = 30)
t = p.add_track('video')
t.remove()
t.clips
",
    );
    assert!(
        message.contains("stale track handle"),
        "a handle to a removed track reported {message:?}"
    );
}

#[test]
fn an_unknown_sticker_names_what_could_have_been_asked_for() {
    let message = refused(c"aria.Sticker('not-a-sticker')");
    assert!(
        message.starts_with("ValueError:"),
        "an unknown sticker reported {message:?}"
    );
    let known = aria_models::sticker_catalog();
    let first = known
        .first()
        .expect("the sticker catalog ships with entries");
    assert!(
        message.contains(first.id.as_str()),
        "the refusal listed no catalog id, so a script cannot recover from it: {message:?}"
    );
}

#[test]
fn an_unknown_canvas_is_refused_before_a_project_exists() {
    let message = refused(c"aria.Project('scripted', canvas = 'not-an-aspect')");
    assert!(
        message.starts_with("ValueError:") && message.contains("not-an-aspect"),
        "an unknown aspect reported {message:?}"
    );
}

/// The lane a generator needs is the model's rule, not the script's guess, and a
/// script that guesses wrong has to be told which lane it put the clip on. The
/// class this closes is a refusal that names neither the lane nor the content.
#[test]
fn a_shape_on_a_picture_lane_names_the_lane_it_was_put_on() {
    let message = refused(
        c"p = aria.Project('scripted', fps = 30)
t = p.add_track('video')
t.add(aria.Rect(), 0.0, 1.0)
",
    );
    assert!(
        message.contains("Video") && message.contains("cannot hold"),
        "a shape on a picture lane reported {message:?}"
    );
    let shape = aria_models::generator_spec_by_id("shape").expect("the shape generator ships");
    assert_eq!(
        shape.lane.id(),
        "sticker",
        "the lane a shape needs moved, so the refusal above is now about the wrong lane"
    );
}

#[test]
fn concurrent_getters_do_not_panic_from_borrow_mut() {
    with_aria(|py, globals| {
        let code = c"
p = aria.Project('test', fps=30)
t = p.add_track('text', 'Titles')
c1 = t.add(aria.Text('first'), 0.0, 1.0)
c2 = t.add(aria.Text('second'), 1.0, 1.0)

# Concurrent/nested reads across tracks and clips
assert t.name == 'Titles'
assert len(t.clips) == 2
assert [c.start for c in t.clips] == [0.0, 1.0]
assert [c.duration for c in t.clips] == [1.0, 1.0]
assert [c.text for c in t.clips] == ['first', 'second']
";
        py.run(code, Some(globals), None)
    });
}

#[test]
fn param_coercion_rejects_nan_and_inf() {
    let msg_nan = refused(c"
p = aria.Project('test', fps=30)
t = p.add_track('text')
c = t.add(aria.Text('hello'), 0.0, 1.0)
c.animate(scale=float('nan'))
");
    assert!(msg_nan.starts_with("ValueError:"));

    let msg_inf = refused(c"
p = aria.Project('test', fps=30)
t = p.add_track('text')
c = t.add(aria.Text('hello'), 0.0, 1.0)
c.animate(position=(1.0, float('inf')))
");
    assert!(msg_inf.starts_with("ValueError:"));
}

#[test]
fn set_style_typo_key_is_refused_with_key_name() {
    let message = refused(c"
p = aria.Project('test', fps=30)
t = p.add_track('text')
c = t.add(aria.Text('hello'), 0.0, 1.0)
c.set_style(colr='#ffffff')
");
    assert!(message.starts_with("ValueError:"));
    assert!(message.contains("colr"));
}

#[test]
fn speed_range_validation() {
    let msg_low = refused(c"
p = aria.Project('test', fps=30)
t = p.add_track('text')
c = t.add(aria.Text('hello'), 0.0, 1.0)
c.set_speed(0.01)
");
    assert!(msg_low.starts_with("ValueError:"));

    let msg_high = refused(c"
p = aria.Project('test', fps=30)
t = p.add_track('text')
c = t.add(aria.Text('hello'), 0.0, 1.0)
c.set_speed(200.0)
");
    assert!(msg_high.starts_with("ValueError:"));
}
