# Keyboard shortcuts

Modifiers: Ctrl on Linux and Windows, Cmd on macOS (`platform` in GPUI).

A Shift chord on a digit or punctuation key also fires when the platform
reports the shifted symbol in its place, as GPUI on Linux does: Shift+6 arrives
as `^` and Shift+, as `<`. Symbols are read by their position on a US layout.

Every shortcut below is a registered binding, not a hardcoded key. The shipped
table is `keys::bindings::SHIPPED_BINDINGS`; a plugin registers its own through
`KeyHost::add`, and a chord that is already taken is refused rather than
overriding the binding that holds it. See
[plugin-development.md](plugin-development.md#keys).

Two keys are the shell's own and not in the table: text keys while a rename, a
search box, or the palette holds focus, and Escape in the editor, which cancels
whatever drag is in flight.

## Inside a text box

A box takes keys only while it holds focus, so the bare-letter tool shortcuts
keep working with a search box open.

| Shortcut | Action |
| --- | --- |
| Any printable key | Append the character |
| Backspace | Delete the last character |
| Enter | Commit: rename, run the search, send the prompt |
| Esc | Leave the box without committing |
| Ctrl+V | Paste the clipboard as text |
| Up / Down | Walk this box's own history, where it keeps one |

A paste is held to the same rules as typing: the timecode box takes digits and
separators only, a name stops at its length cap, and control characters are
dropped, so a path pasted with a trailing newline arrives as a path.

Typed text and an IME commit land the same way. The app registers a platform
input handler on the window, so a character arrives as the layout produces it,
capitals and shifted symbols included, and a preedit run is marked and then
replaced by the finished text rather than appended to it.

## Playback and playhead

| Shortcut | Action |
| --- | --- |
| Space | Play / pause |
| Home | Playhead to start |
| End | Playhead to sequence end |
| Left / Right | Nudge playhead one frame, or nudge the selected clip |
| Shift+Left / Shift+Right | Nudge ten frames |
| J | Shuttle reverse. Tap again for 2x, 4x, 8x. |
| K | Stop shuttle. The transport returns to 1x. |
| L | Shuttle forward. Tap again for 2x, 4x, 8x. |
| M | Add marker at playhead. Named and coloured automatically. |
| Alt+M | Delete the marker at the playhead |
| Shift+M | Playhead to next marker |
| Shift+Alt+M | Playhead to previous marker |
| I | Set In point at playhead. With a source loaded in the monitor, marks the source instead. |
| O | Set Out point at playhead. With a source loaded, marks the source (exclusive, so the frame on screen is kept). |
| Shift+I | Playhead to In point |
| Shift+O | Playhead to Out point |
| Alt+I | Clear In point, or the source's when one is loaded |
| Alt+O | Clear Out point, or the source's when one is loaded |
| , | Insert the loaded source's marked range at the sequence In point (playhead when unmarked), rippling the lane |
| . | Overwrite with the loaded source's marked range. A marked sequence span makes either edit four-point: the take is retimed to fill it. |
| Alt+F | Match Frame: open the source of the clip under the playhead at that frame |
| Shift+, | Insert the selected pool item at the playhead, rippling that lane right |
| Ctrl+Alt+M | Move the marker nearest the playhead (within one second) onto it |
| Alt+Left / Alt+Right | Shift the selected clip's whole lane one second earlier or later |

Above 1x and in reverse the audio is silent. The mixer plays a sequence forward
at its own rate: it neither resamples nor reads backwards, so there is no sound
that matches the picture at those speeds. The rate appears under the preview with
a muted speaker beside it while it is not 1x; click it to return to 1x with
sound. Space and K also return to 1x, as does reaching either end of the
sequence.

## Edit

| Shortcut | Action |
| --- | --- |
| S | Split selected clip at playhead |
| Q | Trim the head of the selected clip to the playhead |
| W | Trim the tail of the selected clip to the playhead |
| Delete / Backspace | Delete selection. Ripple mode closes the gap, Overwrite mode leaves it. |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z or Ctrl+Y | Redo |
| Ctrl+C | Copy selected clip |
| Ctrl+V | Paste clipboard at playhead, on the source's own lane |
| Ctrl+Shift+V | Paste Insert. Ripples that lane right first, so nothing is overwritten. |
| Ctrl+Alt+V | Paste Attributes. Writes the copied clip's transform, filter, and effect chain onto the selection. Timing is unchanged. |
| Ctrl+D | Duplicate selected clip after itself |
| Alt+D | Cycle the ducking amount |
| Shift+Alt+D | Duck the selection under the voice track at that amount |
| Shift+Alt+S | Detect shots on the selected clip |
| Shift+Alt+Q | Detect silence on the selected clip |
| Shift+Alt+B | Detect beats on the selected clip |
| Alt+R | Reset every effect on the selected clip to its catalog defaults |

## Project

| Shortcut | Action |
| --- | --- |
| Ctrl+S | Save project |
| Ctrl+Shift+S | Save As |
| Ctrl+A | Select all unlocked clips |
| Ctrl+O | Open a project. On Home, switch to the Open rail. |
| Ctrl+W | Close project, return Home |
| Ctrl+N | New project |
| Ctrl+E | Export dialog |
| Ctrl+, | Settings |
| Ctrl+Shift+P | Command palette |
| Ctrl+Shift+H | History |
| With palette open | Printable keys filter. Backspace deletes. Enter runs the first hit. Esc closes. Every command, every menu row and every effect in the catalog is offered by name, so typing an effect's name and pressing Enter adds it to the selection. |
| With a rename open | Printable keys type, up to 64 characters. Backspace deletes. Enter applies. Esc cancels. An empty or unchanged name makes no change. |
| With a dialog open | Enter runs the dialog's primary action: Export queues the export, the unsaved-changes prompt saves and continues. Every other dialog has no primary action and swallows the key. Esc closes. |

## View

| Shortcut | Action |
| --- | --- |
| + / = | Zoom timeline in |
| - | Zoom timeline out |
| F | Fit timeline to sequence |
| Shift+F or Shift+Z | Fit timeline to selection |
| Escape | Cancel an in-progress splitter drag, then close dialog, then context, then menu, then return to the pointer, then clear selection. No-op on Home except dialog, context, and menu. |
| V or A | Pointer tool |
| Shift+A | Toggle assistant / preview |
| Shift+6 | Toggle mixer / preview |
| B | Razor tool |
| H | Hand tool. Drag the timeline to pan. |
| Z | Zoom tool. Click zooms in around the pointer. Alt-click zooms out. |
| Y | Slip tool. Drag inside a clip to change what it shows without moving it. |
| U | Slide tool. Drag a clip between its neighbours; they absorb the move. |
| N | Roll tool. Drag a cut to move it, keeping both clips' total length. |
| R | Rate tool. Drag a clip edge to retime it instead of trimming. |
| P | Ripple trim tool. Drag an edge and the rest of the lane follows. |

## Pointer

| Gesture | Action |
| --- | --- |
| Click clip | Select the clip and everything linked to it |
| Alt+click clip | Select the clicked clip only, ignoring its links |
| Shift+click clip | Add to selection |
| Ctrl/Cmd+click clip | Toggle clip in selection |
| Drag clip body | Move (snaps on release) |
| Shift+drag clip body | Ripple-move: close the hole, open the drop |
| Drag clip edge | Trim (snaps on release) |
| Locked clip | Not-allowed cursor. No move or trim. |
| Double-click clip | Expand / collapse keyframe strip |
| Right-click clip / track / preview | Context menu |
| Click ruler | Seek playhead |
| Shift+scroll / trackpad | Scroll timeline |
| Ctrl/Cmd+scroll | Zoom timeline |
| Click media tile | Place clip at playhead |

## Rebinding

Settings > Keys lists every binding the registry holds, with the chord it
answers to now. Click a row and press the key you want; Esc cancels. A chord
another binding on the same surface already answers is refused, and the refusal
names what holds it. Clear leaves a binding with no key at all. Revert restores
one row, Reset all restores every row.

Rebindings are stored in `ui-settings.json` under `keys`, by binding id, spelled
the way this page spells them. A saved key this build no longer has, or a key it
cannot bind, is dropped on load rather than kept as a chord that does nothing.

The chords listed above are the shipped ones. A rebound key is shown by the
Settings row and by the tooltip of the control it drives; this page is not
rewritten when you change one.
