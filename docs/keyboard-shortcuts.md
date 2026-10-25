# Keyboard shortcuts

Modifiers: Ctrl on Linux and Windows, Cmd on macOS (`platform` in GPUI).

A Shift chord on a digit or punctuation key also fires when the platform
reports the shifted symbol in its place, as GPUI on Linux does: Shift+6 arrives
as `^` and Shift+, as `<`. Symbols are read by their position on a US layout.

Every shortcut below is a registered binding, not a hardcoded key. The shipped
table is `keys::bindings::SHIPPED_BINDINGS`; a plugin registers its own through
`KeyHost::add`, and a chord that is already taken is refused rather than
overriding the binding that holds it. See
[plugins/keys.md](plugins/keys.md).

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

A paste is held to the same rules as typing: the timecode box takes digits,
the separators `:`, `;` and `.`, and a leading `+` or `-` only, a name stops at its length cap, and control characters are
dropped, so a path pasted with a trailing newline arrives as a path.

### A title in the Viewer

With the Type tool, a title clicked in the Viewer takes the keys until Esc,
another tool, or another selection.

| Shortcut | Action |
| --- | --- |
| Any printable key | Replace the selection, or insert at the caret |
| Backspace / Delete | Delete the selection, or the character before or after the caret |
| Left / Right | Move the caret one character, or to the edge of the selection |
| Ctrl+Left / Ctrl+Right | Move the caret one word |
| Home / End | Move the caret to the start or end of the line |
| Ctrl+Home / Ctrl+End | Move the caret to the start or end of the text |
| Up / Down | Move the caret one line |
| Shift with any caret key | Extend the selection |
| Ctrl+A | Select all of the text |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, or paste over the selection |
| Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y | Undo or redo; a burst of typing is one step |
| Enter | Start a new line |
| Esc | Stop typing |

Typed text and an IME commit land the same way. The app registers a platform
input handler on the window, so a character arrives as the layout produces it,
capitals and shifted symbols included, and a preedit run is marked and then
replaced by the finished text rather than appended to it.

## Panels

| Shortcut | Action |
| --- | --- |
| Shift+1 | Project panel forward |
| Shift+2 | Viewer forward. With the Viewer focused, switch between Program and the Source tab focused last, keeping that tab's position and marks. |
| Shift+3 | Timeline forward |
| Shift+4 | Effect Controls forward |
| Shift+5 | Effects & Presets forward, with its search box focused |
| Shift+6 | Toggle Audio Track Mixer / Viewer |
| Shift+7 | Scopes panel forward |
| Shift+8 | Info panel forward |
| Shift+9 | Audio Meters forward |
| Shift+0 | Toggle Assistant / Viewer |
| ` | Maximize the focused frame, or restore the split layout |
| Ctrl+Tab / Ctrl+Shift+Tab | Focus the next / previous frame |
| Ctrl+W | Close the active tab of the focused frame. In the Timeline, close the open nested sequence; in the Viewer, close the focused Source tab. |
| Shift+Alt+1 / Shift+Alt+2 / Shift+Alt+3 / Shift+Alt+4 / Shift+Alt+5 / Shift+Alt+6 / Shift+Alt+7 / Shift+Alt+8 / Shift+Alt+9 | Switch to workspace 1 to 9 in switcher order |

## Application

| Shortcut | Action |
| --- | --- |
| Ctrl+Alt+N | New project |
| Ctrl+N | New sequence with the open sequence's frame rate, canvas, and audio layout, one video and one audio track, opened |
| Ctrl+O | Open a project. On Home, switch to the Open rail. |
| Ctrl+Shift+W | Close project, return Home |
| Ctrl+S / Ctrl+Shift+S | Save / Save As |
| Ctrl+Alt+S | Save a Copy. The open project keeps its own path and unsaved state. |
| Ctrl+I | Import media |
| Ctrl+M | Export dialog |
| Ctrl+Shift+E | Export Frame: write the frame under the playhead to a PNG at the canvas size |
| Ctrl+Shift+R | Rulers along the top and left of the Program view on or off. Drag from a ruler to pull out a guide. |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z or Ctrl+Y | Redo |
| Ctrl+X / Ctrl+C / Ctrl+V | Cut / Copy / Paste. Paste lands at the playhead on the source's own lane. |
| Ctrl+Shift+V | Paste Insert. Ripples that lane right first, so nothing is overwritten. |
| Ctrl+Alt+V | Paste Attributes. Writes the copied clip's transform, filter, and effect chain onto the selection. Timing is unchanged. |
| Ctrl+D | Duplicate selected clip after itself |
| Ctrl+F | Find: the Project panel forward with its search box focused |
| Ctrl+Shift+P | Command palette |
| F10 | Open the menu bar at File with its first row lit. Pressed again, close it. |
| Alt alone | Same as F10: press and release Alt with no key, click, or wheel between. Alt with a letter is a property reveal and never opens a menu. |
| Ctrl+Shift+H | History panel forward |
| Ctrl+Alt+K | Keyboard shortcuts in Settings |
| Ctrl+, | Settings |
| Ctrl+Q | Quit |
| With palette open | Printable keys filter. Ctrl+V pastes into the filter. Backspace deletes. Up / Down light the next or previous hit and wrap. Enter runs the lit hit. Esc closes. Every command and every menu row is offered by name. An effect is offered as its Effect menu row, after its category (`Blur & Sharpen: Gaussian Blur`), so typing an effect's name and pressing Enter applies it to the selection. A generator is offered by its own name and places a clip at the playhead. |
| With a menu open | Up / Down light the next or previous row and wrap. Left / Right open the neighboring menu. Home / End light the first or last row. A letter or digit lights the next row starting with it. Enter or Space runs the lit row. Esc closes. The pointer moves the light over a lit menu. |
| With a rename open | Printable keys type, up to 64 characters. Backspace deletes. Enter applies. Esc cancels. An empty or unchanged name makes no change. |
| With a dialog open | Enter runs the dialog's primary action: Export queues the export, the unsaved-changes prompt saves and continues. Every other dialog has no primary action and swallows the key. Esc closes. |

## Tools

| Shortcut | Tool |
| --- | --- |
| V | Select |
| A | Track Select Forward. Click to select from that point forward; Shift-click selects on one track. |
| Shift+A | Track Select Backward. Click to select up to that point; Shift-click selects on one track. |
| B | Ripple Trim. Drag an edge and the rest of the lane follows. |
| N | Rolling Edit. Drag a cut to move it, keeping both clips' total length. |
| R | Rate Stretch. Drag a clip edge to retime it instead of trimming. |
| C | Razor |
| Y | Slip. Drag inside a clip to change what it shows without moving it. |
| U | Slide. Drag a clip between its neighbors; they absorb the move. |
| H | Hand. Drag the timeline to pan. |
| Z | Zoom. Click zooms in around the pointer. Alt-click zooms out. |
| P | Pen. Add keyframes on a clip's line or motion path; draw or edit masks in the Viewer. |
| T | Type. Create or edit a text clip in the Viewer. |

## Transport

| Shortcut | Action |
| --- | --- |
| Space | Play / pause |
| Shift+Space | Play work area (`In` to `Out`, or the whole sequence when unset) |
| Enter | Render In to Out: pre-render the work area (`In` to `Out`, or the whole sequence when unset) into the render-files cache |
| J | Shuttle reverse. Tap again for 2x, 4x, 8x. |
| K | Stop shuttle. The transport returns to 1x. |
| L | Shuttle forward. Tap again for 2x, 4x, 8x. |
| Shift+J / Shift+L | Step shuttle speed down / up by 10% (`[-8x, 8x]`) |
| Hold K + tap J / Hold K + tap L | Step one frame back / forward |
| Hold K + hold J / Hold K + hold L | Play at 1/4 speed in reverse / forward until release |
| Left / Right | Playhead one frame back / forward |
| Shift+Left / Shift+Right | Playhead five frames back / forward |
| Up / Down | Playhead to the previous / next cut on the targeted tracks |
| Shift+Up / Shift+Down | Playhead to the previous / next cut on every track |
| Home / End | Playhead to sequence start / end |
| Ctrl+Shift+L | Toggle loop playback |
| Digits, then Enter | Over the Timeline or the Program view: playhead to the typed timecode |
| Keypad + or -, digits, then Enter | With nothing selected: playhead forward or back by the typed duration |

Audio plays at every forward and reverse shuttle speed up to 4x with its pitch
held. Above 4x (`|speed| > 4x`) the audio is muted and the rate appears in the
Viewer transport with a muted speaker beside it; click the badge to return to
1x with sound. Space and K also return to 1x, as does reaching either end of
the sequence when loop playback is off.

With the Viewer focused on a Source tab, the transport keys play and move that
tab instead of the sequence: Space, Shift+Space (the source In to Out), J, K,
L, K with J or L, Left, Right, Shift+Left, Shift+Right, Home, End, Shift+I, and
Shift+O. J and L reach 8x; the source plays without sound. Starting either
clock stops the other, and focusing another tab stops the source.

## Marking

| Shortcut | Action |
| --- | --- |
| I | Set In point at playhead. With a source loaded in the monitor, marks the source instead. |
| O | Set Out point at playhead. With a source loaded, marks the source (exclusive, so the frame on screen is kept). |
| X | Mark In and Out around the clip under the playhead on the targeted tracks |
| Shift+I / Shift+O | Playhead to In / Out point |
| Ctrl+Shift+I / Ctrl+Shift+O | Clear In / Out point, or the source's when one is loaded |
| Ctrl+Shift+X | Clear In and Out |
| M | Add marker at playhead, named and colored automatically. On an existing marker, rename it. |
| Shift+M / Ctrl+Shift+M | Playhead to next / previous marker |
| Ctrl+Alt+M | Delete the marker at the playhead |
| Ctrl+Shift+Alt+M | Delete every marker, as one undo step |
| F | Match Frame: open the source of the clip under the playhead at that frame |

## Editing

| Shortcut | Action |
| --- | --- |
| , | Insert the focused Source tab's marked range at the sequence In point (playhead when unmarked) on the patched lanes, rippling sync-locked lanes. With Program focused, insert the selected pool items end to end from the playhead. |
| . | Overwrite with the focused Source tab's marked range on the patched lanes. With In and Out on both the source and the sequence, either edit is four-point: the take is retimed to fill the sequence span. With Program focused, overwrite with the selected pool items end to end from the playhead. |
| Shift+, | Insert the selected pool item at the playhead, rippling that lane right |
| ; / ' | Lift / Extract the In to Out range on the targeted tracks. Extract closes the gap. |
| Ctrl+K | Add Edit: cut the selected clips under the playhead, or every targeted track when nothing is selected |
| Ctrl+Shift+K | Add Edit to every unlocked track |
| Q / W | Ripple trim the head / tail of the clip under the playhead to the playhead |
| Alt+[ / Alt+] | Trim the head / tail to the playhead, leaving the gap |
| Delete / Backspace | Clear the selection and leave its gap, or close a selected gap |
| Shift+Delete / Shift+Backspace | Ripple Delete |
| Ctrl+Left / Ctrl+Right | Trim the selected edge one frame, or move the selected clip when no edge is selected |
| Ctrl+Shift+Left / Ctrl+Shift+Right | The same by five frames |
| Keypad + or -, digits, then Enter | Move the selected clips later or earlier by the typed duration, as one undo step, or trim the selected edge by it |
| Ctrl+T | Default transition at the selected edge, or at the cut nearest the playhead on the targeted tracks |
| Ctrl+Shift+T | Constant Power crossfade at the selected edge, or at the cut nearest the playhead on the targeted audio tracks |
| D | Select the clips under the playhead on the targeted tracks |
| Ctrl+A / Ctrl+Shift+A | Select all unlocked clips / deselect all |
| S | Snapping on or off |
| Ctrl+L | Link the selection, or unlink it |
| ] / [ | Selected clip gain up / down 1 dB |
| Shift+] / Shift+[ | Selected clip gain up / down 6 dB |
| Alt+Left / Alt+Right | Shift the selected clip's whole lane one second earlier or later |
| Alt+D | Cycle the ducking amount |
| Shift+Alt+D | Duck the selection under the voice track at that amount |
| Shift+Alt+C | Detect shots on the selected clip |
| Shift+Alt+Q | Detect silence on the selected clip |
| Shift+Alt+B | Detect beats on the selected clip |
| Ctrl+Alt+R | Reset every effect on the selected clip to its catalog defaults |
| Shift+E | Enable or disable the selected clips. A disabled clip draws at 40% and is left out of the preview, export, and mix. |
| Ctrl+G / Ctrl+Shift+G | Group the selected clips / dissolve their groups |
| Ctrl+R | Speed/Duration dialog for the selected clips |
| Ctrl+Alt+T | Time Remap on the selected clip |
| G | Audio Gain dialog for the selected clips |

## Properties and Keyframes

| Shortcut | Action |
| --- | --- |
| Alt+P / Shift+Alt+P | Reveal Position on the selected clip / add or remove Position from the visible property rows |
| Alt+S / Shift+Alt+S | Reveal Scale / add or remove Scale |
| Alt+R / Shift+Alt+R | Reveal Rotation / add or remove Rotation |
| Alt+T / Shift+Alt+T | Reveal Opacity / add or remove Opacity |
| Alt+A / Shift+Alt+A | Reveal Anchor Point / add or remove Anchor Point |
| Alt+E / Shift+Alt+E | Reveal Effects / add or remove Effects |
| Alt+M / Shift+Alt+M | Reveal Masks / add or remove Masks |
| Alt+L / Shift+Alt+L | Reveal Audio Levels / add or remove Audio Levels |
| Alt+U / Shift+Alt+U | Reveal animated properties (pressed twice: all properties modified from default) / add or remove animated properties |
| Alt+` | Close every open property row |
| Shift+Alt+Left / Shift+Alt+Right | Playhead to the selected clip's previous / next keyframe on any property |
| F9 | Easy Ease on selected keyframes (33.33% incoming and outgoing influence, 0 speed) |
| Shift+F9 / Ctrl+Shift+F9 | Easy Ease In / Easy Ease Out on selected keyframes |
| Ctrl+Alt+H | Toggle Hold interpolation on selected keyframes |
| Shift+F3 | Toggle the timeline graph editor |

## Timeline view

| Shortcut | Action |
| --- | --- |
| = or + / - | Zoom in / out around the playhead |
| \ | Fit the sequence |
| Shift+F or Shift+Z | Fit the selection |
| Ctrl+= / Ctrl+- | Every track taller / shorter by one step |
| Escape | Cancel an in-progress splitter drag, then close dialog, then context, then menu, then return to the pointer, then clear selection. No-op on Home except dialog, context, and menu. |

## Viewer

These keys apply while the Viewer has focus and outrank the timeline's.

| Shortcut | Action |
| --- | --- |
| Left / Right / Up / Down | Move the selected clip 1 px. With nothing selected, Left and Right step the playhead one frame and Up and Down move it to the previous or next cut. On a Source tab, Left and Right step the source one frame. |
| Shift+Left / Shift+Right / Shift+Up / Shift+Down | Move the selected clip 10 px. With nothing selected, Shift+Left and Shift+Right step the playhead five frames. On a Source tab, Shift+Left and Shift+Right step the source five frames. |
| Shift+/ | Fit the frame to the Viewer |

## Pointer

| Gesture | Action |
| --- | --- |
| Click clip | Select the clip and everything linked to it |
| Alt+click clip | Select the clicked clip only, ignoring its links |
| Shift+click clip | Add to selection |
| Ctrl/Cmd+click clip | Toggle clip in selection |
| Drag clip body | Move (snaps on release) |
| Shift+drag clip body | Ripple-move: close the hole, open the drop |
| Click or drag clip edge | Select edge (`Ctrl+Left` / `Ctrl+Right` trims) or drag to trim with live delta feedback |
| Click empty lane gap | Select the gap (`Delete` / `Backspace` ripple-deletes it) |
| Drag V / A divider | Adjust height split between video and audio lane groups |
| Locked clip | Not-allowed cursor. No move or trim. |
| Double-click clip | Rename clip (name band); otherwise open a media clip's take in a linked Source tab, open a nested sequence, or open a generated clip's property rows |
| Right-click clip / track / preview | Context menu |
| Click or drag ruler | Scrub playhead |
| Shift+scroll / trackpad | Scroll timeline horizontally |
| Shift+scroll track header | Step that track's height through Mini (20 px), Small (32 px), Medium (48 px), and Large (72 px) |
| Alt+scroll or Ctrl/Cmd+scroll | Zoom timeline around pointer |
| Click media tile | Select it. Ctrl/Cmd+click toggles it in the selection; Shift+click selects the run from the last tile picked. |
| Double-click media tile | Open it in a Source tab of the Viewer, or focus its open tab |
| Drag media tile to a lane | Place it there through source patching |
| Click a Viewer tab | Focus Program or that Source tab. The close button or a middle click closes a Source tab. |
| Drag from a Viewer ruler | Pull out a guide; drop it back on its ruler or off the frame to delete it |
| Click a title with the Type tool | Place the caret at the nearest character boundary |
| Drag across a title with the Type tool | Select characters |
| Shift+click a title with the Type tool | Extend the selection to the click |
| Drag a Settings > Keys row onto the keyboard image | Bind it to that key with the modifiers the image holds |
| Click Ctrl, Shift or Alt on the keyboard image | Show the chords with that modifier held |

## Rebinding

Settings > Keys lists every binding the registry holds, with the chord it
answers to now. Click a row and press the key you want; Esc cancels. A chord
another binding on the same surface already answers asks before it moves: the
row names the holder and offers Move and Keep. Enter moves the chord and leaves
the holder with no key; Esc leaves the chord with the holder and ends the
capture. Clear leaves a binding with no key at all. Revert restores one row,
Reset all restores every row.

The Settings filter also searches the bindings, by name and by chord: `razor`
and `ctrl+k` each list the rows they match.

Above the list, a keyboard image colors each key by the kinds of command it
holds: Editor, Timeline, Viewer, Home, and Plugin for a command a plugin
contributed. Click Ctrl, Shift or Alt on the image to show the chords with that
modifier held, and hover a key to read what it runs. Drag a row onto a key to
bind it there with the modifiers the image holds. With a row armed, clicking a
key binds it the same as pressing it.

Rebindings are stored in `ui-settings.json` under `keys`, by binding id, spelled
the way this page spells them. A saved key this build no longer has, or a key it
cannot bind, is dropped on load rather than kept as a chord that does nothing.

### Keymaps

A keymap is a named set of rebindings in `<data dir>/keymaps/<name>.toml`. Type
a name into Save keys as and press Enter to save the keys in force under it.
Click a set's name to load it in place of every rebinding. Delete set deletes
the file of the set in force and leaves its keys in force. Import adds a keymap
file to the sets under its file name, or `<name> 2` when that name is taken,
and loads it. Export writes the keys in force to a file.

```toml
version = 1

[keys]
key-play = "Ctrl+space"
key-tool-razor = ""
```

A keymap holds only the rows that differ from the shipped chords. An empty
chord is a binding cleared on purpose. A file with another version, an unknown
field, or a chord no key can carry is refused whole, with the reason. A row for
a binding this build does not have is skipped on load, and the status line
names it.

The chords listed above are the shipped ones. A rebound key is shown by the
Settings row and by the tooltip of the control it drives; this page is not
rewritten when you change one.
