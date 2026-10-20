//! Render and replace, through the editor's controls.
//!
//! The class these close: a render that goes somewhere it should not, or a swap
//! that cannot be undone in one step. The rendering itself needs a GPU and an
//! encoder, so what is exercised here is everything around it — where the file
//! is decided to live, which requests are refused before a minute of encoding
//! is spent, and the adoption of a file that is already on disk, which is the
//! same code path the job's last act runs.
//!
//! What they do not catch: the encode. That is `aria-render`'s export coverage.
//! One test does let a render really start, because a sound render needs neither
//! a GPU nor the platform video encoder: what it proves is that a started render
//! is reported as started, with a job on the board, rather than as done.

use aria_commands::{Command, EditCommand};
use aria_plugin::PluginContext;

use super::AppState;
use crate::app::fixtures::{bake_destination, bakeable, rendered_file};
use crate::app::premiere_tests::with_clip;
use crate::scratch_pool::scratch;

#[test]
fn a_project_that_was_never_saved_is_refused_by_name() {
    let (mut state, _) = bakeable();
    assert!(state.project_path.is_none(), "the fixture was saved");

    let refusal = state.render_and_replace().expect_err("no project file");
    assert!(
        refusal.contains("Save the project first"),
        "the refusal does not say what to do: {refusal}"
    );
    assert!(
        state.job_view.is_empty(),
        "a refused render still queued a job"
    );
}

#[test]
fn a_clip_with_nothing_on_it_is_refused_by_name() {
    let (mut state, _) = with_clip();
    state.project_path = Some(scratch("bake-plain").join("plain.aria"));

    let refusal = state.render_and_replace().expect_err("nothing to render");
    assert!(
        refusal.contains("re-encode"),
        "the refusal does not say why: {refusal}"
    );
    assert!(
        state.job_view.is_empty(),
        "a refused render still queued a job"
    );
}

#[test]
fn a_render_needs_something_selected() {
    let mut state = AppState::new(PluginContext::new(), scratch("bake-empty")).expect("state");
    state.new_project().expect("a project");

    let refusal = state.render_and_replace().expect_err("nothing selected");
    assert!(
        refusal.contains("Select a clip"),
        "the refusal does not say what is missing: {refusal}"
    );
}

#[test]
fn restoring_a_clip_that_was_never_rendered_is_refused() {
    let (mut state, _) = bakeable();

    let refusal = state.restore_unrendered().expect_err("not rendered");
    assert!(
        !refusal.is_empty(),
        "a refusal with nothing in it reports nothing"
    );
}

/// The file's name is a digest of what the render would show, so changing the
/// clip's look asks for a different file instead of adopting the old one.
#[test]
fn the_name_of_the_file_follows_what_it_shows() {
    let (mut state, clip) = bakeable();
    state.project_path = Some(scratch("bake-name").join("named.aria"));
    let first = bake_destination(&state, clip);

    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("a second effect");
    state.sync_from_engine();

    assert_ne!(
        bake_destination(&state, clip),
        first,
        "a clip that now renders differently would adopt the old file"
    );
    assert_eq!(
        first.parent().and_then(|dir| dir.file_name()),
        Some(std::ffi::OsStr::new("named Renders")),
        "the render does not live beside the project"
    );
}

/// The whole swap, minus the encode: a file that is already where the render
/// would go is adopted, and one undo puts back both the clip and the pool.
#[test]
fn a_render_already_on_disk_is_adopted_and_one_undo_puts_everything_back() {
    let (mut state, clip) = bakeable();
    let dir = scratch("bake-adopt");
    state.project_path = Some(dir.join("adopted.aria"));
    let dest = bake_destination(&state, clip);
    std::fs::create_dir_all(dest.parent().expect("a parent")).expect("the renders directory");
    std::fs::copy(rendered_file(), &dest).expect("stand a render in");

    let pool_before = state.store.media.len();
    let started = state
        .render_and_replace()
        .expect("adopt the render on disk");
    assert!(
        started.is_none(),
        "a file already on disk was queued for rendering again"
    );

    {
        let engine = state.engine.lock();
        let baked = engine.project().clip(clip).expect("the clip");
        assert!(baked.is_baked(), "the clip did not take the rendered file");
        assert!(
            baked.effects.is_empty(),
            "the look the render folded in was not spent"
        );
    }
    assert_eq!(
        state.store.media.len(),
        pool_before + 1,
        "the rendered file did not join the pool"
    );

    assert!(state.engine.lock().undo(), "the swap left no history entry");
    state.sync_from_engine();
    {
        let engine = state.engine.lock();
        let back = engine.project().clip(clip).expect("the clip");
        assert!(!back.is_baked(), "one undo did not put the clip back");
        assert_eq!(back.effects.len(), 1, "the clip came back without its look");
    }
    assert_eq!(
        state.store.media.len(),
        pool_before,
        "one undo left the imported render in the pool"
    );

    // Asking again adopts the same file rather than rendering a second one.
    state
        .render_and_replace()
        .expect("adopt the same render again");
    assert!(
        state.job_view.is_empty(),
        "adopting a file on disk queued a render"
    );
}

/// A clip on a sound lane renders too, and its file is named by the sink that
/// will write it. The whole shell path is exercised: the pass, the destination,
/// and the adoption of the file that lands there, which is the job's last act.
#[test]
fn a_sound_clip_renders_to_the_sound_sink_and_swaps_in() {
    let (mut state, clip) = crate::app::fixtures::with_audio();
    state.project_path = Some(scratch("bake-sound").join("sound.aria"));
    state
        .apply_command(Command::Edit(EditCommand::SetClipDenoise {
            clip,
            denoise: true,
        }))
        .expect("something for the render to fold in");

    let dest = bake_destination(&state, clip);
    assert_eq!(
        dest.extension().and_then(|ext| ext.to_str()),
        Some(aria_encoder::AUDIO_CAPABILITY.extension),
        "a sound render was named after the picture container: {}",
        dest.display()
    );
    std::fs::create_dir_all(dest.parent().expect("a parent")).expect("the renders directory");
    std::fs::copy(
        crate::app::fixtures::asset("fixture-sine-48k-2s.wav"),
        &dest,
    )
    .expect("stand a render in");

    let started = state
        .render_and_replace()
        .expect("adopt the sound render on disk");
    assert!(
        started.is_none(),
        "a file already on disk was re-encoded instead of adopted"
    );
    {
        let engine = state.engine.lock();
        let baked = engine.project().clip(clip).expect("the clip");
        assert!(baked.is_baked(), "the clip did not take the rendered file");
        assert!(
            !baked.denoise,
            "the sound the render folded in was not spent"
        );
    }

    assert!(state.engine.lock().undo(), "the swap left no history entry");
    state.sync_from_engine();
    let engine = state.engine.lock();
    let back = engine.project().clip(clip).expect("the clip");
    assert!(!back.is_baked(), "one undo did not put the clip back");
    assert!(back.denoise, "the clip came back without its sound");
}

/// The assistant renders the clip it was given, not whatever is selected. The
/// selection is deliberately a different clip that could not be rendered at all,
/// so a path that read the selection fails here instead of passing by accident.
#[test]
fn the_assistant_renders_the_clip_it_names_rather_than_the_selection() {
    let (mut state, clip) = crate::app::fixtures::adopted_render("bake-assistant");
    let other = crate::app::fixtures::asset("fixture-sine-48k-2s.wav");
    let media = state.import_media(other).expect("import the sound fixture");
    state
        .drop_media_on_timeline(media, None, 96)
        .expect("place a second clip");
    let selected = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|row| aria_models::ClipId::from_raw(row.id))
        .find(|row| *row != clip)
        .expect("a second clip");
    state.selection = vec![selected];
    state.sync_from_engine();

    let reported = state
        .run_host_request(crate::app::assistant_host::HostRequest::RenderClip { clip })
        .expect("the named clip's render");
    assert!(
        reported.contains("was already rendered"),
        "the reported line does not say the file was adopted: {reported}"
    );

    let engine = state.engine.lock();
    assert!(
        engine.project().clip(clip).expect("the clip").is_baked(),
        "the named clip did not take its rendered file"
    );
    assert!(
        !engine
            .project()
            .clip(selected)
            .expect("the other clip")
            .is_baked(),
        "the selection was rendered instead of the clip that was named"
    );
}

/// A render the assistant has to wait for is reported as started, and the job
/// board carries it. A sound render needs neither a GPU nor the platform video
/// encoder, so this one really spawns rather than adopting a file.
#[test]
fn a_render_the_assistant_starts_is_reported_as_started_with_a_job() {
    let (mut state, clip) = crate::app::fixtures::with_audio();
    state.project_path = Some(scratch("bake-assistant-sound").join("started.aria"));
    state
        .apply_command(Command::Edit(EditCommand::SetClipDenoise {
            clip,
            denoise: true,
        }))
        .expect("something for the render to fold in");
    let dest = bake_destination(&state, clip);
    assert!(
        !dest.exists(),
        "the render was already on disk, so nothing would start: {}",
        dest.display()
    );

    let reported = state
        .run_host_request(crate::app::assistant_host::HostRequest::RenderClip { clip })
        .expect("start the render");
    assert!(
        reported.starts_with("started rendering"),
        "a render that has not been written was reported as done: {reported}"
    );
    assert!(
        reported.contains("Jobs board"),
        "the report does not state where to watch it: {reported}"
    );
    assert!(
        !state.job_view.is_empty(),
        "a started render left no job on the board"
    );
    assert!(
        !state
            .engine
            .lock()
            .project()
            .clip(clip)
            .expect("the clip")
            .is_baked(),
        "the clip was swapped before the render finished"
    );
}
