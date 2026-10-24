# Panels, Dialogs, and Tabs Reference

Every panel, modal dialog, overlay, Project panel tab, and Home rail view is registered in `crates/aria-ui/src/plugins/ui.rs` and `crates/aria-ui/src/types_*.rs`. The shell queries `PanelHost` and `OverlayHost` rather than importing panel modules directly.

## Worked example

Open the **Scopes** and **Color** panels from the keyboard and maximize the focused frame:

1. Press `Shift+7` (or choose **Window > Scopes**) to open and focus `aria.panel.scopes` in the left dock frame.
2. Choose **Window > Color** to open and focus `aria.panel.color` in the right dock frame.
3. Press `` ` `` to maximize the focused frame across the editor area, and press `` ` `` again to restore the split tree.
4. Press `Ctrl+W` to close the active tab in the focused dock frame (every panel except `aria.panel.timeline` and `aria.panel.viewer` can be closed).

## Built-in panels

| ID | Title | Location | Default frame | Min width | Min height | Focus target |
| --- | --- | --- | --- | --- | --- | --- |
| `aria.panel.media` | Project | `Sidebar` | `Left` | `160` | `80` | `Media` |
| `aria.panel.timeline` | Timeline | `Timeline` | `Bottom` | `400` | `120` | `Timeline` |
| `aria.panel.inspector` | Effect Controls | `Inspector` | `Left` | `160` | `80` | `Inspector` |
| `aria.panel.viewer` | Viewer | `Viewer` | `Center` | `320` | `180` | `Preview` |
| `aria.panel.mixer` | Audio Track Mixer | `Viewer` | `Center` | `160` | `80` | `Mixer` |
| `aria.panel.audio_meters` | Audio Meters | `Inspector` | `Bottom` | `64` | `80` | `Mixer` |
| `aria.panel.jobs` | Jobs | `Inspector` | `Right` | `160` | `80` | `Timeline` |
| `aria.panel.character` | Character | `Inspector` | `Right` | `160` | `80` | `Inspector` |
| `aria.panel.paragraph` | Paragraph | `Inspector` | `Right` | `160` | `80` | `Inspector` |
| `aria.panel.info` | Info | `Inspector` | `Right` | `160` | `80` | `Inspector` |
| `aria.panel.align` | Align | `Inspector` | `Right` | `160` | `80` | `Inspector` |
| `aria.panel.history` | History | `Inspector` | `Right` | `160` | `80` | `Timeline` |
| `aria.panel.markers` | Markers | `Sidebar` | `Left` | `160` | `80` | `Timeline` |
| `aria.panel.scopes` | Scopes | `Inspector` | `Left` | `160` | `80` | `Preview` |
| `aria.panel.color` | Color | `Inspector` | `Right` | `160` | `80` | `Inspector` |
| `aria.panel.effects` | Effects & Presets | `Sidebar` | `Right` | `160` | `80` | `Media` |
| `aria.panel.assistant` | Assistant | `Viewer` | `Center` | `160` | `80` | `Ai` |
| `aria.panel.home` | Home | `Home` | `none` | `none` | `none` | `none` |
| `aria.panel.duck` | Duck | `Inspector` | `none` | `none` | `none` | `none` |
| `aria.panel.ai-tools` | Tools | `Inspector` | `none` | `none` | `none` | `none` |
| `aria.panel.looks` | Looks | `Inspector` | `none` | `none` | `none` | `none` |

Rows with a default frame carry a `DockSpec` and appear as dockable tabs in the workspace tree and on the **Window** menu. Rows without one render on the Home route (`aria.panel.home`) or as conditional cards inside the Inspector column when their visibility predicate holds (`duck_precheck().is_ok()`, `ai_tools::visible`, and `selected_clip().is_some()`).

## Built-in dialogs and overlays

| ID | Trigger | Summary |
| --- | --- | --- |
| `aria.dialog.export` | `Dialog(Export)` | Export settings sheet (`Ctrl+M` or **File > Export > Media…**) for scale, frame rate, codec, bitrate, keyframe interval, audio toggle, range, and destination path. |
| `aria.dialog.settings` | `Dialog(Settings)` | Application and project settings dialog (`Ctrl+,` or **Edit > Preferences…**). |
| `aria.dialog.palette` | `Dialog(Palette)` | Searchable command palette (`Ctrl+Shift+P` or **Window > Command Palette**). |
| `aria.dialog.jobs` | `Dialog(Jobs)` | Modal jobs board (`Jobs` in the command palette) showing active and completed background jobs. |
| `aria.dialog.unsaved` | `Dialog(UnsavedChanges)` | Confirmation prompt (**Save**, **Discard**, **Stay**) shown before New, Open, Close, or Quit replaces a dirty project. |
| `aria.dialog.install-plugin` | `Dialog(InstallPlugin)` | Permission and tier confirmation prompt shown before unpacking an `.ariaplugin` archive. |
| `aria.dialog.project-report` | `Dialog(ProjectReport)` | Project-wide counts (sources, folders, lanes, clips, markers, content end, transitions, soloed lane kinds) and one row per clip question with the number of clips that answer it; clicking a row selects the next such clip (**Window > Project Report**). |
| `aria.dialog.media-support` | `Dialog(MediaSupport)` | Supported containers, codecs, and pixel formats, what the selected clip's source is made of, and the Re-read, Verify decode, Loudness overview, Measure exposure, and Normalise selection actions (**Media Support Report** in the command palette). |
| `aria.dialog.timeline-report` | `Dialog(TimelineReport)` | Each item a timeline import or export placed or wrote the nearest way it could; opens when such a run finishes, and closing it drops the report. |
| `aria.dialog.speed-duration` | `Dialog(SpeedDuration)` | Clip retiming dialog (`Ctrl+R` or **Clip > Speed/Duration…**) for constant speed (`5%` to `10000%`), duration, reverse, and pitch lock. |
| `aria.dialog.audio-gain` | `Dialog(AudioGain)` | Clip gain dialog (`G` or **Clip > Audio Gain…**) for Set Gain, Adjust Gain, Normalize Max Peak, and Normalize All Peaks. |
| `aria.dialog.about` | `Dialog(About)` | Version and build information dialog (**Help > About**). |
| `aria.dialog.remove-attributes` | `Dialog(RemoveAttributes)` | Attribute reset checklist (**Edit > Remove Attributes…**) for Motion, Opacity, Crop, Time Remapping, Volume, Pan, Effects, Audio Effects, Grade, Mask, and Chroma Key. |
| `aria.dialog.keyframe-velocity` | `Dialog(KeyframeVelocity)` | Incoming and outgoing velocity and influence dialog (**Animation > Keyframe Velocity…** or double-clicking a keyframe). |
| `aria.dialog.marker-edit` | `Dialog(MarkerEdit)` | Marker editor (**Markers > Edit Marker…** or pressing `M` on an existing marker) for name, comment, colour, duration, and kind. |
| `aria.overlay.context-menu` | `ContextMenu` | Right-click context menu for clips, transitions, keyframes, tracks, pool items, and the Viewer. |
| `aria.overlay.rename` | `Rename` | Centered inline text bar (`F2`) for renaming clips, tracks, markers, effects, workspaces, presets, or shifting selection by frames. |

## Project panel tabs

`SidebarTab::ALL` (`crates/aria-ui/src/types_sidebar.rs`) defines the fifteen tabs inside the **Project** panel (`aria.panel.media`), grouped under five top-level group chips (`SidebarGroup::ALL`):

| ID | Group | Label | Summary |
| --- | --- | --- | --- |
| `Media` | `Media` | Media | Media pool with search, view modes (`List`, `Icon`, `Freeform`), sort order, bins, and multicam creation. |
| `Audio` | `Media` | Audio | Audio-filtered media pool listing imported audio assets and sound effects. |
| `Titles` | `Graphics` | Titles | Built-in title templates and cloud text preset packs; clicking places a text clip at the playhead. |
| `Shapes` | `Graphics` | Shapes | Seven vector shape generators (`rounded_rect`, `ellipse`, `polygon`, `star`, `line`, `arrow`, `heart`). |
| `Stickers` | `Graphics` | Stickers | Built-in sticker catalog (`docs/reference/stickers-and-masks.md`) and Lottie imports. |
| `Masks` | `Graphics` | Masks | Seven mask shapes (`linear`, `mirror`, `circle`, `rectangle`, `heart`, `star`, `path`) applied to the selected clip. |
| `Effects` | `Effects` | Effects | Unified browser tree for Video Effects, Audio Effects, Transitions, Generators, and Animation Presets. |
| `Transitions` | `Effects` | Transitions | Picture and audio transition catalog applied to the selected cut. |
| `Generators` | `Effects` | Generators | Seven procedural GPU generators (`docs/reference/generators.md`) and plugin clip types. |
| `Animations` | `Effects` | Animations | In, Out, and Loop clip animation catalog (`docs/reference/animations.md`) with hover preview. |
| `Stock` | `Library` | Stock | Video, photo, and audio stock search via Pexels, Pixabay, or the Aria cloud backend. |
| `CloudSfx` | `Library` | SFX | Cloud sound-effect catalog with download caching and pool import. |
| `Generate` | `Library` | Generate | Prompt-driven image, video, and voice generation through `fal` / `elevenlabs`. |
| `Captions` | `Analyze` | Captions | Offline Whisper speech-to-text transcription and caption/marker commit controls. |
| `Analysis` | `Analyze` | Analysis | Offline shot, silence, and beat detectors with moment navigation and split/ripple-delete actions. |

## Home rail items

When `UiRoute::Home` is active (`aria.panel.home`), the left rail offers five views (`HomeRail` in `crates/aria-ui/src/types_editor.rs`):

| ID | Label | Summary |
| --- | --- | --- |
| `New` | New | Project creation card with canvas aspect, resolution, frame rate, and template options. |
| `Open` | Open | Project file picker and recent `.aria` project opener (`Ctrl+O` on Home). |
| `Recents` | Recents | Default start view with quick-start actions (**New Project**, **Open Project**, **Use Template**) and recent projects list. |
| `Samples` | Samples | Cloud template bundles that download and prompt for each replaceable slot. |
| `Learn` | Learn | Interactive keyboard shortcut reference and onboarding guide (**Help > Keyboard Shortcuts**). |

## Effect Controls inspector tabs

`crate::panels::inspector::host_of()` (`crates/aria-ui/src/panels/inspector/tab_host.rs`) defines the eight shipped tabs inside **Effect Controls** (`aria.panel.inspector`), filtered by the selected clip's `UiClipKind` (`UiClipKind::ALL` in `crates/aria-ui/src/types_clip.rs`: `Media`, `Text`, `Shape`, `Generator`, `Audio`):

| ID | Label | Applicable `UiClipKind` variants |
| --- | --- | --- |
| `transform` | Transform | `Media, Text, Shape, Generator` |
| `look` | Look | `Media, Text, Shape, Generator` |
| `effects` | Effects | `Media, Text, Shape, Generator` |
| `speed` | Speed | `Media, Audio` |
| `audio` | Audio | `Media, Audio` |
| `text` | Text | `Text` |
| `shape` | Shape | `Shape` |
| `info` | Info | `Media, Text, Shape, Generator` |

## Media pool views and sort orders

Defined in `crates/aria-ui/src/types_pool.rs` (`PoolView::ALL`, `PoolLayout`, `POOL_CELL_WIDTH = 96.0`) and `crates/aria-ui/src/types_editor.rs` (`MediaSort::ALL`).

| Variant | Serde name | Label | Picture | Facts | Inline | Wraps | Freeform | Cell width | Layout (`PoolLayout`) | Default |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `Tiles` | `tiles` | Tiles | `true` | `true` | `false` | `false` | `false` | `none` | `Stack` | `yes` |
| `List` | `list` | List | `false` | `true` | `true` | `false` | `false` | `none` | `Stack` | `no` |
| `Icons` | `icons` | Icons | `true` | `false` | `false` | `true` | `false` | `96.0` | `Wrap` | `no` |
| `Freeform` | `freeform` | Freeform | `true` | `false` | `false` | `false` | `true` | `96.0` | `Freeform` | `no` |

## Media pool sort orders

| Variant | Label | Default |
| --- | --- | --- |
| `Name` | Name | `yes` |
| `Duration` | Duration | `no` |
| `Resolution` | Resolution | `no` |
| `DateAdded` | Date added | `no` |

## Viewer overlay modes

Defined in `crates/aria-ui/src/types_editor.rs` (`OverlayMode::ALL`).

| Variant | Label | Next (`next()`) | Default |
| --- | --- | --- | --- |
| `Transform` | Transform | `Crop` | `yes` |
| `Crop` | Crop | `Mask` | `no` |
| `Mask` | Mask | `Transform` | `no` |

## Keyboard focus targets

Defined in `crates/aria-ui/src/types_editor.rs` (`ActivePanel::ALL`, `#[serde(rename_all = "camelCase")]`).

| Variant | Serde name | Primary dock panel ID (`panel_id()`) |
| --- | --- | --- |
| `Media` | `media` | `aria.panel.media` |
| `Preview` | `preview` | `aria.panel.viewer` |
| `Timeline` | `timeline` | `aria.panel.timeline` |
| `Inspector` | `inspector` | `aria.panel.inspector` |
| `Ai` | `ai` | `aria.panel.assistant` |
| `Mixer` | `mixer` | `aria.panel.mixer` |

## Assistant transcript entry kinds

Defined in `crates/aria-ui/src/types_editor.rs` (`AgentEntryKind::ALL`).

| Variant | Bubble label (`label()`) | Saved key (`key()`) |
| --- | --- | --- |
| `You` | `you` | `you` |
| `Assistant` | `assistant` | `assistant` |
| `Reasoning` | `thinking` | `reasoning` |
| `Action` | `edit` | `action` |
| `Error` | `error` | `error` |

## Cloud library feeds (`CloudFeed::ALL`)

Defined in `crates/aria-ui/src/cloud.rs` (`CloudFeed::ALL`).

| Variant | Label (`label()`) | Search feed (`is_search()`) | Catalog asset kind (`asset_kind()`) |
| --- | --- | --- | --- |
| `Stock` | `Stock` | `true` | `none` |
| `Templates` | `Templates` | `false` | `Template` |
| `Sfx` | `SFX` | `false` | `Sfx` |
| `Luts` | `LUTs` | `false` | `Lut` |
| `TextPresets` | `Text presets` | `false` | `TextPreset` |
| `Lottie` | `Lottie` | `false` | `Lottie` |
| `Skills` | `Skills` | `false` | `Skill` |

## Generate panel media kinds (`GenerateKind::ALL`)

Defined in `crates/aria-ui/src/app/generate.rs` (`GenerateKind::ALL`).

| Variant | Label (`label()`) | Fallback extension (`fallback_extension()`) |
| --- | --- | --- |
| `Image` | `Image` | `png` |
| `Video` | `Video` | `mp4` |
| `Tts` | `Voice` | `mp3` |

## Analysis panel detector kinds (`AnalysisKind::ALL`)

Defined in `crates/aria-ui/src/app/analysis_kind.rs` (`AnalysisKind::ALL`).

| Variant | ID (`id()`) | Label (`label()`) |
| --- | --- | --- |
| `Shots` | `shots` | `Shots` |
| `Silence` | `silence` | `Silence` |
| `Beats` | `beats` | `Beats` |

## Effects browser entry kinds (`EntryKind::ALL`)

`EntryKind::ALL` (`crates/aria-ui/src/app/effects_browser.rs`) defines the entry kinds listed in the **Project > Effects** browser (`sidebar-tab-effects`):

| Variant | Folder (`group().label()`) | Favourite prefix (`tag()`) | Drag payload (`drag()`) |
| --- | --- | --- | --- |
| `VideoEffect` | `Video Effects` | `video` | `Effect` |
| `AudioEffect` | `Audio Effects` | `audio` | `AudioEffect` |
| `Transition` | `Video Transitions` | `transition` | `Transition` |
| `AudioTransition` | `Audio Transitions` | `audio-transition` | `Transition` |
| `Generator` | `Generators` | `generator` | `Generator` |
| `ClipType` | `Generators` | `clip-type` | `ClipType` |
| `Preset` | `Animation Presets` | `preset` | `Preset` |

## Effects browser folder groups (`Group::ALL`)

`Group::ALL` (`crates/aria-ui/src/app/effects_browser.rs`) defines the top-level folders in the **Project > Effects** tree (`sidebar-tab-effects`):

| Variant | Label (`label()`) | Single-group caption (`only_label()`) |
| --- | --- | --- |
| `Favourites` | `Favourites` | `Favourites only` |
| `VideoEffects` | `Video Effects` | `Video effects only` |
| `AudioEffects` | `Audio Effects` | `Audio effects only` |
| `Transitions` | `Video Transitions` | `Video transitions only` |
| `AudioTransitions` | `Audio Transitions` | `Audio transitions only` |
| `Generators` | `Generators` | `Generators only` |
| `Presets` | `Animation Presets` | `Animation presets only` |
| `Failed` | `Failed to Load` | `Failures only` |
