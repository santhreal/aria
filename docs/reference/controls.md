# Controls

Every interactive button, chip, icon button, and text field in the Aria shell is identified by a `motion::ControlId` (`crates/aria-ui/src/motion_key.rs`). Static controls use a fixed `&'static str` identifier (`ControlId::Fixed`), while controls repeated across dynamic rows construct a hashed `MotionKey::new("<kind>", ...)` (`ControlId::Key`) so hover and press transitions stay isolated per row. Controls with hover tooltips or keyboard shortcut badges register a `ControlTip` entry in `crate::tooltip::CONTROL_TIPS` (`crates/aria-ui/src/tooltip.rs`), which links the control identifier to its human-readable label, an optional detail line, and an optional `keys::Binding` ID; shared widget constructors are defined in `crates/aria-ui/src/components.rs` (`button`, `ghost_button`, `danger_button`, `chip`, `text_box`, `icon_button`) and `crates/aria-ui/src/pending.rs` (`stop_button`).

## Example

1. Hover the Razor tool button (`tool-razor`) in the Tool Strip or the Play button (`play`) in the Viewer transport bar.
2. After `tooltip::SHOW_DELAY` (400 ms), the shell reads the control's entry in `CONTROL_TIPS` through `tooltip::tip_for` and shows its name beside the chord its binding has now (`Razor  C` or `Play  Space`). A tip with a detail shows it on a dimmed line under the name: `Ripple Trim  B` over `Trim and close the gap`.
3. Left-click the control to dispatch its action (`EditorTool::Razor` selection or `AppState::toggle_playback`), or press the bound chord (`C` or `Space`) directly to run the same action without moving the pointer.

## Tool Strip

The editor tool strip (`crates/aria-ui/src/chrome/tool_strip.rs` and `crates/aria-ui/src/tool_catalog.rs`) renders one icon button per editing tool plus the timeline drop-mode toggle.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `edit-mode` | Ripple / Overwrite | Toggles the timeline drop mode between Ripple and Overwrite. | None |
| `tool-hand` | Hand | Pans the Timeline horizontally by dragging without moving clips. | `H` |
| `tool-pen` | Pen | Adds keyframes on timeline curves or draws and edits Bézier mask points in the Viewer. | `P` |
| `tool-pointer` | Select | Selects, moves, and trims clips on the timeline and handles in the Viewer. | `V` |
| `tool-rate` | Rate Stretch | Drags a clip edge to change playback speed while preserving the source in point. | `R` |
| `tool-razor` | Razor | Splits the targeted clip at the clicked frame on the timeline. | `C` |
| `tool-ripple` | Ripple Trim | Trims a clip edge and shifts downstream clips on the track to close or open the gap. | `B` |
| `tool-roll` | Rolling Edit | Moves the cut point between two adjacent clips without changing the combined duration. | `N` |
| `tool-slide` | Slide | Moves a clip along the track while trimming the neighboring clips to keep their outer bounds fixed. | `U` |
| `tool-slip` | Slip | Shifts the source media in and out points inside a clip without moving its timeline bounds. | `Y` |
| `tool-track-backward` | Track Select Backward | Selects every clip on the targeted track or all unlocked tracks up to the clicked frame. | `Shift+A` |
| `tool-track-forward` | Track Select Forward | Selects every clip on the targeted track or all unlocked tracks from the clicked frame forward. | `A` |
| `tool-type` | Type | Creates a new title clip or enters inline text editing in the Viewer. | `T` |
| `tool-zoom` | Zoom | Zooms the Timeline horizontally in on click or out on Alt-click around the clicked position. | `Z` |

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
| `viewer-display-settings` | Viewer display settings | Opens the Viewer Display Settings menu for Split Viewer, Show Rulers, Show Guides, Snap to Guides, Clear Guides, Title Safe, Action Safe, Transparency Grid, and Grid. | None |
| `viewer-tab-close` | Close source tab | Closes the corresponding source media tab in the Viewer header. | `Ctrl+W` |
| `viewer-tab-program` | Program tab | Switches the Viewer pane to the active sequence output. | `Shift+2` |
| `viewer-tab-source` | Source tab | Switches the Viewer pane to the opened source media item; middle-click closes the tab. | `Shift+2` |

## Timeline

The Timeline pane (`crates/aria-ui/src/panels/timeline/*.rs` and `crates/aria-ui/src/panels/timeline_report.rs`) renders header toggles, sequence timecode and property search fields, track creation and zoom-fit controls, and graph editor mode chips and tangent buttons.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `add-track` | Add track | Appends a new video track to the active sequence. | None |
| `fit-timeline` | Zoom percentage | Displays the timeline zoom percentage and fits the entire sequence into the viewport when clicked. | `\` |
| `graph-btn-easy-ease` | Easy Ease | Converts selected keyframes or the keyframe at the playhead to smooth Bézier tangents. | `F9` |
| `graph-btn-hold` | Hold | Toggles hold interpolation on selected keyframes or the keyframe at the playhead. | `Ctrl+Alt+H` |
| `graph-mode-speed` | Speed | Switches the graph editor to the speed curve of the selected property. | None |
| `graph-mode-value` | Value | Switches the graph editor to the value curve of the selected property. | None |
| `property-search` | Search | Filters visible timeline property and keyframe rows by name. | None |
| `property-search-clear` | Clear search | Clears the active timeline property search filter. | None |
| `timeline-display-settings` | Timeline display settings | Opens the Timeline Display Settings menu for clip thumbnails (Head Thumbnails, Head and Tail Thumbnails, Continuous Thumbnails), audio waveforms (Logarithmic Waveforms, Linear Waveforms, Hide Waveforms), and playhead follow (Page Scroll, Smooth Scroll). | None |
| `timeline-graph` | Graph editor | Toggles the keyframe value and speed curve editor inside the timeline pane. | `Shift+F3` |
| `timeline-linked` | Linked selection | Toggles whether selecting or trimming a clip also acts on its linked audio or video partner. | None |
| `timeline-marker` | Add marker | Adds a marker at the playhead or opens the marker editor when a marker is already at that frame. | `M` |
| `timeline-snap` | Snapping | Toggles magnetic snapping of clips, cuts, and the playhead to timeline landmarks. | `S` |
| `timeline-tc` | Sequence timecode | Edits or seeks the sequence playhead position from the timeline header. | None |

## Inspector

The Inspector panel (`crates/aria-ui/src/panels/inspector/**/*.rs`) renders parameter reset buttons, transform parenting chips and pick whip, effect rack row controls, preset management controls, framing, mask, and matte buttons, animation, speed ramp, and layer style presets, audio role and ducking controls, template slot chips, clip link controls, and transition pickers.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `anim-clear` (per row) | Clear | Removes the in, out, or combo animation preset from the selected clip. | None |
| `anim-preset` (per row) | Preset name | Applies the animation preset (Fade in, Slide up, Zoom in, and the rest of the animation catalog) to the in, out, or combo slot of the selected clip; a preset with no renderer is labeled `(no renderer)`. | None |
| `aud-role` (per row) | Music / Sound FX / Voiceover / Extracted | Tags the selected clip with the audio role. | None |
| `aud-role-clear` | Untag | Clears the assigned audio role tag (Music, Sound FX, Voiceover, Extracted) from the selected clip. | None |
| `blur-toggle` | On / Off | Toggles motion blur evaluation for transform animation on the selected clip. | None |
| `chroma-clear` | Remove key | Removes the chroma key filter from the selected clip. | None |
| `chroma-key-color` (per row) | Green / Blue / Cyan | Keys the selected clip on the green, blue, or cyan screen color. | None |
| `clip-type-detach` | Detach | Detaches the linked audio stream from the selected video clip onto its own track item. | None |
| `crop-clear` | Reset framing | Resets the crop rectangle and horizontal and vertical flip flags on the selected clip to their defaults. | None |
| `denoise-toggle` | On / Off | Runs the selected clip's audio through RNNoise noise reduction, or stops it; drawn in the Inspector Audio tab. | None |
| `duck-run` | Duck under voice | Computes and applies volume keyframes that lower music or effects gain during dialogue segments. | None |
| `duck-setting` (per row) | Dip to / Attack / Release value | Steps the duck depth, attack time, or release time to its next value. | None |
| `flip-h` | On / Off | Toggles horizontal mirroring of the selected clip; drawn on the Mirror section's Horizontal line. | None |
| `flip-v` | On / Off | Toggles vertical mirroring of the selected clip; drawn on the Mirror section's Vertical line. | None |
| `fx-add` (per row) | Effect name | Appends the chosen effect from the inline picker to the selected clip's effect stack. | None |
| `fx-choice` (per row) | Option label | Selects a discrete enum option for an effect parameter. | None |
| `fx-curve` (per row) | Curve preset | Applies a named curve preset (Linear, S-Curve, Invert) to an effect curve parameter. | None |
| `fx-font` (per row) | Font family | Selects the font family for an effect text parameter. | None |
| `fx-grad` (per row) | Gradient preset | Applies a preset color stop ramp to an effect gradient parameter. | None |
| `fx-group` (per row) | Parameter group | Expands or collapses a collapsible parameter group inside an applied effect. | None |
| `fx-layer` (per row) | Layer clip | Selects another timeline clip as a secondary layer input for an effect parameter. | None |
| `fx-layer-none` (per row) | None | Clears the secondary layer input slot on an effect parameter. | None |
| `fx-param-bool` (per row) | On / Off | Toggles a boolean parameter on an applied clip effect. | None |
| `fx-text-clear` (per row) | Clear | Clears the text string of an effect text parameter. | None |
| `inspector-fx-bypass` (per row) | fx | Toggles bypass for an individual effect row on the selected clip. | None |
| `inspector-fx-down` (per row) | Move later | Moves the effect one slot later in the clip's evaluation order. | None |
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
| `inspector-fx-up` (per row) | Move earlier | Moves the effect one slot earlier in the clip's evaluation order. | None |
| `inspector-parent-whip` | Parent pick whip | Attaches the transform parenting pick whip tooltip and arms clip parenting on click or drag. | None |
| `inspector-pitch` | On / Off | Toggles pitch preservation when the selected clip plays at a non-unit speed. | None |
| `inspector-replace-media` | Media name | Repoints the selected clip at the pooled media and keeps its placement, transform, and effects; drawn under Replace media in the Info tab. | None |
| `inspector-reset` | Reset transform | Resets position, scale, rotation, anchor, and opacity on the selected clip. | None |
| `inspector-reverse` | On / Off | Toggles reverse playback on the selected clip. | None |
| `inspector-slot-hint` | Edit hint… | Opens the prompt to edit the placeholder instruction text for a template slot. | None |
| `inspector-slot-hint-clear` | Clear hint | Clears the placeholder instruction text from a template slot. | None |
| `inspector-slot-toggle` | Mark as template slot / Clear template slot | Marks the selected clip as a replaceable template slot or removes its slot designation. | None |
| `inspector-text-editable` | On / Off | Toggles whether a title clip's text remains editable when packaged as a template; drawn on the Info tab's Text editable in template line. | None |
| `look-choice` (per row) | Look name | Applies a bundled creative look grade to the selected clip. | None |
| `look-clear` | Clear | Clears the applied creative look from the selected clip. | None |
| `lut-clear` | Remove LUT | Removes the loaded 3D `.cube` LUT from the selected clip. | None |
| `lut-load` | Load .cube… | Opens a file picker to load a `.cube` 3D LUT file onto the selected clip. | None |
| `mask-down` (per row) | Move down | Moves the mask one place lower in the selected clip's mask stack. | None |
| `mask-invert` (per row) | Invert | Inverts the mask at that place in the mask stack. | None |
| `mask-mode` (per row) | None / Add / Subtract / Intersect / Lighten / Darken / Difference | Sets how the mask combines with the masks above it in the stack. | None |
| `mask-remove` (per row) | Remove | Deletes the mask from the selected clip's mask stack. | None |
| `mask-shape` (per row) | Linear / Mirror / Circle / Rectangle / Heart / Star / Path | Adds a mask of the shape to the bottom of the selected clip's mask stack. | None |
| `mask-up` (per row) | Move up | Moves the mask one place higher in the selected clip's mask stack. | None |
| `matte-clear` | Remove matte | Clears the track matte assignment from the selected clip. | None |
| `matte-invert` | On / Off | Inverts the alpha or luma channel read from the assigned matte clip; drawn on the Track matte section's Invert line. | None |
| `matte-source` (per row) | Alpha / Luma | Sets whether the track matte reads the alpha or the luma of the layer above the selected clip. | None |
| `parent-choice` (per row) | Clip name | Assigns the clicked timeline clip as the transform parent of the selected clip. | None |
| `parent-none` | None | Detaches the selected clip from its current transform parent. | None |
| `parent-whip` | @ | Arms or disarms the transform parent pick whip for clicking or dragging onto a target clip. | None |
| `relink-media` | Relink | Opens the Relink Media file picker, filtered to media files, and relinks the missing media pool item to the chosen file. Drawn in the Info tab and on a missing pool item. | None |
| `speed-clear` | Clear ramp | Removes variable speed ramp segments and returns the clip to constant speed. | None |
| `speed-preset` (per row) | Ramp up / Ramp down / Montage / Hero / Bullet / Jump cut / Flash in / Flash out | Applies the speed ramp preset to the selected clip. | None |
| `style-toggle` (per row) | On / Off | Adds or removes the layer style on the selected clip; drawn on the style's line (Shadow, Glow, Outline, Background). | None |
| `styles-clear` | Clear styles | Removes drop shadow, stroke, glow, and background layer styles from the selected clip. | None |
| `template-accepts` (per row) | Any visual / Video only / Image only / Audio only | Sets the media a template slot on the selected clip accepts when the template is filled. | None |
| `tr-bool` (per row) | On / Off | Toggles a boolean parameter on the selected timeline transition. | None |
| `tr-choice` (per row) | Option label | Selects an enum option for a parameter on the selected timeline transition. | None |
| `transition-align` (per row) | Alignment | Sets transition placement across the cut (Center at Cut, Start at Cut, End at Cut). | None |
| `transition-choice` (per row) | Transition kind | Selects the transition type (Cross Dissolve, Dip to Black, Wipe, Slide) on the active cut. | None |
| `transition-clear` | Remove transition | Removes the transition from the selected cut point. | None |

## Project Panel

The Project panel (`crates/aria-ui/src/panels/sidebar_*.rs`, `crates/aria-ui/src/surfaces/bin_strip.rs`, `crates/aria-ui/src/panels/generate.rs`, and `crates/aria-ui/src/panels/analysis.rs`) renders bin management buttons, media pool search and layout toggles, multicam sync and angle controls, subclip and generator actions, stock media kind chips, and media analysis controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `analysis-clear` (per row) | × | Clears cached analysis results for the specified detector kind on the selected clip. | None |
| `analysis-confidence` (per row) | ≥0% / ≥25% / ≥50% / ≥75% | Hides detected moments whose confidence is below the chosen value. | None |
| `analysis-cut-silence` | Ripple-delete silence | Splits and ripple-deletes detected silent spans on the selected timeline clip. | None |
| `analysis-kind` (per row) | All / Shots / Silence / Beats | Lists the moments of every detector, or of the chosen detector only. | None |
| `analysis-loudness` | Loudness | Measures the windowed dBFS loudness contour across the selected clip's audio stream. | None |
| `analysis-next` | Next | Moves the playhead to the next detected analysis moment. | None |
| `analysis-prev` | Previous | Moves the playhead to the previous detected analysis moment. | None |
| `analysis-refresh` | Refresh | Reloads stored analysis moments for the selected clip from the on-disk moments index into the Analysis panel. | None |
| `analysis-run` (per row) | Shots / Silence / Beats | Runs the shot, silence, or beat detector on the selected clip. | None |
| `analysis-split-shots` | Split at shots | Splits the selected timeline clip at every detected shot boundary. | None |
| `bin-delete` | Delete bin | Deletes the currently open media bin and moves its contents to the parent bin. | None |
| `bin-name` | Bin name | Edits the name of the bin currently being renamed. | None |
| `bin-new` | New bin | Creates a new media bin inside the current Project panel folder. | None |
| `bin-rename` | Rename | Focuses the inline name field to rename the currently open bin. | None |
| `create-multicam-group` | New multicam | Creates a new multicam group from the selected media pool items. | None |
| `create-subclip` | Make Subclip | Creates a new subclip item in the media pool from the current Source Monitor in/out range. | None |
| `generate-kind` (per row) | Generator kind | Selects the AI generation output kind (Image, Video, or Voice) in the Project panel Generate tab. | None |
| `generate-prompt` | Describe what to generate | Edits the prompt the Generate tab submits; Enter submits it. | None |
| `generate-submit` | Generate | Submits the prompt in the Generate tab to the fal AI generation provider as a background job and imports the generated media file into the pool. | None |
| `media-import` | Import | Opens the File > Import file picker: preset files go to the preset library, a timeline file is read into the timeline, and other files are imported into the media pool and placed on the timeline from the playhead. | `Ctrl+I` |
| `media-query` | Search | Filters media pool items in the active bin by filename or metadata. | None |
| `nudge-multicam-back` (per row) | Nudge angle back | Shifts the multicam angle offset one frame earlier relative to the group sync map. | None |
| `nudge-multicam-forward` (per row) | Nudge angle forward | Shifts the multicam angle offset one frame later relative to the group sync map. | None |
| `place-multicam-group` (per row) | Place | Places the multicam group onto the active sequence at the playhead. | None |
| `pool-audio-only` | Audio only | Filters the media pool view to show audio assets only. | None |
| `pool-sort` | Sort order | Opens the media pool sort order menu (Name, Duration, Resolution, Date added). | None |
| `pool-view-freeform` | Freeform | Switches the media pool layout to a freeform spatial canvas. | None |
| `pool-view-icons` | Icons | Switches the media pool layout to large thumbnail icons. | None |
| `pool-view-list` | List | Switches the media pool layout to a columnar metadata list. | None |
| `pool-view-tiles` | Tiles | Switches the media pool layout to compact tiles with metadata. | None |
| `relink-all` | Relink All | Opens a file picker to relink all missing media items in the project. | None |
| `sidebar-group` (per row) | Group tab | Switches the top-level Project panel category (Media, Graphics, Effects, Library, Analyze). | None |
| `sidebar-tab` (per row) | Tab label | Switches the Project panel to the tab (Media, Titles, Shapes, Stickers, Generators, Animations, Masks, Transitions, Effects, Stock, SFX, Generate, Captions, Analysis, Audio) within the open group. | None |
| `stock-kind` (per row) | Video / Photo / Audio | Selects the media kind a stock search returns. | None |
| `stock-query` | Search stock media | Edits the stock media search query; Enter runs the search. | None |
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

The Audio Mixer and shared audio insert chain (`crates/aria-ui/src/panels/insert_chain.rs`) render per-slot effect adding, bypass, key lane, ordering, and removal controls for track, clip, and master insert racks.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `insert-add` (per row) | + Audio effect | Opens or closes the audio effect picker for the track, clip, or master insert chain. | None |
| `insert-add-effect` (per row) | Effect name | Appends the audio effect to the end of the insert chain. | None |
| `insert-bypass` (per row) | On / Bypassed | Bypasses the audio insert effect, or puts it back in the signal path. | None |
| `insert-down` | Move later | Moves the audio insert effect one slot later in the track, clip, or master insert chain. | None |
| `insert-key` (per row) | Lane name | Sets the lane whose audio keys the insert effect, such as a sidechain compressor. | None |
| `insert-key-none` (per row) | No key | Clears the key lane of the insert effect. | None |
| `insert-remove` | Remove audio effect | Removes the audio insert effect from the track, clip, or master insert chain. | None |
| `insert-up` | Move earlier | Moves the audio insert effect one slot earlier in the track, clip, or master insert chain. | None |

## Captions

The Captions panel (`crates/aria-ui/src/panels/captions.rs`) renders speech-to-text transcription and timeline commit controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `caption-commit` | Commit | Writes the transcript into the project as the text clips or markers `caption-target` selects, one undo entry per caption. | None |
| `caption-language` | Language | Edits the language code transcription runs in, such as `en`; an empty box detects the language. Enter starts transcription. | None |
| `caption-run` | Transcribe | Starts speech-to-text transcription on the selected media-backed timeline clip. | None |
| `caption-target` (per row) | Text clips / Markers | Selects whether Commit places one timed text clip per caption or one timeline marker per caption. | None |
| `caption-threads` (per row) | 1 thread / 2 threads / 4 threads / 8 threads | Sets how many CPU threads the transcription model decodes with. | None |
| `caption-words` | One clip per word | Commits one caption per transcribed word instead of one per segment, for segments that carry word timings. | None |

## Text

The Character, Paragraph, and Inspector Text surfaces (`crates/aria-ui/src/panels/inspector/tabs/text.rs` and `crates/aria-ui/src/panels/text_panel.rs`) render title text editing, font size stepping, alignment, letter case, text style, and text effect controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `font-step` (per row) | − / + | Steps the selected title's font size down or up in the Character and Inspector Text panels. | None |
| `text-align-h` (per row) | Left / Center / Right / Justify | Sets the horizontal alignment of the selected title. | None |
| `text-align-v` (per row) | Top / Middle / Bottom | Sets the vertical alignment of the selected title block in the canvas. | None |
| `text-case` (per row) | As typed / Upper / Lower / Title | Sets the letter case the selected title is drawn in. | None |
| `text-edit` | Edit text… | Opens the text content prompt for the selected title clip. | None |
| `text-effect` (per row) | Neon / Shadow / Outline / Glow / Retro / Chrome | Applies the text effect preset to the selected title, or removes it when it is already applied. | None |
| `text-switch` (per row) | Bold / Italic / Underline / Kerning / Wrap / Stroke / Shadow / Card | Toggles the text style on the selected title. | None |

## Markers

The Markers panel (`crates/aria-ui/src/panels/markers.rs`) renders playhead marker creation, color filter reset, and per-marker edit, rename, note, and removal controls.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `marker-add` | Add at playhead | Adds a new sequence marker at the current playhead frame. | None |
| `marker-edit` (per row) | Edit… | Opens the Marker Edit dialog for the marker row. | None |
| `marker-filter-all` | All | Clears the marker color filter in the Markers panel to show all markers. | None |
| `marker-note` (per row) | Note | Opens the inline editor for the marker's comment. | None |
| `marker-remove` (per row) | Remove | Deletes the marker row from the sequence or clip. | None |
| `marker-rename` (per row) | Rename | Opens the inline editor for the marker's name. | None |

## Color

The Color panel and grading blocks (`crates/aria-ui/src/panels/color_panel.rs`, `crates/aria-ui/src/panels/inspector/blocks/grade.rs`, and `wheels.rs`) render section headers, color wheel resets, blend mode and stabilization level buttons, and grade reset buttons.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `color-grade-clear` | Reset grade | Resets all color correction sections on the selected clip from the Color panel header. | None |
| `color-section` (per row) | Section title | Expands or collapses a grading section (Basic Correction, Creative, RGB Curves, Hue Curves, Color Wheels, HSL Secondary, Vignette) in the Color panel. | None |
| `grade-blend` (per row) | Normal / Darken / Multiply / Color Burn / Lighten / Screen / Color Dodge / Add / Overlay / Soft Light / Hard Light / Difference / Exclusion | Sets the blend mode the selected clip composites with. | None |
| `grade-clear` | Reset grade | Resets all color grade adjustments on the selected clip from the Inspector Look tab. | None |
| `inspector-wheel-reset` | Reset wheel | Resets the Lift, Gamma, or Gain color wheel back to neutral. | None |
| `stabilize-level` (per row) | Recommended / Smooth / Max smooth | Enables video stabilization on the selected clip at the smoothing level. | None |
| `stabilize-off` | Off | Disables video stabilization on the selected clip. | None |

## Scopes

The Scopes surface (`crates/aria-ui/src/panels/preview/scopes.rs`) renders the Viewer scope strip toggle for histogram, waveform, RGB parade, and vectorscope displays, and the waveform mode chips.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `preview-scopes-toggle` | Scope strip | Toggles the video scopes strip (Histogram, Waveform, Parade, Vectorscope) in the Viewer. | None |
| `scope-waveform-mode` (per row) | Luma / RGB / YCbCr | Sets whether the waveform monitor shows luma, an RGB parade, or a YCbCr parade. | None |

## Audio Meters

The Audio Meters surface (`crates/aria-ui/src/panels/preview_peak.rs` and `crates/aria-ui/src/panels/audio_meters.rs`) renders the transport stereo peak meter and clip indicator reset.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `preview-meter` | Output peaks | Displays stereo output peak bars in the transport row and clears the clip indicator when clicked. | None |

## Alignment

The Alignment panel (`crates/aria-ui/src/panels/align_panel.rs`) renders the alignment reference target chips (Selection or Frame) and the align and distribute buttons.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `align-edge` (per row) | Left / Center / Right / Top / Middle / Bottom | Aligns the selected clips to the edge or center line of the selection bounds or the frame. | None |
| `align-spread` (per row) | Across / Down | Distributes three or more selected clips evenly across or down. | None |
| `align-to` (per row) | Selection / Frame | Selects whether the Alignment panel aligns clips relative to the selection bounds or the sequence frame. | None |

## Keyframes

The Inspector's Keyframes and Graph blocks and property row keyframe navigator (`crates/aria-ui/src/panels/inspector/blocks/keyframes.rs`, `curve_editor/picker.rs`, `widgets.rs`, and `crates/aria-ui/src/panels/timeline/property_rows.rs`) render keyframe stepping, keyframe toggle diamonds, easing presets, the graphed property and axis, animation stopwatches, procedural driver controls and pick whips, and property resets.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `curve-axis` (per row) | X / Y | Selects the axis of a two-dimensional property the Graph block draws. | None |
| `curve-param` (per row) | Property name | Selects the keyframed property the Graph block draws. | None |
| `inspector-driver-whip` | Driver pick whip | Arms the expression driver pick whip to bind a parameter to another property. | None |
| `keyframe-easing` (per row) | Snappy / Overshoot / Anticipate | Applies the bezier easing preset to the segment leaving the keyframe at the playhead. | None |
| `keyframe-piecewise` (per row) | Bounce out / Elastic out / Back out | Applies the piecewise easing preset to the segment leaving the keyframe at the playhead. | None |
| `kf-at` | ◆ / ◇ | Adds a keyframe at the playhead or removes the existing keyframe on the property row. | None |
| `kf-driver` (per row) | ƒ Driver / ƒ Remove | Attaches or removes a procedural parameter driver on the property of the Keyframes block row. | None |
| `kf-flatten` (per row) | Flatten | Removes all keyframes on the property and holds the value sampled at the playhead as a constant. | None |
| `kf-next` (per row) | › | Moves the playhead to the next keyframe on the property. | None |
| `kf-prev` (per row) | ‹ | Moves the playhead to the previous keyframe on the property. | None |
| `kf-toggle` (per row) | ◆ / ◇ | Toggles a keyframe at the playhead on the property of the Keyframes block row. | None |
| `param-whip` (per row) | @ | Arms or disarms the driver pick whip for the property row being edited. | None |
| `path-watch` | Path stopwatch | Enables or disables keyframe animation for a mask path on the timeline. | None |
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

The Export surface (`crates/aria-ui/src/panels/export_dialog.rs`) renders a chip for each export choice, the save-as and image-sequence buttons, and the export start and cancel buttons. Each chip row lights the choice in force; a row with one choice states it as text instead.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `export-audio` (per row) | Included / Video only | Sets whether the exported file carries the sequence's audio track. Shown unless the chosen plugin encoder writes no audio. | None |
| `export-bitrate` (per row) | Encoder default / 4 Mb/s / 8 Mb/s / 16 Mb/s / 40 Mb/s | Sets the target video bitrate. Shown where the platform encoder honors a bitrate. | None |
| `export-cancel` | Cancel | Closes the Export dialog without starting an export job. | None |
| `export-codec` (per row) | H.264 / H.265 | Sets the video codec, from the codecs this machine's encoder opens and the codec in force. Shown once the codec probe has answered and more than one codec is offered. | None |
| `export-dest` | Export As… | Opens the Export save dialog with the format's extension and the proposed file name, then starts the export to the chosen path and opens the Jobs dialog. | None |
| `export-format` (per row) | Platform format / plugin encoder | Selects the platform encoder or a mounted plugin encoder. Shown when a plugin registers an encoder. | None |
| `export-frames` | Export Frames… | Opens a folder picker and starts an image-sequence export job to the chosen folder, opening the Jobs dialog. | None |
| `export-go` | Export | Starts rendering the sequence or selected range with the current export settings. | None |
| `export-gop` (per row) | Codec default / Every 15 / Every 30 / Every 60 | Sets the maximum keyframe interval in frames. | None |
| `export-range` (per row) | Whole timeline / marked span | Renders the whole timeline or only the span between In and Out; with nothing marked, the marked choice reports how to mark one. | None |
| `export-rate` (per row) | Project / 24 / 25 / 30 / 50 / 60 | Sets the output frame rate: the sequence's own or a fixed delivery rate. | None |
| `export-scale` (per row) | 100% / 75% / 50% / 25% | Sets the output size as a fraction of the canvas; the row shows the resulting size. | None |

## Settings

The Settings dialog (`crates/aria-ui/src/panels/settings_dialog.rs` and `crates/aria-ui/src/panels/settings_dialog/*.rs`) renders preferences controls across AI providers, playback, cache directories, keyboard shortcut sets, workspace layouts, plugins and their settings and WASM limits, storage quotas, and local transcription models.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `agent-dir-reveal` | Reveal agent files | Creates the AI agent configuration directory on demand and opens it in the system file manager. | None |
| `ai-autonomy` (per row) | Ask / Full | Sets whether each destructive AI agent tool call needs a confirmation in the agent panel (Ask) or every tool runs without one (Full). | None |
| `ai-list-models` | List models | Queries the configured AI provider endpoint for available model identifiers. | None |
| `ai-protocol` (per row) | Chat Completions / Responses | Selects the API the AI provider is called through: Chat Completions or Responses. | None |
| `ai-reasoning` (per row) | Auto / Off | Requests the most detailed reasoning summary the AI provider supports, or none. | None |
| `ai-source` (per row) | Local / OpenRouter / Custom | Selects the AI provider: a local OpenAI-compatible server, the OpenRouter gateway, or a custom endpoint. | None |
| `ai-test-connection` | Test connection | Verifies network and credential connectivity with the configured AI provider. | None |
| `analysis-cache-clear` | Clear analysis | Deletes cached shot, silence, and loudness analysis records from disk. | None |
| `cache-clear` (per row) | Clear | Deletes all cached files inside the selected storage cache category. | None |
| `cache-move` (per row) | Move… | Opens the Move Cache folder picker, moves the category's cache files to the chosen folder, and updates its path. | None |
| `cache-redirect` (per row) | Redirect… | Opens the Redirect Cache folder picker and points the category at the chosen folder without moving existing files. | None |
| `cache-reset` (per row) | Use default | Resets the selected cache category path back to the default storage root. | None |
| `caches-copy-paths` | Copy paths | Copies the formatted report of all cache directory paths and sizes to the clipboard. | None |
| `caches-measure` | Measure | Scans disk usage across all registered cache directories and updates their byte totals. | None |
| `model-install` (per row) | Install / Reinstall | Downloads and installs the selected local transcription model. | None |
| `plugin-folder` | Open Folder | Opens the installed plugins directory in the system file manager. | None |
| `plugin-install` | Install Plugin… | Opens a file picker to install a `.ariap` plugin archive. | None |
| `plugin-log-copy` (per row) | Copy | Copies the diagnostic log output of the selected plugin to the clipboard. | None |
| `plugin-row-action` (per row) | Enable / Disable / Reload / Uninstall / Open Folder / Log / Hide Log | Enables, disables, reloads, or uninstalls the plugin, opens its folder, or shows or hides its log. | None |
| `plugin-setting-option` (per row) | Option label | Saves the option of a plugin's toggle or choice setting. | None |
| `quota-dec` | −512 MiB | Decreases the download cache quota by 512 MiB. | None |
| `quota-inc` | +512 MiB | Increases the download cache quota by 512 MiB. | None |
| `settings-audio-scrubbing` | Audio scrubbing: On / Off | Toggles a 40 ms audio grain at each pointer position while the playhead, the ruler, or a timeline scrub is dragged. | None |
| `settings-done` | Done | Closes the Settings dialog. | None |
| `settings-field` (per row) | Field label | Edits the row's text setting: the AI provider's base URL, model, key, or key variable, the cloud base URL, the section filter, the keymap set name, or a plugin's text setting. Enter saves it and Esc leaves the saved value. | None |
| `settings-key-clear` (per row) | Clear | Unbinds the keyboard shortcut from the command row. | None |
| `settings-key-keep` | Keep | Keeps the existing binding when a newly pressed chord conflicts with another command. | None |
| `settings-key-move` | Move | Reassigns a conflicting keyboard chord to the newly selected command. | None |
| `settings-key-revert` (per row) | Revert | Restores the command row's keyboard shortcut to its shipped default chord. | None |
| `settings-keymap` (per row) | Keymap set name | Loads the saved keyboard shortcut set and makes it active. | None |
| `settings-keymap-delete` | Delete set | Deletes the selected custom keymap preset from disk. | None |
| `settings-keymap-export` | Export… | Exports the current keyboard shortcut bindings to a `.json` keymap file. | None |
| `settings-keymap-import` | Import… | Imports keyboard shortcut bindings from a `.json` keymap file. | None |
| `settings-keys-reset` | Reset all | Restores every keyboard shortcut in the active set to its default chord. | None |
| `settings-log-dropped-frames` | Log dropped frames: On / Off | Toggles writing each dropped playback frame and its cause to the log under the `aria::playback` target. | None |
| `settings-workspace-forget` (per row) | Forget | Deletes the saved custom workspace layout. | None |
| `settings-workspace-reset` | Reset | Resets the active workspace dock layout to its factory pane arrangement. | None |
| `settings-workspace-save` | Save layout / Overwrite | Saves the current dock pane arrangement into the active workspace slot. | None |
| `storage-clear` | Clear cache | Clears downloaded cloud and stock media assets from the download cache. | None |
| `storage-relocate` | Move storage… | Opens the Move Storage folder picker and moves the root storage directory to the chosen folder. | None |
| `timeout-dec` (per row) | Halve | Halves the execution timeout in seconds for the selected transcription step. | None |
| `timeout-inc` (per row) | Double | Doubles the execution timeout in seconds for the selected transcription step. | None |
| `wasm-limit-dec` (per row) | Halve | Halves the WASM fuel per call or the WASM memory limit for plugins. | None |
| `wasm-limit-inc` (per row) | Double | Doubles the WASM fuel per call or the WASM memory limit for plugins. | None |

## Project Home

The Project Home surface (`crates/aria-ui/src/panels/home/*.rs` and `crates/aria-ui/src/panels/cloud.rs`) renders the remote catalog loader for cloud templates and stock feeds alongside the Home route tiles (`home-new`, `home-open`, `home-sample`, `home-template`), the recent-project card actions, and the recovery banner. A press on a recent card away from its actions runs the card's `recent-open` action.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `cloud-load` | Load catalog | Fetches the remote cloud template or stock media catalog for the active feed. | None |
| `recent-open` (per row) | Open or Relink | Opens the recent project, or prompts for its new location when the file is missing. | None |
| `recent-reveal` (per row) | Reveal | Shows the recent project's file in the system file manager. | None |
| `recovery` (per row) | Restore or Discard | Opens the recovered copy of the last session, or deletes the copy. | None |

## Window & Dock Chrome

The application window frame, dock tab bars, and status strip (`crates/aria-ui/src/chrome.rs`, `crates/aria-ui/src/shell/frame_menu.rs`, and `crates/aria-ui/src/shell/strip.rs`) render client-side window caption buttons, dock tab overflow menus, nested sequence breadcrumbs, and dropped-frame counters. Docked plugin panels (`crates/aria-ui/src/panels/plugin_panel.rs`) draw the controls a plugin declares with the same shared widgets.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `caption-close` | Close window | Prompts to save unsaved project changes if dirty and closes the application window. | None |
| `caption-max` | Maximize window | Toggles the application window between maximized and restored geometry. | None |
| `caption-min` | Minimize window | Minimizes the application window to the system taskbar or dock. | None |
| `dock-tab-more` (per row) | More panels | Opens the overflow menu listing dock panels hidden by narrow frame width. | None |
| `plugin-panel-open` (per row) | Open | Loads the plugin package that declares the waiting panel and opens the panel. | None |
| `plugin-ui-button` (per row) | Button text | Sends a press event to the plugin that declared the panel button. | None |
| `plugin-ui-choice` (per row) | Option text | Sends the chosen option of a panel choice control to the plugin. | None |
| `plugin-ui-input` (per row) | Field text | Gives the plugin panel text field keyboard focus. | None |
| `plugin-ui-toggle` (per row) | Toggle text | Sends the new state of a panel toggle control to the plugin. | None |
| `status-dropped-frames` | Dropped frames | Displays the count of dropped playback frames in the status strip and resets the counter when clicked. | None |
| `status-nest-breadcrumb` | Nest breadcrumb | Returns the timeline from an opened nested sequence to the root sequence. | None |

## Modal Dialogs

Modal dialogs (`crates/aria-ui/src/panels/*_dialog.rs`, `crates/aria-ui/src/panels/project_report.rs`, `crates/aria-ui/src/panels/media_support.rs`, `crates/aria-ui/src/panels/timeline_report.rs`, `crates/aria-ui/src/panels/palette/mod.rs` and `crates/aria-ui/src/pending.rs`) render confirmation, cancellation, and parameter controls for About, Audio Gain, Command Palette, Install Plugin, Jobs, Keyframe Velocity, Marker Edit, Media Support, Project Report, Remove Attributes, Speed / Duration, Timeline File, and Unsaved Changes.

| Control ID | Label | Action | Shortcut |
| --- | --- | --- | --- |
| `about-close` | Close | Closes the About Aria dialog. | None |
| `about-data` | Open Data Folder | Opens the application data directory in the system file manager. | None |
| `audio-gain-apply` | Apply | Applies the configured gain or peak normalization settings to the selected clips. | None |
| `audio-gain-cancel` | Cancel | Closes the Audio Gain dialog without modifying clip gain. | None |
| `audio-gain-mode` (per row) | Set Gain to / Adjust Gain by / Normalize Max Peak to / Normalize All Peaks to | Selects how the Audio Gain dialog changes the gain of the selected clips. | None |
| `audio-gain-preset` (per row) | -12 dB … +6 dB / -12 dBFS … 0 dBFS | Sets the gain or the normalization target in the Audio Gain dialog to the preset value. | None |
| `audio-gain-step` (per row) | -3 dB / -1 dB / -0.5 dB / +0.5 dB / +1 dB / +3 dB | Steps the gain or the normalization target in the Audio Gain dialog by the value. | None |
| `install-plugin-cancel` | Cancel | Cancels the pending plugin archive installation and closes the confirmation dialog. | None |
| `install-plugin-confirm` | Install / Update | Confirms permissions and installs or updates the selected plugin package. | None |
| `job-reveal` (per row) | Reveal | Reveals the output file of a completed background job in the system file manager. | None |
| `job-stop` (per row) | Stop | Requests cancellation of an in-flight background job. | None |
| `jobs-done` | Done | Closes the background Jobs dialog. | None |
| `keyframe-velocity-apply` | OK | Applies the incoming and outgoing speed and influence values to the selected keyframes. | None |
| `keyframe-velocity-cancel` | Cancel | Closes the Keyframe Velocity dialog without changing keyframe tangents. | None |
| `keyframe-velocity-continuous` | Continuous: On / Off | Toggles continuous Bézier tangent coupling between incoming and outgoing keyframe handles. | None |
| `marker-edit-cancel` | Cancel | Closes the Marker Edit dialog without saving changes. | None |
| `marker-edit-field` (per row) | Name / Comment / Duration (timecode) | Edits the marker's name, comment, or duration in the Marker Edit dialog; Enter saves the marker. | None |
| `marker-edit-kind` (per row) | Comment / Chapter / Segmentation | Sets the kind of the marker in the Marker Edit dialog. | None |
| `marker-edit-save` | Save | Saves the edited marker name, notes, duration, color, and kind. | None |
| `media-exposure` | Measure exposure | Measures the exposure of the frame on screen and keeps the readout. | None |
| `media-loudness` | Loudness overview | Reports where the loudest moment of the selected source's audio stream is. | None |
| `media-normalise` | Normalize selection | Sets the selected clip's gain so its measured peak lands on the target, from the loudness contour the Analysis panel measured. | None |
| `media-refresh` | Re-read | Rebuilds the Media Support report. | None |
| `media-support-close` | Close | Closes the Media Support dialog. | None |
| `media-verify` | Verify decode | Decodes the selected clip's still or animation from its path and from its bytes and compares the two. | None |
| `palette-query` | Type to filter… | Filters the Command Palette's commands by name as it is typed into; Up and Down move the highlight and Enter runs the highlighted command. | None |
| `project-report-close` | Close | Closes the Project Report dialog. | None |
| `remove-attr` (per row) | Motion / Opacity / Crop / Time Remapping / Volume / Pan / Effects / Audio Effects / Grade / Mask / Chroma Key | Ticks or unticks the attribute group the Remove Attributes dialog strips. | None |
| `remove-attr-apply` | Remove | Strips the checked attribute groups and effects from the selected timeline clips. | None |
| `remove-attr-cancel` | Cancel | Closes the Remove Attributes dialog without modifying selected clips. | None |
| `speed-duration-apply` | Apply | Applies the configured playback speed percentage, duration, reverse, and pitch settings to the selected clip. | None |
| `speed-duration-cancel` | Cancel | Closes the Speed / Duration dialog without modifying the clip. | None |
| `speed-duration-length-step` (per row) | -24f / -5f / -1f / +1f / +5f / +24f | Steps the clip duration in the Speed / Duration dialog by the number of frames. | None |
| `speed-duration-pitch` | Maintain Audio Pitch: On / Off | Toggles audio pitch preservation in the Speed / Duration dialog. | None |
| `speed-duration-preset` (per row) | 25% … 400% | Sets the speed in the Speed / Duration dialog to the preset percentage. | None |
| `speed-duration-reverse` | Reverse Speed: On / Off | Toggles reverse playback in the Speed / Duration dialog. | None |
| `speed-duration-speed-step` (per row) | -25% / -5% / -1% / +1% / +5% / +25% | Steps the speed in the Speed / Duration dialog by the percentage. | None |
| `timeline-report-close` | Close | Closes the Timeline File dialog and drops its report. | None |
| `unsaved-cancel` | Stay | Cancels the pending close, quit, or project switch and returns to the open project. | None |
| `unsaved-discard` | Discard | Discards unsaved project modifications and proceeds with the pending action. | None |
| `unsaved-save` | Save | Saves the current project and proceeds with the pending close, quit, or project switch. | None |
| `velocity-influence-preset` (per row) | Influence preset | Sets incoming or outgoing Bézier handle influence to a preset percentage. | None |
| `velocity-influence-step` (per row) | Influence step | Steps incoming or outgoing Bézier handle influence down or up by a fixed delta. | None |
| `velocity-speed-step` (per row) | Speed step | Steps incoming or outgoing keyframe velocity down or up by a fixed delta. | None |
| `velocity-speed-zero` (per row) | 0 | Sets incoming or outgoing keyframe velocity to zero. | None |
