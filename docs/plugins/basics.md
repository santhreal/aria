# Plugin basics

## Plugin trait

```rust
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub requires: &'static [&'static str],
    pub uses: &'static [&'static str],
}

pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError>;
    fn unmount(&self, _ctx: &PluginContext) {}
}
```

`ctx.mount(plugin)` refuses a plugin whose `requires` ids are not already
mounted. Services registered during `mount` stay alive until `unmount`.

A failed mount is recorded as well as returned. `ctx.failures()` lists every
plugin that did not mount and the reason it gave; a later successful mount of the
same id clears its entry. `ctx.mounted()` lists what did mount, each with the
service type names it registered. Settings shows both.

`ctx.unmount(id)` drops that plugin's registrations, so its services are gone from
the registry. A surface that needs one asks `try_get` and hides itself when the
answer is `None`.

## Load order

`aria_plugin::load_order(&manifests)` returns the order to mount in, derived from
`requires`. Plugins that do not depend on each other keep the order they were
listed in, because mount order is handler order. A cycle is refused and the error
names every plugin in it. A dependency outside the set is left to `mount`, which
names it.

## Mount order

`aria_ui::plugins::builtin(data_dir)` is the built-in list, in mount order:

`aria.storage`, `aria.settings`, `aria.decoder`, `aria.encoder`,
`aria.compositor`, `aria.render`, `aria.effects`, `aria.transitions`,
`aria.engine`, `aria.interchange`, `aria.cloud`, `aria.analysis`, `aria.jobs`,
`aria.transcription`, `aria.ai`, `aria.ui`.

`mount_all` mounts every one of them and returns the failures instead of stopping
at the first, so a host without a GPU still starts. `builtin_by_id` returns one
plugin for re-mounting after it was disabled.

`mount_all` takes the data directory to read external plugins and catalog
overrides from. Pass the same directory the session uses, so a process pointed at
another profile does not load code and catalogs from the default one.

That directory reaches exactly one plugin. `aria.storage` owns the root and
registers a `StorageLayout` and a `StorageService` for it; a plugin that needs a
directory asks that service instead of resolving one. `aria.transcription`
refuses to mount without it rather than downloading models into a directory the
session never reads.

## Capabilities

`uses` is the list of services a plugin reads, by short name:

```rust
uses: &["StorageService", "SettingsService"],
```

`mount` hands the plugin a context scoped to that list. `try_get` of anything else
returns `None` and `get` panics, whatever is in the registry — a plugin cannot
reach a capability it did not declare. The refusal is recorded, not logged:
`ctx.denials()` names the plugin and the service, and the Plugins list shows it as
`refused <Service>`. A clone of the scoped context keeps the scope, so a handler
registered during `mount` is bound by the same list. Registration is never scoped:
providing a capability is not consuming one.

A plugin that only registers services and handlers declares `uses: &[]`.

## Services

Register a value with `ctx.register(service)` or `ctx.register_arc(arc)`.
Look it up with `ctx.get::<T>()` or `ctx.try_get::<T>()`.

Service traits in `aria_plugin::service`:

| Trait | Role |
| --- | --- |
| `DecoderService` | Open a `VideoDecoder` for a path |
| `EncoderService` | Create a `VideoEncoder` from `EncoderConfig` |
| `EffectService` | Effect catalog. `render_effect` draws one catalog effect over an image on the GPU; see [Effects and transitions](effects.md) |
| `TransitionService` | Transition catalog. `render_transition` blends two images of one size at a progress in 0..1 |
| `StorageService` | Cache and project directories |
| `SettingsService` | `~/.aria/config.toml` get / set / save |
| `UiPanelService` | Panel metadata; see [Panels](panels.md#panels) for the host that draws it |

A service holds one value per type. Anything several plugins must see in order is
an event instead.

## Logging

Log a line at a level with the call of the plugin's tier:

| Tier | Call |
| --- | --- |
| In-process | `ctx.log(LogLevel::Warn, "cache is cold")` on the context `mount` received or a clone of it |
| WASM component | `log(log-level, text)`, a host import |
| Native library | `aria_sdk::Host::log(LogLevel, &str)` |

Levels are `trace`, `debug`, `info`, `warn`, and `error`. A host that runs a
plugin's code logs for it with `ctx.log_for(id, level, text)`.

Settings > Plugins keeps the newest 256 lines of each plugin for the session.
A line longer than 4096 bytes is cut on a character boundary and ends with
` [truncated]`. Each line is also sent to `tracing` with the plugin id. The
host logs a failed mount and a failure after mount at `error`, with the
backtrace from the panicking frame when a panic caused it, and a recovered
fault at `warn`.

A row with lines offers Log, which shows them oldest first and coloured by
level, and Copy, which puts them on the clipboard as `level text` lines. The
log of a plugin with an error or a fault opens by default. Lines outlive an
unmount, a failure, and a reload; uninstalling the package drops them.
`ctx.plugin_logs(id)` reads them and `ctx.logs_revision()` moves with each
line.
