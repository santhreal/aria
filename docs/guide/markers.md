# Markers

## Markers

`M` adds a marker at the playhead: on the selected clip in clip-source time when
a selected clip spans the playhead, or on the sequence ruler otherwise. Pressing
`M` when a marker is already at the playhead, or choosing **Markers > Edit
Marker…**, opens the Edit Marker dialog to set its name, comment, color
swatch, duration timecode, and kind (`Comment`, `Chapter`, or `Segmentation`) as
one undo step. Negative durations and durations that run past the sequence end
or the clip's source range are rejected with a reason. `Shift+M` and
`Ctrl+Shift+M` step to the next and previous marker, `Ctrl+Alt+M` removes the
marker at the playhead, and `Ctrl+Shift+Alt+M` clears all sequence markers.

The **Markers** panel (**Window > Markers**) lists sequence and clip markers in time order with
color filter chips (`All` plus the eight palette colors). Each row shows the
marker's color, timecode, duration when positive, clip badge for clip markers,
kind badge when not `Comment`, name, and note. Clicking a row moves the playhead
to the marker. Clicking the color swatch cycles the eight-color palette,
**Edit…** opens the Edit Marker dialog, **Rename** edits the name inline,
**Note** edits the comment inline (an empty note clears the comment), and
**Remove** deletes the marker. Every edit is one undo step. A marker with a
positive duration draws as a range bar on the timeline ruler or on its clip
card, and snapping targets both ends of a range marker.


## Example

Add sequence and clip markers, assign a color and duration in the Edit Marker dialog, and navigate between markers:

1. With no clip selected (or with a clip selected to mark in clip-source time), move the playhead to the target frame and press <kbd>M</kbd> (`key-marker-add`) or click [`timeline-marker`](../reference/controls.md#timeline) in the timeline header (or choose **Markers > Add Marker** / run **Add Marker** (`pal-marker`)). A marker pin appears on the timeline ruler (or on the selected clip card).
2. Press <kbd>M</kbd> (`key-marker-add`) again at the same frame or choose **Markers > Edit Marker…**. The Edit Marker dialog opens over Aria.
3. Type a marker name and comment, pick a color swatch, enter a positive duration timecode to turn the marker into a range bar, and click [`marker-edit-save`](../reference/controls.md#modal-dialogs). The timeline ruler draws the colored range marker across the specified duration.
4. Choose **Window > Markers** to open the Markers panel, filter by color or click [`marker-filter-all`](../reference/controls.md#markers) to show all colors, and press <kbd>Shift+M</kbd> (`key-marker-next`) and <kbd>Ctrl+Shift+M</kbd> (`key-marker-prev`) (or choose **Markers > Go to Next Marker** and **Markers > Go to Previous Marker**) to jump the playhead between markers, or click [`marker-remove`](../reference/controls.md#markers) (or press <kbd>Ctrl+Alt+M</kbd> (`key-marker-remove`)) to delete a marker. The playhead seeks directly to each chosen marker frame in the Viewer.

## Reference

See [Menus](../reference/menus.md#markers-menu), [Controls](../reference/controls.md#markers), and [Keyboard shortcuts](../keyboard-shortcuts.md) for all marker commands and chords.
