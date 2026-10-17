//! The editor's activation triggers: what loads a waiting package.
//!
//! Closes the class "a trigger the host owns never fires, fires after the
//! code that needed the package, or fires under the engine lock": each case
//! defers a package on the session's own context and drives the call a user
//! action reaches (a drop, an edit, a settings read and write), then asserts
//! how many times the package loaded and what it saw. It does not cover the
//! native file dialog, which hands its paths to the same calls, or opening a
//! waiting panel, which `plugin_panels_tests` covers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aria_plugin::package::{COMPONENT_FILE, MANIFEST_FILE, Package};
use aria_plugin::{
    Importer, ImporterSpec, InterchangeReport, PluginContext, Registration, SettingsService,
    TransactError, Transaction,
};
use parking_lot::Mutex;

use super::AppState;
use super::premiere_tests::with_clip;

/// The code package `id`, named `name`, with `rest` after `[plugin]`, in a
/// scratch folder of its own.
pub(super) fn lazy_package(id: &str, name: &str, rest: &str) -> Package {
    let dir = crate::scratch_pool::scratch(&format!("lazy-{id}"));
    std::fs::write(
        dir.join(MANIFEST_FILE),
        format!("[plugin]\nid = {id:?}\nname = {name:?}\nversion = \"1.0.0\"\napi = 1\n{rest}"),
    )
    .expect("write the manifest");
    std::fs::write(dir.join(COMPONENT_FILE), "\0asm").expect("write the component");
    Package::open(&dir).expect("the package reads")
}

/// How often a deferred package loaded, and what its loads registered.
#[derive(Default)]
struct Loads {
    count: AtomicUsize,
    held: Mutex<Vec<Registration>>,
}

impl Loads {
    fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

/// Defer `package` on the session's context. A load counts itself, then runs
/// `load`, whose registrations stay held for the test.
fn defer(
    state: &AppState,
    package: &Package,
    load: impl Fn(&PluginContext) -> Result<Option<Registration>, String> + Send + Sync + 'static,
) -> Arc<Loads> {
    let loads = Arc::new(Loads::default());
    let seen = Arc::clone(&loads);
    state
        .ctx
        .defer_package(package, move |ctx| {
            seen.count.fetch_add(1, Ordering::SeqCst);
            if let Some(registration) = load(ctx)? {
                seen.held.lock().push(registration);
            }
            Ok(())
        })
        .expect("the package defers");
    loads
}

/// Reads any file and records which it read.
struct Recorder(Arc<Mutex<Vec<PathBuf>>>);

impl Importer for Recorder {
    fn import(
        &self,
        file: &Path,
        _: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        self.0.lock().push(file.to_path_buf());
        Ok(InterchangeReport::default())
    }
}

/// A drop loads the package waiting for its extension before the drop is
/// classified, so the importer the package registers reads the file. A drop
/// of another extension loads nothing.
#[test]
fn a_dropped_file_loads_its_package_before_it_is_classified() {
    let (mut state, _) = with_clip();
    let package = lazy_package(
        "com.test.reader",
        "Reader",
        "[activation]\non = [\"file:lazy\"]\n",
    );
    let read: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let reads = Arc::clone(&read);
    let loads = defer(&state, &package, move |ctx| {
        let spec = ImporterSpec {
            id: "com.test.reader.lazy".to_owned(),
            name: "Lazy".to_owned(),
            extensions: vec!["lazy".to_owned()],
        };
        ctx.register_importer(spec, Recorder(Arc::clone(&reads)))
            .map(Some)
            .map_err(|err| err.to_string())
    });
    let dir = crate::scratch_pool::scratch("lazy-drops");
    let other = dir.join("cut.unread");
    let take = dir.join("take.LAZY");
    std::fs::write(&other, "x").expect("write");
    std::fs::write(&take, "x").expect("write");

    let _ = state.handle_dropped_paths(vec![other]);
    assert_eq!(loads.count(), 0, "another extension loaded the package");
    assert!(state.ctx.is_waiting("com.test.reader"));

    state
        .handle_dropped_paths(vec![take.clone()])
        .expect("the drop is read");
    assert_eq!(loads.count(), 1);
    assert_eq!(*read.lock(), vec![take.clone()]);
    assert!(!state.ctx.is_waiting("com.test.reader"));

    let _ = state.handle_dropped_paths(vec![take]);
    assert_eq!(loads.count(), 1, "a loaded package loaded again");
    assert_eq!(read.lock().len(), 2);
}

/// An edit that makes the project use an effect a package waits for loads the
/// package once, with the engine lock released, before the store is built.
#[test]
fn an_edit_that_uses_a_waiting_effect_loads_its_package_without_the_engine_lock() {
    let (mut state, _) = with_clip();
    let package = lazy_package(
        "com.test.looks",
        "Looks",
        "[activation]\non = [\"effect:vignette\"]\n",
    );
    let engine = Arc::clone(state.engine());
    let unlocked = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&unlocked);
    let loads = defer(&state, &package, move |_| {
        seen.store(engine.try_lock().is_some(), Ordering::SeqCst);
        Ok(None)
    });
    state.sync_from_engine();
    assert_eq!(loads.count(), 0, "a project without the effect loaded it");

    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    assert_eq!(loads.count(), 0, "another effect loaded it");

    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");
    assert_eq!(loads.count(), 1);
    assert!(
        unlocked.load(Ordering::SeqCst),
        "the package loaded under the engine lock"
    );
    assert!(!state.ctx.is_waiting("com.test.looks"));

    state.undo();
    state.redo();
    assert_eq!(loads.count(), 1, "a loaded package loaded again");
}

/// Showing a waiting package's setting does not load it; a read through the
/// settings service and a save from Settings do.
#[test]
fn a_setting_read_or_saved_loads_its_package_and_showing_it_does_not() {
    let dir = crate::scratch_pool::scratch("lazy-settings");
    let ctx = PluginContext::new();
    ctx.mount(&crate::plugins::SettingsPlugin {
        path: dir.join("settings.toml"),
    })
    .expect("the settings plugin mounts");
    let mut state = AppState::new(ctx, dir).expect("state");
    let declares = "[activation]\non = [\"setting:mode\"]\n\n\
                    [[settings]]\nkey = \"mode\"\nlabel = \"Mode\"\n";
    let read = defer(
        &state,
        &lazy_package("com.test.read", "Read", declares),
        |_| Ok(None),
    );
    let saved = defer(
        &state,
        &lazy_package("com.test.saved", "Saved", declares),
        |_| Ok(None),
    );

    assert_eq!(state.plugin_setting("com.test.read.mode"), "");
    assert_eq!(read.count(), 0, "showing the value loaded the package");

    let settings = state.ctx.get::<dyn SettingsService>();
    let _ = settings.get("com.test.read.mode");
    assert_eq!(read.count(), 1);
    assert_eq!(saved.count(), 0, "a read of another key loaded it");

    state
        .set_plugin_setting("com.test.saved.mode", "wide")
        .expect("saves");
    assert_eq!(saved.count(), 1);
    assert_eq!(state.plugin_setting("com.test.saved.mode"), "wide");
    let _ = settings.get("com.test.saved.mode");
    assert_eq!(saved.count(), 1, "a loaded package loaded again");
}
