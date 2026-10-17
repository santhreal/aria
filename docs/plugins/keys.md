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
and `Option` as `Alt`. The key must be in `aria_plugin::keys::BINDABLE_KEYS`;
`aria plugin check` refuses any other. `route` is `editor` (the default) or
`home`, which binds some of the same chords to different actions.

The binding is placed while the command is registered. A chord that a shipped
binding or an earlier plugin already holds is refused: the first binding keeps
the key, so a plugin cannot take Ctrl+S away from saving. A refused key is
listed in **Settings > Keys** with no chord, under the conflicts, and can be
bound there like any other row. A rebinding is kept across remounts.

Built-in code adds a binding that acts on `AppState` through the `KeyHost`.
`aria.ui` mounts it and adds the shipped bindings; the shell resolves each
keystroke through it and holds no chord of its own:

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

`after` is the side effect the action cannot do itself, because it needs the
window: `Play` starts the playback clock, `Preview` asks for a fresh frame.
`add` returns `false` when the chord is already bound on that route, and records
the refusal in `conflicts()`. Text keys and Escape never reach the registry: the
shell answers those first.
