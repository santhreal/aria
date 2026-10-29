# Status

The current build compared with each chapter. A row is removed when the
chapter's behavior ships with its tests; this page is updated in the same
change.

## Plugins ([plugins.md](plugins.md))

| Area | Current build |
| --- | --- |
| Host | `aria_plugin::PluginContext`: a typed service registry (one value per type) and an event bus. A built-in's manifest borrows `'static` strings; a package's manifest holds owned copies (`PluginManifest` fields are `Cow<'static, str>`). |
| Effects and transitions | A plugin registers an effect or transition as a catalog spec plus a WGSL effect graph or a `CpuEffect` callback (`PluginContext::register_effect`, `register_cpu_effect`, `register_transition`, `register_texture`; WASM `register-effect`, `register-effect-graph`, `register-cpu-effect`, `register-transition`, `register-generator`, `register-texture`, `set-effect-category`, `cpu-effect-process`; C ABI `register_effect`, `register_cpu_effect`, `register_transition`, `register_generator`, `register_texture`, `cpu_effect_process`; package `passes` tables and `[[textures]]`; `crates/aria-compositor/src/registered_passes.rs`, `crates/aria-compositor/src/registered_graph.rs`, `crates/aria-compositor/src/plugin_textures.rs`, `crates/aria-compositor/src/cpu_effect.rs`). Effects, generators, and transitions declare parameters across all 15 kinds (`scalar`, `int`, `bool`, `choice`, `color`, `vec2`, `point3`, `size`, `angle`, `curve`, `gradient`, `text`, `font`, `file`, `layer`), with `soft_min`/`soft_max`, `unit`, `display` (`slider`, `angle_dial`, `percentage`), `tooltip`, `visible_when` conditions, and collapsible `groups` (`EffectGroupSpec`). An effect graph is up to 8 named compute or fragment passes, each reading the source frame, earlier passes, the clip a layer parameter references, or plugin textures, and writing the input size, a scaled size, or a fixed size; `aria_models::plan_graph` rejects cycles, unknown inputs, and passes the last pass does not read, naming the pass. Pipelines build once per registration (`Compositor::registered_pipeline_builds`). `EffectSpec::bounds` widens the viewer pick of a layer, and `time_dependent` hands a pass `aria.time`; a pass of any other effect reads time 0. `reads_outside` is recorded in the frame key and changes no sampling: every pass reads its whole canvas-sized input, clamped to the edge, and pixels of a layer outside the canvas are not available to its effects. Compute passes write `rgba8unorm` storage textures, and a device without compute (GLES) leaves a compute effect's input unchanged. WGSL effects and transitions read unbounded `params: array<f32>` storage buffers (`@group(0) @binding(3) var<storage, read> aria: AriaPass` / `AriaTransition`) with generated `param_<name>()` accessors, and CPU effects across in-process Rust, C ABI (`AriaCpuEffectCall` `u16` buffers and `params_json`, `aria_sdk::f16`), and WASM (`video-frame.pixels: list<u16>`) receive `RGBA16F` `CpuFrameIn`/`CpuFrameOut` views with `EffectParams` using reused staging buffers and are not cached. A scene with a `stateful` effect has no render-file key and is not cached. `EffectService::render_effect` and `TransitionService::render_transition` render on the device registered by `aria.render` (`crates/aria-ui/src/plugins/gpu.rs`). |
| Frames | `aria_plugin::RenderHost` renders the open sequence, the root or a nested sequence, or one clip, at a time and a size, as a straight-alpha image, one image per tick of a range, or into an `Rgba8Unorm` texture with no CPU copy (`crates/aria-plugin/src/render_host.rs`). Frames render one at a time on a renderer separate from the preview; no pipelined range render, and no render of a time range to a file. |
| Generators | A plugin generator is a registered WGSL or CPU effect whose spec sets `generator: true`. It is placed as a transparent solid clip with the effect first in its chain (`crates/aria-ui/src/app/media_import.rs`), and the instance saves `generator: true`. A clip whose generator is missing, failed, or disabled draws a slate stating `Missing generator` and the effect id (`crates/aria-render/src/resolve/clip.rs`); an unknown effect passes its input through, and an unknown transition plays as a cut (`crates/aria-render/src/resolve.rs`). The clip keeps every id, parameter, and keyframe, and the timeline draws `Missing plugin: <id>` on it for the first unknown clip type, effect, audio effect, or transition (`Clip::missing_plugin_id`, `crates/aria-ui/src/projection_clip.rs`). |
| Events | Every event in the chapter's table is dispatched (`docs/plugins/events.md`). `CommandApplied` holds the command, its `ApplyOutcome`, the engine revision, and the origin, with no undo step and no list of changed ids. `ResolveDropped` (`Dropped::Path` and `Dropped::Url`) runs handlers ordered by priority (higher first) then mount order, returning `DropAction` (including `DropAction::Handled`); an unclaimed dropped URL sets the status strip to `No plugin accepts <url>`. |
| Project access | `aria_plugin::ProjectHost` and `PluginContext`: immutable snapshots, single commands, named transactions that return each command's outcome, nested groups with savepoints, undo, and queries by id, time range, tick, selection, markers, and In/Out (`clip`, `clips_in_range`, `clips_at`, `selected_clips`, `markers_in_range`, `in_out` in `crates/aria-plugin/src/queries.rs`), exposed across in-process Rust, C ABI (`AriaHostV1` / `aria_sdk::Host`), and WASM (`aria:plugin/plugin@1.0.0`) under `project` read permission. The selection, playhead, and In/Out come from the `EditorView` service as of the last drawn frame. |
| UI contributions | A package of either code tier contributes, without linking `aria-ui`, palette commands, menu rows, default keys, and a settings section (`[[commands]]`, `[[menus]]`, `[[keys]]`, `[[settings]]` in `plugin.toml`, or `PluginContext::register_command`, `register_menu_entry`, `register_key_default`, `register_settings`), and docked panels drawn from a declarative `UiNode` tree (`crates/aria-plugin/src/ui_tree.rs`, `crates/aria-ui/src/app/plugin_panels.rs`). Menu rows reach the menu bar and the clip, track, transition, media, and Viewer context menus; no panel, keyframe, or marker menus exist. A command has an optional SVG icon, drawn in the palette, the menu bar, and context menus, and an optional enablement condition over the editor's selection, marks, playback, and focused panel (`crates/aria-plugin/src/command_icon.rs`, `crates/aria-plugin/src/enablement.rs`); a disabled command is dimmed in the palette, refused with its condition, and left out of menus. Viewer overlays draw lines, rectangles, ellipses, paths, text, and draggable handles over the sequence frame through `PluginContext::register_viewer_overlay`, `aria_sdk::Host::register_viewer_overlay`, and WIT `register-viewer-overlay` (`crates/aria-plugin/src/viewer_overlay.rs`, `crates/aria-ui/src/app/plugin_overlays.rs`). |
| Importers and exporters | `PluginContext::register_importer` and `register_exporter` (`crates/aria-plugin/src/interchange.rs`). An import is one transaction; an export writes a partial file and renames it over the destination. `aria.interchange` (`crates/aria-interchange`) reads and writes CMX 3600 EDL, Final Cut Pro XML, OpenTimelineIO, SubRip, and WebVTT, reached from File > Import Timeline, File > Export Timeline, File > Import, and a drop. A subtitle file becomes text clips on new text tracks, and the shown text tracks export as cues. `aria.effects` reads a `.cube` as a LUT filter bar across the sequence; a `.cube` dropped with a clip selected grades that clip. `PluginContext::register_encoder` (`crates/aria-plugin/src/interchange.rs`), `aria_sdk::Host::register_encoder`, and WIT `register-encoder` register media output formats for the Export dialog; an export writes to a sibling staging file (`TransactionalFile`) and atomically replaces the destination on finish, and a panic or trap in `open`, `push`, `push_audio`, or `finish` fails the plugin for the session. Animation presets (`.ariapreset`) are saved to and read from `<data dir>/presets/*.ariapreset`, contributed by plugins via `PluginContext::register_preset` or `presets/*.ariapreset` in a package (`crates/aria-plugin/src/presets.rs`), and imported from a drop or File > Import (`crates/aria-ui/src/app/presets.rs`). |
| Decoders | `PluginContext::register_decoder` (`crates/aria-plugin/src/decoders.rs`) adds a decoder to the process-wide list in `crates/aria-decoder/src/external.rs`; probe, import, preview, export, and analysis try the decoders that claim a file's extension by priority before the platform decoder, and the import dialogs offer their extensions. Frames are CPU NV12 or I420 planes. Native Rust plugins register an `ExternalDecoder`; C ABI plugins register through `register_decoder` and the `decoder_*` table entries (`crates/aria-plugin/src/external/decoders.rs`); WASM plugins through `register-decoder` and the `decoder-*` exports (`crates/aria-plugin-wasm/src/decoders.rs`). `ForeignDecoder` (`crates/aria-plugin/src/foreign_decoder.rs`) adapts the C ABI and WASM calls to `ExternalDecoder`. The example WASM plugin reads YUV4MPEG2. |
| Jobs | `PluginContext::start_job` and `start_stepped_job` (`crates/aria-plugin/src/jobs.rs`) list a plugin's background work on the Jobs board as `<plugin name>: <label>`, with progress, a detail line, Cancel, and a commit step after which Cancel is refused. A WASM plugin starts one with `start-job` and runs it in `job-step` and `job-end`; a native ABI plugin with `Host::start_job`, `Plugin::job_step`, and `Plugin::job_end` (`docs/plugins/jobs.md`). Unmounting or failing a plugin cancels its jobs, and a panic in a job fails the plugin. A plugin job's result is its last detail line; it attaches no output files. |
| AI tools | `PluginContext::register_ai_tool` (`crates/aria-plugin/src/ai_tools.rs`) offers the assistant a tool named `plugin_<name>` with a JSON Schema, a tier, and a handler that returns text and PNG images (`docs/plugins/ai-tools.md`). The desktop host lists the tools of every mounted plugin, authorizes each call at the tier the plugin registers when the call runs, prompts before a System call under Autonomy Ask, and runs the handler on the turn thread, where an edit joins the turn's undo step (`crates/aria-ui/src/app/assistant_host.rs`). WASM plugins register with `register-ai-tool` and answer in `ai-tool-call`; native plugins with `register_ai_tool` and `ai_tool_call`. A handler cannot stream progress or prompt for input of its own. |
| Scopes | `PluginContext::register_scope` (`crates/aria-plugin/src/scopes.rs`) adds a scope that measures the Viewer frame on the `aria-preview` thread and draws an RGBA image of up to 512 by 512 pixels and one readout line below the built-in scopes (`crates/aria-ui/src/app/scope_view.rs`, `docs/plugins/scopes.md`). WASM plugins register with `register-scope` and measure in `scope-measure`; native plugins with `register_scope` and `scope_measure`. A scope reads the frame at the size the Viewer rendered it and has no audio input. |
| Analyzers | `PluginContext::register_analyzer` (`crates/aria-plugin/src/analyzers.rs`) adds an audio analyzer with clip, sequence, or In-to-Out scope to Clip > Analyze or Sequence > Analyze and the palette. It runs as a job over `AudioHost::mix` at 48 kHz with progress and Cancel, and its beats, markers, and ranges apply as one undo step named after the analyzer; loudness is reported in the job result (`docs/plugins/analyzers.md`). The shipped beat, silence, and loudness detectors are registered the same way. WASM plugins register with `register-analyzer` and run in `analyzer-open`, `analyzer-push`, `analyzer-finish`, and `analyzer-drop`; native plugins with `Host::register_analyzer`. An analyzer has no parameters and no preview of its findings before they apply. |
| Clip types | `PluginContext::register_clip_type`, WASM `register-clip-type`, C ABI `register_clip_type`, and `[[clip_types]]` in `plugin.toml` (`crates/aria-plugin/src/clip_types.rs`, `crates/aria-models/src/clip_types.rs`) list a named kind of clip drawn by a generator, with a timeline color, an icon, and up to 8 Viewer handles bound to the generator's vec2 parameters (`docs/plugins/clip-types.md`). The Generators tab and the palette place a type as a transparent solid with the generator first in its chain and the type's id on the clip; the inspector shows the type with Detach, and a handle drag writes one keyframe at release (`crates/aria-ui/src/app/canvas_overlay_type_handles.rs`). A type's data is its generator's effect parameters. `PluginContext::define_clip_type`, WASM `define-clip-type`, the C ABI `schema` object of `register_clip_type`, `aria-sdk` `Host::define_clip_type` and `define_cpu_clip_type`, and a `[[clip_types]]` entry with `shader` or `passes` define a type with its own fields, of any effect parameter kind, and its own renderer, WGSL passes or a CPU callback handed the field values, the time, and an `RGBA16F` frame of the clip's size; the host registers a generator under the type's id that the Generators tab, the Effects panel, and the palette list through the type only (`crates/aria-ui/src/catalog_text.rs`). Its fields are rows in the inspector and the timeline's Effects group, saved with the clip, edited as one undo step, and keyframed (`crates/aria-ui/tests/registered_catalog.rs`). The shipped text, shape, adjustment, and multicam clips are not clip types. |
| External plugins | Packages under `<data dir>/plugins` (`crates/aria-plugin/src/package/`) in three tiers: data (declared WGSL effects, transitions, and generators), WASM components run by `wasmtime` and type-checked against `aria:plugin/plugin@1.0.0` at load (`crates/aria-plugin-wasm`), and native libraries through the stable C ABI (`crates/aria-plugin-abi`, `API_MAJOR = 1`), written with `aria-sdk`. Both WASM components and native C ABI plugins register commands, events, importers, exporters, encoders, decoders, audio effects, analyzers, WGSL and CPU visual effects, generators, transitions, clip types, driver languages, presets, themes, workspaces, panels, viewer overlays, AI tools, scopes, and jobs. A WASM plugin reaches its storage and settings, the project under `project`, and files under `fs`; its world has no network, process, clipboard, media, or GPU import. The native API checks `project`. A native library unloads once its last registration drops. An `.ariaplugin` installs from a drop, File > Install Plugin, or the Plugins section after a prompt with its tier and permissions. A WASM or data package reloads when its files change; the package named by `ARIA_PLUGIN_DEV` (set by `aria plugin dev`) loads from its source folder over an installed one and reloads at any tier. A code package with activation events loads on the first of them (`crates/aria-plugin/src/activation.rs`); until then its declared effects, commands, menu rows, keys, settings, and presets are listed, its declared panels show an Open card, and its Plugins row reads `waits for` with the events. A package with no events, `startup`, no code, a requirement of one that loads at start, or the development package loads at start. WASM fuel and memory limits come from Settings > Plugins (`[plugin_limits]`), and a change remounts mounted WASM packages. Every `plugin.toml` key and data-tier file field is documented in `docs/plugins/packages.md`, `presets.md`, `workspaces.md`, `effects.md`, and `docs/theme-tokens.md`, every native ABI item in `docs/plugins/native-abi-reference.md` and `aria_plugin.h`, both generated from `crates/aria-plugin-abi/src/lib.rs`, and every WIT item in `docs/plugins/wit-reference.md`; `crates/aria-plugin/tests/manifest_reference/`, `crates/aria-plugin-abi/tests/abi_reference/`, and `crates/aria-plugin-wasm/tests/wit_reference.rs` fail when one has no entry. |
| Faults and cost | A native panic is caught at every host entry and a WASM trap by `wasmtime`; the plugin is unmounted and listed in `PluginContext::failures()`. Effect, transition, and generator callbacks are timed against the sequence frame period, and a call over it is reported as `EffectFaultKind::MissedDeadline` with the measured time while the plugin stays mounted. An audio processor that misses three consecutive block deadlines in playback is bypassed and reported (`crates/aria-render/src/export_audio_inserts.rs`). Mount, per-frame, and per-audio-block cost per plugin is kept in `PluginContext` (`crates/aria-plugin/src/cost.rs`) and shown in Settings > Plugins. Each plugin's newest 256 log lines (`crates/aria-plugin/src/plugin_log.rs`) are kept for the session and shown under the row's Log toggle; a failure, a fault, and a caught panic are logged, a panic with the backtrace from its frame. |

## Layout ([layout.md](layout.md))

Recursive split and tab-group tree (`DockTree` in `crates/aria-ui/src/dock/`),
with 24 px frame tab bars, frames 6 px apart and 6 px in from the bars and
window edges (`DOCK_GUTTER` in `crates/aria-ui/src/dock/mod.rs`),
draggable splitters drawn only under the pointer, tab reordering, five-zone
docking (`Center`, `Left`, `Right`, `Top`, `Bottom`), floating panel windows,
frame maximize (`key-pane-maximize`, `` ` ``), focused-frame accent border, and
a Viewer-over-Timeline fallback below `1024x640` (`MIN_WINDOW_WIDTH` and
`MIN_WINDOW_HEIGHT` in `crates/aria-ui/src/settings.rs`). Five workspaces ship (`Editing`,
`Animation`, `Color`, `Audio`, `Effects` in `crates/aria-ui/src/layouts.rs`),
with per-workspace remembered dock trees, `Window > Reset to Saved Layout`,
named workspace files under `<data dir>/workspaces/<name>.json`, JSON import and
export, and optional serialized `tree` definitions on plugin `WorkspacePanes`.
Project, Viewer, Timeline, Effect Controls, Audio Track Mixer, Assistant,
Scopes, Color, Info, Effects & Presets, Align, Character, Paragraph, Audio
Meters, Jobs, History, and Markers are dock panels (`BUILTIN_PANELS` in
`crates/aria-ui/src/plugins/ui.rs`). The Editing workspace opens with six of them
(`DockTree::editing_for_width`). The Window menu holds
one row per registered dock panel, plugin panels included, checked while the
panel is open. The tool bar draws one button per `ToolGroup`, showing the
group's armed tool or its first tool, and none of the Timeline header's
toggles (`crates/aria-ui/src/tool_catalog.rs`,
`crates/aria-ui/src/chrome/tool_strip.rs`). Closing a floating window, from
Close Window or from the window manager, closes its panels and takes its frame
out of the tree (`AppState::close_floating_frame` in
`crates/aria-ui/src/app/layouts.rs`). Opening a panel whose default frame has
closed re-creates that frame at its shipped edge, and Audio Meters opens right
of the Timeline (`crates/aria-ui/src/dock/restore.rs`). The status bar's right side draws the
frame rate, playback resolution, and GPU only during playback, and the
dropped-frame count only after a drop (`crates/aria-ui/src/shell/strip.rs`).

## Look ([look.md](look.md))

Dark and Light themes from one token set (`docs/theme-tokens.md`), System
mode, four accent families (Blue, Ember, Teal, Graphite in
`crates/aria-ui/src/theme_accent.rs`), UI text in the embedded Inter, with
tabular figures on readouts only (`crates/aria-ui/src/theme_type.rs`), four motion levels, and
theme files read from
`<data dir>/themes/` and from plugins (`crates/aria-ui/src/theme_file.rs`,
`crates/aria-plugin/src/themes.rs`). The dark surfaces, text, `surface_header`,
`hairline`, and `value` tokens take the values in the Dark surfaces table
(`crates/aria-ui/src/theme.rs`). Clips have one of the sixteen label colors
or the default for their kind (`crates/aria-models/src/label_color.rs`). Every
inspector and property-row number is a scrubbable value
(`crates/aria-ui/src/app/value_scrub.rs`); the drag locks the pointer, which
hides and holds its place, so travel never stops at a screen edge. Text sizes
come from the type scale on `Theme` (UI text 11 px, titles 12 px, timecode
18 px), and list, menu, and property rows are 22 px
(`crates/aria-ui/src/theme.rs`). A menu row lit from the keyboard draws a 1 px
accent focus ring (`crates/aria-ui/src/chrome/menu_view.rs`); an open value
field or text box draws an accent border. Motion runs on `gpui::motion` with the
timings in the Motion rules: lane heights and twirl-downs chase their target
over 180 ms `ease_out`, workspace changes morph the frames over 180 ms
`ease_out`, and during playback every hover fade and glide draws its end state
(`crates/aria-ui/src/motion.rs`, `crates/aria-ui/src/motion_glide.rs`). Tab
underlines and value hover underlines draw over their box and take no layout
(`Theme::TAB_UNDERLINE` in `crates/aria-ui/src/theme.rs`,
`crates/aria-ui/src/panels/value_readout.rs`), and the alignment rules in
Density are checked on every painted quad and text run of the shell in every
workspace at `1280x800`, `1600x976` and `1920x1080`
(`crates/aria-ui/src/shell/spacing_tests.rs`).

## Viewer ([viewer.md](viewer.md))

Program and Source tabs (`crates/aria-ui/src/app/viewer_tabs.rs`), a split with
the Source tab shown last beside Program, safe-margin guides, scopes, and rulers
with guides and guide snapping (`crates/aria-ui/src/app/viewer_guides.rs`). A
double-clicked timeline clip opens in a Source tab whose In and Out trim the clip
(`crates/aria-ui/src/app/viewer_match.rs`). View > Compare holds the frame at
the playhead, a still, or the Source monitor's frame beside Program as a
vertical wipe, a horizontal wipe, or side by side, with a draggable divider
(`crates/aria-ui/src/app/viewer_compare.rs`). The Pen tool draws a path mask
vertex by vertex and reshapes one with vertex and tangent handles, inserting,
deleting, and converting vertices, each gesture one `SetClipMask` edit, and
closing a new path one `AddClipMask` edit (`crates/aria-ui/src/app/mask_path.rs`,
`crates/aria-ui/src/panels/preview/mask_path.rs`); the renderer cuts what any
visual clip draws along it, generated clips and adjustment layers included,
before the clip's effects, LUT, and styles, and alone or as a side of a
transition or matte (`aria_shapes::PathRaster::rasterize_mask`,
`crates/aria-compositor/src/compositor/stencil.rs`). A clip holds a stack of up
to 32 masks, each with a mode, an opacity, an expansion, and an invert toggle,
combined top to bottom in one GPU pass
(`crates/aria-models/src/look/mask.rs`,
`crates/aria-compositor/shaders/mask_stack.wgsl`); the timeline outline and the
Look tab hold one row group per mask (`mask-<index>`), and the active mask is
the one the Viewer handles, the Pen, and mask tracking edit
(`crates/aria-ui/src/app/clip_mask.rs`). A path is a curve of whole-path
keyframes (`crates/aria-models/src/look/mask_path.rs`): the Viewer edits it at
the playhead, a vertex insert or delete reaches every keyframe, and each path
mask's timeline group starts with a Path row whose stopwatch, navigator,
keyframe drag, and keyframe menu each send one `SetClipMask` edit
(`crates/aria-ui/src/app/mask_path_keys.rs`).

## Timeline ([timeline.md](timeline.md))

Video and audio lanes sit in separate groups above (`V1..Vn` upward) and below
(`A1..An` downward) a draggable divider (`crates/aria-ui/src/panels/timeline/`),
each group painted in a single virtualized canvas element culled to the visible
lanes and time range. A 49 px header over the outline column
(`crates/aria-ui/src/panels/timeline/header.rs`) holds the sequence timecode,
scrubbable and editable; the property
search, which filters the property rows of every opened clip to the names that
match and opens the groups holding them
(`crates/aria-ui/src/app/property_rows_search.rs`); and the snapping, linked
selection, add marker, display settings, and Graph Editor buttons. Ruler ticks
adapt to zoom so labels never overlap, the labels read in the sequence's time
display, and a press or drag in the ruler draws the playhead under the pointer
without easing. The work area bar under the ruler spans In to Out: dragging it
moves both points, dragging an end moves one, Escape puts the range back, and a
double-click sets it to the sequence (`crates/aria-ui/src/app/work_area.rs`).
The zoom scroll bar along the bottom of the timeline scrolls from its thumb and
zooms from the round knob at either end, the knob under the pointer and, as
Settings > Timeline > Zoom bar knobs sets, the playhead's pixel or the other
end held; it keeps the pressed point under the pointer, follows a drag past the
bar, puts the view back on Escape, and zooms about the playhead under a wheel
turned over it (`crates/aria-ui/src/app/zoom_bar.rs`,
`crates/aria-ui/src/panels/timeline/overview.rs`). A clip's twirl control or the
`Alt` reveal keys (`Alt+P`, `Alt+S`, `Alt+R`, `Alt+T`, `Alt+A`, `Alt+E`,
`Alt+M`, `Alt+L`, `Alt+U`, ``Alt+` ``, and `Shift+` additive variants) open
collapsible property rows (`Transform`, `Effects`, `Masks`, `Audio`) with
stopwatch and driver toggles, scrubbable values, `<` / `◆` / `>` keyframe
stepping, virtualized keyframe icons (Linear diamond, Bezier hourglass, Hold
square, Roving circle), keyframe marquee selection, snapping time drag, copy,
paste, delete, and the Timeline Graph Editor (`Shift+F3`, Value and Speed
graphs with incoming and outgoing influence handles;
`crates/aria-ui/src/app/property_rows.rs`,
`crates/aria-ui/src/panels/timeline/property_rows.rs`). A `3px` render bar along the bottom of the ruler (`crates/aria-ui/src/panels/timeline/render_bar.rs`, `crates/aria-ui/src/app/render_files.rs`) colors frame runs green when stored in `RenderFiles`, red when unrendered and holding a CPU or stateful effect, a nested sequence, motion blur, or more than four picture layers, and yellow otherwise. Track heights clamp to `20..=200` px with four presets (`Mini` 20 px, `Small` 32 px, `Medium` 48 px, `Large` 72 px) in the track context menu and on `Shift+scroll` over a track header (`crates/aria-models/src/track.rs`, `crates/aria-ui/src/app/lanes.rs`, `crates/aria-ui/src/panels/timeline/headers.rs`). Transitions support `Center`, `Start`, and `End` alignment (`TransitionAlignment`, `SetTransitionAlignment` in `crates/aria-models/src/transition.rs`, `crates/aria-commands/src/command.rs`, `crates/aria-ui/src/app/clip_transitions.rs`) from the inspector Transition block and the transition context menu (`Center at Cut`, `Start at Cut`, `End at Cut`), shifting the timeline band, video blend window, and audio crossfade span accordingly. Clip bars draw a 14 px name strip with the speed label, the drift offset, and the fx badge (gray, purple for intrinsic properties, yellow for applied effects, yellow over a purple underline for both), a red Media Offline fill, a generator's name and swatch, and the property line chosen from the badge menu (`Clip::line_param`, `SetClipLineParam`), which a drag sets and Ctrl+click keys, with keyframe dots on lanes 48 px and taller (`crates/aria-ui/src/panels/timeline/clip/bar.rs`, `crates/aria-ui/src/app/clip_line.rs`, `crates/aria-ui/src/surfaces/context_menu/badge.rs`).

## Editing ([editing.md](editing.md))

The command set in `crates/aria-commands/src/edit_command.rs` covers moves, trims,
ripple, roll, slip, slide, rate, split, lift, extract, insert, overwrite,
nesting, multicam, sync lock, clip enable/disable (`SetClipsEnabled`), clip
groups (`GroupClips`, `UngroupClips`), sequence and clip markers with range durations and kinds (`Comment`, `Chapter`, `Segmentation`), and keyframes, with linked
video/audio companion sync, Track Select Forward/Backward (`A` / `Shift+A`),
edge selection with keyboard trim (`Ctrl+Left` / `Ctrl+Right`), gap selection
with ripple delete, on-screen trim feedback, source patching (`SetSourcePatch`)
with V and A indicators in the lane headers, and three- and four-point Source
edits and media drops (`SourceEdit`) that place picture and sound on their
patched lanes, linked, with Fit to Fill retiming both. The shipped keys follow
[keyboard.md](keyboard.md). Settings > Keys rebinds any of them, lists them by
name or chord under the Settings filter, draws them on a keyboard image colored
by command kind with a row dropped on a key binding it there, asks before taking
a chord another binding holds, and saves, loads, imports and exports named
keymaps (`crates/aria-ui/src/keys/keymap_sets.rs`). Digits and the keypad `+` and `-` open the playhead timecode over
the Timeline and the Program view, and a signed value moves the selected clips,
trims the selected edge, or moves the playhead. A trim, ripple trim, roll,
slide or rate stretch, by drag or by key, is held inside the room the model
allows it (`Project::trim_reach`, `crates/aria-ui/src/app/drag_room.rs`): it
stops at the end of the take, the clip beside the edge, the sequence start or
one frame and lands there.

## Animation ([animation.md](animation.md))

Keyframes support Linear, EaseIn, EaseOut, EaseInOut, Hold, CSS cubic bezier,
and per-keyframe incoming and outgoing temporal speed and influence
(`TemporalEase`, `0.1%..=100%`, `SetKeyframeEase`; `F9` Easy Ease, `Shift+F9`
Easy Ease In, `Ctrl+Shift+F9` Easy Ease Out, `Ctrl+Alt+H` Toggle Hold;
`crates/aria-models/src/param/easing.rs`), 2D spatial Bezier tangents
(`in_tangent`, `out_tangent`), and roving keyframes (`SetKeyframeRoving`) across
interior `Position`, `Anchor Point`, and `Mask Path` segments. Animation presets
(`.ariapreset`, `crates/aria-models/src/preset.rs`) capture a clip's effects,
audio inserts, and keyframed properties, a mask's by its index in the stack
(`property_row`), and apply them anchored at the playhead or clip start.
Drivers ship as described, with `aria.expressions` built in and
driver languages registered by plugins (`crates/aria-render/src/driver.rs`,
`docs/plugins/drivers.md`); a driver's fault shows in the inspector and in the
Info panel. Time Remap (`Clip::speed_curve`) and the Speed/Duration dialog
(`DialogKind::SpeedDuration`) ship. Parenting ships as described (`Clip::parent`,
`SetClipParent`, `crates/aria-models/src/clip/parent.rs`,
`crates/aria-render/src/parent.rs`): the Transform tab's Parent row lists the
clips the model accepts and has a pick whip that lands on a timeline clip on
press or release, Shift snaps, the clip context menu has Release Parent, a
chain composes outermost first up to 64 parents, a parent's motion blurs its
children, and a drag on the canvas moves a child with the pointer. A deleted
parent leaves the child on its own transform until the parent is restored or
released. The Viewer draws the selected clip's Position and Anchor Point motion
paths with keyframe squares and tangent handles
(`crates/aria-ui/src/app/canvas_overlay_motion_path.rs`).

## Playback ([playback.md](playback.md))

| Area | Current build |
| --- | --- |
| Present | On a `wgpu` window, `aria-preview` shares the window device (`GpuContext::from_parts`), copies each composited frame into a pooled `wgpu::Texture`, and draws it with `gpui::surface` with no GPU-to-CPU frame readback, with scopes shown or hidden. CPU readback and BGRA conversion (`crates/aria-ui/src/frame.rs`) run when the window has no shared `wgpu` device, when the transparency grid is on, and while scopes are shown on a device that runs no compute pass or with a plugin scope registered. |
| Threads | Viewer frames, render-ahead, and pool pictures render on one `aria-preview` thread (`crates/aria-ui/src/preview_worker.rs`). The ruler's render bar is computed on one `aria-render-bar` thread, started at the first request and ended with the session (`crates/aria-ui/src/app/render_bar.rs`). A recovery copy is written on an `aria-autosave` thread from a snapshot of the project, one copy at a time (`crates/aria-ui/src/app/recovery.rs`). Other background work still starts a thread per job (`crates/aria-ui/src/handoff.rs`, `on_a_thread`). |
| Decode | Serial, no decode-ahead, no frame cache. On the Desktop machine the first frame took 527 ms, 513 ms of it decode, after zero-copy DMABuf import failed and decode fell back to CPU NV12. |
| Scopes | Measured on the `aria-preview` thread with the frame they describe, only while the scope strip is open or the Scopes panel is the front tab of its frame. A texture frame is counted on the device by a compute pass (`GpuScopes`, `crates/aria-compositor/src/scopes.rs`) and only the counts are read back; a read-back frame is counted on the CPU (`crates/aria-analysis/src/scopes.rs`). Both use the same fixed-point BT.709 arithmetic and count the same pixels into the same bins. During playback every `scope_pace`-th slot is measured, at most 30 a second (`crates/aria-ui/src/preview_worker.rs`), the strip holds the last reading between them, and the frame playback stops on is measured. Plugin scopes are measured in the same pass from the frame read back. The waveform counts luma, an RGB parade, or a YCbCr parade (`WaveformTrace`, `ui_settings.waveform`); the mode reaches the worker through `PreviewWorker::set_waveform`, and a reading counted in another mode is dropped (`AppState::adopt_scopes`). The vectorscope graticule holds the skin tone line at 123° and the 75 % and 100 % targets, placed through the same cell mapping as the trace. |
| Resolution | Playback at Full, 1/2, 1/4, 1/8, or Auto, paused at Full or 1/2 (`crates/aria-ui/src/resolution.rs`). A reduced frame is rendered small with `Engine::get_frame_fit` and scaled up by the Viewer. |
| Proxies | The Viewer's Proxy button and View > Proxies switch previews between proxies and originals (`Engine::set_preview_proxies`); the choice is saved, export reads originals, and a proxy missing from disk reads the original. |
| Render files | Content-addressed QOI full-canvas cache (`CacheId::RenderFiles`, `crates/aria-render/src/render_files.rs`). `Sequence > Render In to Out` (`Enter`) renders uncached frames in the work area (or whole sequence) at full canvas resolution with motion blur enabled; `Sequence > Render Selection` renders the frames the selected clips cover; `Sequence > Delete Render Files` deletes cached files for the open sequence. Preview entry points read matching render files and scale to fit; export bypasses them. |

## Audio ([audio.md](audio.md))

Waveforms, clip volume, the Audio Gain dialog (`DialogKind::AudioGain`: set
gain, adjust gain by dB, normalize max peak, normalize all peaks), a mixer with
per-lane and master faders, constant-power stereo pan, mute, solo-in-place, peak
meters, output routing, pre/post-fader sends, volume and pan automation modes (`Off`, `Read`, `Write`, `Touch`,
`Latch`), ducking, beat and silence detection, and the shipped audio effects
exist. Insert chains sit on clips (the inspector's Audio block), lanes, and the
master (the mixer's strips); a plugin adds an audio effect with
`PluginContext::register_audio_effect` (`crates/aria-render/src/audio_processor.rs`).
Every processor is stereo in and stereo out, with no declared channel layout.
Beat, silence, and loudness detection are analyzers registered with
`PluginContext::register_analyzer`; a plugin analyzer appears in Clip > Analyze
or Sequence > Analyze and runs as a job ([analyzers](../plugins/analyzers.md)). A cut on an audio lane takes a
Constant Power, Constant Gain, or Exponential Fade crossfade
(`aria_models::AudioTransition`); each side plays into its source handle across
the window when its span is linear, and a retimed side ramps over its own half.

## Panels ([panels.md](panels.md))

Six panes (`ActivePanel` in `crates/aria-ui/src/types_editor.rs`): Media,
Preview, Timeline, Inspector, Assistant, and Mixer, each a dock
panel. The Markers panel lists markers with color filtering and opens the Edit Marker dialog (`DialogKind::MarkerEdit`), the History panel lists every undo step and jumps to one on a click, the Scopes panel lays the waveform, vectorscope, histogram, and plugin scopes across its frame, and the Color panel groups the selected clip's grade into Basic Correction, Creative, RGB Curves, Hue Curves, Color Wheels, HSL Secondary, and Vignette sections that open and close from their titles, with draggable Lift, Gamma, and Gain wheels. Effect Controls (the inspector's Effects tab, `crates/aria-ui/src/panels/inspector/tabs/effects_params.rs`) reads every effect parameter at the playhead. Each animatable parameter, and each catalog property row in the inspector and the timeline, has a stopwatch, a `<` / `◆` / `>` navigator, and a reset that writes the property's default (`ClipParam::default_value` in `crates/aria-models/src/clip/param_axis.rs`) as a keyframe at the playhead when the property is animated; the inspector rows add, right of a value column shared by every row, a time area over the clip with its own playhead and keyframes, lined up down the panel, whose press and drag are the timeline's keyframe drag (`crates/aria-ui/src/app/key_strip.rs`). With several clips selected, an edit in Effect Controls reaches every selected clip that has the property as one undo step, and a value the clips read differently at the playhead draws as a dash (`crates/aria-ui/src/app/shared_edits.rs`, `crates/aria-ui/src/app/selected_detail.rs`). The plugin
list is a Settings page with each plugin's version, tier, state, permissions,
contributions, and measured cost, and the command palette and the
export dialog exist. Window > Jobs docks the Jobs panel, and the status bar's
center shows the first running job and opens it. Character and Paragraph
(`crates/aria-ui/src/panels/text_panel.rs`) edit the selected title's font,
face, case, kerning, treatments, size, tracking, leading, colors, alignment,
justification, and wrapping, and share their controls with the inspector's
Text tab. With the Type tool, a press on a title in the Viewer places a caret
at the glyph boundary the renderer lays out (`crates/aria-ui/src/app/text_caret.rs`),
a drag or Shift+press selects, the keys edit the text with one undo step per
typing burst, and with characters selected the face switches, font, size,
tracking, and fill read and write a style run over the selection
(`crates/aria-ui/src/app/clip_text_range.rs`); leading, case, alignment, and the
treatments stay whole-title properties. Info
(`crates/aria-ui/src/panels/info_panel.rs`, Shift+8) shows the canvas pixel and
color under the pointer in the Viewer, read from the frame on screen and from
its GPU texture when it has no CPU copy, the selected clip's In, Out, Duration,
and source times, the drag in progress, and driver and plugin faults. Align
(`crates/aria-ui/src/panels/align_panel.rs`) aligns the selected clips' edges
and centers to the selection, the frame, or the first selected clip, and
distributes three or more clips' centers evenly, measured on the bounds the
compositor draws at the playhead, as one undo step.

## Performance ([performance.md](performance.md))

`crates/aria-bench` runs every scenario and writes per-scenario JSON reports
to `bench/results/<scenario>.json`; `cargo run -p aria-bench --release -- fixtures`
synthesizes the media and project fixtures into `bench/fixtures/`. Measured
on the Server machine (`santhserver`, Intel Core i7-11700K, RTX 3080 Ti PCI with
headless software Vulkan `llvmpipe` Mesa 25.2.8). Server (RTX 3080 Ti) figures run on
the NVIDIA 580.173.02 Vulkan driver, NVDEC, and NVENC. Desktop (RTX 5090) figures
come from earlier builds:

| Scenario | Target | Measured on Server | Status |
| --- | --- | --- | --- |
| `startup` | Cold $\le 1\text{ s}$, warm $\le 500\text{ ms}$, $\le 1\text{ s}$ with media named on the command line | Process spawn to the first frame drawn on a refresh, over four runs: cold p50 between $751.2\text{ ms}$ and $759.1\text{ ms}$ (window mapped at $494.1\text{ ms}$ to $500.3\text{ ms}$), warm p50 between $207.0\text{ ms}$ and $210.3\text{ ms}$ (mapped at $155.9\text{ ms}$ to $159.2\text{ ms}$); with `h264-1080p60`, `hevc-2160p60` and `prores-2160p30` named on the command line p50 between $822.5\text{ ms}$ and $834.7\text{ ms}$ (mapped at $497.3\text{ ms}$ to $505.0\text{ ms}$), from between $1125.5\text{ ms}$ and $1200.7\text{ ms}$ in paired runs whose first frame waited for the first picture's decode; warm in-process session+Shell draw p50 $4.7\text{ ms}$ | Pass |
| `project-io` | `project-10k` open $\le 500\text{ ms}$, save $\le 200\text{ ms}$, autosave on the UI thread $\le 4\text{ ms}$ | Open to interactive p50 $37.1\text{ ms}$ to $38.1\text{ ms}$ over four runs, from $41.1\text{ ms}$ to $41.5\text{ ms}$ in paired runs that read a document indented with two spaces (deserialize p50 $19.8\text{ ms}$ to $20.1\text{ ms}$, from $23.5\text{ ms}$ to $24.1\text{ ms}$); save p50 $174.3\text{ ms}$ to $175.3\text{ ms}$ to the Server's hard disk, from $211.4\text{ ms}$ to $250.2\text{ ms}$ in paired runs that indent with two spaces, streamed through a 64 KiB buffer and synced before the rename; autosave on the UI thread p50 $0.074\text{ ms}$ and $0.072\text{ ms}$, from $208.9\text{ ms}$ and $225.0\text{ ms}$ in paired runs that write the recovery copy on the UI thread, and an edit made while the copy is written p50 $9.1\text{ ms}$ and $8.9\text{ ms}$, from $0.9\text{ ms}$ and $0.9\text{ ms}$, which copies the project the writer still holds | Pass |
| `play-start` | Warm $\le 100\text{ ms}$, cold $\le 300\text{ ms}$ | Warm first frame p95 $4.7\text{ ms}$ and $4.4\text{ ms}$ over two runs (p50 $4.2\text{ ms}$ and $4.1\text{ ms}$); cold first frame p95 $32.6\text{ ms}$ and $32.6\text{ ms}$ (p50 $22.8\text{ ms}$ and $24.6\text{ ms}$), from $44.6\text{ ms}$ and $43.1\text{ ms}$ (p50 $27.5\text{ ms}$ and $29.6\text{ ms}$) in paired runs that open each clip's decoder at stream start; paused frame after open $85.7\text{ ms}$ and $82.6\text{ ms}$; Server (RTX 3080 Ti) warm first frame p95 $1.3\text{ ms}$ and $1.5\text{ ms}$ (p50 $1.12\text{ ms}$ and $1.11\text{ ms}$), cold first frame p95 $251.6\text{ ms}$ and $250.4\text{ ms}$ (p50 $29.3\text{ ms}$ and $221.4\text{ ms}$; a sample that opens its NVDEC decoder while the previous sample's decoder closes takes $218\text{ ms}$ to $251\text{ ms}$, and with $700\text{ ms}$ between samples every sample takes $38\text{ ms}$ to $49\text{ ms}$), paused frame after open $1821\text{ ms}$ and $1812\text{ ms}$, which waits for the CUDA device that `aria_decoder::warm_up` brings up; Desktop (RTX 5090) cold p95 $230.5\text{ ms}$ | Pass |
| `playback` (`reverse`) | `h264-1080p60` Full, `hevc-2160p60` Full, `stack-4k` 1/2, `stills-4k` 1/2, `long-gop-scrub` reverse 1x: 0 dropped frames | Dropped frames over two runs: `h264-1080p60` 0 and 0 (frame p50 $3.3\text{ ms}$ and $3.3\text{ ms}$, from $4.3\text{ ms}$ and $4.2\text{ ms}$ in paired runs that draw every layer and effect pass with blending), `hevc-2160p60` 25 and 24 (p50 $15.7\text{ ms}$ and $14.5\text{ ms}$), from 21 and 24 (p50 $12.8\text{ ms}$ and $14.6\text{ ms}$) in paired runs that allocate the planes of every frame, and a mean of 25.25 against 24.00 over eight more pairs (every drop a late software decode), `stack-4k` 87 and 87 (p50 $73.3\text{ ms}$ and $72.7\text{ ms}$, from $76.6\text{ ms}$ and $76.2\text{ ms}$; every drop a late render on software Vulkan), `stills-4k` 0 and 0 (p50 $1.73\text{ ms}$ and $1.77\text{ ms}$, from $1.94\text{ ms}$ and $1.80\text{ ms}$ in paired runs that begin a render pass for each layer draw), reverse `long-gop-scrub` 0 and 0 (p50 $2.5\text{ ms}$ and $2.5\text{ ms}$, from $3.0\text{ ms}$ and $3.0\text{ ms}$); `hevc-2160p60` with 4-sample motion blur uploads each decoded frame once per render, at composite CPU p50 $2.3\text{ ms}$ and $2.5\text{ ms}$ (from $8.2\text{ ms}$ and $8.3\text{ ms}$) and frame p50 $90.4\text{ ms}$ and $89.7\text{ ms}$ (from $94.6\text{ ms}$ and $94.5\text{ ms}$); Server (RTX 3080 Ti) `h264-1080p60` 0 and 0 (p50 $1.26\text{ ms}$ and $1.25\text{ ms}$), `hevc-2160p60` 0 and 0, from 0 and 0 (p50 $5.67\text{ ms}$ and $5.65\text{ ms}$, p95 $5.88\text{ ms}$ and $5.92\text{ ms}$, from $8.22\text{ ms}$ and $7.97\text{ ms}$, p95 $13.29\text{ ms}$ and $13.33\text{ ms}$, in paired runs that convert the P010 frames of `nvh265dec` in `videoconvert` and allocate the planes of every frame), `stack-4k` 5 and 5, every one a late decode (from 17 and 20; p50 $11.41\text{ ms}$ and $11.08\text{ ms}$, from $16.10\text{ ms}$ and $12.41\text{ ms}$), `stills-4k` 0 and 0, reverse `long-gop-scrub` 0 and 0 (p50 $0.53\text{ ms}$ and $0.52\text{ ms}$); Desktop (RTX 5090) 0 / 0 / 1 / 0, the `stack-4k` drop one warm-up frame of $18.1\text{ ms}$ (p50 $12.8\text{ ms}$, p95 $16.7\text{ ms}$) decoding and compositing three 4K HEVC Main 10 layers and a text layer in series; in the editor on Desktop `nvh265dec` offers no DMABuf output, so each layer is decoded to system memory and converted to NV12 on the CPU, which runs three concurrent `hevc-2160p60` streams at $66\text{ fps}$ each ($95\text{ fps}$ each without the conversion); render thread performs $52$ heap allocations per frame after $1\text{ s}$ | Fail |
| `av-sync` | Presentation within $\pm 0.5\text{ frame}$ of audio clock | Max absolute offset $0.00\text{ frames}$ over 180 frames at $48\text{ kHz}$ | Pass |
| `scrub` | Warm scrub $\le 16.67\text{ ms}$, cold seek $\le 150\text{ ms}$ | Warm scrub p95 $11.1\text{ ms}$ and $11.2\text{ ms}$ over two runs (p50 $10.4\text{ ms}$ and $10.1\text{ ms}$, renderer p50 $3.2\text{ ms}$ and $3.1\text{ ms}$); cold seek, a decoder opened at the target frame over `h264-1080p60`, `hevc-2160p60` and `prores-2160p30`, p50 $37.7\text{ ms}$ and $37.0\text{ ms}$ (p95 $55.0\text{ ms}$ and $55.3\text{ ms}$), from $52.6\text{ ms}$ and $48.1\text{ ms}$ (p95 $120.0\text{ ms}$ and $109.1\text{ ms}$) in paired runs that open it at stream start; `long-gop-scrub` exact seek p50 $25.1\text{ ms}$ and $27.0\text{ ms}$; Server (RTX 3080 Ti) warm scrub p95 $6.45\text{ ms}$ and $6.36\text{ ms}$ (p50 $5.67\text{ ms}$ and $5.50\text{ ms}$, renderer p50 $3.83\text{ ms}$ and $3.75\text{ ms}$), cold seek p50 $36.6\text{ ms}$ and $36.3\text{ ms}$ (p95 $63.4\text{ ms}$ and $60.4\text{ ms}$), `long-gop-scrub` exact seek p50 $40.2\text{ ms}$ and $40.0\text{ ms}$; Server paused ruler scrub, 30 moves $8\text{ ms}$ apart, each move's update p50 $0.32\text{ ms}$ and $0.33\text{ ms}$ (max $0.75\text{ ms}$ and $0.71\text{ ms}$) over `h264-1080p60` at $0.1\text{ s}$ a move, and p50 $0.29\text{ ms}$ and $0.33\text{ ms}$ (max $0.34\text{ ms}$ and $0.71\text{ ms}$) over `long-gop-scrub` at $1\text{ s}$ a move; Server filmstrip cell, rendered on a renderer of its own, p50 $36.6\text{ ms}$ to $37.8\text{ ms}$ over three runs, of which the session is held p50 $0.001\text{ ms}$, and the Viewer playback frame drawn after a cell p50 $1.75\text{ ms}$ to $1.82\text{ ms}$, from a cell p50 $82.7\text{ ms}$ to $84.2\text{ ms}$, all of it holding the session, and a frame after it p50 $21.3\text{ ms}$ to $21.5\text{ ms}$ in paired runs that render cells on the session's renderer; Server filmstrip refill over a clip's first fill, four zooms and three scrolls p50 $220.6\text{ ms}$ to $249.8\text{ ms}$ over five runs (first fill $936.3\text{ ms}$ to $950.0\text{ ms}$), from $857.9\text{ ms}$ to $862.8\text{ ms}$ (first fill $1008.9\text{ ms}$ to $1018.5\text{ ms}$) in paired runs that render cells on four renderers; Server filmstrip first fill draws no empty cell after $133.3\text{ ms}$ to $139.9\text{ ms}$ over three runs, and over `hevc-2160p60` after $717.3\text{ ms}$ to $760.2\text{ ms}$, from $818.9\text{ ms}$ to $872.9\text{ ms}$ and $7675.7\text{ ms}$ to $9099.4\text{ ms}$ in paired runs that render every cell exactly | Pass |
| `param-latency` | Paused effect drag visible within 2 display frames ($\le 33.33\text{ ms}$) | 1080p paused effect drag p95 $0.40\text{ frames}$ and $0.40\text{ frames}$ over two runs ($6.7\text{ ms}$ and $6.6\text{ ms}$, p50 $0.29\text{ frames}$ and $0.29\text{ frames}$); `stack-4k` drag p50 $53.5\text{ ms}$ and $53.9\text{ ms}$, from $56.8\text{ ms}$ and $55.8\text{ ms}$ in paired runs that draw every effect pass with blending; Server (RTX 3080 Ti) 1080p drag p95 $0.043\text{ frames}$ and $0.040\text{ frames}$ ($0.72\text{ ms}$ and $0.67\text{ ms}$), `stack-4k` drag p50 $5.4\text{ ms}$ and $5.4\text{ ms}$ | Pass |
| `timeline-frame` | `project-10k` scroll, zoom, drag UI frame build $\le 4\text{ ms}$ | `Shell` `window.draw()` p95 $1.98\text{ ms}$ and $1.98\text{ ms}$ over two runs, from $2.17\text{ ms}$ and $2.15\text{ ms}$ in paired runs that lay out with taffy 0.13 (scroll p50 $1.83\text{ ms}$ and $1.84\text{ ms}$, from $1.98\text{ ms}$ and $2.00\text{ ms}$; zoom p50 $1.76\text{ ms}$ and $1.75\text{ ms}$, from $1.92\text{ ms}$ and $1.92\text{ ms}$; drag p50 $0.95\text{ ms}$ and $0.95\text{ ms}$, from $1.16\text{ ms}$ and $1.17\text{ ms}$; the window chrome with every panel body replaying its last drawing p50 $1.09\text{ ms}$ and $1.08\text{ ms}$, from $1.25\text{ ms}$ and $1.27\text{ ms}$; one held pointer move of an Inspector value scrub on a clip's Opacity and the frame it draws p50 $6.09\text{ ms}$ (between $6.01\text{ ms}$ and $6.51\text{ ms}$ over eight runs), from $6.38\text{ ms}$ (between $6.34\text{ ms}$ and $6.61\text{ ms}$) in paired runs that walk every clip for the `Auto` canvas size on each scrub write; 50 lanes, 9 visible lanes, 10,000 clips, 29 visible clips); Desktop (RTX 5090) p95 $3.00\text{ ms}$ and $3.07\text{ ms}$ (scroll p50 $2.74\text{ ms}$, zoom p50 $2.79\text{ ms}$, drag p50 $2.88\text{ ms}$) | Pass |
| `edit-apply` | Single edit or undo on `project-10k`: $\le 1\text{ ms}$ in engine, $\le 16.67\text{ ms}$ visible on next frame | Engine edit/undo p95 $0.53\text{ ms}$ and $0.53\text{ ms}$ over two runs (edit p50 $0.009\text{ ms}$, edits and undos together p50 $0.008\text{ ms}$); full `AppState::apply_command` + `UiEditorStore` re-projection + `window.draw()` p95 between $3.42\text{ ms}$ and $3.75\text{ ms}$ over eight runs (p50 between $3.35\text{ ms}$ and $3.44\text{ ms}$, from between $3.52\text{ ms}$ and $4.04\text{ ms}$ in paired runs that walk every clip for the `Auto` canvas size on each edit); Desktop (RTX 5090) visible p95 $11.3\text{ ms}$ and $9.9\text{ ms}$, engine p95 $0.36\text{ ms}$ | Pass |
| `export` | `h264-1080p60` to H.264 1080p on hardware encoder $\ge 4\times$ real time | Server (RTX 3080 Ti) `nvh264enc` $4.61\times$ and $4.19\times$ real time over two runs (frame p50 $1.62\text{ ms}$ and $1.87\text{ ms}$) and $4.34\times$ to $4.58\times$ over three more writing to its hard disk, and $6.83\times$ to $6.92\times$ over three runs writing to tmpfs, the difference being the sync of the exported file; with the packaged NVIDIA userspace, which does not match the loaded kernel module, `nvh264enc` and `nvcudah264enc` are skipped as demoted plugins (no VAAPI), and `pick_encoder`, which tries NVENC, VAAPI, then `x264enc` and logs each skipped encoder with its cause, exports with `x264enc` at $3.03\times$ and $3.85\times$ real time over two runs (frame p50 $3.56\text{ ms}$ and $3.58\text{ ms}$); Desktop (RTX 5090) `nvh264enc` $4.30\times$ to $5.23\times$ real time over 5 runs (p50 $4.66\times$, frame p50 $2.4\text{ ms}$ to $2.7\text{ ms}$) | Pass (Server RTX 3080 Ti, Desktop) |
| `audio-load` | 32 tracks $\times$ 4 shipped effects at $48\text{ kHz}$, 256-sample blocks: 0 underruns in $60\text{ s}$ | 11,250 blocks ($5.333\text{ ms}$ budget): block p50 $0.64\text{ ms}$, p95 $0.82\text{ ms}$, p99 $0.84\text{ ms}$, max $2.51\text{ ms}$, 0 underruns in $60\text{ s}$ | Pass |
| `memory` | Idle with `project-10k` open (frame cache excluded) $\le 500\text{ MB}$ RSS | Isolated `VmRSS` $244.4\text{ MiB}$, $244.1\text{ MiB}$ and $244.5\text{ MiB}$ over three runs ($+53.9\text{ MiB}$, $+53.9\text{ MiB}$ and $+53.7\text{ MiB}$ over empty `AppState` at $190.4\text{ MiB}$, $190.3\text{ MiB}$ and $190.8\text{ MiB}$ with the decode warm-up finished; $+0.7\text{ MiB}$, $+0.7\text{ MiB}$ and $+0.8\text{ MiB}$ across 1,000 undo steps) | Pass |
| `plugin-cost` | Host call $\le 1\ \mu\text{s}$ native / $\le 5\ \mu\text{s}$ WASM; WASM activation $\le 50\text{ ms}$; plugin WGSL within $5\%$ of shipped effect | Native host call p50 $0.06\ \mu\text{s}$; WASM host call p50 $0.84\ \mu\text{s}$; WASM activation p50 $0.16\text{ ms}$; 4-pass 1080p vignette plugin WGSL vs shipped effect overhead $0.0\%$ | Pass |

On X11 each window draws at the refresh rate of the monitor that shows the
largest part of it. Measured on the Desktop (RTX 5090, GNOME on Xorg, a
$240\text{ Hz}$ and a $60\text{ Hz}$ monitor, window on the $60\text{ Hz}$
monitor) over the first $20\text{ s}$ after a launch that opens four media
files, in three paired runs: frame draw and present p50 between
$2.33\text{ ms}$ and $2.79\text{ ms}$ (p95 $4.31\text{ ms}$ to $5.35\text{ ms}$),
from between $13.27\text{ ms}$ and $16.04\text{ ms}$ (p95 $16.73\text{ ms}$ to
$16.93\text{ ms}$) in runs that drew every window at the first monitor's rate;
frames that waited $12\text{ ms}$ to $18\text{ ms}$ for the display between 0
and 2 per run, from between 123 and 133; UI thread time in frames between
$329.9\text{ ms}$ and $513.2\text{ ms}$, from between $2038.7\text{ ms}$ and
$2205.9\text{ ms}$.
