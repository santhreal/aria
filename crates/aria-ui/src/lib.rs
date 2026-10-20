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
pub mod fluent;
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

/// Everything the headed binary passes in.
pub struct UiConfig {
    pub data_dir: PathBuf,
    pub ctx: PluginContext,
    /// Paths named on the command line, in the order they were given.
    pub open_paths: Vec<PathBuf>,
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
            aria_decoder::exit_process(1);
        }
    };
    // Paths from the command line are the same question a drop on Home asks, so
    // they go through the same owner: a project file opens, media is imported and
    // laid end to end in a fresh session, a LUT or a skill installs. Answered
    // before the window so the operator's first frame is the project they named.
    if !config.open_paths.is_empty()
        && let Err(err) = state.drop_on_home(config.open_paths)
    {
        tracing::error!(error = %err, "could not open the paths given on the command line");
        eprintln!("aria could not open what you asked for: {err}");
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
            let closing = state.clone();
            window.on_window_should_close(cx, move |win, cx| {
                let wb = win.window_bounds();
                let inner = wb.get_bounds();
                let maximized =
                    matches!(wb, WindowBounds::Maximized(_) | WindowBounds::Fullscreen(_));
                let disp_rects: Vec<[f32; 4]> = cx
                    .displays()
                    .into_iter()
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
                let cx_x = f32::from(inner.origin.x) + f32::from(inner.size.width) * 0.5;
                let cx_y = f32::from(inner.origin.y) + f32::from(inner.size.height) * 0.5;
                let display_index = disp_rects
                    .iter()
                    .position(|&[dx, dy, dw, dh]| {
                        cx_x >= dx && cx_x <= dx + dw && cx_y >= dy && cx_y <= dy + dh
                    })
                    .unwrap_or(0);
                closing.update(cx, |state, cx| {
                    state.ui_settings.record_window_placement(
                        &disp_rects,
                        settings::WindowPlacement {
                            x: f32::from(inner.origin.x),
                            y: f32::from(inner.origin.y),
                            width: f32::from(inner.size.width),
                            height: f32::from(inner.size.height),
                            maximized,
                            display_index,
                        },
                    );
                    // The session is ending, so a status line would never be
                    // read: the failure goes to the log with the path to fix.
                    if let Err(err) = state.ui_settings.save(&state.data_dir) {
                        tracing::warn!(
                            dir = %state.data_dir.display(),
                            "window placement not saved for the next launch: {err}"
                        );
                    }
                    if !state.quit_requested() {
                        state.request_quit();
                        cx.notify();
                    }
                    state.quit_requested()
                })
            });
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
    .expect("failed to open window");
    appearance::reapply_window_background(cx);
}
