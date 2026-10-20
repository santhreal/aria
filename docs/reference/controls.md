# Controls

Every interactive button, chip, icon button, and text field in the Aria shell is identified by a `motion::ControlId` (`crates/aria-ui/src/motion_key.rs`). Static controls use a fixed `&'static str` identifier (`ControlId::Fixed`), while controls repeated across dynamic rows construct a hashed `MotionKey::new("<kind>", ...)` (`ControlId::Key`) so hover and press transitions stay isolated per row. Controls with hover tooltips or keyboard shortcut badges register a `ControlTip` entry in `crate::tooltip::CONTROL_TIPS` (`crates/aria-ui/src/tooltip.rs`), which links the control identifier to its human-readable label and optional `keys::Binding` ID; shared widget constructors are defined in `crates/aria-ui/src/components.rs` (`button`, `ghost_button`, `danger_button`, `chip`, `text_box`, `icon_button`) and `crates/aria-ui/src/pending.rs` (`stop_button`).

## Example

1. Hover the Razor tool button (`tool-razor`) in the Tool Strip or the Play button (`play`) in the Viewer transport bar.
2. After `tooltip::SHOW_DELAY` (220 ms), the shell queries `tooltip::lookup` in `CONTROL_TIPS` and displays the control's label alongside the active chord from `keys::default_bindings()` (`Razor  C` or `Play  Space`).
3. Left-click the control to dispatch its action (`EditorTool::Razor` selection or `AppState::toggle_playback`), or press the bound chord (`C` or `Space`) directly to run the same action without moving the pointer.

## Tool Strip

The editor tool strip (`crates/aria-ui/src/chrome/tool_strip.rs` and `crates/aria-ui/src/tool_catalog.rs`) renders one icon button per editing tool plus the timeline drop-mode toggle.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `edit-mode` | Ripple / Overwrite | Toggles the timeline drop mode between Ripple and Overwrite. | None |
| `tool-hand` | Hand | Pans the Timeline or Viewer canvas by dragging. | `H` |
| `tool-pen` | Pen | Adds keyframes on timeline curves or draws and edits Bézier mask points in the Viewer. | `P` |
| `tool-pointer` | Select | Selects, moves, and trims clips on the timeline and handles in the Viewer. | `V` |
| `tool-rate` | Rate stretch | Drags a clip edge to change playback speed while preserving the source in point. | `R` |
| `tool-razor` | Razor | Splits the targeted clip at the clicked frame on the timeline. | `C` |
| `tool-ripple` | Ripple trim | Trims a clip edge and shifts downstream clips on the track to close or open the gap. | `B` |
| `tool-roll` | Rolling edit | Moves the cut point between two adjacent clips without changing the combined duration. | `N` |
| `tool-slide` | Slide | Moves a clip along the track while trimming the neighbouring clips to keep their outer bounds fixed. | `U` |
| `tool-slip` | Slip | Shifts the source media in and out points inside a clip without moving its timeline bounds. | `Y` |
| `tool-track-backward` | Track Select Backward | Selects every clip on the targeted track or all unlocked tracks up to the clicked frame. | `Shift+A` |
| `tool-track-forward` | Track Select Forward | Selects every clip on the targeted track or all unlocked tracks from the clicked frame forward. | `A` |
| `tool-type` | Type | Creates a new title clip or enters inline text editing in the Viewer. | `T` |
| `tool-zoom` | Zoom | Zooms the Timeline or Viewer in on click or out on Alt-click. | `Z` |

## Viewer & Source Monitor

The Viewer and Source Monitor (`crates/aria-ui/src/panels/preview*.rs`) render transport buttons, frame steppers, mark buttons, magnification controls, handle mode toggles, resolution and proxy toggles, and source three-point edit controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `go-end` | Go to end | Moves the playhead to the end of the active sequence or source clip. | `End` |
| `go-start` | Go to start | Moves the playhead to the start of the active sequence or source clip. | `Home` |
| `next-frame` | Next frame | Steps the playhead forward by one frame. | `Right` |
| `pause` | Pause | Pauses active playback at the current frame. | `Space` |
| `play` | Play | Starts playback in the active Viewer or Source Monitor tab. | `Space` |
| `playhead-tc` | Playhead timecode | Edits or seeks the playhead position in `HH:MM:SS:FF` timecode format. | None |
| `prev-frame` | Previous frame | Steps the playhead backward by one frame. | `Left` |
| `preview-handles-crop` | Crop handles | Activates interactive left, top, right, and bottom crop handles on the Viewer canvas. | None |
| `preview-handles-mask` | Mask handles | Activates interactive mask path vertices and feather handles on the Viewer canvas. | None |
| `preview-handles-transform` | Transform handles | Activates interactive position, scale, and rotation handles on the Viewer canvas. | None |
| `preview-loop` | Loop playback | Toggles continuous looping between the active in and out marks or across the full timeline. | `Ctrl+Shift+L` |
| `preview-proxies-toggle` | Proxy toggle | Toggles playback decoding between generated proxy files and original source media. | None |
| `preview-resolution` | Resolution | Opens the playback and paused raster divisor picker (Full, 1/2, 1/4). | None |
| `preview-shuttle` | Shuttle speed | Displays the active J/K/L shuttle multiplier and resets playback speed to 1x when clicked. | `L` |
| `preview-zoom-in` | Zoom in | Steps the Viewer canvas magnification up one preset level. | None |
| `preview-zoom-label` | Zoom percentage | Displays the current magnification percentage and fits the frame to the pane when clicked. | None |
| `preview-zoom-out` | Zoom out | Steps the Viewer canvas magnification down one preset level. | None |
| `program-mark-in` | In | Sets the sequence in mark at the current playhead frame. | `I` |
| `program-mark-out` | Out | Sets the sequence out mark at the current playhead frame. | `O` |
| `source-close` | Sequence | Closes the Source Monitor view and returns focus to the active sequence. | `Space` |
| `source-insert` | Insert | Inserts the marked source range at the sequence playhead and ripples downstream clips. | `,` |
| `source-mark-clear` | Clear | Clears both in and out marks on the active source clip. | None |
| `source-mark-in` | In | Sets the source clip in point at the Source Monitor playhead. | `I` |
| `source-mark-out` | Out | Sets the source clip out point at the Source Monitor playhead. | `O` |
| `source-overwrite` | Overwrite | Overwrites the timeline at the sequence playhead with the marked source range. | `.` |
| `viewer-display-settings` | Viewer display settings | Opens the Viewer overlay menu for safe margins, grid, guides, A/B wipe, and background fill. | None |
| `viewer-tab-close` | Close source tab | Closes the corresponding source media tab in the Viewer header. | `Ctrl+W` |
| `viewer-tab-program` | Program tab | Switches the Viewer pane to the active sequence output. | `Shift+2` |
| `viewer-tab-source` | Source tab | Switches the Viewer pane to the opened source media item; middle-click closes the tab. | `Shift+2` |

## Timeline

The Timeline pane (`crates/aria-ui/src/panels/timeline/*.rs` and `crates/aria-ui/src/panels/timeline_report.rs`) renders header toggles, sequence timecode and property search fields, track creation and zoom-fit controls, and graph editor tangent buttons.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `add-track` | Add track | Appends a new video track to the active sequence. | None |
| `fit-timeline` | Zoom percentage | Displays the timeline zoom percentage and fits the entire sequence into the viewport when clicked. | `\` |
| `graph-btn-easy-ease` | Easy Ease | Converts selected keyframes or the keyframe at the playhead to smooth Bézier tangents. | `F9` |
| `graph-btn-hold` | Hold | Toggles hold interpolation on selected keyframes or the keyframe at the playhead. | `Ctrl+Alt+H` |
| `property-search` | Search | Filters visible timeline property and keyframe rows by name. | None |
| `property-search-clear` | Clear search | Clears the active timeline property search filter. | None |
| `timeline-display-settings` | Timeline display settings | Opens the timeline display menu for video thumbnails, audio waveforms, and duplicate frame bars. | None |
| `timeline-graph` | Graph editor | Toggles the keyframe value and speed curve editor inside the timeline pane. | `Shift+F3` |
| `timeline-linked` | Linked selection | Toggles whether selecting or trimming a clip also acts on its linked audio or video partner. | None |
| `timeline-marker` | Add marker | Adds a marker at the playhead or opens the marker editor when a marker is already at that frame. | `M` |
| `timeline-report-dismiss` | Dismiss | Dismisses the timeline diagnostic banner. | None |
| `timeline-snap` | Snapping | Toggles magnetic snapping of clips, cuts, and the playhead to timeline landmarks. | `S` |
| `timeline-tc` | Sequence timecode | Edits or seeks the sequence playhead position from the timeline header. | None |

## Inspector

The Inspector panel (`crates/aria-ui/src/panels/inspector/**/*.rs`) renders parameter reset buttons, transform parenting chips and pick whip, effect rack row controls, preset management controls, framing and matte buttons, clip link controls, and transition pickers.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `aud-role-clear` | Untag | Clears the assigned audio role tag (Dialogue, Music, Effects) from the selected clip. | None |
| `blur-toggle` | On / Off | Toggles motion blur evaluation for transform animation on the selected clip. | None |
| `chroma-clear` | Remove key | Removes the chroma key filter from the selected clip. | None |
| `clip-type-detach` | Detach | Detaches the linked audio stream from the selected video clip onto its own track item. | None |
| `crop-clear` | Reset framing | Resets all crop insets, feathering, and flip flags on the selected clip to their defaults. | None |
| `duck-run` | Duck under voice | Computes and applies volume keyframes that lower music or effects gain during dialogue segments. | None |
| `flip-h` | Flip H | Toggles horizontal mirroring of the selected clip. | None |
| `flip-v` | Flip V | Toggles vertical mirroring of the selected clip. | None |
| `fx-add` (per row) | Effect name | Appends the chosen effect from the inline picker to the selected clip's effect stack. | None |
| `fx-choice` (per row) | Option label | Selects a discrete enum option for an effect parameter. | None |
| `fx-curve` (per row) | Curve preset | Applies a named curve preset (Linear, S-Curve, Invert) to an effect curve parameter. | None |
| `fx-font` (per row) | Font family | Selects the font family for an effect text parameter. | None |
| `fx-grad` (per row) | Gradient preset | Applies a preset colour stop ramp to an effect gradient parameter. | None |
| `fx-group` (per row) | Parameter group | Expands or collapses a collapsible parameter group inside an applied effect. | None |
| `fx-layer` (per row) | Layer clip | Selects another timeline clip as a secondary layer input for an effect parameter. | None |
| `fx-layer-none` (per row) | None | Clears the secondary layer input slot on an effect parameter. | None |
| `fx-param-bool` (per row) | On / Off | Toggles a boolean parameter on an applied clip effect. | None |
| `fx-text-clear` (per row) | Clear | Clears the text string of an effect text parameter. | None |
| `inspector-fx-bypass` (per row) | fx | Toggles bypass for an individual effect row on the selected clip. | None |
| `inspector-fx-down` (per row) | ↓ | Moves the effect one slot later in the clip's evaluation order. | None |
| `inspector-fx-menu` | Effect actions | Opens the per-effect menu to rename, duplicate, copy, paste, or save the effect as a preset. | None |
| `inspector-fx-plugin-preset` (per row) | Plugin preset | Applies a plugin-contributed effect preset to the selected clip. | None |
| `inspector-fx-preset` (per row) | Preset name | Applies a saved effect chain preset to the selected clip. | None |
| `inspector-fx-preset-delete` (per row) | Delete | Deletes a saved effect preset from the local preset library. | None |
| `inspector-fx-preset-name` | Name this preset | Enters the name for a new effect preset before saving. | None |
| `inspector-fx-preset-part-audio` | Audio section | Toggles inclusion of audio parameters when saving a multi-part preset. | None |
| `inspector-fx-preset-part-fx` (per row) | Effect row | Toggles inclusion of a specific applied effect when saving a preset. | None |
| `inspector-fx-preset-part-prop` (per row) | Property group | Toggles inclusion of a built-in property group when saving a preset. | None |
| `inspector-fx-preset-save` | Save as preset / Save | Opens the preset naming form or commits the new effect preset to disk. | None |
| `inspector-fx-remove` | Remove effect | Removes the effect row from the selected clip's stack. | None |
| `inspector-fx-reset` | Reset effect | Resets all parameters of the effect row to their default values. | None |
| `inspector-fx-up` (per row) | ↑ | Moves the effect one slot earlier in the clip's evaluation order. | None |
| `inspector-parent-whip` | Pick whip | Attaches the transform parenting pick whip tooltip and arms clip parenting on click or drag. | None |
| `inspector-pitch` | On / Off | Toggles pitch preservation when the selected clip plays at a non-unit speed. | None |
| `inspector-replace-media` | Media name | Opens a file picker to replace the source media of a template slot clip. | None |
| `inspector-reset` | Reset transform | Resets position, scale, rotation, anchor, and opacity on the selected clip. | None |
| `inspector-reverse` | Reverse | Toggles reverse playback on the selected clip. | None |
| `inspector-slot-hint` | Edit hint… | Opens the prompt to edit the placeholder instruction text for a template slot. | None |
| `inspector-slot-hint-clear` | Clear hint | Clears the placeholder instruction text from a template slot. | None |
| `inspector-slot-toggle` | Mark as template slot / Clear template slot | Marks the selected clip as a replaceable template slot or removes its slot designation. | None |
| `inspector-text-editable` | Text editable in template | Toggles whether a title clip's text remains editable when packaged as a template. | None |
| `look-choice` (per row) | Look name | Applies a bundled creative look grade to the selected clip. | None |
| `look-clear` | Clear | Clears the applied creative look from the selected clip. | None |
| `lut-clear` | Remove LUT | Removes the loaded 3D `.cube` LUT from the selected clip. | None |
| `lut-load` | Load .cube… | Opens a file picker to load a `.cube` 3D LUT file onto the selected clip. | None |
| `matte-clear` | Remove matte | Clears the track matte assignment from the selected clip. | None |
| `matte-invert` | Invert | Inverts the alpha or luma channel read from the assigned matte clip. | None |
| `parent-choice` (per row) | Clip name | Assigns the clicked timeline clip as the transform parent of the selected clip. | None |
| `parent-none` | None | Detaches the selected clip from its current transform parent. | None |
| `parent-whip` | @ | Arms or disarms the transform parent pick whip for clicking or dragging onto a target clip. | None |
| `relink-media` | Relink | Opens a file picker to relink an offline source file from the Info tab. | None |
| `speed-clear` | Clear ramp | Removes variable speed ramp segments and returns the clip to constant speed. | None |
| `styles-clear` | Clear styles | Removes drop shadow, stroke, glow, and background layer styles from the selected clip. | None |
| `tr-bool` (per row) | On / Off | Toggles a boolean parameter on the selected timeline transition. | None |
| `tr-choice` (per row) | Option label | Selects an enum option for a parameter on the selected timeline transition. | None |
| `transition-align` (per row) | Alignment | Sets transition placement across the cut (Center at Cut, Start at Cut, End at Cut). | None |
| `transition-choice` (per row) | Transition kind | Selects the transition type (Cross Dissolve, Dip to Black, Wipe, Slide) on the active cut. | None |
| `transition-clear` | Remove transition | Removes the transition from the selected cut point. | None |

## Project Panel

The Project panel (`crates/aria-ui/src/panels/sidebar_*.rs`, `crates/aria-ui/src/surfaces/bin_strip.rs`, `crates/aria-ui/src/panels/generate.rs`, and `crates/aria-ui/src/panels/analysis.rs`) renders bin management buttons, media pool search and layout toggles, multicam sync and angle controls, subclip and generator actions, and media analysis controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `analysis-clear` (per row) | × | Clears cached analysis results for the specified detector kind on the selected clip. | None |
| `analysis-confidence` (per row) | ≥0% / ≥25% / ≥50% / ≥75% | Hides detected moments whose confidence is below the chosen value. | None |
| `analysis-cut-silence` | Ripple-delete silence | Splits and ripple-deletes detected silent spans on the selected timeline clip. | None |
| `analysis-kind` (per row) | All / Shots / Silence / Beats | Lists the moments of every detector, or of the chosen detector only. | None |
| `analysis-loudness` | Loudness | Runs EBU R128 integrated loudness, range, and true-peak measurement on the selected clip. | None |
| `analysis-next` | Next | Moves the playhead to the next detected analysis moment. | None |
| `analysis-prev` | Previous | Moves the playhead to the previous detected analysis moment. | None |
| `analysis-refresh` | Refresh | Re-runs the active media analysis pass on the selected clip. | None |
| `analysis-split-shots` | Split at shots | Splits the selected timeline clip at every detected shot boundary. | None |
| `bin-delete` | Delete bin | Deletes the currently open media bin and moves its contents to the parent bin. | None |
| `bin-name` | Bin name | Edits the name of the bin currently being renamed. | None |
| `bin-new` | New bin | Creates a new media bin inside the current Project panel folder. | None |
| `bin-rename` | Rename | Focuses the inline name field to rename the currently open bin. | None |
| `create-multicam-group` | New multicam | Creates a new multicam group from the selected media pool items. | None |
| `create-subclip` | Make Subclip | Creates a new subclip item in the media pool from the current Source Monitor in/out range. | None |
| `generate-kind` (per row) | Generator kind | Selects the procedural generator category in the Project panel Generate tab. | None |
| `generate-submit` | Generate | Creates the configured procedural generator item and adds it to the media pool. | None |
| `media-import` | Import | Opens the file picker to import video, audio, or image files into the active bin. | None |
| `media-query` | Search | Filters media pool items in the active bin by filename or metadata. | None |
| `nudge-multicam-back` (per row) | Nudge angle back | Shifts the multicam angle offset one frame earlier relative to the group sync map. | None |
| `nudge-multicam-forward` (per row) | Nudge angle forward | Shifts the multicam angle offset one frame later relative to the group sync map. | None |
| `place-multicam-group` (per row) | Place | Places the multicam group onto the active sequence at the playhead. | None |
| `pool-audio-only` | Audio only | Filters the media pool view to show audio assets only. | None |
| `pool-sort` | Sort order | Opens the media pool sort order menu (Name, Duration, Kind, Date). | None |
| `pool-view-freeform` | Freeform | Switches the media pool layout to a freeform spatial canvas. | None |
| `pool-view-icons` | Icons | Switches the media pool layout to large thumbnail icons. | None |
| `pool-view-list` | List | Switches the media pool layout to a columnar metadata list. | None |
| `pool-view-tiles` | Tiles | Switches the media pool layout to compact tiles with metadata. | None |
| `relink-all` | Relink All | Opens a file picker to relink all missing media items in the project. | None |
| `sidebar-group` (per row) | Group tab | Switches the top-level Project panel category (Media, Stock, Templates, Audio, Generate, Analyze). | None |
| `stock-search` | Search | Executes a stock media search query against the configured stock catalog. | None |
| `switch-multicam-angle` (per row) | Angle label | Switches the active camera angle for the multicam group. | None |
| `sync-multicam-group` (per row) | Sync angles | Aligns all angles in the multicam group by matching their audio waveforms. | None |

## Effects Browser

The Effects Browser panel (`crates/aria-ui/src/panels/effects_browser.rs`) renders the search box and filter clear controls for browsing video and audio effects and transitions.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `effects-filter-clear` | Show All | Clears the active category filter in the Effects Browser to list all effects and transitions. | None |
| `effects-query` | Search | Filters the Effects Browser catalog by effect, transition, or preset name. | None |
| `effects-query-clear` | Clear search | Clears the active search query in the Effects Browser. | None |

## Audio Mixer

The Audio Mixer and shared audio insert chain (`crates/aria-ui/src/panels/insert_chain.rs`) render per-slot effect ordering and removal controls for track, clip, and master insert racks.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `insert-down` | ↓ | Moves the audio insert effect one slot later in the track or master insert chain. | None |
| `insert-remove` | Remove audio effect | Removes the audio insert effect from the track, clip, or master insert chain. | None |
| `insert-up` | ↑ | Moves the audio insert effect one slot earlier in the track or master insert chain. | None |

## Captions

The Captions panel (`crates/aria-ui/src/panels/captions.rs`) renders speech-to-text transcription and timeline commit controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `caption-commit` | Commit | Writes the transcript into the project as the text clips or markers `caption-target` selects, one undo entry per caption. | None |
| `caption-run` | Transcribe | Starts speech-to-text transcription on the selected clip or sequence audio. | None |
| `caption-target` (per row) | Text clips / Markers | Selects whether Commit places one timed text clip per caption or one timeline marker per caption. | None |
| `caption-threads` (per row) | 1 / 2 / 4 / 8 thread(s) | Sets how many CPU threads the transcription model decodes with. | None |
| `caption-words` | One clip per word | Commits one caption per transcribed word instead of one per segment, for segments that carry word timings. | None |

## Text

The Character, Paragraph, and Inspector Text surfaces (`crates/aria-ui/src/panels/inspector/tabs/text.rs` and `crates/aria-ui/src/panels/text_panel.rs`) render title text editing and font size stepping controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `font-step` (per row) | − / + | Steps the selected title's font size down or up in the Character and Inspector Text panels. | None |
| `text-edit` | Edit text… | Opens the text content prompt for the selected title clip. | None |

## Markers

The Markers panel (`crates/aria-ui/src/panels/markers.rs`) renders playhead marker creation, colour filter reset, and per-marker edit and removal controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `marker-add` | Add at playhead | Adds a new sequence marker at the current playhead frame. | None |
| `marker-edit` (per row) | Edit… | Opens the Marker Edit dialog for the marker row. | None |
| `marker-filter-all` | All | Clears the marker colour filter in the Markers panel to show all markers. | None |
| `marker-remove` (per row) | Remove | Deletes the marker row from the sequence or clip. | None |

## Color

The Color panel and grading blocks (`crates/aria-ui/src/panels/color_panel.rs`, `crates/aria-ui/src/panels/inspector/blocks/grade.rs`, `wheels.rs`, and `curve_editor/picker.rs`) render section headers, colour wheel resets, curve channel and axis chips, and grade reset buttons.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `ce-preset` (per row) | Preset name | Applies a saved curve shape preset inside the colour curve editor. | None |
| `color-grade-clear` | Reset grade | Resets all colour correction sections on the selected clip from the Color panel header. | None |
| `color-section` (per row) | Section title | Expands or collapses a grading section (Basic Correction, Creative, RGB Curves, Hue Curves, Color Wheels) in the Color panel. | None |
| `curve-axis` (per row) | Curve axis | Selects the active hue or luma curve axis in the colour curve editor. | None |
| `curve-param` (per row) | Curve channel | Selects the active RGB or luma channel in the colour curve editor. | None |
| `denoise-toggle` | On / Off | Toggles temporal/spatial video denoise on the selected clip. | None |
| `grade-clear` | Reset grade | Resets all colour grade adjustments on the selected clip from the Inspector Look tab. | None |
| `inspector-wheel-reset` | Reset wheel | Resets the Lift, Gamma, or Gain colour wheel back to neutral. | None |
| `stabilize-off` | Off | Disables video stabilization on the selected clip. | None |

## Scopes

The Scopes surface (`crates/aria-ui/src/panels/preview/scopes.rs`) renders the Viewer scope strip toggle for histogram, waveform, RGB parade, and vectorscope displays.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `preview-scopes-toggle` | Scope strip | Toggles the video scopes strip (Histogram, Waveform, Parade, Vectorscope) in the Viewer. | None |

## Audio Meters

The Audio Meters surface (`crates/aria-ui/src/panels/preview_peak.rs` and `crates/aria-ui/src/panels/audio_meters.rs`) renders the transport stereo peak meter and clip indicator reset.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `preview-meter` | Output peaks | Displays stereo output peak bars in the transport row and clears the clip indicator when clicked. | None |

## Alignment

The Alignment panel (`crates/aria-ui/src/panels/align_panel.rs`) renders the alignment reference target chips (Selection or Frame).

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `align-to` (per row) | Selection / Frame | Selects whether the Alignment panel aligns clips relative to the selection bounds or the sequence frame. | None |

## Keyframes

The Keyframes panel and property row keyframe navigator (`crates/aria-ui/src/panels/inspector/blocks/keyframes.rs`, `widgets.rs`, and `crates/aria-ui/src/panels/timeline/property_rows.rs`) render keyframe stepping, keyframe toggle diamonds, animation stopwatches, procedural driver controls, and property resets.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `inspector-driver-whip` | Driver pick whip | Arms the expression driver pick whip to bind a parameter to another property. | None |
| `kf-at` | ◆ / ◇ | Adds a keyframe at the playhead or removes the existing keyframe on the property row. | None |
| `kf-driver` | ƒ Driver / ƒ Remove | Attaches or removes a procedural parameter driver on the selected property. | None |
| `kf-flatten` | Flatten | Bakes an active procedural driver into explicit keyframes across the clip. | None |
| `kf-next` | › | Moves the playhead to the next keyframe on the property. | None |
| `kf-prev` | ‹ | Moves the playhead to the previous keyframe on the property. | None |
| `kf-toggle` | ◆ / ◇ | Toggles a keyframe at the current playhead frame in the Keyframes panel. | None |
| `path-watch` | Stopwatch | Enables or disables keyframe animation for a mask path on the timeline. | None |
| `prop-reset` | Reset property | Resets the property to its default value, writing a keyframe when the property is animated. | None |
| `prop-watch` | Stopwatch | Enables or disables keyframe animation on the property; Alt-click attaches or removes a driver. | None |

## History

The History panel (`crates/aria-ui/src/panels/history_panel.rs`) renders each applied or undone command in the session stack as a direct row target (`history-row-<depth>`) rather than a `components` button or chip. Clicking any non-current row moves the project to the state after that command; `Ctrl+Z` and `Ctrl+Shift+Z` step backward and forward through the same stack.

## AI Assistant

The AI Assistant panel (`crates/aria-ui/src/panels/ai_panel*.rs`) renders the prompt input, send and stop buttons, plan-only mode toggle, staged plan approval buttons, tool permission buttons, and transcript copy and clear controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `ai-ask-allow` | Allow | Approves a pending tool action requested by the AI assistant. | None |
| `ai-ask-deny` | Deny | Rejects a pending tool action requested by the AI assistant. | None |
| `ai-clear` | Clear | Clears the current conversation transcript in the AI Assistant panel. | None |
| `ai-copy` (per row) | Copy | Copies a single assistant message from the transcript to the clipboard. | None |
| `ai-copy-all` | Copy | Copies the full AI Assistant conversation transcript to the clipboard. | None |
| `ai-plan-apply` | Apply | Executes the staged edit plan proposed by the AI assistant. | None |
| `ai-plan-discard` | Discard | Discards the staged edit plan without modifying the project. | None |
| `ai-plan-only` | Plan only | Toggles plan-only mode so the assistant stages edits for review before execution. | None |
| `ai-prompt` | Message Aria… | Enters a natural-language prompt or edit instruction for the AI assistant. | None |
| `ai-send` | Send | Submits the current prompt to the configured AI assistant model. | None |
| `ai-stop` | Stop | Cancels the in-flight AI assistant generation or tool turn. | None |

## Export & Transcode

The Export surface (`crates/aria-ui/src/panels/export_dialog.rs`) renders destination path selection, image-sequence mode toggle, and export start and cancel buttons.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `export-cancel` | Cancel | Closes the Export dialog without starting an export job. | None |
| `export-dest` | Choose… | Opens a file save dialog to choose the output file path for export. | None |
| `export-frames` | Frames | Switches the export output target to an image sequence folder. | None |
| `export-go` | Export | Starts rendering the sequence or selected range with the current export settings. | None |

## Settings

The Settings dialog (`crates/aria-ui/src/panels/settings_dialog.rs` and `crates/aria-ui/src/panels/settings_dialog/*.rs`) renders preferences controls across AI providers, cache directories, keyboard shortcut sets, workspace layouts, plugins, storage quotas, and local transcription models.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `agent-dir-reveal` | Reveal agent files | Creates the AI agent configuration directory on demand and opens it in the system file manager. | None |
| `ai-list-models` | List models | Queries the configured AI provider endpoint for available model identifiers. | None |
| `ai-test-connection` | Test connection | Verifies network and credential connectivity with the configured AI provider. | None |
| `analysis-cache-clear` | Clear analysis | Deletes cached shot, silence, and loudness analysis records from disk. | None |
| `cache-clear` (per row) | Clear | Deletes all cached files inside the selected storage cache category. | None |
| `cache-move` (per row) | Move… | Moves existing cache files for the selected category to a new folder and updates its path. | None |
| `cache-redirect` (per row) | Redirect… | Points the selected cache category at a new folder without moving existing files. | None |
| `cache-reset` (per row) | Use default | Resets the selected cache category path back to the default storage root. | None |
| `caches-copy-paths` | Copy paths | Copies the formatted report of all cache directory paths and sizes to the clipboard. | None |
| `caches-measure` | Measure | Scans disk usage across all registered cache directories and updates their byte totals. | None |
| `model-install` (per row) | Install / Reinstall | Downloads and installs the selected local transcription model. | None |
| `plugin-folder` | Open Folder | Opens the installed plugins directory in the system file manager. | None |
| `plugin-install` | Install Plugin… | Opens a file picker to install a `.ariap` plugin archive. | None |
| `plugin-log-copy` (per row) | Copy | Copies the diagnostic log output of the selected plugin to the clipboard. | None |
| `quota-dec` | −512 MiB | Decreases the download cache quota by 512 MiB. | None |
| `quota-inc` | +512 MiB | Increases the download cache quota by 512 MiB. | None |
| `settings-done` | Done | Closes the Settings dialog. | None |
| `settings-key-clear` (per row) | Clear | Unbinds the keyboard shortcut from the command row. | None |
| `settings-key-keep` | Keep | Keeps the existing binding when a newly pressed chord conflicts with another command. | None |
| `settings-key-move` | Move | Reassigns a conflicting keyboard chord to the newly selected command. | None |
| `settings-key-revert` (per row) | Revert | Restores the command row's keyboard shortcut to its shipped default chord. | None |
| `settings-keymap-delete` | Delete set | Deletes the selected custom keymap preset from disk. | None |
| `settings-keymap-export` | Export… | Exports the current keyboard shortcut bindings to a `.json` keymap file. | None |
| `settings-keymap-import` | Import… | Imports keyboard shortcut bindings from a `.json` keymap file. | None |
| `settings-keys-reset` | Reset all | Restores every keyboard shortcut in the active set to its default chord. | None |
| `settings-workspace-forget` (per row) | Forget | Deletes the saved custom workspace layout. | None |
| `settings-workspace-reset` | Reset | Resets the active workspace dock layout to its factory pane arrangement. | None |
| `settings-workspace-save` | Save layout / Overwrite | Saves the current dock pane arrangement into the active workspace slot. | None |
| `storage-clear` | Clear cache | Clears downloaded cloud and stock media assets from the download cache. | None |
| `storage-relocate` | Move storage… | Opens a folder picker to relocate the root storage directory. | None |
| `timeout-dec` (per row) | Halve | Halves the execution timeout in seconds for the selected transcription step. | None |
| `timeout-inc` (per row) | Double | Doubles the execution timeout in seconds for the selected transcription step. | None |

## Project Home

The Project Home surface (`crates/aria-ui/src/panels/home/*.rs` and `crates/aria-ui/src/panels/cloud.rs`) renders the remote catalog loader for cloud templates and stock feeds alongside the Home route tiles (`home-new`, `home-open`, `home-sample`, `home-template`) and recent-project card actions (`recent-open`, `recent-reveal`, `recovery`).

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `cloud-load` | Load catalog | Fetches the remote cloud template or stock media catalog for the active feed. | None |

## Window & Dock Chrome

The application window frame, dock tab bars, and status strip (`crates/aria-ui/src/chrome.rs`, `crates/aria-ui/src/shell/frame_menu.rs`, and `crates/aria-ui/src/shell/strip.rs`) render client-side window caption buttons, dock tab overflow menus, nested sequence breadcrumbs, and dropped-frame counters.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `caption-close` | Close window | Prompts to save unsaved project changes if dirty and closes the application window. | None |
| `caption-max` | Maximise window | Toggles the application window between maximised and restored geometry. | None |
| `caption-min` | Minimise window | Minimises the application window to the system taskbar or dock. | None |
| `dock-tab-more` (per row) | More panels | Opens the overflow menu listing dock panels hidden by narrow frame width. | None |
| `status-dropped-frames` | Dropped frames | Displays the count of dropped playback frames in the status strip and resets the counter when clicked. | None |
| `status-nest-breadcrumb` | Nest breadcrumb | Returns the timeline from an opened nested sequence back to its parent sequence. | None |

## Modal Dialogs

Modal dialogs (`crates/aria-ui/src/panels/*_dialog.rs` and `crates/aria-ui/src/pending.rs`) render confirmation, cancellation, and parameter controls for About, Audio Gain, Install Plugin, Jobs, Keyframe Velocity, Marker Edit, Remove Attributes, Speed / Duration, and Unsaved Changes.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `about-close` | Close | Closes the About Aria dialog. | None |
| `about-data` | Open Data Folder | Opens the application data directory in the system file manager. | None |
| `audio-gain-apply` | Apply | Applies the configured gain or peak normalization settings to the selected clips. | None |
| `audio-gain-cancel` | Cancel | Closes the Audio Gain dialog without modifying clip gain. | None |
| `install-plugin-cancel` | Cancel | Cancels the pending plugin archive installation and closes the confirmation dialog. | None |
| `install-plugin-confirm` | Install / Update | Confirms permissions and installs or updates the selected plugin package. | None |
| `job-reveal` (per row) | Reveal | Reveals the output file of a completed background job in the system file manager. | None |
| `job-stop` (per row) | Stop | Requests cancellation of an in-flight background job. | None |
| `jobs-done` | Done | Closes the background Jobs dialog. | None |
| `keyframe-velocity-apply` | OK | Applies the incoming and outgoing speed and influence values to the selected keyframes. | None |
| `keyframe-velocity-cancel` | Cancel | Closes the Keyframe Velocity dialog without changing keyframe tangents. | None |
| `keyframe-velocity-continuous` | Continuous: On / Off | Toggles continuous Bézier tangent coupling between incoming and outgoing keyframe handles. | None |
| `marker-edit-cancel` | Cancel | Closes the Marker Edit dialog without saving changes. | None |
| `marker-edit-save` | Save | Saves the edited marker name, notes, duration, colour, and kind. | None |
| `remove-attr-apply` | Remove | Strips the checked attribute groups and effects from the selected timeline clips. | None |
| `remove-attr-cancel` | Cancel | Closes the Remove Attributes dialog without modifying selected clips. | None |
| `speed-duration-apply` | Apply | Applies the configured playback speed percentage, duration, reverse, and pitch settings to the selected clip. | None |
| `speed-duration-cancel` | Cancel | Closes the Speed / Duration dialog without modifying the clip. | None |
| `speed-duration-pitch` | Maintain Audio Pitch: On / Off | Toggles audio pitch preservation in the Speed / Duration dialog. | None |
| `speed-duration-reverse` | Reverse Speed: On / Off | Toggles reverse playback in the Speed / Duration dialog. | None |
| `unsaved-cancel` | Stay | Cancels the pending close, quit, or project switch and returns to the open project. | None |
| `unsaved-discard` | Discard | Discards unsaved project modifications and proceeds with the pending action. | None |
| `unsaved-save` | Save | Saves the current project and proceeds with the pending close, quit, or project switch. | None |
| `velocity-influence-preset` (per row) | Influence preset | Sets incoming or outgoing Bézier handle influence to a preset percentage. | None |
| `velocity-influence-step` (per row) | Influence step | Steps incoming or outgoing Bézier handle influence down or up by a fixed delta. | None |
| `velocity-speed-step` (per row) | Speed step | Steps incoming or outgoing keyframe velocity down or up by a fixed delta. | None |
| `velocity-speed-zero` (per row) | 0 | Sets incoming or outgoing keyframe velocity to zero. | None |
