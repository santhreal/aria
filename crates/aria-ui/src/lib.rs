//! Aria GPUI viewport.
//!
//! Design: PLAN.md Phase 1. Motion, theme, loaders, popover, frost, and
//! edge-fade are ported from [Comet](https://github.com/zeronsh/comet)
//! (MIT License — see LICENSES/comet-MIT.txt).

use std::borrow::Cow;
use std::path::PathBuf;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

use aria_plugin::PluginContext;

pub mod app;
pub mod appearance;
pub mod audio;
pub mod catalog_text;
pub mod chrome;
pub mod cloud;
pub mod commands;
pub mod components;
pub(crate) mod const_table;
pub mod datefmt;
pub mod dock;
pub mod drag_hint;
pub mod edge_fade;
pub mod filmstrips;
pub mod frame;
pub mod frost;
pub mod handoff;
pub mod icons;
pub mod interaction;
pub(crate) mod interned;
pub mod keys;
pub mod layouts;
pub mod loaders;
pub mod motion;
pub mod os_drop;
pub mod panels;
pub mod pending;
pub mod plugins;
pub mod popover;
pub mod preview_worker;
pub mod projection;
pub mod resolution;
pub mod scopes;
pub(crate) mod session_end;
pub mod settings;
pub mod shell;
pub mod snap;
pub mod surfaces;
pub mod theme;
pub mod timecode;
pub mod timeline_display;
pub(crate) mod timeline_playhead;
pub mod tool_catalog;
pub mod tooltip;
pub mod transport;
pub mod types;
pub mod visual_diff;
pub mod waveforms;
pub mod workload;

#[cfg(test)]
#[path = "magnet_index_tests.rs"]
mod magnet_index_tests;
#[cfg(test)]
mod scratch_pool;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "types_census.rs"]
mod types_census;

pub use appearance::AppearanceMode;
pub use projection::project_to_ui;
pub use theme::{Appearance, Theme};
pub use types::{ActivePanel, UiEditorStore};

/// The global allocator of the editor process. `aria-desktop` installs it and
/// `aria-bench` counts allocations through it, so the benches measure the
/// allocator the editor ships with.
pub use mimalloc::MiMalloc as Allocator;

/// Embedded UI typeface: Inter in four static weights, © The Inter Project
/// Authors, licensed under the SIL Open Font License 1.1 (text in
/// `LICENSES/Inter-OFL-1.1.txt`). Static faces, because the text system picks
/// a weight by face and does not instance a variable font.
static FONT_INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static FONT_INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
static FONT_INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
static FONT_INTER_BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");

pub fn register_fonts(cx: &App) {
    if let Err(err) = cx.text_system().add_fonts(vec![
        Cow::Borrowed(FONT_INTER),
        Cow::Borrowed(FONT_INTER_MEDIUM),
        Cow::Borrowed(FONT_INTER_SEMIBOLD),
        Cow::Borrowed(FONT_INTER_BOLD),
    ]) {
        tracing::warn!(error = %err, "failed to register the embedded Inter fonts");
    }
}

/// The application on the display server the environment sets, with the
/// shipped assets. Without a display server that accepts the connection, it
/// prints the reason and the fix to stderr and exits with status 1: the
/// editor never runs without a window.
pub fn windowed_application() -> gpui::Application {
    match gpui_platform::windowed_application() {
        Ok(app) => app.with_assets(icons::Assets),
        Err(reason) => exit_without_window(
            &reason,
            "Run aria in a graphical session: WAYLAND_DISPLAY or DISPLAY set to \
             a display server that is running, and ZED_HEADLESS unset.",
        ),
    }
}

/// The display server accepted the connection and the window still did not
/// open: print `reason` and the fix to stderr and exit with status 1.
pub fn window_failed(reason: impl std::fmt::Display) -> ! {
    exit_without_window(
        &reason,
        "The window draws through Vulkan or OpenGL; check that the graphics \
         driver provides one of them.",
    )
}

fn exit_without_window(reason: &dyn std::fmt::Display, fix: &str) -> ! {
    let reason = format!("{reason:#}");
    tracing::error!(error = %reason, "aria cannot open its window");
    eprintln!("aria cannot open its window: {reason}");
    eprintln!("{fix}");
    aria_decoder::exit_process(1);
}

/// Everything the headed binary passes in.
pub struct UiConfig {
    pub data_dir: PathBuf,
    pub ctx: PluginContext,
    /// Paths named on the command line, in the order they were given.
    pub open_paths: Vec<PathBuf>,
}

impl std::fmt::Debug for UiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiConfig")
            .field("data_dir", &self.data_dir)
            .field("open_paths", &self.open_paths)
            .finish_non_exhaustive()
    }
}

/// Run the headed app on the default data directory, opening nothing, and end
/// the process when it quits.
pub fn run_app(ctx: PluginContext) -> ! {
    run_app_with(UiConfig {
        data_dir: settings::default_data_dir(),
        ctx,
        open_paths: Vec::new(),
    });
}

/// Run the headed app until it quits, then end the process with
/// [`aria_decoder::exit_process`].
pub fn run_app_with(config: UiConfig) -> ! {
    // This process is the editor on a desktop, so a reveal may open the file
    // manager. Nothing else in the crate switches this on: a test
    // process that reveals a path records the request instead of opening a
    // window on whatever machine is running the suite.
    app::launch::allow_desktop_launch();
    // The session is built before the window: a failure here means there is no
    // editor to show, and the old fallback rebuilt the state from an empty plugin
    // context — dropping every mounted plugin — only to panic on the same failure.
    let mut state = match app::AppState::new(config.ctx, config.data_dir.clone()) {
        Ok(state) => state,
        Err(err) => {
            tracing::error!(error = %err, "failed to start the editing session");
            eprintln!("aria cannot start: {err}");
            eprintln!(
                "The editor needs a GPU device it can open and a writable data \
                 directory ({}).",
                config.data_dir.display()
            );
            aria_decoder::exit_process(1);
        }
    };
    // Paths from the command line are the same question a drop on Home asks, so
    // they go through the same owner: a project file opens, media is imported and
    // laid end to end in a fresh session, a LUT or a skill installs. Answered
    // before the window so the first frame is the named project.
    if !config.open_paths.is_empty()
        && let Err(err) = state.drop_on_home(config.open_paths)
    {
        tracing::error!(error = %err, "could not open the paths given on the command line");
        eprintln!("aria could not open what you asked for: {err}");
    }
    // The hardware decode device opens while the window builds, so the first
    // clip's decoder finds it open. It starts once the command-line media is
    // probed: loading the device plugin holds GStreamer's plugin loader for
    // about 1.5 s, and a probe that loads a demuxer meanwhile waits for it.
    #[cfg(target_os = "linux")]
    aria_decoder::warm_up();
    // The session resolved the profile root (a relative `ARIA_DATA_DIR` is
    // resolved against the working directory), so the settings the first frame
    // reads come from the directory the session is actually using.
    let data_dir = state.data_dir.clone();
    let app = windowed_application();
    app.run(move |cx: &mut App| {
        register_fonts(cx);
        let saved = settings::UiSettings::load(&data_dir);
        // Before the first window: the splash and the app-open sequence are
        // themselves motion, so a reduced-motion choice has to be installed
        // before anything animates, not after the first frame.
        saved.motion.apply(cx);
        motion::seed_scale_from_env(cx);
        let theme_file = saved.theme_file.as_deref().and_then(|stem| {
            let found = state.theme_files.find(stem).cloned();
            if found.is_none() {
                tracing::warn!(
                    theme = stem,
                    "saved theme not applied: it is not in the data dir or a mounted plugin, or it failed to read (Settings lists the reason)"
                );
            }
            found
        });
        appearance::init(saved.appearance, theme_file, data_dir.clone(), cx);
        open_main_window(state, cx);
        cx.activate(true);
    });
    // A process that decoded on NVDEC must not end through `exit` (see
    // `exit_process`); a device warm-up still running ends with the process.
    aria_decoder::exit_process(0)
}

fn open_main_window(state: app::AppState, cx: &mut App) {
    let displays_vec = cx.displays();
    let display_rects: Vec<[f32; 4]> = displays_vec
        .iter()
        .map(|d| {
            let b = d.bounds();
            [
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ]
        })
        .collect();
    let placement = state.ui_settings.resolve_window_placement(&display_rects);
    let display_id = displays_vec.get(placement.display_index).map(|d| d.id());
    let bounds = if display_rects.is_empty() {
        Bounds::centered(
            display_id,
            size(px(placement.width), px(placement.height)),
            cx,
        )
    } else {
        Bounds::new(
            gpui::point(px(placement.x), px(placement.y)),
            size(px(placement.width), px(placement.height)),
        )
    };
    let window_bounds = if placement.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    let theme = Theme::of(cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(window_bounds),
            display_id,
            window_min_size: Some(size(
                px(crate::settings::MIN_WINDOW_WIDTH),
                px(crate::settings::MIN_WINDOW_HEIGHT),
            )),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(gpui::point(px(14.), px(14.))),
            }),
            window_decorations: cfg!(target_os = "linux")
                .then_some(gpui::WindowDecorations::Client),
            window_background: theme.window_background_appearance(),
            app_id: Some("aria".into()),
            ..Default::default()
        },
        move |window, cx| {
            appearance::observe_window(window, cx).detach();
            let state = cx.new(|_cx| state);
            // The state has no context of its own, so this is where the task
            // that opens native dialogs is installed: every pick a row, a chord
            // or a button asks for is answered through it.
            app::picker::watch_picks(&state, cx);
            // The window manager's close request leaves the session too, so it
            // goes through the unsaved prompt: a clean session closes at once,
            // a dirty one stays open until the prompt is answered.
            session_end::close_on_request(window, &state, cx);
            // After the state entity exists and before the first frame: the
            // platform bar is built from the mounted registry, and on Linux and
            // Windows this is a no-op because the CSD bar draws the same rows.
            chrome::native_menus::install(&state, cx);
            // `App::activate` is a no-op on Linux, so the window requests the
            // input focus itself: through the window manager's focus policy
            // when one runs, directly when none does. A key pressed before the
            // first click then reaches the editor.
            window.activate_window();
            cx.new(|cx| shell::Shell::new(state, cx))
        },
    )
    .unwrap_or_else(|reason| window_failed(reason));
    appearance::reapply_window_background(cx);
}
