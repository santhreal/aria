# Commands

A plugin adds a palette command through its context. The command runs on the
thread that asks for it, through the plugin's own view of the context, so it
reaches only the services its manifest declares:

```rust
use aria_plugin::{CommandIcon, CommandSpec, Enablement, PluginContext, PluginError};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    let mut spec = CommandSpec::new("com.example.notes.clear", "Clear Notes");
    let refused = PluginError::CommandRefused;
    spec.icon = Some(CommandIcon::from_svg(include_bytes!("clear.svg")).map_err(refused)?);
    spec.enablement = Some(Enablement::parse("selection && !playing").map_err(refused)?);
    ctx.register_command(spec, |ctx: &PluginContext| clear_notes(ctx))?;
    Ok(())
}
```

The palette lists the command by its title while it is registered, and Enter
runs it. An `Err` is shown in the status bar as `{title}: {reason}`. A command
that panics fails its plugin for the session. Registration is refused when the
id is empty or taken, or the title is empty. A registration made during mount
drops when the plugin unmounts.

## Icons

An icon is an SVG document of at most 64 KiB and 4096 XML nodes. It is parsed
once, when the command is registered, and kept as the SVG the parser writes
back: `<image>` elements and text are dropped, and a document with a DTD is
refused. The palette and the menus draw the icon at 14 px as a mask in the text
colour of the row, so only its shape shows; draw it in one solid colour on a
transparent ground. `CommandIcon::from_svg` returns the reason a document is
refused.

## Enablement

An enablement condition states the editor state a command needs. It is a
boolean expression over these keys (`ContextKey::ALL` in `crates/aria-plugin/src/enablement.rs`):

| Key | Holds when |
| --- | --- |
| `clips` | The sequence holds at least one clip |
| `selection` | At least one clip is selected |
| `single-selection` | A single clip is selected |
| `mark-in` | The sequence has an In point |
| `mark-out` | The sequence has an Out point |
| `in-out` | The sequence has both an In and an Out point |
| `playing` | Playback is running |
| `focus.media` | The Project panel has focus |
| `focus.viewer` | The Viewer has focus |
| `focus.timeline` | The Timeline has focus |
| `focus.inspector` | Effect Controls has focus |
| `focus.assistant` | The Assistant has focus |
| `focus.mixer` | The Audio Track Mixer has focus |

`!` negates, `&&` binds tighter than `||`, and parentheses group:
`(selection || in-out) && !playing`. A condition is at most 256 bytes and nests
`!` and parentheses at most 16 deep. `Enablement::parse` refuses an unknown key
or a malformed condition with the column of the fault.

The editor publishes the keys with every drawn frame through
`PluginContext::set_context_keys`. While a condition is false, the palette draws
the command dimmed, the menu bar and context menus leave its rows out, and
`run_command` refuses it with `CommandError::Disabled`; the status bar shows
`{title}: needs {condition}`. A context no host publishes to holds no key:
`selection` is false there and `!playing` is true.

## Declaring commands

A package lists its commands in `plugin.toml`, so `[[menus]]` and `[[keys]]` can
name them and `aria plugin check` can count them:

```toml
[[commands]]
id = "com.example.notes.clear"
title = "Clear Notes"
icon = "icons/clear.svg"
enablement = "selection && !playing"
```

`icon` is a path inside the package. The package is refused when the file is
missing or is not an icon `CommandIcon::from_svg` accepts, or when the
condition does not parse. The declared icon and condition apply to the command
the package's code registers under the same id.

A WASM component registers each one with `register-command` during `mount`, and
its `command-run` export runs it. A native library registers through the host
table; a listed command the library did not register is registered for it,
and its `command_run` callback runs it. A data package cannot list commands.

Code built into `aria-ui` that needs `&mut AppState` registers with the
`CommandHost` instead. `aria.ui` mounts it and adds the shipped commands from
`commands::BUILTIN_COMMANDS`:

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

An id that already exists is replaced, which is how built-in code overrides a
shipped command. The palette lists `CommandHost` rows first, then plugin
commands, then menu rows, each id once.

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
