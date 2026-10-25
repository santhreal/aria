# Menus Reference

The application menu bar contains ten top-level menus (`AppMenu::ALL` in `crates/aria-ui/src/types_editor.rs`), assembled by `items_for(menu, state)` in `crates/aria-ui/src/chrome/menu_items.rs` from one module per menu: `menu_file.rs`, `menu_edit.rs`, `menu_clip.rs`, `menu_sequence.rs`, `menu_markers.rs`, `menu_effect.rs`, `menu_animation.rs`, `menu_view_rows.rs`, `menu_window.rs`, and `menu_help.rs`. Each row includes a stable ID, an optional submenu caption (`under`), a display label, an optional keyboard binding ID (`key`), a disabled-when condition (`RowBlocked` in `crates/aria-ui/src/chrome/menu_guards.rs`), an optional check-mark predicate (`RowChecked`), and an action handler (`RowRun`).

## Worked example

Set an in-point and out-point around a range, add a sequence marker, and render a preview cache for that span:

1. Move the playhead to the start of the range and choose **Markers > Mark In** (`seq-mark-in`, `I`).
2. Move the playhead to the end of the range and choose **Markers > Mark Out** (`seq-mark-out`, `O`).
3. Choose **Markers > Add Marker** (`seq-marker`, `M`), then choose **Markers > Edit Marker…** (`seq-marker-rename`) to assign a name and color.
4. Choose **Sequence > Render In to Out** (`seq-render-in-to-out`, `Enter`) to start a preview-render job covering the marked span.

## File menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `file-new` | `none` | New Project | `Ctrl+Alt+N` | Always enabled | Starts a new project (opens `aria.dialog.unsaved` first when the active project has unsaved edits). |
| `file-new-sequence` | `none` | New Sequence | `Ctrl+N` | Always enabled | Creates a new sequence in the open project and switches the timeline to it. |
| `file-open` | `none` | Open… | `Ctrl+O` | Always enabled | Opens the file picker for `.aria` project files (opens `aria.dialog.unsaved` first when the active project has unsaved edits). |
| `file-use-template` | `none` | Use Template… | `none` | Always enabled | Opens the file picker for `.ariat` template bundles and prompts for each replaceable media slot. |
| `file-home` | `Open Recent` | All Recent Projects… | `none` | Always enabled | Switches the window route to Home (`UiRoute::Home`). |
| `file-close` | `none` | Close | `Ctrl+Shift+W` | Always enabled | Closes the open project and returns to Home (`close_project`, opening `aria.dialog.unsaved` first when the active project has unsaved edits). |
| `file-rename` | `none` | Rename Project… | `none` | Always enabled | Opens `aria.overlay.rename` (`RenameTarget::Project`) to rename the project. |
| `file-save` | `none` | Save | `Ctrl+S` | Always enabled | Saves the project to its `.aria` path, or to `<data dir>/untitled.aria` when untitled. |
| `file-save-as` | `none` | Save As… | `Ctrl+Shift+S` | Always enabled | Prompts for a new `.aria` path, writes the project there, and updates the session path. |
| `file-save-copy` | `none` | Save a Copy… | `Ctrl+Alt+S` | Always enabled | Writes a copy of the project to a chosen `.aria` path without changing the session path. |
| `file-save-template` | `none` | Save as Template… | `none` | Always enabled | Prompts for a destination `.ariat` path and packages the project as a template bundle. |
| `file-revert` | `none` | Revert | `none` | Project has never been saved or has no unsaved changes (`guards::revertible`) | Reloads the project from its last saved `.aria` file on disk. |
| `file-import` | `none` | Import… | `Ctrl+I` | Always enabled | Opens the Import file picker: preset files go to the preset library, a timeline file is read into the timeline, and other files are imported into the media pool and placed on the timeline from the playhead. Each timeline or media file that arrives heads File > Import Recent. |
| `file-import-recent-clear` | `Import Recent` | Clear Recent Imports | `none` | No file has been imported yet (`guards::import_recents`) | Clears the persisted recent imports list (`import-recents.json`). |
| `file-import-timeline` | `none` | Import Timeline… | `none` | Always enabled | Opens the file picker for timeline interchange files (`.fcpxml`, `.otio`, `.edl`, `.srt`, `.vtt`). |
| `file-relink-all` | `none` | Relink All… | `none` | Always enabled | Prompts for a search folder and relinks each offline media pool item that has a unique filename match with matching frame size, duration, and audio presence. |
| `file-export` | `Export` | Media… | `Ctrl+M` | Always enabled | Opens the Export dialog (`aria.dialog.export`). |
| `file-export-frame` | `Export` | Frame… | `Ctrl+Shift+E` | Always enabled | Renders the frame at the playhead at full sequence resolution and saves it as a PNG file. |
| `file-export-timeline` | `Export` | Timeline… | `none` | No timeline exporter is mounted (`guards::exporters`) | Prompts for a destination file and exports the active sequence through the chosen timeline exporter. |
| `file-install-plugin` | `none` | Install Plugin… | `none` | Always enabled | Prompts for an `.ariaplugin` archive and opens `aria.dialog.install-plugin`. |
| `file-project-settings` | `none` | Project Settings… | `none` | Always enabled | Opens `aria.dialog.settings` on the `Canvas` section. |
| `file-quit` | `none` | Quit | `Ctrl+Q` | Always enabled | Requests application shutdown (opens `aria.dialog.unsaved` first when the project is dirty). |

## Edit menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `edit-undo` | `none` | Undo | `Ctrl+Z` | Nothing to undo (`guards::undo`) | Reverts the most recent edit step on the undo stack. |
| `edit-redo` | `none` | Redo | `Ctrl+Shift+Z` | Nothing to redo (`guards::redo`) | Reapplies the most recently undone edit step. |
| `edit-cut` | `none` | Cut | `Ctrl+X` | No clip is selected (`guards::selection`) | Copies selected clips to the clipboard and removes them from the sequence. |
| `edit-copy` | `none` | Copy | `Ctrl+C` | No clip is selected (`guards::selection`) | Copies selected clips and their attributes to the clipboard. |
| `edit-paste` | `none` | Paste | `Ctrl+V` | Nothing has been copied or cut (`guards::clipboard`) | Pastes the clipboard clip onto its source track at the playhead when the target span is free. |
| `edit-paste-insert` | `none` | Paste Insert | `Ctrl+Shift+V` | Nothing has been copied or cut (`guards::clipboard`) | Ripple-inserts clipboard clips at the playhead, shifting downstream clips right. |
| `edit-paste-attributes` | `none` | Paste Attributes | `Ctrl+Alt+V` | Nothing has been copied or cut (`guards::clipboard`) | Copies the animated transform, filter preset, and video effect chain from the clipboard clip onto selected clips. |
| `edit-remove-attributes` | `none` | Remove Attributes… | `none` | No clip is selected (`guards::selection`) | Opens `aria.dialog.remove-attributes` to reset chosen attribute categories on selected clips. |
| `edit-paste-fit` | `none` | Paste to Fit | `none` | Nothing has been copied or cut (`guards::clipboard`) | Pastes the clipboard clip scaled in speed to fit the active In-to-Out span. |
| `edit-clear` | `none` | Clear | `Backspace` | No clip is selected (`guards::selection`) | Deletes selected clips without closing the timeline gap. |
| `edit-ripple` | `none` | Ripple Delete | `Shift+Backspace` | No clip is selected (`guards::selection`) | Deletes selected clips and shifts subsequent clips left to close the gap. |
| `edit-dup` | `none` | Duplicate | `Ctrl+D` | No clip is selected (`guards::selection`) | Places a copy of each selected clip immediately after the original on the same track. |
| `edit-select-all` | `none` | Select All | `Ctrl+A` | Always enabled | Selects every clip on unlocked tracks in the active sequence. |
| `edit-deselect` | `none` | Deselect All | `Ctrl+Shift+A` | No clip is selected (`guards::selection`) | Clears the clip, transition, and keyframe selection. |
| `edit-find` | `none` | Find… | `Ctrl+F` | Always enabled | Focuses the Project panel media search field. |
| `edit-label-violet` | `Label` | Violet | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Violet`. |
| `edit-label-iris` | `Label` | Iris | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Iris`. |
| `edit-label-caribbean` | `Label` | Caribbean | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Caribbean`. |
| `edit-label-lavender` | `Label` | Lavender | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Lavender`. |
| `edit-label-cerulean` | `Label` | Cerulean | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Cerulean`. |
| `edit-label-forest` | `Label` | Forest | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Forest`. |
| `edit-label-rose` | `Label` | Rose | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Rose`. |
| `edit-label-mango` | `Label` | Mango | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Mango`. |
| `edit-label-purple` | `Label` | Purple | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Purple`. |
| `edit-label-blue` | `Label` | Blue | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Blue`. |
| `edit-label-teal` | `Label` | Teal | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Teal`. |
| `edit-label-magenta` | `Label` | Magenta | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Magenta`. |
| `edit-label-tan` | `Label` | Tan | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Tan`. |
| `edit-label-green` | `Label` | Green | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Green`. |
| `edit-label-brown` | `Label` | Brown | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Brown`. |
| `edit-label-yellow` | `Label` | Yellow | `none` | No clip is selected (`guards::selection`) | Sets the label color of selected clips to `LabelColor::Yellow`. |
| `edit-label-none` | `Label` | None | `none` | No clip is selected (`guards::selection`) | Clears the label color (`None`) on selected clips. |
| `edit-keyboard-shortcuts` | `none` | Keyboard Shortcuts… | `Ctrl+Alt+K` | Always enabled | Opens `aria.dialog.settings` on the `Shortcuts` section. |
| `edit-preferences` | `none` | Preferences… | `Ctrl+,` | Always enabled | Opens `aria.dialog.settings`. |

## Clip menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `clip-rename` | `none` | Rename… | `none` | No clip is selected (`guards::selection`) | Opens `aria.overlay.rename` to edit the selected clip's name. |
| `clip-subclip` | `none` | Make Subclip | `none` | Open a source in the Source monitor first (`guards::source_range`) | Creates a subclip media item in the pool from the active Source monitor range. |
| `clip-speed-duration` | `none` | Speed/Duration… | `Ctrl+R` | No clip is selected (`guards::selection`) | Opens `aria.dialog.speed-duration`. |
| `clip-time-remap` | `none` | Time Remapping | `Ctrl+Alt+T` | No clip is selected (`guards::selection`) | Toggles variable-speed time remapping on the selected clip. |
| `clip-enable` | `none` | Enable | `Shift+E` | No clip is selected (`guards::selection`) | Toggles `enabled` on every selected clip. |
| `seq-link` | `none` | Link | `none` | Select two or more clips (`guards::several`) | Links selected clips into a shared `link_group`. |
| `seq-unlink` | `none` | Unlink | `none` | Selected clips are not linked (`guards::linked`) | Clears the `link_group` on selected clips. |
| `clip-group` | `none` | Group | `Ctrl+G` | Select two or more clips (`guards::several`) | Assigns a shared group ID to all selected clips. |
| `clip-ungroup` | `none` | Ungroup | `Ctrl+Shift+G` | Selected clips are not a group (`guards::grouped`) | Clears the group ID on selected clips. |
| `seq-nest` | `none` | Nest… | `none` | No clip is selected (`guards::selection`) | Moves selected clips into a new nested sequence and replaces them with a compound clip. |
| `clip-replace-source` | `Replace with Clip` | From Source Monitor | `none` | Open a source in the Source monitor first (`guards::source`) | Replaces the selected clip's media source with the active Source monitor item. |
| `seq-freeze` | `none` | Freeze Frame | `none` | No clip is selected (`guards::selection`) | Inserts a frozen still-frame hold at the playhead on the selected clip. |
| `seq-bake-clip` | `none` | Render and Replace | `none` | No clip is selected (`guards::selection`) | Renders the selected clip with its effects to a media file and replaces its source. |
| `seq-bake-restore` | `none` | Restore Unrendered | `none` | No clip is selected (`guards::selection`) | Restores a rendered-and-replaced clip back to its original source and effects. |
| `clip-audio-gain` | `none` | Audio Gain… | `G` | No clip is selected (`guards::selection`) | Opens `aria.dialog.audio-gain`. |
| `clip-channels-stereo` | `Audio Channels` | Stereo | `none` | Select a clip with sound (`guards::sound`) | Routes the clip's left and right source channels as normal stereo (`AudioChannels::Stereo`). |
| `clip-channels-swap` | `Audio Channels` | Swap Channels | `none` | Select a clip with sound (`guards::sound`) | Swaps the clip's left and right audio channels (`AudioChannels::Swap`). |
| `clip-channels-left` | `Audio Channels` | Left to Both | `none` | Select a clip with sound (`guards::sound`) | Sends the clip's left source channel to both stereo outputs (`AudioChannels::LeftToBoth`). |
| `clip-channels-right` | `Audio Channels` | Right to Both | `none` | Select a clip with sound (`guards::sound`) | Sends the clip's right source channel to both stereo outputs (`AudioChannels::RightToBoth`). |
| `clip-channels-mono` | `Audio Channels` | Mono | `none` | Select a clip with sound (`guards::sound`) | Sums the clip's left and right channels to mono (`AudioChannels::Mono`). |
| `seq-extract-audio` | `none` | Extract Audio | `none` | No clip is selected (`guards::selection`) | Extracts the selected video clip's audio stream onto an audio lane. |
| `seq-flip-h` | `Transform` | Flip Horizontal | `none` | No clip is selected (`guards::selection`) | Toggles horizontal mirror flip on the selected clips. |
| `seq-flip-v` | `Transform` | Flip Vertical | `none` | No clip is selected (`guards::selection`) | Toggles vertical mirror flip on the selected clips. |
| `seq-crop-reset` | `Transform` | Reset Framing | `none` | No clip is selected (`guards::selection`) | Clears all four crop edges on the selected clips. |
| `seq-matte-luma` | `Track Matte` | Luma Matte from Above | `none` | No clip is selected (`guards::selection`) | Uses the luminance of the clip directly above as the selected clip's track matte (`MatteSource::Luma`). |
| `seq-matte-alpha` | `Track Matte` | Alpha Matte from Above | `none` | No clip is selected (`guards::selection`) | Uses the alpha channel of the clip directly above as the selected clip's track matte (`MatteSource::Alpha`). |
| `seq-matte-clear` | `Track Matte` | Remove Track Matte | `none` | No clip is selected (`guards::selection`) | Clears the track matte source on the selected clips. |
| `clip-analyze:aria.beats` | `Analyze` | Detect Beats | `none` | Select a clip with sound (`guards::sound`) | Runs the registered beat analyzer (`aria.beats`) on the selected clip and adds beat markers. |

## Sequence menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `seq-settings` | `none` | Sequence Settings… | `none` | Always enabled | Opens `aria.dialog.settings` on the `Canvas` section. |
| `seq-render-in-to-out` | `none` | Render In to Out | `Enter` | The sequence has no clips (`guards::clips`) | Renders preview cache files across the marked In/Out span (or the whole sequence). |
| `seq-render-selection` | `none` | Render Selection | `none` | No clip is selected (`guards::selection`) | Renders preview cache files covering the selected clips. |
| `seq-delete-render-files` | `none` | Delete Render Files | `none` | Always enabled | Deletes cached preview render files for the sequence. |
| `seq-add-edit` | `none` | Add Edit | `Ctrl+K` | The sequence has no clips (`guards::clips`) | Splits selected clips (or unlocked tracks) at the playhead. |
| `seq-add-edit-all` | `none` | Add Edit to All Tracks | `Ctrl+Shift+K` | The sequence has no clips (`guards::clips`) | Splits every clip intersecting the playhead across all unlocked tracks. |
| `seq-trim-edit` | `none` | Trim Edit | `none` | The sequence has no clips (`guards::clips`) | Selects the edit point nearest the playhead for keyboard trim nudging. |
| `seq-extend-edit` | `none` | Extend Edit | `none` | Select an edit point first (`guards::edge`) | Extends the selected edit point to the current playhead tick. |
| `seq-default-transitions` | `none` | Apply Default Transitions | `Ctrl+T` | The sequence has no clips (`guards::clips`) | Applies the default picture transition (`crossfade`) at the selected cut. |
| `seq-lift` | `none` | Lift | `;` | Mark an In and an Out point first (`guards::range`) | Removes material between In and Out and leaves a timeline gap. |
| `seq-extract` | `none` | Extract | `'` | Mark an In and an Out point first (`guards::range`) | Removes material between In and Out and ripple-closes the gap. |
| `seq-snap` | `none` | Snap | `S` | Always enabled | Toggles magnetic timeline snapping (`snap_enabled`). |
| `seq-linked-selection` | `none` | Linked Selection | `none` | Always enabled | Toggles whether clicking a linked clip selects its linked siblings (`linked_selection`). |
| `seq-v` | `Add Tracks` | Video Track | `none` | Always enabled | Appends a new video track (`V<n>`) to the active sequence. |
| `seq-a` | `Add Tracks` | Audio Track | `none` | Always enabled | Appends a new audio track (`A<n>`) to the active sequence. |
| `seq-delete-tracks` | `none` | Delete Empty Tracks | `none` | Every track holds a clip (`guards::empty_tracks`) | Deletes empty unlocked video and audio tracks from the active sequence while keeping at least one video track and one audio track. |
| `seq-adjustment` | `none` | Add Adjustment Layer | `none` | Always enabled | Places an adjustment layer clip on a free or new adjustment track across the marked In/Out range, the selected clips' span, the full sequence, or five seconds from the start of an empty sequence. |
| `seq-match-frame` | `none` | Match Frame | `F` | No clip is selected (`guards::selection`) | Opens the selected clip's source in the Source monitor at the matching source frame. |
| `seq-source-insert` | `Source` | Insert | `none` | Open a source in the Source monitor first (`guards::source_range`) | Ripple-inserts the active Source monitor range at the playhead. |
| `seq-source-overwrite` | `Source` | Overwrite | `none` | Open a source in the Source monitor first (`guards::source_range`) | Overwrites the active Source monitor range onto the sequence at the playhead. |
| `seq-ripple-insert` | `Source` | Insert Pool Selection | `none` | No media is selected in the pool (`guards::pool_selection`) | Ripple-inserts the selected media pool items at the playhead. |
| `seq-ripple-in` | `none` | Ripple Trim In | `none` | No clip is selected (`guards::selection`) | Trims the selected clip's in-point to the playhead and ripples the lane. |
| `seq-ripple-out` | `none` | Ripple Trim Out | `none` | No clip is selected (`guards::selection`) | Trims the selected clip's out-point to the playhead and ripples the lane. |
| `seq-transition-crossfade` | `none` | Add Cross Fade at Cut | `none` | No clip is selected (`guards::selection`) | Applies a `crossfade` transition to the selected clip's cut. |
| `seq-transition-clear` | `none` | Remove Transition at Cut | `none` | No clip is selected (`guards::selection`) | Removes the transition from the selected clip's cut. |
| `seq-lane-shift-later` | `none` | Shift Lane One Second Later | `none` | Always enabled | Shifts every clip on the selected clip's track later by one second. |
| `seq-lane-shift-earlier` | `none` | Shift Lane One Second Earlier | `none` | Always enabled | Shifts every clip on the selected clip's track earlier by one second. |
| `seq-canvas-background` | `none` | Reset Canvas Background | `none` | Always enabled | Restores the sequence canvas background color to `[0, 0, 0, 255]`. |
| `seq-duck-amount` | `Ducking` | Cycle Duck Amount | `none` | Always enabled | Cycles the sidechain ducking attenuation depth (`-6 dB`, `-12 dB`, `-18 dB`, `-24 dB`). |
| `seq-duck-attack` | `Ducking` | Cycle Duck Attack | `none` | Always enabled | Cycles the sidechain ducking attack time. |
| `seq-duck-release` | `Ducking` | Cycle Duck Release | `none` | Always enabled | Cycles the sidechain ducking release time. |
| `seq-nest-open` | `Nested Sequence` | Open Nested Sequence | `none` | Always enabled | Opens the selected nested sequence clip's timeline. |
| `seq-nest-leave` | `Nested Sequence` | Leave Nested Sequence | `none` | Always enabled | Returns from an open nested sequence to the root sequence. |
| `seq-nest-place` | `Nested Sequence` | Place Nest at Playhead | `none` | Always enabled | Places a clip referencing the selected nested sequence at the playhead. |
| `seq-analyze:aria.silence` | `Analyze` | Detect Silence | `none` | The sequence has no clips (`guards::clips`) | Runs the registered silence analyzer (`aria.silence`) across the active sequence. |
| `seq-analyze:aria.loudness` | `Analyze` | Measure Loudness | `none` | The sequence has no clips (`guards::clips`) | Runs the registered EBU R128 loudness analyzer (`aria.loudness`) across the sequence or In/Out range. |

## Markers menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `seq-mark-in` | `none` | Mark In | `I` | Always enabled | Sets the In point at the playhead on the active monitor. |
| `seq-mark-out` | `none` | Mark Out | `O` | Always enabled | Sets the Out point at the playhead on the active monitor. |
| `mark-clip` | `none` | Mark Clip | `X` | The sequence has no clips (`guards::clips`) | Sets sequence In and Out points to the span of the clip at the playhead. |
| `mark-go-in` | `none` | Go to In | `Shift+I` | No In point is set (`guards::mark_in`) | Seeks the playhead to the sequence In point. |
| `mark-go-out` | `none` | Go to Out | `Shift+O` | No Out point is set (`guards::mark_out`) | Seeks the playhead to the sequence Out point. |
| `seq-clear-in` | `none` | Clear In | `Ctrl+Shift+I` | No In point is set (`guards::mark_in`) | Clears the active In point. |
| `seq-clear-out` | `none` | Clear Out | `Ctrl+Shift+O` | No Out point is set (`guards::mark_out`) | Clears the active Out point. |
| `mark-clear` | `none` | Clear In and Out | `Ctrl+Shift+X` | Neither an In nor an Out point is set (`guards::marks`) | Clears both the In point and the Out point. |
| `seq-marker` | `none` | Add Marker | `M` | Always enabled | Adds a sequence marker at the playhead, or opens `aria.dialog.marker-edit` when a marker is already at the playhead. |
| `seq-marker-next` | `none` | Go to Next Marker | `Shift+M` | The sequence has no markers (`guards::markers`) | Seeks the playhead to the next sequence marker. |
| `seq-marker-prev` | `none` | Go to Previous Marker | `Ctrl+Shift+M` | The sequence has no markers (`guards::markers`) | Seeks the playhead to the previous sequence marker. |
| `seq-marker-remove` | `none` | Clear Marker | `Ctrl+Alt+M` | The sequence has no markers (`guards::markers`) | Removes the sequence marker at the playhead. |
| `marker-clear-all` | `none` | Clear All Markers | `Ctrl+Shift+Alt+M` | The sequence has no markers (`guards::markers`) | Removes every sequence marker from the active sequence. |
| `seq-marker-rename` | `none` | Edit Marker… | `none` | The sequence has no markers (`guards::markers`) | Opens `aria.dialog.marker-edit` for the marker at the playhead. |
| `seq-marker-snap` | `none` | Move Nearest Marker to Playhead | `none` | The sequence has no markers (`guards::markers`) | Moves the nearest sequence marker within one second onto the playhead tick. |

## Effect menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `fx-controls` | `none` | Effect Controls | `Shift+4` | Always enabled | Opens and focuses `aria.panel.inspector`. |
| `fx-last` | `none` | Apply Last Effect | `none` | No effect has been applied yet or no clip is selected (`guards::last_effect`) | Applies the most recently applied video effect (`last_effect`) to the selected clips. |
| `fx-paste` | `none` | Paste Effect | `none` | No effect has been copied or no clip is selected (`guards::copied_effect`) | Pastes the copied effect onto the selected clips. |
| `fx-remove-all` | `none` | Remove All Effects | `none` | Selected clips have no effects (`guards::effects`) | Removes all video effects from the selected clips. |
| `seq-effect-reset` | `none` | Reset Effect Parameters | `Ctrl+Alt+R` | Selected clips have no effects (`guards::effects`) | Resets every parameter of every effect on the selected clip to its catalog default. |
| `seq-grade-reset` | `none` | Reset Grade | `none` | No clip is selected (`guards::selection`) | Clears color adjustments (`ColorAdjustments`) on the selected clips. |
| `seq-mask-clear` | `none` | Remove Mask | `none` | The selected clip has no mask (`guards::mask`) | Removes all masks from the selected clips. |
| `seq-chroma-clear` | `none` | Remove Chroma Key | `none` | No clip is selected (`guards::selection`) | Removes the chroma keyer from the selected clips. |

In addition to the eight fixed rows above, `items_for(AppMenu::Effect, state)` appends:
- One `fx-apply:<id>` row under its category submenu for every `EntryKind::VideoEffect` in `crate::catalog_text::effect_entries()` (`docs/reference/effects.md`), disabled when no clip is selected (`guards::selection`).
- One `fx-apply-audio:<id>` row under its category submenu for every `EntryKind::AudioEffect` in `crate::catalog_text::effect_entries()` (`docs/reference/audio-effects.md`), disabled when no clip with sound is selected (`guards::sound`).

## Animation menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `anim-add-keyframe` | `none` | Add Keyframe | `none` | Select a property row in the timeline (`guards::property_rows`) | Inserts keyframes at the playhead on all selected timeline property rows. |
| `anim-toggle-hold` | `none` | Toggle Hold Keyframe | `Ctrl+Alt+H` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Toggles step-hold interpolation (`Easing::Hold`) on the target keyframes. |
| `anim-interp-linear` | `Keyframe Interpolation` | Linear | `none` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Sets the target keyframes' interpolation to `KeyInterpolation::Linear`. |
| `anim-interp-bezier` | `Keyframe Interpolation` | Bezier | `none` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Sets the target keyframes' interpolation to `KeyInterpolation::Bezier`. |
| `anim-interp-hold` | `Keyframe Interpolation` | Hold | `none` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Sets the target keyframes' interpolation to `KeyInterpolation::Hold`. |
| `anim-keyframe-velocity` | `none` | Keyframe Velocity… | `none` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Opens `aria.dialog.keyframe-velocity` to edit incoming and outgoing velocity and influence. |
| `anim-easy-ease` | `Keyframe Assistant` | Easy Ease | `F9` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Sets target keyframes to smooth ease-in-out (`Easing::EaseInOut`). |
| `anim-easy-ease-in` | `Keyframe Assistant` | Easy Ease In | `Shift+F9` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Sets target keyframes to ease-in (`Easing::EaseIn`). |
| `anim-easy-ease-out` | `Keyframe Assistant` | Easy Ease Out | `Ctrl+Shift+F9` | Select a keyframe, or put the playhead on one (`guards::keyframes`) | Sets target keyframes to ease-out (`Easing::EaseOut`). |
| `anim-time-reverse` | `Keyframe Assistant` | Time-Reverse Keyframes | `none` | Select two or more keyframes on one property (`guards::keyframe_span`) | Reverses the values and easing of two or more selected keyframes along their span on each property. |
| `anim-add-expression` | `none` | Add Expression | `none` | Select a property row of the selected clip (`guards::clip_property_row`) | Attaches a default expression to the selected property row of the selected clip. |
| `anim-reveal-keyed` | `none` | Reveal Properties with Keyframes | `Alt+U` | No clip is selected (`guards::selection`) | Reveals animated or modified property rows on the selected clips in the timeline outline. |
| `anim-reveal-modified` | `none` | Reveal Modified Properties | `none` | No clip is selected (`guards::selection`) | Reveals all non-default property rows on the selected clips in the timeline outline. |
| `anim-graph-editor` | `none` | Graph Editor | `Shift+F3` | Always enabled | Toggles the inline value/speed curve graph editor in the timeline (`toggle_timeline_graph_editor`). |

## View menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `seq-in` | `none` | Zoom In | `=` | Always enabled | Multiplies timeline horizontal zoom by `ZOOM_STEP`. |
| `seq-out` | `none` | Zoom Out | `-` | Always enabled | Divides timeline horizontal zoom by `ZOOM_STEP`. |
| `seq-fit` | `none` | Fit | `\` | Always enabled | Fits the entire sequence horizontally inside the timeline viewport. |
| `seq-fit-selection` | `none` | Fit Selection | `Shift+F` | No clip is selected (`guards::selection`) | Fits the selected clips horizontally inside the timeline viewport. |
| `view-playback-resolution` | `Resolution` | Playback Resolution | `none` | Always enabled | Cycles the Viewer playback raster resolution (`Full`, `1/2`, `1/4`). |
| `view-paused-resolution` | `Resolution` | Paused Resolution | `none` | Always enabled | Toggles the Viewer paused raster resolution (`Full` or `Match Playback`). |
| `view-split` | `Viewer Display Settings` | Split Viewer | `none` | Always enabled | Toggles the side-by-side Source and Program split in the Viewer. |
| `view-rulers` | `Viewer Display Settings` | Show Rulers | `Ctrl+Shift+R` | Always enabled | Toggles pixel rulers along the top and left edges of the Viewer. |
| `view-guides-show` | `Viewer Display Settings` | Show Guides | `none` | Always enabled | Toggles visibility of custom Viewer guide lines. |
| `view-guide-snap` | `Viewer Display Settings` | Snap to Guides | `none` | Always enabled | Toggles snapping of Viewer canvas drags to custom guide lines. |
| `view-guides-clear` | `Viewer Display Settings` | Clear Guides | `none` | Always enabled | Removes all custom guide lines from the Viewer. |
| `view-title-safe` | `Viewer Display Settings` | Title Safe | `none` | Always enabled | Toggles the 90% title-safe rectangle (5% inset per edge) in the Viewer. |
| `view-action-safe` | `Viewer Display Settings` | Action Safe | `none` | Always enabled | Toggles the 93% action-safe rectangle (3.5% inset per edge) in the Viewer. |
| `view-transparency-grid` | `Viewer Display Settings` | Transparency Grid | `none` | Always enabled | Toggles the checkerboard transparency backdrop in the Viewer. |
| `view-grid` | `Viewer Display Settings` | Grid | `none` | Always enabled | Toggles the rule-of-thirds grid overlay in the Viewer. |
| `view-proxies` | `none` | Proxies | `none` | Always enabled | Toggles whether the Viewer decodes media pool items through their proxy files. |
| `view-compare-frame` | `Compare` | Current Frame | `none` | Always enabled | Captures the current Program frame as the comparison reference (`Reference::Frame`). |
| `view-compare-still` | `Compare` | Still… | `none` | Always enabled | Prompts for an image file on disk as the comparison reference (`Reference::Still`). |
| `view-compare-source` | `Compare` | Source Tab | `none` | Open a source in the Source monitor first (`guards::source`) | Uses the open Source monitor tab as the comparison reference (`Reference::Source`). |
| `view-compare-vertical` | `Compare` | Vertical Wipe | `none` | Always enabled | Sets the Viewer comparison split to a vertical wipe (`CompareLayout::Vertical`). |
| `view-compare-horizontal` | `Compare` | Horizontal Wipe | `none` | Always enabled | Sets the Viewer comparison split to a horizontal wipe (`CompareLayout::Horizontal`). |
| `view-compare-side` | `Compare` | Side by Side | `none` | Always enabled | Sets the Viewer comparison split to side-by-side (`CompareLayout::SideBySide`). |
| `view-compare-stop` | `Compare` | Stop Comparing | `none` | Nothing is being compared (`guards::comparing`) | Clears the active Viewer comparison reference. |
| `view-scopes` | `none` | Scope Strip | `none` | Always enabled | Toggles the scope strip below the Viewer. |
| `view-waveform-luma` | `Scope Waveform` | Luma | `none` | Always enabled | Sets the scope waveform monitor to luma (`WaveformMode::Luma`). |
| `view-waveform-rgb` | `Scope Waveform` | RGB Parade | `none` | Always enabled | Sets the scope waveform monitor to RGB parade (`WaveformMode::RgbParade`). |
| `view-waveform-ycbcr` | `Scope Waveform` | YCbCr Parade | `none` | Always enabled | Sets the scope waveform monitor to YCbCr parade (`WaveformMode::YcbcrParade`). |
| `view-zoom-fit` | `none` | Fit Preview | `none` | Always enabled | Resets Viewer zoom to fit the viewport. |
| `view-zoom-actual` | `none` | Preview 100% | `none` | Always enabled | Sets Viewer zoom to `100%`. |
| `view-handles-transform` | `Canvas Handles` | Transform | `none` | Always enabled | Sets the Viewer on-canvas handles to `OverlayMode::Transform`. |
| `view-handles-crop` | `Canvas Handles` | Crop | `none` | Always enabled | Sets the Viewer on-canvas handles to `OverlayMode::Crop`. |
| `view-handles-mask` | `Canvas Handles` | Mask | `none` | Always enabled | Sets the Viewer on-canvas handles to `OverlayMode::Mask`. |
| `view-fullscreen` | `none` | Fullscreen Preview | `none` | Always enabled | Toggles fullscreen preview mode. |
| `view-loop` | `none` | Loop Playback | `Ctrl+Shift+L` | Always enabled | Toggles looped transport playback (`looping`). |
| `view-thumbs-head` | `Timeline Display Settings` | Head Thumbnails | `none` | Always enabled | Draws a single thumbnail at the head of each video clip (`Thumbnails::Head`). |
| `view-thumbs-head-tail` | `Timeline Display Settings` | Head and Tail Thumbnails | `none` | Always enabled | Draws thumbnails at the head and tail of each video clip (`Thumbnails::HeadAndTail`). |
| `view-thumbs-continuous` | `Timeline Display Settings` | Continuous Thumbnails | `none` | Always enabled | Draws a continuous filmstrip of thumbnails across each video clip (`Thumbnails::Continuous`). |
| `view-waveform-log` | `Timeline Display Settings` | Logarithmic Waveforms | `none` | Always enabled | Draws timeline audio waveforms on a logarithmic dB scale (`WaveformScale::Logarithmic`). |
| `view-waveform-linear` | `Timeline Display Settings` | Linear Waveforms | `none` | Always enabled | Draws timeline audio waveforms on a linear amplitude scale (`WaveformScale::Linear`). |
| `view-waveform-hidden` | `Timeline Display Settings` | Hide Waveforms | `none` | Always enabled | Hides audio waveforms on timeline clips (`WaveformScale::Hidden`). |
| `view-follow-page` | `Timeline Display Settings` | Page Scroll | `none` | Always enabled | Advances the timeline viewport in page jumps during playback (`Follow::Page`). |
| `view-follow-smooth` | `Timeline Display Settings` | Smooth Scroll | `none` | Always enabled | Scrolls the timeline viewport continuously with the playhead during playback (`Follow::Smooth`). |

## Window menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `win-workspace-editing` | `Workspaces` | Editing | `none` | Always enabled | Applies the `Editing` workspace layout (`docs/reference/workspaces.md`). |
| `win-workspace-animation` | `Workspaces` | Animation | `none` | Always enabled | Applies the `Animation` workspace layout. |
| `win-workspace-colour` | `Workspaces` | Color | `none` | Always enabled | Applies the `Color` workspace layout. |
| `win-workspace-audio` | `Workspaces` | Audio | `none` | Always enabled | Applies the `Audio` workspace layout. |
| `win-workspace-effects` | `Workspaces` | Effects | `none` | Always enabled | Applies the `Effects` workspace layout. |
| `win-workspace-save` | `Workspaces` | Save Workspace | `none` | Always enabled | Saves the current arrangement to the active workspace file and the `Custom` slot. |
| `win-workspace-reset` | `Workspaces` | Reset to Saved Layout | `none` | Always enabled | Resets the active workspace to its saved or shipped layout. |
| `win-media` | `none` | Project | `none` | Always enabled | Toggles the `aria.panel.media` dock panel. |
| `win-effects` | `none` | Effects & Presets | `Shift+5` | Always enabled | Opens and focuses `aria.panel.effects`. |
| `win-program` | `none` | Viewer | `none` | Always enabled | Focuses the Viewer (`ActivePanel::Preview`). |
| `win-timeline` | `none` | Timeline | `Shift+3` | Always enabled | Focuses the Timeline (`ActivePanel::Timeline`). |
| `win-inspector` | `none` | Effect Controls | `none` | Always enabled | Toggles the `aria.panel.inspector` dock panel. |
| `win-assistant` | `none` | Assistant | `Shift+0` | Always enabled | Toggles the Assistant tab (`ActivePanel::Ai`). |
| `win-mixer` | `none` | Audio Track Mixer | `Shift+6` | Always enabled | Toggles the Audio Track Mixer tab (`ActivePanel::Mixer`). |
| `win-jobs` | `none` | Jobs | `none` | Always enabled | Toggles the `aria.panel.jobs` dock panel. |
| `win-panel:aria.panel.audio_meters` | `none` | Audio Meters | `Shift+9` | Always enabled | Toggles the `aria.panel.audio_meters` dock panel. |
| `win-panel:aria.panel.character` | `none` | Character | `none` | Always enabled | Toggles the `aria.panel.character` dock panel. |
| `win-panel:aria.panel.paragraph` | `none` | Paragraph | `none` | Always enabled | Toggles the `aria.panel.paragraph` dock panel. |
| `win-panel:aria.panel.info` | `none` | Info | `Shift+8` | Always enabled | Toggles the `aria.panel.info` dock panel. |
| `win-panel:aria.panel.align` | `none` | Align | `none` | Always enabled | Toggles the `aria.panel.align` dock panel. |
| `win-panel:aria.panel.history` | `none` | History | `Ctrl+Shift+H` | Always enabled | Toggles the `aria.panel.history` dock panel. |
| `win-panel:aria.panel.markers` | `none` | Markers | `none` | Always enabled | Toggles the `aria.panel.markers` dock panel. |
| `win-panel:aria.panel.scopes` | `none` | Scopes | `Shift+7` | Always enabled | Toggles the `aria.panel.scopes` dock panel. |
| `win-panel:aria.panel.color` | `none` | Color | `none` | Always enabled | Toggles the `aria.panel.color` dock panel. |
| `win-palette` | `none` | Command Palette | `Ctrl+Shift+P` | Always enabled | Opens the searchable Command Palette (`aria.dialog.palette`). |
| `win-home` | `none` | Home | `none` | Always enabled | Switches the window route to Home (`UiRoute::Home`). |

## Help menu

| ID | Submenu | Label | Shortcut | Disabled when | Action |
| --- | --- | --- | --- | --- | --- |
| `help-learn` | `none` | Keyboard Shortcuts | `none` | Always enabled | Switches the Home rail to `HomeRail::Learn` and opens the Home route. |
| `help-plugins` | `none` | Plugin Development | `none` | Always enabled | Opens the repository plugin development guide (`docs/plugins/README.md`) in the system browser. |
| `help-data` | `none` | Open Data Folder | `none` | Always enabled | Reveals the profile data directory (`state.data_dir`) in the system file manager. |
| `help-updates` | `none` | Check for Updates | `none` | Always enabled | Checks the configured update manifest URL (`updates.manifest_url`) for a newer release. |
| `help-about` | `none` | About | `none` | Always enabled | Opens `aria.dialog.about`. |

## Dynamic menu rows

Several menus append rows at render time when session state or mounted plugins provide them (`crates/aria-ui/src/chrome/menu_file.rs`, `menu_effect.rs`, and `menu_window.rs`):

- `file-recent:<path>` (under **File > Open Recent**): opens the recent `.aria` project at `<path>` from `state.recents()`, disabled when the file is missing on disk (`guards::recent`).
- `file-import-recent:<path>` (under **File > Import Recent**): imports the recent media file at `<path>` from `state.import_recents()`, disabled when the file is missing on disk (`guards::import_recent`).
- `file-export-as:<exporter_id>` (under **File > Export**): runs a registered `TimelineExporter` (`fcpxml`, `otio`, `edl`, `srt`, or `vtt` when `aria-interchange` is mounted).
- `fx-apply:<effect_id>` (under **Effect > `<Category>`**): appends the catalog video effect `<effect_id>` (`docs/reference/effects.md`) to the selected clip.
- `fx-apply-audio:<effect_id>` (under **Effect > `<Category>`**): appends the built-in audio effect `<effect_id>` (`docs/reference/audio-effects.md`) to the selected clip.
- `win-workspace-custom`, `win-workspace-file:<name>`, and `win-workspace-plugin:<id>` (under **Window > Workspaces**): switches to the saved custom workspace, a named workspace JSON file under `<data dir>/workspaces/`, or a plugin-registered workspace.
- `win-panel:<panel_id>` (under **Window**): toggles a plugin-contributed dock panel.
- `win-project-report` (under **Window**, label `Project Report`): opens `aria.dialog.project-report` once the project has at least one media source or timeline clip (`state.project_report_available()`).

## Audio Gain dialog modes (`AudioGainMode::ALL`)

Defined in `crates/aria-ui/src/app/clip_ops.rs` (`AudioGainMode::ALL`, opened by `clip-audio-gain` / **Clip > Audio Gain…**).

| Variant | Label (`label()`) | Default | Summary |
| --- | --- | --- | --- |
| `SetGain` | `Set Gain to` | `yes` | Sets `ClipParam::Volume` on selected audio clips to the linear gain for the target dB value (`-60.0..=+12.0` dB). |
| `AdjustGain` | `Adjust Gain by` | `no` | Adds the signed dB delta to each selected clip's existing `gain_db`. |
| `NormalizeMaxPeak` | `Normalize Max Peak to` | `no` | Shifts all selected clips by a single dB offset so the loudest clip peak across the selection lands at the target dB ceiling. |
| `NormalizeAllPeaks` | `Normalize All Peaks to` | `no` | Shifts each selected clip independently so every clip's own maximum sample peak lands at the target dB ceiling. |

## Audio Channels routing presets (`AudioChannels::ALL`)

Defined in `crates/aria-ui/src/app/audio_channels.rs` (`AudioChannels::ALL`, under **Clip > Audio Channels**).

| Variant | Menu row ID (`row()`) | Label (`label()`) | Audio effect ID (`tool()`) |
| --- | --- | --- | --- |
| `Stereo` | `clip-channels-stereo` | `Stereo` | `none` |
| `Swap` | `clip-channels-swap` | `Swap Channels` | `swap_channels` |
| `LeftToBoth` | `clip-channels-left` | `Left to Both` | `fill_right` |
| `RightToBoth` | `clip-channels-right` | `Right to Both` | `fill_left` |
| `Mono` | `clip-channels-mono` | `Mono` | `mono` |

## Track matte sources (`MatteSource::ALL`)

Defined in `crates/aria-models/src/look/matte.rs` (`MatteSource::ALL`, cycled by **Clip > Set Matte** in the clip context menu).

| Variant | ID (`id()`) | Label (`label()`) |
| --- | --- | --- |
| `Alpha` | `alpha` | `Alpha` |
| `Luma` | `luma` | `Luma` |

## Action decline reasons (`Decline::ALL`)

Defined in `crates/aria-ui/src/app/decline.rs` (`Decline::ALL`). When a command finds no applicable target in the current selection or cut, the status line displays `reason()`:

| Variant | Status line (`reason()`) |
| --- | --- |
| `NoSelection` | `No clip is selected` |
| `NoAbuttingCut` | `No clip on the other side of that cut` |
| `UnknownTransition` | `No such transition` |
| `NoTransitionAtCut` | `No transition at that cut` |
| `PictureCut` | `That cut is between pictures: Ctrl+T puts a transition on it` |
| `SoundCut` | `That cut is between sounds: Ctrl+Shift+T crossfades it` |
| `OneAngleOnly` | `A group needs a second camera to sync against` |
| `AnglesStillDecoding` | `The angles' sound is still being read` |
| `NoAngleSound` | `An angle has no sound to sync by` |
| `AnglesDoNotMatch` | `The angles' sound does not match` |
| `AnglesRepeat` | `The angles' sound repeats, so it cannot say where it is` |

## Context menu item tables (`PREVIEW_ITEMS`, `CLIP_ITEMS`, `TRANSITION_ITEMS`, `TRACK_ITEMS`, `POOL_ITEMS`, `RECENT_ITEMS`, `EFFECT_ITEMS`, `BADGE_ITEMS`, `KEYFRAME_ITEMS`, `MASK_PATH_KEY_ITEMS`)

Defined in `crates/aria-ui/src/surfaces/context_menu.rs` and `crates/aria-ui/src/surfaces/context_menu/*.rs`. Each context menu target draws its rows in slice order; `Labels`, `Blends`, and `Properties` expand inline from the label swatch list, transition catalog, and clip line-parameter list. A row whose `checked` is set draws a check mark in its lead column while that state holds; `Default check` is `on` or `off` for such a row against the default facts (an enabled clip, a Medium lane, no transition alignment, a keyframe that neither holds nor roves), and `none` for a row that draws no check mark. The applied blend and the drawn property are checked the same way.

| Target slice | Entry kind | Row ID | Default label | Default check |
| --- | --- | --- | --- | --- |
| `PREVIEW_ITEMS` | `Row` | `ctx-fit` | `Fit Timeline` | `none` |
| `PREVIEW_ITEMS` | `Row` | `ctx-reset-transform` | `Reset Transform` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-split` | `Split` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-delete` | `Delete` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-duplicate` | `Duplicate` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-copy` | `Copy` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-rename-clip` | `Rename Clip` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-freeze` | `Freeze Frame` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-extract-audio` | `Extract Audio` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-link` | `Link Clips` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-unlink` | `Unlink Clips` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-enable` | `Enable` | `on` |
| `CLIP_ITEMS` | `Row` | `ctx-group` | `Group` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-ungroup` | `Ungroup` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-release-parent` | `Release Parent` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-speed-duration` | `Speed/Duration…` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-time-remap` | `Time Remapping` | `off` |
| `CLIP_ITEMS` | `Row` | `ctx-audio-gain` | `Audio Gain…` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-shift-selection` | `Shift Selection…` | `none` |
| `CLIP_ITEMS` | `Labels` | `none` | `none` | `none` |
| `CLIP_ITEMS` | `Row` | `ctx-properties` | `Properties` | `none` |
| `TRANSITION_ITEMS` | `Row` | `ctx-transition-remove` | `Remove Transition` | `none` |
| `TRANSITION_ITEMS` | `Row` | `ctx-transition-align-center` | `Center at Cut` | `off` |
| `TRANSITION_ITEMS` | `Row` | `ctx-transition-align-start` | `Start at Cut` | `off` |
| `TRANSITION_ITEMS` | `Row` | `ctx-transition-align-end` | `End at Cut` | `off` |
| `TRANSITION_ITEMS` | `Blends` | `none` | `none` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-add-track` | `Add Track` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-rename-track` | `Rename Track` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-delete-track` | `Delete Track` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-mute-track` | `Mute` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-track-up` | `Move Track Up` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-track-down` | `Move Track Down` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-track-duck` | `Duck Source` | `off` |
| `TRACK_ITEMS` | `Row` | `ctx-track-open-gap` | `Open Gap at Playhead` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-track-close-gap` | `Close Gap at Playhead` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-collapse` | `Collapse Lane` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-waveform` | `Hide Waveforms` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-filmstrip` | `Hide Filmstrips` | `none` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-mini` | `Lane Height: Mini` | `off` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-small` | `Lane Height: Small` | `off` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-medium` | `Lane Height: Medium` | `on` |
| `TRACK_ITEMS` | `Row` | `ctx-lane-large` | `Lane Height: Large` | `off` |
| `POOL_ITEMS` | `Row` | `ctx-pool-in-use` | `Select Clips in Timeline` | `none` |
| `POOL_ITEMS` | `Row` | `ctx-pool-remove` | `Remove from Project` | `none` |
| `POOL_ITEMS` | `Row` | `ctx-pool-proxy-clear` | `Decode from Original File` | `none` |
| `POOL_ITEMS` | `Row` | `ctx-pool-proxy-use` | `Decode from Proxy` | `none` |
| `POOL_ITEMS` | `Labels` | `none` | `none` | `none` |
| `RECENT_ITEMS` | `Row` | `ctx-recent-open` | `Open` | `none` |
| `RECENT_ITEMS` | `Outside` | `ctx-recent-reveal` | `Reveal` | `none` |
| `RECENT_ITEMS` | `Row` | `ctx-recent-remove` | `Remove from List` | `none` |
| `RECENT_ITEMS` | `Row` | `ctx-recent-delete` | `Delete File…` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-reset` | `Reset` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-rename` | `Rename Effect` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-duplicate` | `Duplicate` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-copy` | `Copy` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-paste` | `Paste Below` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-save-preset` | `Save as Preset…` | `none` |
| `EFFECT_ITEMS` | `Row` | `ctx-fx-remove` | `Remove Effect` | `none` |
| `BADGE_ITEMS` | `Row` | `ctx-badge-controls` | `Show in Effect Controls` | `none` |
| `BADGE_ITEMS` | `Properties` | `none` | `none` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-linear` | `Linear` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-easy-ease` | `Easy Ease` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-easy-ease-in` | `Easy Ease In` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-easy-ease-out` | `Easy Ease Out` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-hold` | `Hold` | `off` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-rove` | `Rove Across Time` | `off` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-copy` | `Copy Keyframes` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-paste` | `Paste Keyframes at Playhead` | `none` |
| `KEYFRAME_ITEMS` | `Row` | `ctx-key-delete` | `Delete Keyframes` | `none` |
| `MASK_PATH_KEY_ITEMS` | `Row` | `ctx-path-key-linear` | `Linear` | `none` |
| `MASK_PATH_KEY_ITEMS` | `Row` | `ctx-path-key-ease` | `Easy Ease` | `none` |
| `MASK_PATH_KEY_ITEMS` | `Row` | `ctx-path-key-ease-in` | `Easy Ease In` | `none` |
| `MASK_PATH_KEY_ITEMS` | `Row` | `ctx-path-key-ease-out` | `Easy Ease Out` | `none` |
| `MASK_PATH_KEY_ITEMS` | `Row` | `ctx-path-key-hold` | `Hold` | `off` |
| `MASK_PATH_KEY_ITEMS` | `Row` | `ctx-path-key-delete` | `Delete Keyframe` | `none` |
