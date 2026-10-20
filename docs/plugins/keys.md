# Keys

A plugin gives a command a default chord:

```rust
use aria_plugin::{Chord, KeyDefault, KeyRoute};

ctx.register_key_default(KeyDefault {
    command: "com.example.notes.clear".into(),
    chord: Chord::ctrl_shift("l"),
    route: KeyRoute::Editor,
})?;
```

In `plugin.toml`:

```toml
[[keys]]
command = "com.example.notes.clear"
key = "Ctrl+Shift+L"
route = "editor"
```

`key` on a `[[commands]]` row is shorthand for one `[[keys]]` row on the editor.

A chord is exact: Alt+I and I are different bindings, and `Ctrl` is
control-or-command. `Cmd`, `Command`, `Control`, and `Platform` read as `Ctrl`,
and `Option` as `Alt`. The key name must be in
`aria_plugin::keys::BINDABLE_KEYS`; `aria plugin check` rejects any other.

| Route | `KeyRoute::label()` | Scope |
| --- | --- | --- |
| `Editor` | `"Editor"` | Editor window (default route) |
| `Timeline` | `"Timeline"` | Timeline panel when focused |
| `Viewer` | `"Viewer"` | Viewer panel when focused |
| `Home` | `"Home"` | Project Home screen |

`BINDABLE_KEYS` (`75` entries) lists:

- letters `a` through `z`;
- digits `0` through `9`;
- function keys `f1` through `f12`;
- navigation and editing keys `up`, `down`, `left`, `right`, `home`, `end`,
  `pageup`, `pagedown`, `space`, `tab`, `escape`, `enter`, `backspace`,
  `delete`, `insert`;
- punctuation keys `-`, `=`, `+`, `[`, `]`, `\`, `;`, `'`, `,`, `.`, `/`, `` ` ``.

`SHIFTED_SYMBOLS` (`21` US-layout pairs: `!`/`1`, `@`/`2`, `#`/`3`, `$`/`4`,
`%`/`5`, `^`/`6`, `&`/`7`, `*`/`8`, `(`/ `9`, `)`/`0`, `_`/`-`, `+`/`=`,
`{`/`[`, `}`/`]`, `|`/`\`, `:`/`;`, `"`/`'`, `<`/`,`, `>`/`.`, `?`/`/`,
`~`/`` ` ``) maps shifted symbols reported by the windowing layer back to
`Shift` plus the underlying key via `unshifted(name)`.

The binding is placed while the command is registered. A chord that a shipped
binding or an earlier plugin already registered on the same route is rejected:
the first binding keeps the chord, so a plugin cannot take Ctrl+S away from
saving. A rejected key is listed in **Settings > Keys** with no chord, under
the conflicts, and can be bound there like any other row. A rebinding persists
across remounts.

Built-in code adds a binding that acts on `AppState` through `KeyHost`.
`aria.ui` mounts it and adds the shipped bindings; the shell resolves each
keystroke through it and stores no chord of its own:

```rust
use aria_ui::keys::{host_of, After, Binding, Chord, KeyBody, KeyRoute};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    if let Some(host) = host_of(ctx) {
        host.add(Binding {
            id: "example-notes-open",
            chord: Chord::ctrl_shift("n"),
            route: KeyRoute::Editor,
            run: KeyBody::Plain(|state| state.open_dialog(DialogKind::Jobs)),
            after: After::Nothing,
        });
    }
    Ok(())
}
```

| Type | Fields / Variants |
| --- | --- |
| `Chord` | `key`, `ctrl`, `shift`, `alt` |
| `KeyDefault` | `command`, `chord`, `route` |
| `After` | `Nothing`, `Play`, `Preview` |
| `KeyBody` | `Plain`, `Windowed`, `Command` |
| `Binding` | `id`, `chord`, `route`, `run`, `after` |
| `KeyConflict` | `held_by`, `refused`, `chord` |
| `BindingRow` | `binding`, `chord`, `overridden` |
| `KeyOverrideError` | `Unknown`, `Taken` |

| `KeyHost` call | Effect |
| --- | --- |
| `add(binding)` | Adds `binding`, or returns `false` and records a `KeyConflict` if taken on `route` |
| `resolve(route, key, ctrl, shift, alt)` | Returns the effective `Binding` on `route` for the keystroke |
| `resolve_focused(focus, key, ctrl, shift, alt)` | Resolves against focused panel route first, then `Editor` (or `Home` alone) |
| `all()` | Every registered `Binding` in registration order |
| `conflicts()` | Every `KeyConflict` in refusal order |
| `chord_of(id)` | The effective `Chord` for `id`, or `None` when unbound or unknown |
| `set_override(id, chord)` | Rebinds `id` to `chord` (`None` clears the binding) |
| `remove_override(id)` | Restores `id` to its shipped or plugin default chord |
| `reset_overrides()` | Restores every binding to its default chord |
| `overrides()` | Active overrides as `Vec<(&'static str, Option<Chord>)>` |
| `rows()` | Every `BindingRow`, followed by refused plugin bindings |
| `sync_plugin_keys(ctx)` | Synchronizes `ctx.key_defaults()` with `ctx.commands()` when `command_revision()` changes |
| `label_of(id)` | Display title for `id` (plugin command title or humanized `key-` stem) |

`after` is the window side effect the action cannot perform directly: `Play`
starts the playback clock, `Preview` requests a fresh frame, and `Nothing`
performs no extra window step. Text keys and Escape never reach the registry:
the shell handles those first.
