//! What the crate's tests share: the walker the censuses read the sources
//! with, and the census modules themselves.
//!
//! Each census reads the crate, or the workspace, off disk and asserts something
//! about its shape that no single unit test can see: one owner for temp paths,
//! one module bringing up the GPU, one profile root, one owner for threads, one
//! direction for the dependency between the UI and the plugin boundary. Each one
//! is derived from the sources at run time rather than from a list of known call
//! sites, so a new file is covered on the run that adds it, and each one goes red
//! when a declared owner stops being one, so an allowlist cannot rot into an
//! exemption for a file that does nothing.
//!
//! Scratch directories are not here. `crate::scratch_pool` owns the pool, the
//! lease, and every rule about a pool with no free slot left.

use std::path::{Path, PathBuf};

mod window;
pub(crate) use window::{release_at_teardown, window_view};
pub(crate) mod catalogs;
pub(crate) mod control_sites;
pub(crate) mod doc_tables;
pub(crate) mod sources;

/// Every `.rs` file under `dir`, recursively.
///
/// The censuses below read the sources rather than a list of call sites, and so
/// does the pool's own rule about spawned threads, so the walker is shared.
pub(crate) fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// A headless window with the real shell in it, over a state with the UI plugin
/// mounted and an empty project open.
///
/// The theme is installed first because `Theme::of` panics without one. The
/// project is open because Home opens no dialog but Settings.
pub(crate) fn drawn_shell<'a>(
    cx: &'a mut gpui::TestAppContext,
    tag: &str,
) -> (
    gpui::Entity<crate::app::AppState>,
    &'a mut gpui::VisualTestContext,
) {
    let state = shell_state(cx, tag);
    let owned = state.clone();
    let (_shell, visual) = window_view(cx, |_, cx| crate::shell::Shell::new(owned, cx));
    (state, visual)
}

/// [`drawn_shell`] in a window at `bounds` on the screen, for a test that
/// tells a position in the window from one on the screen. The window
/// [`drawn_shell`] opens fills the screen, so the two are the same there.
pub(crate) fn drawn_shell_at(
    cx: &mut gpui::TestAppContext,
    tag: &str,
    bounds: gpui::Bounds<gpui::Pixels>,
) -> (gpui::Entity<crate::app::AppState>, gpui::VisualTestContext) {
    use gpui::AppContext as _;
    release_at_teardown(cx);
    let state = shell_state(cx, tag);
    let owned = state.clone();
    let window = cx
        .update(|app| {
            app.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| crate::shell::Shell::new(owned, cx)),
            )
        })
        .unwrap_or_else(|err| panic!("a test window at {bounds:?}: {err:?}"));
    let visual = gpui::VisualTestContext::from_window(*window, cx);
    visual.run_until_parked();
    (state, visual)
}

/// The painted bounds of `selector` once background follow-ups have drawn.
///
/// A frame that replays a cached view keeps the bounds the view's debug
/// selectors were painted at, so an element of a panel body the last frame
/// replayed reads as one it rebuilt.
pub(crate) fn panel_bounds(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    visual.run_until_parked();
    visual.debug_bounds(selector)
}

/// Redraws until the boot cover is gone, for a test that reads text where the
/// cover's wordmark lies. The cover holds on the wall clock until the first
/// picture lands or [`crate::shell::SPLASH_MAX`] passes, then fades, so the
/// wait is on the wall clock too, and fails past that ceiling and the fade.
pub(crate) fn wait_out_splash(visual: &mut gpui::VisualTestContext) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now()
        + crate::shell::SPLASH_MAX
        + gpui::motion::presets::SPLASH_OUT.total()
        + Duration::from_secs(1);
    visual.run_until_parked();
    while visual.debug_bounds("boot-splash").is_some() {
        assert!(
            Instant::now() < deadline,
            "the boot cover outlived its ceiling"
        );
        std::thread::sleep(Duration::from_millis(50));
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
    }
}

/// The state [`drawn_shell`] draws: the UI plugin mounted and an empty
/// project open.
fn shell_state(cx: &mut gpui::TestAppContext, tag: &str) -> gpui::Entity<crate::app::AppState> {
    use gpui::AppContext as _;
    cx.update(|app| {
        crate::theme::Theme::install(
            crate::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
        let ctx = aria_plugin::PluginContext::new();
        ctx.mount(&crate::plugins::UiPlugin)
            .unwrap_or_else(|err| panic!("the ui plugin needs nothing: {err:?}"));
        let mut state = crate::app::AppState::new(ctx, crate::scratch_pool::scratch(tag))
            .unwrap_or_else(|err| panic!("editor state: {err:?}"));
        if let Err(err) = state.new_project() {
            panic!("a project to edit: {err:?}");
        }
        app.new(|_| state)
    })
}

/// Whether `inner` lies inside `outer`, edges included. Layout is in floats, so
/// an edge shared with the outer box may land a rounding error past it.
pub(crate) fn bounds_inside(
    inner: gpui::Bounds<gpui::Pixels>,
    outer: gpui::Bounds<gpui::Pixels>,
) -> bool {
    let slack = gpui::px(0.5);
    inner.left() >= outer.left() - slack
        && inner.top() >= outer.top() - slack
        && inner.right() <= outer.right() + slack
        && inner.bottom() <= outer.bottom() + slack
}

/// The text the project supplies: media names and paths, folder names, lane
/// names and clip names. A drawn line that starts with one is written as the
/// project wrote it, so a casing walk skips it.
pub(crate) fn project_text(state: &crate::app::AppState) -> std::collections::HashSet<String> {
    let store = &state.store;
    store
        .media
        .iter()
        .flat_map(|item| [item.name.to_string(), item.path.to_string()])
        .chain(store.bins.iter().map(|bin| bin.name.to_string()))
        .chain(store.sequence.tracks.iter().flat_map(|track| {
            std::iter::once(track.name.to_string())
                .chain(track.clips.iter().map(|clip| clip.name.to_string()))
        }))
        .filter(|text| !text.is_empty())
        .collect()
}

/// Place the texture still at the start of the empty project and select its
/// clip, for a test that draws what a selection shows.
pub(crate) fn select_a_still(state: &mut crate::app::AppState) {
    let still = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    let media = state.import_media(still).expect("import the still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the still");
    let Some(clip) = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .next()
    else {
        panic!("the still placed no clip");
    };
    state.select_clip(clip);
}

/// The device the render plugin brings up, mounted on `ctx` with the plugins
/// it depends on as a session mounts them, or `None` on a host with no GPU.
///
/// Through `PluginContext::mount` only. A direct `Plugin::mount` hands the
/// plugin a context that is not mounting, so every service it registers is
/// dropped when the call returns and the device is never there to find: a
/// test asking for it skips on every host, GPU or not.
pub(crate) fn mounted_gpu(ctx: &aria_plugin::PluginContext) -> Option<aria_compositor::GpuContext> {
    let chain: [&dyn aria_plugin::Plugin; 3] = [
        &crate::plugins::DecoderPlugin,
        &crate::plugins::CompositorPlugin,
        &crate::plugins::RenderPlugin,
    ];
    for plugin in chain {
        ctx.mount(plugin).expect("mount the render chain");
    }
    let gpu = ctx.try_get::<aria_compositor::GpuContext>()?;
    Some(gpu.share())
}

/// A plugin encoder that takes every frame and writes nothing, for a test that
/// needs a format registered rather than a file written.
pub(crate) struct NullEncoder;

impl aria_core::VideoEncoder for NullEncoder {
    fn push(&mut self, _frame: &aria_core::VideoFrame) -> Result<(), aria_core::EncodeError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<(), aria_core::EncodeError> {
        Ok(())
    }
    fn choice(&self) -> aria_core::EncoderChoice {
        aria_core::EncoderChoice::only("null", None)
    }
}

impl aria_plugin::Encoder for NullEncoder {
    fn open(
        &self,
        _path: &Path,
        _config: &aria_core::EncoderConfig,
    ) -> Result<Box<dyn aria_core::VideoEncoder>, aria_core::EncodeError> {
        Ok(Box::new(Self))
    }
}

#[cfg(test)]
mod error_writes;

#[cfg(test)]
mod temp_ownership;

#[cfg(test)]
mod gpu;

#[cfg(test)]
mod profile_root;

#[cfg(test)]
mod temp_growth;

#[cfg(test)]
mod processes;

#[cfg(test)]
mod threads;

#[cfg(test)]
mod dependency_direction;

#[cfg(test)]
mod shell_draw;

#[cfg(test)]
mod deferred_docs;

#[cfg(test)]
mod window_teardown;

#[cfg(test)]
mod persona_nouns;

#[cfg(test)]
mod pseudo_plurals;
