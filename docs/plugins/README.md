# Plugin development

Aria's capabilities are mounted as plugins on a `PluginContext`: a typed
service registry plus an event bus. Built-in plugins are defined in
`crates/aria-plugin`; those that take GPU types are mounted from
`crates/aria-ui/src/plugins/` so engine crates stay cycle-free. See
[spec/plugins.md](../spec/plugins.md) for the target host specification and
[spec/status.md](../spec/status.md#plugins-pluginsmd) for current differences.

## Chapters

- [Plugin basics](basics.md)
- [Packages](packages.md)
- [Activation](activation.md)
- [Effects, effect graphs, plugin textures, and transitions](effects.md)
- [Audio effects](audio-effects.md)
- [Audio analyzers](analyzers.md)
- [Presets](presets.md)
- [Driver languages](drivers.md)
- [Importers, exporters, encoders, and decoders](interchange.md)
- [Events (`CommandProposed`, `ResolveDropped`, notifications, context menus)](events.md)
- [Project, queries, frames, and sound](project.md)
- [Panels and dialogs](panels.md)
- [Viewer overlays](viewer-overlays.md)
- [Scopes](scopes.md)
- [Workspaces](workspaces.md)
- [Clip types, with fields and renderers of their own](clip-types.md)
- [Commands](commands.md)
- [Jobs](jobs.md)
- [AI tools](ai-tools.md)
- [Menus](menus.md)
- [Keys](keys.md)
- [Settings](settings.md)
- [Native plugins](native.md)
- [WebAssembly plugins](wasm.md)
- [Worked example](worked-example.md)
