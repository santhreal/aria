//! End-to-end integration tests for `aria-plugin-wasm`.
//!
//! Defect class closed: WIT host/guest contract drift, capability/permission
//! bypasses (including unscoped `fs-write` outside an active encode target),
//! fuel/memory/deadline exhaustion hanging the host, and guest traps leaving
//! half-registered contributions mounted instead of failing the plugin.
//!
//! What this does not catch: native C-ABI shared library loading (`aria-plugin`)
//! or GPU shader execution (`aria-compositor`).
//!
//! Exercises the compiled reference WASM component (`examples/wasm-plugin`) against a
//! real `PluginContext`, `ProjectHost`, `Engine`, `EditorView`, `SettingsService`, and
//! `ExportAudioMixer`. Because audio effect registration uses process-wide catalogs in
//! `aria-models` and `aria-render`, tests in this binary serialize on `SERIAL`.

mod audio;
mod commands;
mod effects;
mod interchange;
mod limits;
mod surfaces;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aria_core::{Rational, RationalTime};
use aria_engine::{Engine, EngineConfig};
use aria_models::ClipId;
use aria_plugin::package::Package;
use aria_plugin::{
    EditorView, PluginContext, PluginError, ProjectHost, Registration, Service, SettingsService,
    ViewFrame,
};
use parking_lot::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

const FIXTURE_WASM: &[u8] = include_bytes!("../fixtures/example_plugin.wasm");
const MARKER_ICON_SVG: &str = include_str!("../../../../examples/wasm-plugin/icons/marker.svg");

const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

struct TestContext {
    ctx: PluginContext,
    _services: Vec<Registration>,
}

impl std::ops::Deref for TestContext {
    type Target = PluginContext;

    fn deref(&self) -> &Self::Target {
        &self.ctx
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        if self.ctx.is_mounted("com.example.wasm-tools") {
            let _ = self.ctx.unmount("com.example.wasm-tools");
        }
    }
}

struct TestSettings {
    values: Mutex<HashMap<String, serde_json::Value>>,
}

impl TestSettings {
    fn new() -> Self {
        Self {
            values: Mutex::new(HashMap::new()),
        }
    }
}

impl Service for TestSettings {}

impl SettingsService for TestSettings {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.values.lock().get(key).cloned()
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        self.values.lock().insert(key.to_owned(), value);
    }

    fn save(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

fn manifest_toml(project_perm: &str, fs_scopes: &[&str]) -> String {
    let fs_list = fs_scopes
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"[plugin]
id = "com.example.wasm-tools"
name = "Example WASM Tools"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Test WASM component package"
license = "Apache-2.0"

[permissions]
project = {project_perm:?}
fs = [{fs_list}]

[[commands]]
id = "example.add-marker"
title = "Add Example Marker"
icon = "icons/marker.svg"
enablement = "!playing"
"#
    )
}

fn write_package(dir: &Path, project_perm: &str, fs_scopes: &[&str], wasm_bytes: &[u8]) -> Package {
    fs::create_dir_all(dir).expect("create package dir");
    let icons_dir = dir.join("icons");
    fs::create_dir_all(&icons_dir).expect("create icons dir");
    fs::write(icons_dir.join("marker.svg"), MARKER_ICON_SVG).expect("write marker.svg");
    fs::write(
        dir.join("plugin.toml"),
        manifest_toml(project_perm, fs_scopes),
    )
    .expect("write plugin.toml");
    fs::write(dir.join("plugin.wasm"), wasm_bytes).expect("write plugin.wasm");
    Package::open(dir).expect("open valid wasm package")
}

fn setup_host_context() -> (
    TestContext,
    Arc<ProjectHost>,
    Arc<EditorView>,
    Arc<TestSettings>,
) {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let host_reg = ctx.register_arc(Arc::clone(&host));

    let view = Arc::new(EditorView::new());
    view.publish(
        ViewFrame {
            selection: &[ClipId::from_raw(7)],
            playhead: RationalTime::new(15, Rational::FPS_24),
            mark_in: Some(RationalTime::new(0, Rational::FPS_24)),
            mark_out: Some(RationalTime::new(90, Rational::FPS_24)),
            playing: false,
            speed: 1,
        },
        ctx.events(),
    );
    let view_reg = ctx.register_arc(Arc::clone(&view));

    let settings = Arc::new(TestSettings::new());
    let settings_dyn: Arc<dyn SettingsService> = settings.clone();
    let settings_reg = ctx.register_arc(settings_dyn);

    (
        TestContext {
            ctx,
            _services: vec![host_reg, view_reg, settings_reg],
        },
        host,
        view,
        settings,
    )
}
