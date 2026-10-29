# Commands Reference

The Command Palette (`Ctrl+Shift+P` or **Window > Command Palette**) offers every menu bar row by name, plus the commands on this page, which no menu has: the state commands (`BUILTIN_COMMANDS` in `crates/aria-ui/src/commands/mod.rs`) and the background job commands (`BUILTIN_JOB_COMMANDS` in `crates/aria-ui/src/commands/mod.rs`). A menu row in a submenu is offered after its submenu's caption, as in `Export: Media…`. Plugins and tests invoke the commands through `CommandHost` (`crates/aria-ui/src/commands/host.rs`).

## Worked example

Mask a clip, keyframe its opacity, and cut it at the playhead from the Command Palette:

1. Park the playhead inside a clip on `V1` and select the clip.
2. Press `Ctrl+Shift+P`, type `Add Circle Mask`, and press `Enter` (`pal-mask-add-circle`).
3. Press `Ctrl+Shift+P`, type `Keyframe Opacity`, and press `Enter` (`pal-key-opacity`).
4. Press `Ctrl+Shift+P`, type `Add Edit`, and press `Enter`. The palette runs the **Sequence > Add Edit** menu row.

## State commands

| ID | Label | Shortcut | Disabled or refused when | Action |
| --- | --- | --- | --- | --- |
| `pal-play` | Play / Pause | `Space` | Always enabled | Toggles sequence or Source monitor playback (`toggle_playback`). |
| `pal-handles` | Cycle Canvas Handles | `none` | Always enabled | Cycles the Viewer on-canvas handle mode (`Transform`, `Crop`, `Mask`). |
| `pal-bin-new` | New Bin | `none` | Always enabled | Creates a new bin inside the current media pool folder. |
| `pal-bin-up` | Leave Bin | `none` | Already at the root bin | Navigates up one folder level in the media pool. |
| `pal-captions` | Captions | `none` | Always enabled | Switches the Project panel to the `Captions` tab (`SidebarTab::Captions`). |
| `pal-analysis` | Analysis | `none` | Always enabled | Switches the Project panel to the `Analysis` tab (`SidebarTab::Analysis`). |
| `pal-next-moment` | Next Detected Moment | `none` | No detected analysis moments exist | Seeks the playhead to the next detected shot, silence, or beat moment. |
| `pal-previous-moment` | Previous Detected Moment | `none` | No detected analysis moments exist | Seeks the playhead to the previous detected shot, silence, or beat moment. |
| `pal-motion-blur` | Toggle Motion Blur | `none` | No clip is selected | Toggles transform shutter motion blur on the selected clips. |
| `pal-denoise` | Toggle Denoise | `none` | No clip is selected | Toggles audio noise reduction on the selected clips. |
| `pal-pitch-lock` | Toggle Pitch Lock | `none` | No clip is selected | Toggles pitch preservation for retimed audio on the selected clips. |
| `pal-mask-invert` | Invert Mask | `none` | Selected clip has no active mask | Toggles `invert` on the selected clip's active mask. |
| `pal-mask-add-linear` | Add Linear Mask | `none` | No clip is selected | Appends a `linear` split-plane mask to the selected clip's mask stack. |
| `pal-mask-add-mirror` | Add Mirror Mask | `none` | No clip is selected | Appends a `mirror` parallel-band mask to the selected clip's mask stack. |
| `pal-mask-add-circle` | Add Circle Mask | `none` | No clip is selected | Appends a `circle` elliptical mask to the selected clip's mask stack. |
| `pal-mask-add-rectangle` | Add Rectangle Mask | `none` | No clip is selected | Appends a `rectangle` rounded-box mask to the selected clip's mask stack. |
| `pal-mask-add-heart` | Add Heart Mask | `none` | No clip is selected | Appends a `heart` SDF mask to the selected clip's mask stack. |
| `pal-mask-add-star` | Add Star Mask | `none` | No clip is selected | Appends a `star` five-pointed SDF mask to the selected clip's mask stack. |
| `pal-mask-add-path` | Add Path Mask | `none` | No clip is selected | Appends a closed Bézier `path` mask to the selected clip's mask stack. |
| `pal-chroma-green` | Key Green Screen | `none` | No clip is selected | Enables the built-in chroma keyer on the selected clip with target color `[0, 177, 64]`. |
| `pal-slot-toggle` | Mark as Template Slot | `none` | No clip is selected | Toggles whether the selected clip is marked as a replaceable media slot in `.ariat` templates. |
| `pal-text-editable` | Text Editable in Template | `none` | Selected clip is not a title clip | Toggles whether the selected title clip's text is editable when instantiating a template. |
| `pal-key-opacity` | Keyframe Opacity at Playhead | `none` | Playhead does not intersect the selected clip | Inserts an `Opacity` keyframe at the playhead with the clip's current opacity value. |
| `pal-key-position` | Keyframe Position at Playhead | `none` | Playhead does not intersect the selected clip | Inserts a `Position` keyframe at the playhead with the clip's current position. |
| `pal-key-scale` | Keyframe Scale at Playhead | `none` | Playhead does not intersect the selected clip | Inserts a `Scale` keyframe at the playhead with the clip's current scale. |
| `pal-key-remove` | Remove Opacity Keyframe at Playhead | `none` | No opacity keyframe sits at the playhead | Removes the `Opacity` keyframe at the playhead on the selected clip. |
| `pal-shift-selection` | Shift Selection by Frames… | `none` | Always enabled | Opens `aria.overlay.rename` (`RenameTarget::ShiftFrames`) to nudge selected clips by a signed frame count. |
| `pal-track-up` | Move Track Up | `none` | No track is selected or track is already at the top | Moves the selected clip's track one slot higher in the track stack. |
| `pal-track-down` | Move Track Down | `none` | No track is selected or track is already at the bottom | Moves the selected clip's track one slot lower in the track stack. |
| `pal-track-solo` | Solo Track | `none` | No track is selected | Toggles solo status on the selected clip's track. |
| `pal-track-duck` | Toggle Track as Duck Source | `none` | No track is selected | Toggles whether the selected clip's track acts as a dialogue trigger for sidechain audio ducking. |
| `pal-track-open-gap` | Open One-Second Gap at Playhead | `none` | No track is selected | Shifts clips at or after the playhead on the selected track right by one second (`nominal_fps` ticks). |
| `pal-track-close-gap` | Close One-Second Gap at Playhead | `none` | No track is selected | Shifts clips at or after the playhead on the selected track left by one second (`nominal_fps` ticks). |
| `pal-project-clip-under-playhead` | Select Clip Under Playhead | `D` | No unlocked clip intersects the playhead | Selects the topmost unlocked clip intersecting the playhead. |
| `pal-project-next-flagged-clip` | Go to Next Flagged Clip | `none` | No clip has an effect, mask, chroma key, or retiming | Seeks to and selects the next clip with effects, masks, chroma key, or retiming (`aria_models::clip_facts`). |
| `pal-project-duplicate-to-lane` | Duplicate Clip onto Another Lane | `none` | No clip is selected | Copies the selected clip onto the next compatible track at the same timeline tick. |
| `pal-project-reveal-media` | Reveal a File in the Pool | `none` | Always enabled | Prompts for a file path and selects its corresponding item in the media pool. |
| `pal-project-describe-template` | Describe a Template Bundle | `none` | Always enabled | Prompts for a `.ariat` bundle and reports its replaceable media and text slots on the status line. |
| `pal-project-history-pending` | Name the Next Undo and Redo | `none` | Always enabled | Reports the labels of the next undo and redo steps on the status line. |
| `pal-project-content-size` | Report Drawn Size of Selection | `none` | No clip is selected | Reports the post-transform pixel dimensions of the selected clip on the status line. |
| `pal-key-anchor-point` | Keyframe Anchor Point at Playhead | `none` | Playhead does not intersect the selected clip | Inserts an `AnchorPoint` keyframe at the playhead. |
| `pal-key-rotation` | Keyframe Rotation at Playhead | `none` | Playhead does not intersect the selected clip | Inserts a `Rotation` keyframe at the playhead. |
| `pal-key-crop` | Keyframe Crop at Playhead | `none` | Playhead does not intersect the selected clip | Inserts a `Crop` keyframe at the playhead. |
| `pal-key-volume` | Keyframe Volume at Playhead | `none` | Playhead does not intersect an audio-capable selected clip | Inserts a `Volume` keyframe at the playhead. |
| `pal-key-pan` | Keyframe Pan at Playhead | `none` | Playhead does not intersect an audio-capable selected clip | Inserts a `Pan` keyframe at the playhead. |
| `pal-media-filter-clear` | Clear Pool Filter | `none` | Always enabled | Clears the search query string in the Project panel media pool. |
| `pal-font-next` | Next Font on Title | `none` | Selected clip is not a title clip | Cycles the selected title clip's font family forward through available fonts. |
| `pal-font-prev` | Previous Font on Title | `none` | Selected clip is not a title clip | Cycles the selected title clip's font family backward through available fonts. |
| `pal-proxy-clear-all` | Decode Everything from Originals | `none` | Always enabled | Detaches generated proxy files from all media pool items so decoding uses original files. |
| `pal-proxy-create-all` | Proxy Every Pool Item | `none` | Media pool has no video items | Queues proxy transcode jobs for every video item in the media pool. |
| `pal-ai-tools` | Assistant: What the Assistant Can Do | `none` | Always enabled | Opens the Assistant panel (`ActivePanel::Ai`) and displays the assistant tool catalog (`aria.panel.ai-tools`). |
| `pal-ai-system-prompt` | Assistant: Show the System Prompt | `none` | Always enabled | Displays the active AI assistant system prompt in the status/transcript view. |
| `pal-ai-slash` | Assistant: List the Slash Commands | `none` | Always enabled | Lists available `/command` templates from `<agent_dir>/commands/` on the status line. |
| `pal-ai-models` | AI: Choose the Model | `none` | Always enabled | Opens `aria.dialog.settings` filtered to the `AI` section. |
| `pal-ai-connection` | AI: Test the Connection | `none` | Always enabled | Opens `aria.dialog.settings` filtered to the `AI` section. |
| `pal-ai-schema` | Configuration: What This Build Reads | `none` | Always enabled | Opens `aria.dialog.settings` filtered to `Configuration` and reports schema version status. |
| `pal-cloud-checksum` | Cloud: Checksum a Downloaded File | `none` | Always enabled | Prompts for a file and verifies its SHA-256 digest against the download cache. |
| `pal-media-support` | Media Support Report | `none` | Always enabled | Opens the Media Support dialog (`aria.dialog.media-support`), which lists supported containers, codecs, and pixel formats and what the selected clip's source is made of. |
| `pal-media-verify` | Verify Decode of Selected Source | `none` | No media source is selected | Starts a background job that decodes every frame of the selected source to verify stream integrity. |
| `pal-media-loudness` | Loudness Overview of Selected Source | `none` | Selected source has no audio | Starts a background job that samples peak levels across 200 buckets of the selected clip's source audio file and reports the loudest peak in dBFS, its position in the file, and how many buckets reach `-6 dBFS`. |
| `pal-media-exposure` | Measure Exposure of the Frame on Screen | `none` | No frame is currently rendered | Reports luma min, max, mean, and clipping percentages for the current Viewer frame on the status line. |
| `pal-media-normalise` | Normalize Selected Clip to Target | `none` | Selected clip has no audio | Adjusts the selected clip's volume so the peak of its measured Analysis loudness contour matches the active dBFS target level. |
| `pal-media-target` | Cycle Normalize Target Level | `none` | Always enabled | Cycles the audio normalization target peak level (`-23 dBFS`, `-18 dBFS`, `-14 dBFS`, `-9 dBFS`, `-6 dBFS`). |
| `pal-media-import-still` | Import Still or Animation… | `none` | Always enabled | Opens a file picker filtered to still images and animated image sequences and imports them into the pool. |
| `pal-media-lottie` | Import Lottie Animation… | `none` | Always enabled | Opens a file picker for `.json` and `.lottie` files, validates the first frame, and places a Lottie generator clip on the timeline at its intrinsic dimensions. |
| `pal-mixer-add-bus` | Mixer: Add a Submix Bus | `none` | Always enabled | Appends a submix bus lane to the sequence and focuses the Audio Track Mixer. |
| `pal-look-install` | Install Starter LUT Pack | `none` | Always enabled | Writes the shipped starter `.cube` LUT pack into the LUT cache (`<data dir>/luts`). |
| `pal-render-cost` | Last Frame Cost | `none` | Always enabled | Reports the Info panel's Last frame rows (`aria_engine::frame_stages`, the whole frame, and the part that scales with size) on the status line, or that no frame has been rendered yet. |
| `pal-media-reload` | Reload Media from Disk | `none` | No media item is selected | Invalidates decode caches and reloads the selected media item from disk. |
| `pal-media-reload-all` | Reload All Media from Disk | `none` | Always enabled | Invalidates decode caches and reloads every media pool item from disk. |
| `pal-font-check` | Text: Check a Font File | `none` | Always enabled | Prompts for a `.ttf`/`.otf` font file, parses it with `fontdb`, and reports how many font faces it contains. |
| `pal-title-ink` | Text: Report the Selected Title's Ink | `none` | Selected clip is not a title clip | Rasterizes the selected title clip in isolation and reports its non-transparent pixel coverage. |
| `pal-models-verify` | Transcription: Verify Installed Models | `none` | Model store cannot be opened | Starts a background job that re-hashes every installed Whisper model file against its expected SHA-256 digest. |

## Job commands

| ID | Label | Shortcut | Disabled or refused when | Action |
| --- | --- | --- | --- | --- |
| `pal-detect-shots` | Detect Shots | `Shift+Alt+C` | Selected clip or sequence has no video source | Runs the shot-boundary detector (`AnalysisKind::Shots`) and records cut moments in the Analysis tab. |
| `pal-detect-silence` | Detect Silence | `Shift+Alt+Q` | Selected clip or sequence has no audio | Runs the silence detector (`AnalysisKind::Silence`) and records silent spans in the Analysis tab. |
| `pal-detect-beats` | Detect Beats | `Shift+Alt+B` | Selected clip or sequence has no audio | Runs the beat-onset detector (`AnalysisKind::Beats`) and records beat moments in the Analysis tab. |
| `pal-loudness` | Measure Loudness | `none` | Sequence has no audio clips | Runs the EBU R128 loudness measurement job across the sequence and reports LUFS and true peak. |
| `pal-transcribe` | Transcribe Selected Clip | `none` | No audio-capable clip is selected or Whisper model is missing | Runs offline Whisper transcription on the selected clip and populates the Captions tab. |
| `pal-duck` | Duck Under Voice | `Shift+Alt+D` | Duck precheck fails (no dialogue/music track pair) | Analyzes dialogue activity on duck-source tracks and writes volume ducking keyframes on music tracks. |
| `pal-ai-list-models` | AI: Ask the Endpoint Which Models It Serves | `none` | AI provider endpoint is unreachable or unconfigured | Queries the configured AI provider's models endpoint and populates the model picker in Settings. |
| `pal-ai-test-connection` | AI: Test the Provider Connection Now | `none` | AI provider key or endpoint is missing | Sends a minimal health request to the configured AI provider and reports the result on the status line. |
| `pal-track-mask` | Track Mask Forward from the Playhead | `none` | Selected clip has no active mask or video source | Tracks the active mask shape forward frame by frame from the playhead and writes mask path keyframes. |
