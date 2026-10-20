//! Native C ABI scopes. An `aria-sdk` plugin registers a scope with
//! `Host::register_scope`, and the host lists it and measures frames through
//! `scope_measure`: the frame reaches the plugin, the drawing and the readout
//! come back at the registered size, a refusal is returned as written, a
//! panic fails the plugin and takes its scopes off the strip, and a plugin
//! table without the `scope_measure` entry cannot register a scope.
//!
//! Not covered: a plugin built as a separate `cdylib`; the in-process table
//! has the same layout `aria_sdk::export!` builds.

#![allow(unsafe_code)]

use std::sync::LazyLock;

use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin::{PluginContext, ScopeError, ScopeFrame, ScopeImage};
use aria_plugin_abi::{ARIA_API_MAJOR, ARIA_PLUGIN_V1_AI_TOOLS_SIZE};
use aria_sdk::{Host, Plugin as SdkPlugin, PluginRuntime};
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

/// Offers `native.peak`, a 3x1 meter that lights one pixel per frame pixel
/// whose red is over half, and `native.boom`, which panics.
struct Guest;

impl SdkPlugin for Guest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_scope(&aria_sdk::ScopeSpec {
            id: "native.peak".to_owned(),
            name: "Peaks".to_owned(),
            width: 3,
            height: 1,
        })?;
        host.register_scope(&aria_sdk::ScopeSpec {
            id: "native.boom".to_owned(),
            name: "Boom".to_owned(),
            width: 1,
            height: 1,
        })
    }

    fn scope_measure(
        &self,
        _host: &Host,
        id: &str,
        frame: &aria_sdk::ScopeFrame<'_>,
        image: &mut aria_sdk::ScopeImage<'_>,
    ) -> Result<String, String> {
        match id {
            "native.peak" => {
                if frame.width < 2 {
                    return Err("needs two columns".to_owned());
                }
                let mut lit = 0;
                for px in frame.pixels.as_chunks::<4>().0 {
                    if px[0] > 127 {
                        image.put(lit, 0, [255, 255, 255, 255]);
                        lit += 1;
                    }
                }
                Ok(format!("{lit} hot"))
            }
            "native.boom" => panic!("boom"),
            other => Err(format!("no scope {other}")),
        }
    }
}

fn bound() -> PluginRuntime<Guest> {
    let runtime = PluginRuntime::new(Guest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

/// The plugin `table` as package `id`, loaded on a fresh context.
fn loaded(
    temp: &TempDir,
    id: &str,
    table: &aria_plugin_abi::AriaPluginV1,
) -> (PluginContext, Box<dyn aria_plugin::Plugin>) {
    let dir = temp.path().join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Native Scopes"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI scope test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    let pkg = Package::open(&dir).expect("open package");
    let plugin = from_table(&pkg, *table, Some(temp.path())).expect("from_table");
    (PluginContext::new(), plugin)
}

/// A frame of one row whose pixels have the given red values.
fn row(reds: &[u8]) -> Vec<u8> {
    reds.iter().flat_map(|r| [*r, 0, 0, 255]).collect()
}

fn measure(ctx: &PluginContext, id: &str, pixels: &[u8]) -> Result<ScopeImage, ScopeError> {
    let width = u32::try_from(pixels.len() / 4).expect("a short row");
    let frame = ScopeFrame::new(width, 1, pixels).expect("a one-row frame");
    let mut image = ScopeImage::default();
    ctx.measure_scope(id, &frame, &mut image).map(|()| image)
}

#[test]
fn a_native_plugin_draws_its_scope_at_the_registered_size() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(bound);
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativescopes.full", &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount");

    let listed: Vec<_> = ctx
        .scopes()
        .iter()
        .map(|scope| {
            (
                scope.spec.id.clone(),
                scope.spec.name.clone(),
                scope.spec.width,
                scope.spec.height,
                scope.plugin_id.to_string(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                "native.peak".to_owned(),
                "Peaks".to_owned(),
                3,
                1,
                "test.nativescopes.full".to_owned()
            ),
            (
                "native.boom".to_owned(),
                "Boom".to_owned(),
                1,
                1,
                "test.nativescopes.full".to_owned()
            ),
        ]
    );

    let image = measure(&ctx, "native.peak", &row(&[200, 10, 255, 0])).expect("measures");
    assert_eq!((image.width(), image.height()), (3, 1));
    assert_eq!(
        image.pixels(),
        [255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0]
    );
    assert_eq!(image.readout(), "2 hot");

    assert_eq!(
        measure(&ctx, "native.peak", &row(&[200])).map(|image| image.readout().to_owned()),
        Err(ScopeError::Failed("needs two columns".to_owned()))
    );
    let cost = ctx.cost_of("test.nativescopes.full").expect("mounted");
    assert_eq!(cost.frame.calls, 2, "{cost:?}");
    assert!(ctx.failures().is_empty(), "{:?}", ctx.failures());
}

#[test]
fn a_panicking_native_scope_fails_its_plugin_and_leaves_the_strip() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(bound);
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativescopes.boom", &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount");

    let err = measure(&ctx, "native.boom", &row(&[1, 2])).expect_err("a panic fails");
    assert_eq!(
        err,
        ScopeError::Failed("scope native.boom panicked: boom".to_owned())
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("native.boom panicked: boom"),
        "{}",
        failures[0].reason
    );
    assert!(
        ctx.scopes().is_empty(),
        "a failed plugin's scopes are listed"
    );
    assert_eq!(
        measure(&ctx, "native.peak", &row(&[200, 200])).map(drop),
        Err(ScopeError::Unknown("native.peak".to_owned()))
    );
}

#[test]
fn a_plugin_table_without_the_scope_entry_cannot_register_a_scope() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(bound);
    let mut table = plugin_table!(RT);
    table.struct_size = ARIA_PLUGIN_V1_AI_TOOLS_SIZE;
    assert!(table.has_ai_tools() && !table.has_scopes());
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativescopes.short", &table);

    let err = ctx
        .mount(plugin.as_ref())
        .expect_err("register_scope is refused")
        .to_string();
    assert!(err.contains("scope_measure"), "{err}");
    assert!(ctx.scopes().is_empty());
}
