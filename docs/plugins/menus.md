# Menus

A plugin offers a command as a menu row by naming the command and a path:

```rust
use aria_plugin::{MenuCommand, MenuPath, PluginError};

let path = MenuPath::parse("Sequence/Color Tools").map_err(PluginError::CommandRefused)?;
ctx.register_menu_entry(MenuCommand {
    command: "com.example.notes.clear".into(),
    path,
})?;
```

A path starts with a menu-bar menu (`File`, `Edit`, `Sequence`, `Markers`,
`Effect`, `Animation`, `View`, `Window`, `Help`) or a context menu (`Clip`,
`Track`, `Media`, `Viewer`, `Transition`, `Effect Header`, `Clip Badge`), and
may add one group. An `Effect Header` row is drawn in the menu an effect's header
opens in the Effects tab of Effect Controls. A `Clip Badge` row is drawn in the
menu the fx badge on a timeline clip bar opens. A `Clip` row is
drawn in the clip context menu and in the menu bar's Clip menu. In the menu bar a
group is a submenu: `Sequence/Color Tools` puts the row in a `Color Tools`
flyout of the Sequence menu. In a context menu and in the palette a grouped row
reads `Color Tools: Clear Notes`. The row is drawn while a command by that id is
registered, after the shipped rows of the menu. A row id a shipped entry already
holds is dropped, so a plugin cannot shadow a shipped entry. In a context menu,
declared rows come before the rows `MenuRequested` handlers add. The same
command in the same place twice is refused. The row holds no code, so the
command may come from another plugin, or register later. A command whose
enablement condition is false is left out of the menus.

In `plugin.toml`:

```toml
[[menus]]
command = "com.example.notes.clear"
menu = "Clip"
```

`menu` on a `[[commands]]` row is shorthand for one `[[menus]]` row. A row that
names a command no `[[commands]]` row lists is refused.

Built-in code can draw rows from the state with a `MenuHost` contributor.
`aria.ui` mounts the host and adds the shipped tables as its first contributor;
the bar asks the host for one menu's items each time it opens. A contributor is
a function, because an entry that cannot run right now is drawn from the state:

```rust
use std::borrow::Cow;

use aria_ui::chrome::menu_items::{MenuItem, MenuItems};

fn blocked(_id: &str, state: &AppState) -> Option<&'static str> {
    state
        .next_undo_label()
        .is_none()
        .then_some("Nothing has been done yet.")
}

fn notes_items(menu: AppMenu, _state: &AppState) -> MenuItems {
    if menu != AppMenu::Window {
        return Cow::Borrowed(&[]);
    }
    Cow::Owned(vec![
        MenuItem::new("example-menu-notes", "Notes", |_id, state, _| {
            state.open_dialog(DialogKind::Jobs)
        })
        .key("example-notes")
        .when(blocked)
        .checked(|_id, state| state.dialog == DialogKind::Jobs)
        .under("Example"),
    ])
}
```

`MenuItem::new` takes the id, the label, and the function the row runs. The
builder adds the rest:

| Call | Effect |
| --- | --- |
| `.key(binding)` | Draws the binding's current chord right-aligned. A rebinding shows on the next draw. |
| `.when(blocked)` | Draws the row at 40 % opacity while `blocked` returns a reason, with the reason as its tooltip. Choosing the row from the keyboard, the palette or the platform menu bar puts the reason on the status line and runs nothing. |
| `.checked(test)` | Draws a check mark while `test` holds. |
| `.under(caption)` | Puts the row in the submenu `caption`. Rows with one caption share one flyout, drawn where the first of them sits. |

Items land after the ones already registered. An id that a contributor already
used is dropped rather than drawn twice. A row receives its own id, so one
contributor can generate a family of rows from a table instead of writing a
closure per row. The shipped workspaces are drawn that way: the row id is the
workspace it applies. A row that does not need it names the argument `_`.
