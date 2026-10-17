# Settings

`SettingsService` reads and writes `~/.aria/config.toml`. A plugin declares its
fields, and **Settings** draws them in a section with the rows it uses for its
own:

```rust
use aria_plugin::{SettingKind, SettingSpec, SettingsSection};

ctx.register_settings(SettingsSection {
    plugin: "com.example.notes".into(),
    title: "Notes".into(),
    fields: vec![
        SettingSpec {
            key: "com.example.notes.directory".into(),
            label: "Notes directory".into(),
            placeholder: "the project directory".into(),
            kind: SettingKind::Text,
            default: None,
        },
        SettingSpec {
            key: "com.example.notes.strength".into(),
            label: "Strength".into(),
            placeholder: String::new(),
            kind: SettingKind::Number { min: 0.0, max: 1.0, step: 0.05 },
            default: Some("0.5".into()),
        },
    ],
})?;
```

| Kind | Row | Stored as |
| --- | --- | --- |
| `Text` | Text box | The text |
| `Secret` | Text box showing the length | The text |
| `Toggle` | On and Off chips | `true` or `false` |
| `Number { min, max, step }` | Text box; the placeholder states the range | A decimal |
| `Choice(options)` | One chip per option | The option |

Every key sits under the plugin id, and the section belongs to the registering
plugin whatever `plugin` holds. Registration is refused when a key is outside
the plugin id, declared twice, or declared by another section; when a number
range is empty or its step is not above zero; when a choice has no options or
one twice; when a default does not fit its kind; or when the plugin already has
a section. Without a mounted Settings host, nothing is held.

Editing a field checks the value against its kind, writes it through
`SettingsService` under the key, and saves. A value the kind refuses is not
saved: the status bar shows `{label}: {reason}`, such as `Strength: 7 is above
1`, and the field keeps the focus. Clearing a field stores nothing, and the key
reads as its declared default again, through `SettingsService::get` as well as
in the row. A section drops when its plugin unmounts.

`SettingsService::get` and `set` load a package waiting for the key's
`setting:` event first ([Activation](activation.md)). `peek` reads as `get`
does without loading anything; the Settings rows read through it.

In `plugin.toml`, a key is qualified by the plugin id and the section takes the
plugin name as its title:

```toml
[[settings]]
key = "strength"
label = "Strength"
type = "number"
min = 0.0
max = 1.0
step = 0.05
default = 0.5

[[settings]]
key = "mode"
label = "Mode"
type = "choice"
options = ["fast", "accurate"]
default = "fast"
```

`type` is `text`, `secret`, `toggle`, `number`, or `choice`. A number without
`min` or `max` is unbounded on that side, and its step is 1. A WASM component
reads a value with `setting-get` and a native library with `setting_get`, as
JSON text: a stored `0.5` reads `"0.5"`.
