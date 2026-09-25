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
pub mod datefmt;
pub mod drag_hint;
pub mod edge_fade;
pub mod filmstrips;
pub mod fluent;
pub mod frame;
pub mod frost;
pub mod handoff;
pub mod icons;
pub mod interaction;
pub mod keys;
pub mod layouts;
pub mod loaders;
pub mod motion;
pub mod os_drop;
pub mod panels;
pub mod pending;
pub mod plugins;
pub mod popover;
pub mod projection;
pub mod scopes;
pub mod settings;
pub mod shell;
pub mod snap;
pub mod surfaces;
pub mod theme;
pub mod timecode;
pub mod tool_catalog;
pub mod tooltip;
pub mod transport;
pub mod types;
pub mod visual_diff;
pub mod waveforms;
pub mod workload;

#[cfg(test)]
mod scratch_pool;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "types_census.rs"]
mod types_census;
#[cfg(test)]
#[path = "workload_budget_tests.rs"]
mod workload_budget_tests;

pub use appearance::AppearanceMode;
pub use projection::project_to_ui;
pub use theme::{Appearance, Theme};
pub use types::{ActivePanel, UiEditorStore};

/// Embedded UI fonts — Geist and Geist Mono (variable), © Vercel Inc.,
/// licensed under the SIL Open Font License 1.1 (https://openfontlicense.org).
static FONT_GEIST: &[u8] = include_bytes!("../assets/fonts/Geist.ttf");
static FONT_GEIST_MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono.ttf");
static FONT_GEIST_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Geist-Medium.ttf");
static FONT_GEIST_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Geist-SemiBold.ttf");
static FONT_GEIST_BOLD: &[u8] = include_bytes!("../assets/fonts/Geist-Bold.ttf");

pub fn register_fonts(cx: &App) {
    if let Err(err) = cx.text_system().add_fonts(vec![
        Cow::Borrowed(FONT_GEIST),
        Cow::Borrowed(FONT_GEIST_MONO),
        Cow::Borrowed(FONT_GEIST_MEDIUM),
        Cow::Borrowed(FONT_GEIST_SEMIBOLD),
        Cow::Borrowed(FONT_GEIST_BOLD),
    ]) {
        tracing::warn!(error = %err, "failed to register embedded Geist fonts");
    }
}

/// Everything the headed binary passes in.
pub struct UiConfig {
    pub data_dir: PathBuf,
    pub ctx: PluginContext,
    /// Paths named on the command line, in the order they were given.
    pub open_paths: Vec<PathBuf>,
}

/// Run the headed app on the default data directory, opening nothing.
pub fn run_app(ctx: PluginContext) {
    run_app_with(UiConfig {
        data_dir: settings::default_data_dir(),
        ctx,
        open_paths: Vec::new(),
    });
}

pub fn run_app_with(config: UiConfig) {
    // This process is the editor, in front of the operator, so a reveal may open
    // their file manager. Nothing else in the crate switches this on: a test
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
            std::process::exit(1);
        }
    };
    // Paths from the command line are the same question a drop on Home asks, so
    // they go through the same owner: a project file opens, media is imported and
    // laid end to end in a fresh session, a LUT or a skill installs. Answered
    // before the window so the operator's first frame is the project they named.
    if !config.open_paths.is_empty() {
        if let Err(err) = state.drop_on_home(config.open_paths) {
            tracing::error!(error = %err, "could not open the paths given on the command line");
            eprintln!("aria could not open what you asked for: {err}");
        }
    }
    // The session resolved the profile root (a relative `ARIA_DATA_DIR` is
    // resolved against the working directory), so the settings the first frame
    // reads come from the directory the session is actually using.
    let data_dir = state.data_dir.clone();
    let app = gpui_platform::application().with_assets(icons::Assets);
    app.run(move |cx: &mut App| {
        register_fonts(cx);
        let saved = settings::UiSettings::load(&data_dir);
        // Before the first window: the splash and the app-open sequence are
        // themselves motion, so a reduced-motion choice has to be installed
        // before anything animates, not after the first frame.
        saved.motion.apply(cx);
        appearance::init(saved.appearance, data_dir.clone(), cx);
        open_main_window(state, cx);
        cx.activate(true);
    });
}

fn open_main_window(state: app::AppState, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
    let theme = Theme::of(cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(1024.), px(640.))),
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
            // After the state entity exists and before the first frame: the
            // platform bar is built from the mounted registry, and on Linux and
            // Windows this is a no-op because the CSD bar draws the same rows.
            chrome::native_menus::install(&state, cx);
            cx.new(|cx| shell::Shell::new(state, cx))
        },
    )
    .expect("failed to open window");
    appearance::reapply_window_background(cx);
}
