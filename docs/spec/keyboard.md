# Keyboard

Ctrl stands for Cmd on macOS and Alt for Option. Every key below is a binding in
the keymap, not a fixed key; [Customizing](#customizing) covers changes.

## Resolution

A key resolves against the focused panel first, then the window. A text field
with focus takes printable keys, Backspace, Delete, Enter, Escape, Tab, arrows,
and the clipboard chords; every other chord passes through. Bare letters and
digits reach the Timeline and the Viewer only.

Menus open with F10 or a lone Alt press and release. Alt with a letter never
opens a menu; Alt+letter chords are the property reveal keys below.

Escape cancels, in order: the drag in progress, an open menu or popover, an
open dialog, the active text field, the temporary tool, then the selection.

## Panels

| Key | Result |
| --- | --- |
| Shift+1 | Project |
| Shift+2 | Viewer; pressed with the Viewer focused, toggles Program and the most recent Source tab |
| Shift+3 | Timeline |
| Shift+4 | Effect Controls |
| Shift+5 | Effects & Presets |
| Shift+6 | Audio Track Mixer |
| Shift+7 | Scopes |
| Shift+8 | Info |
| Shift+9 | Audio Meters |
| Ctrl+Tab / Ctrl+Shift+Tab | Focus the next / previous frame |
| `` ` `` | Maximize or restore the frame under the pointer |
| Ctrl+W | Close the focused panel, or the sequence tab in the Timeline |
| Alt+Shift+1 to Alt+Shift+9 | Switch to workspace 1 to 9 in switcher order |

## Application

| Key | Result |
| --- | --- |
| Ctrl+Alt+N | New Project |
| Ctrl+N | New Sequence |
| Ctrl+O | Open Project |
| Ctrl+Shift+W | Close Project |
| Ctrl+S / Ctrl+Shift+S / Ctrl+Alt+S | Save / Save As / Save a Copy |
| Ctrl+I | Import |
| Ctrl+M | Export Media |
| Ctrl+Shift+E | Export Frame |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z, Ctrl+Y | Redo |
| Ctrl+X / Ctrl+C / Ctrl+V | Cut / Copy / Paste |
| Ctrl+Shift+V | Paste Insert |
| Ctrl+Alt+V | Paste Attributes |
| Ctrl+D | Duplicate |
| Ctrl+F | Find |
| Ctrl+Shift+P | Command palette |
| Ctrl+Shift+H | History panel |
| Ctrl+Alt+K | Keyboard Shortcuts |
| Ctrl+, | Preferences |
| Ctrl+Q | Quit |

## Tools

| Key | Tool |
| --- | --- |
| V | Select |
| A / Shift+A | Track Select Forward / Backward |
| B | Ripple Trim |
| N | Rolling Edit |
| R | Rate Stretch |
| C | Razor |
| Y | Slip |
| U | Slide |
| P | Pen |
| H | Hand |
| Z | Zoom |
| T | Type |

Holding a tool key uses that tool until release
([editing.md](editing.md#tools)).

## Transport

| Key | Result |
| --- | --- |
| Space | Play or stop |
| Shift+Space | Play the work area |
| J / K / L | Shuttle reverse / stop / forward; repeated J or L doubles the speed up to 8x |
| K+J, K+L | One frame back / forward; held, 1/4 speed |
| Shift+J / Shift+L | Shuttle speed down / up by 10 % |
| Left / Right | One frame back / forward |
| Shift+Left / Shift+Right | Five frames back / forward |
| Up / Down | Previous / next edit on targeted tracks |
| Shift+Up / Shift+Down | Previous / next edit on all tracks |
| Home / End | Sequence start / end |
| Ctrl+Shift+L | Loop on or off |
| Digits, then Enter | Playhead to the typed timecode; a leading keypad `+` or `-` moves relative to the playhead when nothing is selected |
| Enter | Render In to Out |

## Marking

| Key | Result |
| --- | --- |
| I / O | Mark In / Out at the playhead (on the source in a Source tab) |
| X | Mark the clip under the playhead on targeted tracks |
| Shift+I / Shift+O | Playhead to In / Out |
| Ctrl+Shift+I / Ctrl+Shift+O / Ctrl+Shift+X | Clear In / Out / both |
| M | Add marker; pressed twice, edit it |
| Shift+M / Ctrl+Shift+M | Next / previous marker |
| Ctrl+Alt+M | Clear the marker at the playhead |
| Ctrl+Alt+Shift+M | Clear all markers |
| F | Match Frame: open the source of the clip under the playhead at that frame |

## Editing

| Key | Result |
| --- | --- |
| `,` / `.` | Insert / Overwrite from the Source tab |
| `;` / `'` | Lift / Extract |
| Ctrl+K / Ctrl+Shift+K | Add Edit / Add Edit to All Tracks |
| Q / W | Ripple trim head / tail to the playhead |
| Alt+[ / Alt+] | Trim head / tail to the playhead without closing the gap |
| Delete / Shift+Delete | Clear / Ripple Delete |
| Ctrl+Left / Ctrl+Right | Trim the selected edges one frame |
| Ctrl+Shift+Left / Ctrl+Shift+Right | Trim the selected edges five frames |
| Keypad `+` / `-`, digits, Enter | Move the selected clips, or trim the selected edges, by the typed duration |
| Ctrl+T / Ctrl+Shift+T | Default video / audio transition at the selected edges, or at the nearest edit on targeted tracks |
| D | Select the clip at the playhead on targeted tracks |
| Ctrl+A / Ctrl+Shift+A | Select all / deselect all |
| S | Snapping on or off |
| Ctrl+L | Link or unlink |
| Ctrl+G / Ctrl+Shift+G | Group / ungroup |
| Shift+E | Enable or disable the selected clips |
| Ctrl+R | Speed/Duration |
| Ctrl+Alt+T | Time Remap |
| `]` / `[` | Audio clip gain up / down 1 dB; with Shift, 6 dB |
| G | Audio Gain dialog |

## Properties

Pressed with a clip selected, a reveal chord opens the clip's property rows
([timeline.md](timeline.md#property-rows)) showing only the named property.
With Shift added it toggles that property into the current set instead of
replacing it.

| Key | Reveals |
| --- | --- |
| Alt+P | Position |
| Alt+S | Scale |
| Alt+R | Rotation |
| Alt+T | Opacity |
| Alt+A | Anchor Point |
| Alt+E | Effects |
| Alt+M | Masks |
| Alt+L | Audio Volume and Pan |
| Alt+U | Properties with keyframes or drivers |
| Alt+U twice | Properties changed from their defaults |
| Alt+`` ` `` | Close every open property row |

## Keyframes

| Key | Result |
| --- | --- |
| Alt+Shift+Left / Alt+Shift+Right | Previous / next keyframe on the visible property rows |
| F9 | Easy Ease |
| Shift+F9 / Ctrl+Shift+F9 | Easy Ease In / Out |
| Ctrl+Alt+H | Toggle Hold |
| Shift+F3 | Graph editor on or off |
| Ctrl+C / Ctrl+V / Delete | Copy / paste / delete the selected keyframes |

Pointer gestures on keyframes are in [animation.md](animation.md#keyframes).

## Timeline view

| Key | Result |
| --- | --- |
| `=` / `-` | Zoom in / out around the playhead |
| `\` | Fit the sequence |
| Shift+F | Fit the selection |
| Ctrl+= / Ctrl+- | Taller / shorter tracks |

## Viewer

| Key | Result |
| --- | --- |
| Shift+/ | Fit the frame |
| Arrows / Shift+arrows | Nudge the selected clips 1 px / 10 px |
| Ctrl+Shift+R | Rulers on or off |

## Customizing

Edit > Keyboard Shortcuts shows a keyboard image with each key coloured by the
kind of command it holds, the command list with search, and the conflict list.
Dropping a command on a key binds it; pressing a key in a command's row binds
it; a chord already bound in the same context shows the holder and asks before
moving it. Keymaps are saved as named sets in `<data dir>/keymaps/<name>.toml`,
imported and exported as files.

Plugins add bindings through their manifest
([plugins.md](plugins.md#contribution-points)). A plugin binding never replaces
a shipped or user binding; a conflict leaves the plugin's command unbound and
lists it in the conflict list.
