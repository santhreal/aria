# Activation

A package that lists activation events stays unloaded until the first of them.
A package with no `[activation]` section, or one that lists `startup`, loads at
start.

```toml
[activation]
on = ["command:grade.match", "file:cube", "effect:com.example.grade.film"]
```

| Event | Loads the package when |
| --- | --- |
| `startup` | Aria starts |
| `command:<id>` | the declared command runs from the palette, a menu, or a key |
| `file:<ext>` | a file with that extension is dropped, imported, or read as a timeline |
| `effect:<id>` | the open project uses the effect, on a clip or as an audio insert |
| `transition:<id>` | the open project uses the transition |
| `generator:<id>` | the open project uses the generator |
| `panel:<id>` | the declared panel is opened |
| `setting:<key>` | the declared setting is read through `SettingsService` or saved from Settings |

`command:`, `panel:`, and `setting:` name an entry of `[[commands]]`,
`[[panels]]`, or `[[settings]]` in the same manifest; any other id is refused
when the package is read. A `setting:` key is qualified by the package id, as
`[[settings]]` keys are. `file:cube`, `file:.cube`, and `file:.CUBE` are the
same event, and a path matches without case.

## While a package waits

Everything the manifest declares is registered in the package's name:

- declared WGSL effects, transitions, and generators, which render;
- commands, listed in the palette and run through a stand-in that loads the
  package and then runs the command the package registered under that id;
- `[[menus]]` rows, `[[keys]]` defaults, the `[[settings]]` section,
  presets in `presets/`, and themes in `themes/`;
- `[[panels]]`, drawn where they dock as a card with the panel title and an
  Open button.

The Plugins section of Settings lists the package as enabled and `waits for`
its events. Showing a setting's value in Settings does not load the package.

The first matching event drops these registrations and loads the package once.
Its own registrations replace them. A command that the package does not
register under its declared id fails with `<name> did not register <id>`, and
an opened panel it does not register is reported as `<name> did not register
panel <id>`.

## Order and failures

A waiting package's requirements that also wait load first. A package that
mounts at start (one with no events, `startup`, no code, or the development
package from `aria plugin dev`) loads every package it requires at start too.
Installed packages mount in the order their `requires` set.

A load that fails is recorded as a failed mount: the Plugins row shows the
reason and the status line reports `<name> did not load: <reason>`. The package
is no longer waiting; enable it again, or reload it, to wait again.

Mounts do not nest. An event that fires while another plugin mounts leaves the
package waiting for the next event; a project event fires again on the
editor's next read of the project.

Disabling, uninstalling, or reloading a waiting package drops its
registrations. A reload of a loaded package loads it again at once; a reload of
a waiting one leaves it waiting for the new manifest's events.

## Host calls

A host that loads packages defers each with `PluginContext::defer_package`,
passing the loader for its tier, and fires the events:

| Call | Fires |
| --- | --- |
| `activate(&Activation)` | one event, such as `Activation::Panel(id)` |
| `activate_for_file(path)` | `file:` events for the path's extension |
| `activate_setting(key)` | `setting:` events for the qualified key |
| `project_activations(&project, revision)` | returns the `effect:`, `transition:`, and `generator:` events the project uses, for the caller to `activate` once it no longer holds the project |

Each `activate` call returns an `Activated` per package with the load's result.
`waiting_plugins`, `waiting_panels`, `is_waiting`, and `cancel_activation`
read and drop the waiting set. `activation_revision` moves whenever a package
starts or stops waiting, and `panel_revision` moves when a waiting panel
appears or goes. `defer_package` refuses a package that loads at start, and is
refused while a plugin mounts.

Aria fires `file:` before it classifies a drop or reads an import,
`effect:`, `transition:`, and `generator:` each time the editor's projection
of the project is rebuilt, with the engine lock released, and `setting:` from
its `SettingsService`, whose `peek` reads a value without loading anything.
