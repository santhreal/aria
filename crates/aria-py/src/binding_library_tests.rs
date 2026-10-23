//! What a script can do with markers, speed ramps, relinking, templates and
//! clip links — the five capabilities the API reference used to list as not
//! shipped.
//!
//! Each of them existed in the model and was reachable only from the editor, so
//! a script could build a timeline but not annotate it, ramp it, repoint it,
//! reuse it, or keep two clips together. These cases drive the real module the
//! way `import aria` does, and each asserts the model state the call produced,
//! not merely that the call returned.

use std::ffi::CStr;
use std::path::{Path, PathBuf};

use aria_models::{MarkerColor, SlotMedia};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::slot::{SLOT_MEDIA, parse_slot_media, slot_media_name};

/// A still shipped with the repository, so these tests need no encoder and no
/// fixture of their own. Two distinct files, because relinking needs somewhere
/// else to point.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name)
}

fn texture() -> PathBuf {
    fixture("tests/fixtures/texture.png")
}

fn heart() -> PathBuf {
    fixture("assets/stickers/heart.png")
}

/// A path under the sweep-on-exit temp root, unique to `tag` so two tests in
/// this binary never write the same file.
fn scratch(tag: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aria-py-{tag}.{extension}"))
}

/// Run `code` with `aria` bound, then read `expressions` back from the globals
/// it left behind. The module is built by the same wrapper CPython calls, so a
/// class missing from `#[pymodule]` fails here as it would in a script.
fn script<R>(
    code: &CStr,
    read: impl for<'py> FnOnce(Python<'py>, &Bound<'py, PyDict>) -> PyResult<R>,
) -> R {
    Python::attach(|py| {
        let module = pyo3::wrap_pymodule!(crate::aria)(py);
        let globals = PyDict::new(py);
        globals
            .set_item("aria", module)
            .expect("binding a module into a fresh dict cannot fail");
        let outcome = py
            .run(code, Some(&globals), None)
            .and_then(|()| read(py, &globals));
        outcome.unwrap_or_else(|err| {
            err.print(py);
            panic!("the script failed: {err}")
        })
    })
}

/// The message a script would see from `code`, which must not succeed.
fn refused(code: &CStr) -> String {
    Python::attach(|py| {
        let module = pyo3::wrap_pymodule!(crate::aria)(py);
        let globals = PyDict::new(py);
        globals
            .set_item("aria", module)
            .expect("binding a module into a fresh dict cannot fail");
        match py.run(code, Some(&globals), None) {
            Ok(()) => panic!("the script was accepted, so nothing refused it"),
            Err(err) => err.to_string(),
        }
    })
}

/// A script header that imports both fixtures and puts a two-second still on a
/// video track, which is what every media case below starts from.
fn setup() -> String {
    format!(
        "p = aria.Project('library', fps = 30)\n\
         m = p.import_media({texture:?})\n\
         other = p.import_media({heart:?})\n\
         t = p.add_track('video')\n\
         c = t.add(m, 0.0, 2.0)\n",
        texture = texture().display().to_string(),
        heart = heart().display().to_string(),
    )
}

fn code(body: &str) -> std::ffi::CString {
    std::ffi::CString::new(format!("{}{body}", setup())).expect("test source holds no NUL")
}

// --- markers -------------------------------------------------------------

#[test]
fn a_marker_a_script_drops_keeps_the_time_name_colour_and_note_it_was_given() {
    let (time, name, color, comment, count) = script(
        c"p = aria.Project('library', fps = 30)
m = p.add_marker(1.5, 'beat', color = 'red', comment = 'chorus starts')
",
        |py, globals| {
            Ok((
                py.eval(c"m.time", Some(globals), None)?.extract::<f64>()?,
                py.eval(c"m.name", Some(globals), None)?
                    .extract::<String>()?,
                py.eval(c"m.color", Some(globals), None)?
                    .extract::<String>()?,
                py.eval(c"m.comment", Some(globals), None)?
                    .extract::<String>()?,
                py.eval(c"len(p.markers)", Some(globals), None)?
                    .extract::<usize>()?,
            ))
        },
    );
    assert_eq!(time, 1.5);
    assert_eq!(name, "beat");
    assert_eq!(color, "red");
    assert_eq!(comment, "chorus starts");
    assert_eq!(count, 1, "one add left {count} markers");
}

#[test]
fn moving_a_marker_reorders_the_list_it_is_in() {
    let (first, second, times) = script(
        c"p = aria.Project('library', fps = 30)
a = p.add_marker(1.0, 'a')
b = p.add_marker(2.0, 'b')
a.time = 3.0
",
        |py, globals| {
            Ok((
                py.eval(c"p.markers[0].name", Some(globals), None)?
                    .extract::<String>()?,
                py.eval(c"p.markers[1].name", Some(globals), None)?
                    .extract::<String>()?,
                py.eval(c"[m.time for m in p.markers]", Some(globals), None)?
                    .extract::<Vec<f64>>()?,
            ))
        },
    );
    assert_eq!(
        (first.as_str(), second.as_str()),
        ("b", "a"),
        "moving a marker past another left the list in insertion order"
    );
    assert_eq!(times, vec![2.0, 3.0]);
}

#[test]
fn an_unnamed_colour_cycles_the_palette() {
    let colors = script(
        c"p = aria.Project('library', fps = 30)
for i in range(10):
    p.add_marker(float(i), 'm%d' % i)
",
        |py, globals| {
            py.eval(c"[m.color for m in p.markers]", Some(globals), None)?
                .extract::<Vec<String>>()
        },
    );
    let palette: Vec<&str> = MarkerColor::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(colors.len(), 10);
    for (index, color) in colors.iter().enumerate() {
        assert_eq!(
            color,
            palette[index % palette.len()],
            "marker {index} took {color:?} instead of cycling the palette"
        );
    }
}

#[test]
fn every_palette_colour_is_nameable_from_a_script() {
    for color in MarkerColor::ALL {
        let source = std::ffi::CString::new(format!(
            "p = aria.Project('library', fps = 30)\nm = p.add_marker(0.0, 'x', color = {:?})\n",
            color.name()
        ))
        .expect("a palette name holds no NUL");
        let read = script(&source, |py, globals| {
            py.eval(c"m.color", Some(globals), None)?
                .extract::<String>()
        });
        assert_eq!(
            read,
            color.name(),
            "{} did not round-trip through the binding",
            color.name()
        );
    }
}

#[test]
fn an_unknown_marker_colour_names_the_palette_it_could_have_used() {
    let message = refused(
        c"p = aria.Project('library', fps = 30)
p.add_marker(0.0, 'x', color = 'chartreuse')
",
    );
    assert!(
        message.contains("chartreuse"),
        "the refusal did not name what was asked for: {message:?}"
    );
    for color in MarkerColor::ALL {
        assert!(
            message.contains(color.name()),
            "the refusal left out {}, so a script cannot recover from it: {message:?}",
            color.name()
        );
    }
}

#[test]
fn a_handle_to_a_deleted_marker_says_so_instead_of_answering() {
    let message = refused(
        c"p = aria.Project('library', fps = 30)
m = p.add_marker(0.0, 'x')
m.delete()
m.name
",
    );
    assert!(
        message.contains("stale marker handle"),
        "a deleted marker reported {message:?}"
    );
}

// --- speed ramps ---------------------------------------------------------

#[test]
fn a_ramp_a_script_writes_is_the_ramp_the_clip_plays() {
    let (curve, duration) = script(
        &code("c.set_speed_curve([(0.0, 1.0), (0.5, 2.0), (1.0, 1.0)])\n"),
        |py, globals| {
            Ok((
                py.eval(c"c.speed_curve", Some(globals), None)?
                    .extract::<Vec<(f64, f32)>>()?,
                py.eval(c"c.duration", Some(globals), None)?
                    .extract::<f64>()?,
            ))
        },
    );
    assert_eq!(curve, vec![(0.0, 1.0), (0.5, 2.0), (1.0, 1.0)]);
    assert!(
        duration < 2.0,
        "a ramp that speeds the middle up left the clip {duration}s long"
    );
}

#[test]
fn a_preset_ramp_is_reported_as_the_preset_it_came_from() {
    let presets = aria_models::speed_preset_catalog();
    for spec in presets {
        let source = code(&format!(
            "c.set_speed_curve({id:?})\n",
            id = spec.id.as_str()
        ));
        let (named, points) = script(&source, |py, globals| {
            Ok((
                py.eval(c"c.speed_preset", Some(globals), None)?
                    .extract::<Option<String>>()?,
                py.eval(c"c.speed_curve", Some(globals), None)?
                    .extract::<Option<Vec<(f64, f32)>>>()?,
            ))
        });
        assert_eq!(
            named.as_deref(),
            Some(spec.id.as_str()),
            "the ramp from preset {:?} reported {named:?}",
            spec.id
        );
        let points = points.expect("a preset ramp is not flat");
        assert_eq!(
            points.len(),
            spec.points.len(),
            "preset {:?} lost points through the binding",
            spec.id
        );
    }
}

#[test]
fn clearing_a_ramp_returns_the_clip_to_one_rate() {
    let (curve, preset, duration) = script(
        &code(
            "c.set_speed_curve([(0.0, 1.0), (1.0, 4.0)])\n\
             c.set_speed_curve(None)\n",
        ),
        |py, globals| {
            Ok((
                py.eval(c"c.speed_curve", Some(globals), None)?
                    .extract::<Option<Vec<(f64, f32)>>>()?,
                py.eval(c"c.speed_preset", Some(globals), None)?
                    .extract::<Option<String>>()?,
                py.eval(c"c.duration", Some(globals), None)?
                    .extract::<f64>()?,
            ))
        },
    );
    assert!(
        curve.is_none(),
        "a cleared ramp still reads back as {curve:?}"
    );
    assert!(preset.is_none());
    assert_eq!(
        duration, 2.0,
        "clearing the ramp did not restore the length"
    );
}

#[test]
fn a_malformed_ramp_is_refused_with_the_rule_it_broke() {
    for (body, expected) in [
        ("c.set_speed_curve([(0.0, 1.0)])\n", "at least two points"),
        (
            "c.set_speed_curve([(0.0, 1.0), (1.5, 2.0)])\n",
            "outside 0.0",
        ),
        (
            "c.set_speed_curve([(0.5, 1.0), (0.5, 2.0)])\n",
            "share one position",
        ),
        (
            "c.set_speed_curve('not-a-preset')\n",
            "unknown speed preset",
        ),
    ] {
        let message = refused(&code(body));
        assert!(
            message.contains(expected),
            "{body:?} reported {message:?}, which does not say {expected:?}"
        );
    }
}

// --- relink --------------------------------------------------------------

#[test]
fn relinking_a_clip_changes_the_file_and_keeps_the_placement() {
    let (start, duration, path) = script(&code("c.set_media(other)\n"), |py, globals| {
        Ok((
            py.eval(c"c.start", Some(globals), None)?.extract::<f64>()?,
            py.eval(c"c.duration", Some(globals), None)?
                .extract::<f64>()?,
            py.eval(c"c.media.path", Some(globals), None)?
                .extract::<String>()?,
        ))
    });
    assert_eq!((start, duration), (0.0, 2.0), "a relink moved the clip");
    assert!(
        path.ends_with("heart.png"),
        "the clip still plays {path:?} after being relinked"
    );
}

#[test]
fn an_in_point_past_the_end_of_the_new_media_is_refused() {
    // A still repeats one frame for any window, so the in-point that has to be
    // refused is one into real media; the fixture pair is two stills, and the
    // model's own bounds check is what rejects a window past a video's end.
    // Here the check is that the binding forwards the model's refusal rather
    // than clamping silently, using a generated clip that cannot be relinked.
    let message = refused(
        &std::ffi::CString::new(format!(
            "p = aria.Project('library', fps = 30)\n\
             m = p.import_media({texture:?})\n\
             t = p.add_track('sticker')\n\
             c = t.add(aria.Rect(width = 64, height = 64), 0.0, 1.0)\n\
             c.set_media(m)\n",
            texture = texture().display().to_string()
        ))
        .expect("test source holds no NUL"),
    );
    assert!(
        message.contains("only a media clip can be relinked"),
        "relinking a generated clip reported {message:?}"
    );
}

// --- templates -----------------------------------------------------------

#[test]
fn a_template_a_script_saves_lists_the_slots_it_marked() {
    let path = scratch("template-slots", "ariat");
    let source = code(&format!(
        "c.replaceable = aria.Slot(order = 0, accepts = 'image', label = 'your photo')\n\
         t2 = p.save_template({path:?}, 'Photo card', author = 'aria', tags = ['test'])\n\
         loaded = aria.Template.load({path:?})\n",
        path = path.display().to_string()
    ));
    let (name, author, tags, slots, labels, accepts, duration) = script(&source, |py, globals| {
        Ok((
            py.eval(c"loaded.name", Some(globals), None)?
                .extract::<String>()?,
            py.eval(c"loaded.author", Some(globals), None)?
                .extract::<Option<String>>()?,
            py.eval(c"loaded.tags", Some(globals), None)?
                .extract::<Vec<String>>()?,
            py.eval(c"loaded.slot_count", Some(globals), None)?
                .extract::<usize>()?,
            py.eval(c"[s.label for s in loaded.slots]", Some(globals), None)?
                .extract::<Vec<Option<String>>>()?,
            py.eval(c"[s.accepts for s in loaded.slots]", Some(globals), None)?
                .extract::<Vec<String>>()?,
            py.eval(c"loaded.slots[0].duration", Some(globals), None)?
                .extract::<f64>()?,
        ))
    });
    let _ = std::fs::remove_file(&path);
    assert_eq!(name, "Photo card");
    assert_eq!(author.as_deref(), Some("aria"));
    assert_eq!(tags, vec!["test".to_string()]);
    assert_eq!(slots, 1, "one marked clip produced {slots} slots");
    assert_eq!(labels, vec![Some("your photo".to_string())]);
    assert_eq!(accepts, vec!["image".to_string()]);
    assert_eq!(duration, 2.0, "the slot lost the length it locks");
}

#[test]
fn filling_a_template_puts_the_callers_media_in_the_authors_timing() {
    let path = scratch("template-fill", "ariat");
    let source = code(&format!(
        "c.replaceable = aria.Slot(order = 0, accepts = 'image')\n\
         p.save_template({path:?}, 'Photo card')\n\
         loaded = aria.Template.load({path:?})\n\
         filled = loaded.fill([other])\n\
         clip = filled.tracks[0].clips[0]\n",
        path = path.display().to_string()
    ));
    let (duration, start, media, project_duration) = script(&source, |py, globals| {
        Ok((
            py.eval(c"clip.duration", Some(globals), None)?
                .extract::<f64>()?,
            py.eval(c"clip.start", Some(globals), None)?
                .extract::<f64>()?,
            py.eval(c"clip.media.path", Some(globals), None)?
                .extract::<String>()?,
            py.eval(c"filled.duration", Some(globals), None)?
                .extract::<f64>()?,
        ))
    });
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        (start, duration),
        (0.0, 2.0),
        "filling a slot changed the author's timing"
    );
    assert!(
        media.ends_with("heart.png"),
        "the filled slot plays {media:?} instead of the caller's pick"
    );
    assert_eq!(project_duration, 2.0);
}

#[test]
fn filling_with_the_wrong_number_of_picks_is_refused_rather_than_half_done() {
    let path = scratch("template-count", "ariat");
    let message = refused(&code(&format!(
        "c.replaceable = aria.Slot(order = 0, accepts = 'image')\n\
         p.save_template({path:?}, 'Photo card')\n\
         loaded = aria.Template.load({path:?})\n\
         loaded.fill([])\n",
        path = path.display().to_string()
    )));
    let _ = std::fs::remove_file(&path);
    assert!(
        message.contains("1 slot needs 1 pick, got 0"),
        "an empty pick list reported {message:?}"
    );
}

#[test]
fn a_bundle_carries_its_sample_media_to_a_directory_that_had_none() {
    let bundle = scratch("template-bundle", "ariab");
    let dest = std::env::temp_dir().join("aria-py-bundle-install");
    let source = code(&format!(
        "c.replaceable = aria.Slot(order = 0, accepts = 'image')\n\
         p.save_template({bundle:?}, 'Bundled card', bundle = True)\n\
         installed = aria.Template.install({bundle:?}, {dest:?})\n",
        bundle = bundle.display().to_string(),
        dest = dest.display().to_string()
    ));
    let (name, slots, media) = script(&source, |py, globals| {
        Ok((
            py.eval(c"installed.name", Some(globals), None)?
                .extract::<String>()?,
            py.eval(c"installed.slot_count", Some(globals), None)?
                .extract::<usize>()?,
            py.eval(
                c"[s.duration for s in installed.slots]",
                Some(globals),
                None,
            )?
            .extract::<Vec<f64>>()?,
        ))
    });
    let _ = std::fs::remove_file(&bundle);
    let _ = std::fs::remove_dir_all(&dest);
    assert_eq!(name, "Bundled card");
    assert_eq!(slots, 1);
    assert_eq!(media, vec![2.0]);
}

#[test]
fn every_slot_restriction_is_nameable_and_round_trips() {
    for accepts in SLOT_MEDIA {
        let name = slot_media_name(accepts);
        assert_eq!(
            parse_slot_media(name).expect("a name the module printed"),
            accepts,
            "{name:?} parsed back to the wrong restriction"
        );
    }
    let unknown = parse_slot_media("holograms").expect_err("an unknown restriction is refused");
    let message = unknown.to_string();
    for accepts in SLOT_MEDIA {
        assert!(
            message.contains(slot_media_name(accepts)),
            "the refusal left out {:?}: {message:?}",
            slot_media_name(accepts)
        );
    }
}

#[test]
fn a_visual_slot_cannot_be_marked_on_an_audio_lane() {
    let message = refused(&code(
        "c.replaceable = aria.Slot(order = 0, accepts = 'audio')\n",
    ));
    assert!(
        message.contains("cannot sit on a"),
        "an audio slot on a video lane reported {message:?}"
    );
}

// --- links ---------------------------------------------------------------

#[test]
fn linked_clips_know_about_each_other() {
    let (linked_names, back) = script(
        &code(
            "t2 = p.add_track('video')\n\
             d = t2.add(other, 0.0, 2.0)\n\
             c.link_with([d])\n",
        ),
        |py, globals| {
            Ok((
                py.eval(c"len(c.linked)", Some(globals), None)?
                    .extract::<usize>()?,
                py.eval(c"len(d.linked)", Some(globals), None)?
                    .extract::<usize>()?,
            ))
        },
    );
    assert_eq!(
        linked_names, 1,
        "a linked clip listed {linked_names} partners"
    );
    assert_eq!(back, 1, "the link was one-way");
}

#[test]
fn unlinking_dissolves_the_whole_group() {
    let (before, after, partner_after) = script(
        &code(
            "t2 = p.add_track('video')\n\
             d = t2.add(other, 0.0, 2.0)\n\
             c.link_with([d])\n\
             before = len(c.linked)\n\
             c.unlink()\n",
        ),
        |py, globals| {
            Ok((
                py.eval(c"before", Some(globals), None)?
                    .extract::<usize>()?,
                py.eval(c"len(c.linked)", Some(globals), None)?
                    .extract::<usize>()?,
                py.eval(c"len(d.linked)", Some(globals), None)?
                    .extract::<usize>()?,
            ))
        },
    );
    assert_eq!(before, 1);
    assert_eq!(after, 0, "the clip that unlinked is still in a group");
    assert_eq!(
        partner_after, 0,
        "unlinking left the partner believing it is still linked"
    );
}

#[test]
fn unlinking_a_clip_that_is_not_linked_is_refused() {
    let message = refused(&code("c.unlink()\n"));
    assert!(
        !message.is_empty(),
        "unlinking an unlinked clip silently succeeded"
    );
}

#[test]
fn a_clip_from_another_project_cannot_join_the_link_group() {
    let message = refused(&code(
        "q = aria.Project('other', fps = 30)\n\
         qt = q.add_track('sticker')\n\
         qc = qt.add(aria.Rect(width = 64, height = 64), 0.0, 1.0)\n\
         c.link_with([qc])\n",
    ));
    assert!(
        message.contains("different project"),
        "a cross-project link reported {message:?}"
    );
}

/// Every restriction the model declares has a name here, and the mapping is a
/// match rather than a lookup table, so a variant added to [`SlotMedia`] stops
/// this crate compiling instead of becoming a slot a script cannot ask for.
#[test]
fn the_slot_restrictions_are_the_models_own() {
    assert_eq!(
        SLOT_MEDIA.len(),
        4,
        "the model gained a slot restriction; name it in slot_media_name and list it here"
    );
    assert!(SLOT_MEDIA.contains(&SlotMedia::Any));
}
