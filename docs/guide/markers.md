# Markers

## Markers

`M` adds a marker at the playhead: on the selected clip in clip-source time when
a selected clip spans the playhead, or on the sequence ruler otherwise. Pressing
`M` when a marker is already at the playhead, or choosing **Markers > Edit
Marker…**, opens the Edit Marker dialog to set its name, comment, colour
swatch, duration timecode, and kind (`Comment`, `Chapter`, or `Segmentation`) as
one undo step. Negative durations and durations that run past the sequence end
or the clip's source range are rejected with a reason. `Shift+M` and
`Ctrl+Shift+M` step to the next and previous marker, `Ctrl+Alt+M` removes the
marker at the playhead, and `Ctrl+Shift+Alt+M` clears all sequence markers.

The **Markers** panel (**Window > Markers**) lists sequence and clip markers in time order with
colour filter chips (`All` plus the eight palette colours). Each row shows the
marker's colour, timecode, duration when positive, clip badge for clip markers,
kind badge when not `Comment`, name, and note. Clicking a row moves the playhead
to the marker. Clicking the colour swatch cycles the eight-colour palette,
**Edit…** opens the Edit Marker dialog, **Rename** edits the name inline,
**Note** edits the comment inline (an empty note clears the comment), and
**Remove** deletes the marker. Every edit is one undo step. A marker with a
positive duration draws as a range bar on the timeline ruler or on its clip
card, and snapping targets both ends of a range marker.

## Reference

See [Menus](../reference/menus.md#markers-menu), [Controls](../reference/controls.md#markers), and [Keyboard shortcuts](../keyboard-shortcuts.md) for all marker commands and chords.
