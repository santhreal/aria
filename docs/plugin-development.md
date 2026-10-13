# Plugin development

Aria's capabilities are mounted as plugins on a [`PluginContext`]: a typed
service registry plus an event bus. This guide describes the current host; the
target host is specified in [spec/plugins.md](spec/plugins.md), and
[spec/status.md](spec/status.md#plugins-pluginsmd) lists the differences.

Plugins live in `crates/aria-plugin`. Built-in plugins that need GPU types
are mounted from `crates/aria-ui/src/plugins/` so engine crates stay
cycle-free.

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
| `EffectService` | Effect catalog. `render_effect` returns an error: effects render only through the compositor's static pass table |
| `TransitionService` | Transition catalog. `render_transition` returns an error, as above |
| `GeneratorService` | Text, shapes, solids. No caller reads it |
| `RenderInterceptorService` | Registered with a pass-through value. No caller reads it |
| `StorageService` | Cache and project directories |
| `SettingsService` | `~/.aria/config.toml` get / set / save |
| `UiPanelService` | Panel metadata; see [Panels](#panels) for the host that draws it |

A service holds one value per type. Anything several plugins must see in order is
an event instead.

## Events

`ctx.events()` is an `EventBus`:

- `subscribe` / `emit` — every handler runs
- `waterfall` / `emit_waterfall` — a handler can rewrite the event or stop the chain
- `serial` — first non-`None` wins

Handlers return a `Registration` drop guard. Subscribe through
`ctx.on_event`, `ctx.on_waterfall`, or `ctx.on_serial` during `mount` and the
context keeps the guard for the life of the plugin, the same as a service. The
bus methods return the guard to the caller, so a bare `ctx.events().waterfall(…)`
in `mount` unsubscribes immediately.

Handlers run in registration order, which is mount order.

### Shipped events

`CommandProposed` is a waterfall emitted before every command reaches the engine:

| Field | Contents |
| --- | --- |
| `command` | The command; a handler can edit it in place |
| `project` | The project the command applies to, as an `Arc<Project>` |
| `origin` | `Origin::Editor`, `Origin::Assistant`, or `Origin::Plugin(id)` |

`proposed.refuse(reason)` stops the chain: the command is never applied, nothing
enters history, and the reason is shown as written. `EditCommand::targets()`
names the clips and tracks a command touches, so a policy handler does not match
on variants; `ignores_track_lock()` marks the commands a lock must not block. The
engine plugin uses both, with `proposed.project`, to refuse an edit on a locked
lane.

The engine is locked while the chain runs. A handler reads `proposed.project`,
or `ProjectHost::snapshot()`, which returns the same value there; a handler that
locks the engine deadlocks the edit. An edit cannot start inside a handler:
`transact` returns `TransactError::Reentrant`, and `apply`, `group`, `undo`,
`redo`, `history_jump`, and `exclusive` panic.

```rust
ctx.on_waterfall::<CommandProposed, _>(|proposed| {
    if let Command::Edit(edit) = &proposed.command {
        if edit.targets().tracks.contains(&guarded_track) {
            proposed.refuse("That lane is under review");
        }
    }
});
```

`CommandApplied` is emitted once a command can no longer be rolled back: when a
lone command applies, or when the outermost group that contains it commits. It
holds the command, its `ApplyOutcome`, the engine revision right after it, and
its origin. A group that rolls back emits nothing. Handlers run with no lock
held. The editor redraws on a `CommandApplied` whose origin is not
`Origin::Editor`.

`ResolveDroppedPath` is a serial event asking which plugin owns a dropped file.
Return `Some(DropAction)` to claim it — `OpenProject`, `ImportMedia`, `ApplyLut`,
or `InstallSkill`. The decoder claims media extensions, the effects plugin claims
`.cube`, and the AI plugin claims `SKILL.md`. An unclaimed path is imported as
media. The project extension is not up for election: the host opens a `.aria` file
whatever is mounted, so a plugin that fails to load cannot turn a project into a
media import.

```rust
ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
    event
        .path
        .extension()
        .is_some_and(|ext| ext == "srt")
        .then_some(DropAction::ImportMedia)
});
```

## Project

`ProjectHost` is the one path that reads and edits the project. The editor,
the assistant, and render jobs edit through it too. The engine plugin registers
it; a plugin declares `uses: &["ProjectHost"]` and takes it with
`ctx.try_get::<ProjectHost>()`.

| Call | Result |
| --- | --- |
| `snapshot()` | The project as an immutable `Arc<Project>`. A snapshot held across an edit costs one copy of the project |
| `revision()` | The engine revision, bumped by every change |
| `apply(&origin, command)` | Proposes and applies one command as its own undo step, or as part of this thread's open group |
| `transact(origin, label, commands)` | Applies the commands as one undo step named `label`, or none of them |
| `transact_with(origin, label, body)` | Runs `body` with a `Transaction`: `tx.apply(command)` and `tx.project()`. An `Err` or a panic from `body` reverts it |
| `group(&origin)` | Opens an `EditGroup`: every command this thread applies until `commit` is one undo step. Dropping it rolls it back |
| `undo()`, `redo()`, `history_jump(depth)` | Step history. `Ok(false)` while this thread has a group open |
| `exclusive(&origin, f)` | Runs `f` on the engine with no edit in flight, for a session replacement that is not a command |
| `holder()` | The origin holding the project from another thread, if any |

```rust
// `get` panics unless the manifest lists "ProjectHost" in `uses`.
let host = ctx.get::<ProjectHost>();
let result: Result<Receipt, TransactError> = host.transact(
    Origin::plugin("example.review"),
    "Add review lane",
    vec![Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Video,
        name: "Review".into(),
        index: None,
        pinned: true,
    })],
);
```

A plugin's transaction is named `<plugin id>: <label>` in History. When a command
is refused or fails, or the body returns an error or panics, every command the
transaction applied is reverted, and `TransactError` states which: `Refused`,
`Failed`, `Aborted`, or `Panicked`. A group or transaction opened inside another
is a savepoint: rolling it back keeps the commands before it.

`Save`, `Open`, `Load`, `RelinkMedia`, `Export`, `SaveTemplate`, and
`ApplyTemplate` write or replace the session and cannot be undone. Inside a group
or transaction they fail with `TransactError::NotUndoable`.

One thread edits at a time. A thread holds the project from its first command
or group until that command lands or the outermost group closes, and its own
later edits join in. An edit from another thread waits up to
`ProjectHost::CONTENDED_WAIT` (100 ms) and then fails with
`TransactError::Busy`, or `EngineError::Refused` from `apply`, naming the holder:
"the assistant is editing the project; try again when it finishes". A background
job retries until it gets the project or is cancelled.

## Panels

Every panel the editor draws is registered, including the shipped ones.
`aria.ui` mounts a `PanelHost` and adds the media pool, the timeline, and the
inspector to it; the shell then resolves each slot from that host. A plugin
contributes a panel to the same host:

```rust
use aria_ui::plugins::panel_host::{host_of, ALWAYS, PanelEntry};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(PanelEntry {
            id: "example.panel.notes",
            title: "Notes",
            location: PanelLocation::Inspector,
            render: draw_notes,
            visible: ALWAYS,
        });
    }
    Ok(())
}
```

`render` is an `aria-ui` function pointer, so `aria-plugin` stays free of GPUI.
Registering an id that already exists replaces that panel, which is how a plugin
overrides a built-in. Mount after `aria.ui` to reach the host.

A slot holds the panels in registration order, stacked. `Overlay` has no
built-in entry: an overlay contribution is drawn over the viewer.

### Body slots

`Viewer` and `Home` are slots whose entries are alternatives rather than a
stack. The shipped set puts the preview and the assistant in `Viewer` and the
launch screen in `Home`. Which one draws is decided by `visible`, a
`fn(&AppState) -> bool` the entry carries:

```rust
host.add(PanelEntry {
    id: "example.panel.scope",
    title: "Scope",
    location: PanelLocation::Viewer,
    render: draw_scope,
    visible: |state| state.active_panel == ActivePanel::Preview,
});
```

A docked pane uses `ALWAYS`. Two entries whose predicates hold at once both
draw, so a replacement body must either take over an existing id or narrow its
own predicate to a state no shipped entry claims.

## Dialogs and overlays

Dialogs are registered the same way panels are. `aria.ui` mounts an
`OverlayHost` holding Export, Settings, the palette, Jobs, the context menu, and
the rename bar; the shell draws whatever the host has for the current state and
names no dialog module of its own.

```rust
use aria_ui::app::DialogKind;
use aria_ui::plugins::overlay_host::{host_of, OverlayEntry, OverlayWhen};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(OverlayEntry {
            id: "example.dialog.notes",
            when: OverlayWhen::Dialog(DialogKind::Plugin("example.dialog.notes")),
            render: draw_notes_dialog,
        });
    }
    Ok(())
}
```

Open it with `state.open_dialog(DialogKind::Plugin("example.dialog.notes"))` —
from a command, a menu item, or a key binding. One dialog is on screen at a
time, so opening yours closes whatever was up, and Escape closes it.

`OverlayWhen::ContextMenu` and `OverlayWhen::Rename` stack: everything
registered for them draws while that condition holds. A dialog does not stack;
the first entry registered for a kind is the one drawn.

## Commands

The command palette draws a registry too. `aria.ui` mounts a `CommandHost` and
adds the shipped commands from `commands::BUILTIN_COMMANDS`; the
palette lists whatever the host holds, in registration order, and Enter runs the
first row the query matches. A plugin adds a command the same way:

```rust
use aria_ui::commands::host::{host_of, CommandEntry};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(CommandEntry {
            id: "example-notes-clear",
            label: "Clear notes",
            run: |state| state.open_dialog(DialogKind::Jobs),
        });
    }
    Ok(())
}
```

`run` takes `&mut AppState`, so a command does what a menu item does. An id that
already exists is replaced, which is how a plugin overrides a shipped command.
With no host mounted the palette offers nothing.

A command that starts work needs the entity context, which `AppState` does not
have. Register that kind with `add_windowed`:

```rust
use aria_ui::commands::host::{host_of, WindowedEntry};

host.add_windowed(WindowedEntry {
    id: "example-notes-index",
    label: "Index notes",
    run: |state, cx| state.check_for_updates(cx),
});
```

The palette hands a windowed row back to the shell to run, so it reaches
`cx.spawn` and the job list. The two kinds share one id space and one order: a
windowed registration replaces a plain row of the same id. The shipped detectors,
transcription, loudness, and the duck are registered this way, in
`BUILTIN_JOB_COMMANDS`.

## Menus

The app bar reads a registry as well. `aria.ui` mounts a `MenuHost` and adds the
shipped tables as its first contributor; the bar asks the host for one menu's
items each time it opens. A contributor is a function, because an entry that
cannot run right now is drawn from the state:

```rust
use aria_ui::chrome::menu_host::host_of;

fn notes_items(menu: AppMenu, _state: &AppState) -> Vec<MenuItem> {
    if menu != AppMenu::Window {
        return Vec::new();
    }
    vec![MenuItem {
        id: "example-menu-notes",
        label: "Notes",
        run: |_id, state, _| state.open_dialog(DialogKind::Jobs),
    }]
}
```

Items land after the ones already registered, so the shipped entries stay at the
top of each menu. An id that a contributor already used is dropped rather than
drawn twice: ids are element ids, and a plugin must not shadow a shipped entry.

A row is told its own id, so one contributor can generate a family of rows from a
table instead of writing a closure per row. The shipped workspaces are drawn that
way: the id the row carries is the workspace it applies. A row that does not need
it names the argument `_`.

## Keys

`aria.ui` mounts a `KeyHost` and adds the shipped bindings; the shell resolves
each keystroke through it and holds no chord of its own. A binding is data:

```rust
use aria_ui::keys::{host_of, After, Binding, Chord, KeyRoute};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(Binding {
            id: "example-notes-open",
            chord: Chord::ctrl_shift("n"),
            route: KeyRoute::Editor,
            run: |state| state.open_dialog(DialogKind::Jobs),
            after: After::Nothing,
        });
    }
    Ok(())
}
```

A chord is exact: Alt+I and I are different bindings, and `ctrl` means
control-or-command. `route` separates Home from the editor, which bind some of
the same chords to different actions. `after` is the side effect the action
cannot do itself, because it needs the window: `Play` starts the playback clock,
`Preview` asks for a fresh frame.

`add` returns `false` when the chord is already bound on that route, and records
the refusal in `conflicts()` — the first binding keeps the key, so a plugin
cannot take Ctrl+S away from saving. Text keys and Escape never reach the
registry: the shell answers those first.

## Settings

`SettingsService` reads and writes `~/.aria/config.toml`. A value nobody can see
is a value nobody sets, so a plugin also declares what its fields are and the
host draws them:

```rust
use aria_plugin::{settings_schema::schema_of, SettingSpec, SettingsSection};

const SETTINGS: &[SettingSpec] = &[SettingSpec {
    key: "notes.directory",
    label: "Notes directory",
    placeholder: "the project directory",
    secret: false,
}];

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(schema) = schema_of(ctx) {
        schema.declare(SettingsSection {
            plugin: "example.notes",
            title: "Notes",
            fields: SETTINGS,
        });
    }
    Ok(())
}
```

Reading the registry counts as a capability, so the manifest declares
`uses: &["SettingsSchema"]`. The section shows in **Settings** under its title,
one row per field, drawn and key-routed like a shipped field. A `secret` field
shows its length, never its text. Editing writes through `SettingsService` under
the declared key and saves; clearing a field stores nothing rather than an empty
string, so the plugin's own default applies again. Declaring twice replaces the
section, so a remount does not double the rows. `aria.settings` mounts the
registry, and it mounts before every plugin but `aria.storage`.

## Mount order

`aria_ui::plugins::builtin(data_dir)` is the built-in list, in mount order:

`aria.storage`, `aria.settings`, `aria.decoder`, `aria.encoder`,
`aria.compositor`, `aria.effects`, `aria.transitions`, `aria.text`,
`aria.shapes`, `aria.render`, `aria.engine`, `aria.cloud`,
`aria.analysis`, `aria.jobs`, `aria.transcription`, `aria.ai`, `aria.ui`.

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

## External plugins

A plugin does not have to be compiled into the app. `<data dir>/plugins` is
scanned at boot and every shared library in it is loaded and mounted after the
built-ins, in path order.

Build the plugin as a `cdylib` that depends on `aria-plugin` and export it:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
aria-plugin = { path = "…/crates/aria-plugin" }
```

```rust
aria_plugin::export_plugin!(NotesPlugin);
```

```text
cargo build -p aria-notes-plugin --release
cp target/release/libaria_notes_plugin.so ~/.aria/plugins/
```

`examples/notes-plugin` is a working one: it claims `.srt` and vetoes a command
that touches more than eight tracks. Check a library before shipping it with

```text
cargo run -p aria-plugin --example load_external -- ~/.aria/plugins
```

Rust has no stable ABI, so `export_plugin!` also exports the loader version and
the compiler that built the library. A plugin built by a different `rustc`, or
against a different `aria-plugin`, is refused before its code runs and appears in
the Plugins list with the reason. Rebuild it against the same toolchain.
The loader version is 2, so a library built against an `aria-plugin` before
`CommandProposed` held the project and the origin is refused.

A loaded library stays mapped until the process exits, because a manifest borrows
its strings out of the library. Disabling an external plugin is therefore not
offered: its row has no toggle.

## Worked example

One plugin that does all three things a plugin can do: contribute a panel, sit in
front of every command, and claim a file type.

```rust
use aria_plugin::{
    CommandProposed, DropAction, PanelLocation, Plugin, PluginContext, PluginError,
    PluginManifest, ResolveDroppedPath,
};
use aria_ui::plugins::panel_host::{ALWAYS, host_of, PanelEntry};
use gpui::{AnyElement, Entity, IntoElement, ParentElement, Styled, div};

pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "example.notes",
            name: "Notes",
            version: "0.1.0",
            // The panel host is mounted by `aria.ui`, so it has to come first.
            requires: &["aria.ui"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if let Some(host) = host_of(ctx) {
            host.add(PanelEntry {
                id: "example.panel.notes",
                title: "Notes",
                location: PanelLocation::Inspector,
                render: draw_notes,
                visible: ALWAYS,
            });
        }

        // Refuse a command instead of letting it reach the engine. The reason is
        // shown to the user verbatim, so write it as a sentence.
        ctx.on_waterfall::<CommandProposed, _>(|event| {
            if event.command.targets().clips.is_empty() {
                event.refuse("Notes needs a clip selected.");
            }
        });

        // Claim a file type. The first plugin to claim one wins, in mount order.
        ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
            event
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                .then_some(DropAction::InstallSkill)
        });

        Ok(())
    }
}

fn draw_notes(
    _state: &Entity<aria_ui::app::AppState>,
    _cx: &mut gpui::Context<aria_ui::shell::Shell>,
) -> AnyElement {
    div().p_2().child("Notes").into_any_element()
}
```

The GPU catalogs are not contributions: `EffectService`, `TransitionService`,
`DecoderService`, and `EncoderService` hold one value each, owned by the shipped
plugins. A plugin that registers one replaces the shipped catalog rather than
adding to it.

Shipping an effect therefore takes two pieces. The offerable set is
`aria_models::effect_catalog`, so the id and its parameters go in an `effects.toml`
override (see `docs/catalogs.md`); an id the model does not carry is refused by the
engine, so the browser never offers one. Dispatch comes from the mounted
`EffectService`, and the Add list marks an entry passthrough when that service does
not carry a descriptor for it — including when no GPU plugin mounted at all.

The headed app resolves its data directory once, calls
`plugins::mount_all(&ctx, &data_dir)`, then `aria_ui::run_app_with` with the same
directory. Do not remount inside `run_app`.
