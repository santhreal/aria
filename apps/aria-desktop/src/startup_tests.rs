//! What a start costs, measured on the path `main` takes.
//!
//! The boot cover used to be a fixed 1100 ms wall, which is what a start whose
//! cost nobody had measured looks like: fast hosts waited for the wall and slow
//! ones were uncovered before the editor was ready. The cover ends on readiness
//! now, with [`SPLASH_MAX`] only as a ceiling, so the number that matters is how
//! long the two phases below take — if they run past the ceiling, the operator is
//! looking at a splash that the timeout ended rather than at a ready editor.
//!
//! The two phases are exactly what `main` runs before the window opens: the
//! plugin mount, which reads every catalog and registers every panel, command
//! and key, and the session, which builds the engine and the first projection
//! the frame reads. Nothing here opens a window: there is no headless
//! rasterizer on this platform, and a test that opened one would put a window in
//! front of whoever is running the gate.
//!
//! Opening a project from the command line is deliberately not measured. That
//! path waits for a first decoded picture, so its cost is a codec's and a
//! device's, not the editor's, and on a host with no GPU it is the ceiling by
//! design.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aria_ui::shell::SPLASH_MAX;

/// One start, phase by phase.
struct Start {
    mount: Duration,
    session: Duration,
    /// Panels the mount contributed. A frame draws these, so a start that
    /// registered none is not an interactive frame however fast it was.
    panels: usize,
    /// A value only a used profile carries. This is what separates a cold start
    /// from a warm one: the session read a settings file, or there was none.
    sidebar_width: f32,
    /// Every mounted plugin and what it cost, dearest first. The mount is nearly
    /// the whole start, so a number for the start without this is a number with
    /// nothing to act on.
    per_plugin: Vec<(&'static str, Duration)>,
}

impl Start {
    fn total(&self) -> Duration {
        self.mount + self.session
    }

    /// The dearest few mounts, as one line.
    fn worst(&self) -> String {
        self.per_plugin
            .iter()
            .take(4)
            .map(|(id, cost)| format!("{id} {:.1} ms", cost.as_secs_f32() * 1_000.0))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Run the two phases against `data_dir` and time each.
fn start_in(data_dir: &Path) -> Start {
    let ctx = aria_plugin::PluginContext::new();

    let at = Instant::now();
    // Failures are not an error here for the same reason they are not in `main`:
    // a host with no GPU starts with the plugins that did mount, and the start is
    // still the start.
    let _ = aria_ui::plugins::mount_all(&ctx, data_dir);
    let mount = at.elapsed();

    let panels = aria_ui::plugins::panel_host::host_of(&ctx)
        .map(|host| host.ids().len())
        .unwrap_or_default();
    let mut per_plugin: Vec<(&'static str, Duration)> = ctx
        .mounted()
        .into_iter()
        .map(|info| (info.manifest.id, info.mount))
        .collect();
    per_plugin.sort_by(|left, right| right.1.cmp(&left.1));

    let at = Instant::now();
    let state =
        aria_ui::app::AppState::new(ctx, data_dir.to_path_buf()).expect("the session was built");
    let session = at.elapsed();

    Start {
        mount,
        session,
        panels,
        sidebar_width: state.ui_settings.sidebar_width,
        per_plugin,
    }
}

/// A sidebar width no default carries, so reading it back proves the settings
/// file was read rather than defaulted.
const WARM_SIDEBAR: f32 = 321.0;

/// One directory, named and removed, so a gate run leaves nothing behind.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join("aria-desktop-startup");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch profile");
    dir
}

/// **A start fits inside the cover's ceiling, cold and warm.**
///
/// Cold is an empty profile: every catalog is read from the shipped data files,
/// there are no settings and no recents. Warm is the same directory after a
/// session has used it, so the settings file and the recents list are both there
/// to be read and the second start does strictly more work than the first.
///
/// The assertion is the ceiling, not a hand-picked millisecond count: a machine
/// under load is allowed to be slow, but a start that runs past [`SPLASH_MAX`]
/// means the cover was ended by its timeout, which is the state this row exists
/// to keep the editor out of. The measured numbers are printed either way, and
/// recorded in `docs/performance.md`.
#[test]
fn first_interactive_frame_is_measured_cold_and_warm() {
    let dir = scratch();

    let cold = start_in(&dir);
    assert!(
        cold.panels > 0,
        "a start that contributed no panel has no interactive frame to draw"
    );
    assert_ne!(
        cold.sidebar_width, WARM_SIDEBAR,
        "an empty profile cannot already carry the width the warm start writes"
    );

    // Leave the directory in the state a used profile is in: a settings file and a
    // project in the recents list, both of which the next start reads.
    {
        let ctx = aria_plugin::PluginContext::new();
        let _ = aria_ui::plugins::mount_all(&ctx, &dir);
        let mut state = aria_ui::app::AppState::new(ctx, dir.clone()).expect("a session to use");
        state.ui_settings.sidebar_width = WARM_SIDEBAR;
        state.ui_settings.save(&dir).expect("settings written");
        state.new_project().expect("a project to save");
        state.project_path = Some(dir.join("warm.aria"));
        state.save_project().expect("project written");
    }

    let warm = start_in(&dir);
    assert_eq!(
        warm.sidebar_width, WARM_SIDEBAR,
        "the warm start did not read the profile it was pointed at"
    );
    assert!(
        dir.join("recents.json").is_file(),
        "the warm start had no recents file to read, so it did the cold start's work"
    );
    assert_eq!(
        warm.panels, cold.panels,
        "the same mount contributed a different number of panels"
    );

    println!(
        "cold: mount {:?} + session {:?} = {:?}\n\
         warm: mount {:?} + session {:?} = {:?}\n\
         ceiling {:?}, {} panels\n\
         cold dearest: {}\n\
         warm dearest: {}",
        cold.mount,
        cold.session,
        cold.total(),
        warm.mount,
        warm.session,
        warm.total(),
        SPLASH_MAX,
        cold.panels,
        cold.worst(),
        warm.worst()
    );

    // The mount is where a start is spent, so a start whose plugins account for
    // almost none of it is being measured somewhere this test cannot see, and the
    // breakdown printed above would be useless for acting on.
    let attributed: Duration = cold.per_plugin.iter().map(|(_, cost)| *cost).sum();
    assert!(
        attributed * 2 > cold.mount,
        "the plugins account for {attributed:?} of a {:?} mount: the cost has moved somewhere \
         this measurement does not attribute",
        cold.mount
    );

    for (label, start) in [("cold", &cold), ("warm", &warm)] {
        assert!(
            start.total() < SPLASH_MAX,
            "{label} start took {:?} (mount {:?}, session {:?}), past the {SPLASH_MAX:?} the boot \
             cover waits: the operator sees a splash the timeout ended, not a ready editor",
            start.total(),
            start.mount,
            start.session
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
