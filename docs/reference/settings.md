# Settings Reference

Aria separates persisted configuration into two files inside the profile data directory (`~/.aria` by default, or `$ARIA_DATA_DIR`):

1. `config.toml` (`crates/aria-settings/`): operational configuration (`[appearance]`, `[ai]`, `[cloud]`, `[storage]`, `[transcription]`, `[plugin_limits]`, `[providers]`, `[plugins]`, and `[schema]`). Malformed TOML and schema versions outside the supported range fail closed; invalid or out-of-range values keep their defaults and are recorded in `Settings::rejected` for display in the Configuration section of Settings.
2. `ui-settings.json` (`crates/aria-ui/src/settings.rs`): window layout, workspace dock trees, keymap overrides, preview resolution, and timeline display toggles.

## Worked example

Configure a local OpenAI-compatible AI endpoint, set a custom proxy cache path, and cap WASM plugin memory to `128 MiB` in `config.toml`:

```toml
[schema]
ai = 2
appearance = 1
cloud = 2
plugin_limits = 1
plugins = 1
providers = 1
storage = 1
transcription = 1

[appearance]
theme = "graphite"

[ai]
source = "local"
base_url = "http://127.0.0.1:11434/v1"
model = "qwen2.5-coder:14b"
api_protocol = "chat_completions"
reasoning_summary = "off"
autonomy = "ask"

[storage]
root = "/var/cache/aria"
download_quota_mib = 4096

[storage.paths]
proxies = "/mnt/fast-scratch/aria-proxies"

[plugin_limits]
wasm_fuel_per_call = 500000000
wasm_memory_mib = 128

[providers.pexels]
api_key_env = "PEXELS_API_KEY"
```

## Configuration file keys (`config.toml`)

| Key | Type | Default | Allowed values or range | Dialog section | Effect |
| --- | --- | --- | --- | --- | --- |
| `appearance.theme` | `string` | `"blue"` | `"blue"`, `"ember"`, `"teal"`, `"graphite"` (aliases `"default"`, `"dark-blue"`, `"dark_blue"`) | `Appearance` | Selects the accent color family (`ThemeChoice`). |
| `ai.source` | `string` | `"local"` | `"local"`, `"openrouter"`, `"custom"` (aliases `"or"`, `"advanced"`, `"endpoint"`) | `AI` | Selects the AI provider preset (`AiSource::Local`, `AiSource::OpenRouter`, or `AiSource::Custom`). |
| `ai.base_url` | `string` | `""` | `""` or an `http://` / `https://` URL with host | `AI` | OpenAI-compatible endpoint URL used by `local` and `custom`. |
| `ai.model` | `string` | `""` | Model identifier string | `AI` | Model identifier sent in chat completion or responses requests. |
| `ai.api_protocol` | `string` | `"chat_completions"` | `"chat_completions"`, `"responses"` (aliases `"chat-completions"`, `"chat"`, `"response"`) | `AI` | Selects `/v1/chat/completions` (`AiApiProtocol::ChatCompletions`) or `/v1/responses` (`AiApiProtocol::Responses`). |
| `ai.reasoning_summary` | `string` | `"auto"` | `"auto"`, `"off"` (aliases `"on"`, `"none"`) | `AI` | Requests provider-generated reasoning summaries (`ReasoningSummary`) in `Responses` mode when `"auto"`. |
| `ai.api_key` | `string` | `none` | Non-empty API key string | `AI` | Literal bearer token for the AI endpoint. |
| `ai.api_key_env` | `string` | `none` | Environment variable name | `AI` | Environment variable read at request time for the AI bearer token (takes precedence over `ai.api_key`). |
| `ai.autonomy` | `string` | `"ask"` | `"ask"`, `"full"` (alias `"confirm"`) | `AI` | Controls whether destructive System-tier agent tools prompt for confirmation (`Autonomy::Ask`) or run without confirmation (`Autonomy::Full`). |
| `cloud.base_url` | `string` | `""` | `""` or an `http://` / `https://` URL with host | `Cloud` | Overrides the Aria cloud API host (`""` uses `https://api.aria.sh`). |
| `storage.root` | `path` | `none` | Absolute directory path (`none` uses `<data dir>`) | `Storage` | Base directory under which default cache folders resolve. |
| `storage.download_quota_mib` | `integer` | `2048` | `1` to `1048576` MiB (`1 MiB` to `1 TiB`) | `Storage` | Eviction ceiling for the LRU stock, SFX, and cloud asset download cache. |
| `storage.paths.library_thumbnails` | `path` | `none` | Absolute directory path | `Caches` | Overrides the media pool poster thumbnail cache directory. |
| `storage.paths.proxies` | `path` | `none` | Absolute directory path | `Caches` | Overrides the generated proxy media directory. |
| `storage.paths.analysis` | `path` | `none` | Absolute directory path | `Caches` | Overrides the shot, beat, silence, and loudness analysis cache directory. |
| `storage.paths.render_files` | `path` | `none` | Absolute directory path | `Caches` | Overrides the content-addressed rendered timeline frame cache directory. |
| `storage.paths.ai_models` | `path` | `none` | Absolute directory path | `Caches` | Overrides the downloaded AI model weights directory. |
| `storage.paths.download` | `path` | `none` | Absolute directory path | `Caches` | Overrides the downloaded stock and generated asset cache directory. |
| `storage.paths.catalog` | `path` | `none` | Absolute directory path | `Caches` | Overrides the cached remote catalog response and metadata directory. |
| `storage.paths.luts` | `path` | `none` | Absolute directory path | `Caches` | Overrides the downloaded LUT pack directory. |
| `storage.paths.lottie` | `path` | `none` | Absolute directory path | `Caches` | Overrides the downloaded Lottie asset directory. |
| `storage.paths.templates` | `path` | `none` | Absolute directory path | `Caches` | Overrides the downloaded and installed project template bundle directory. |
| `transcription.connect_timeout_secs` | `integer` | `15` | `1` to `86400` s | `Transcription` | Connection timeout for transcription model downloads. |
| `transcription.read_timeout_secs` | `integer` | `30` | `1` to `86400` s | `Transcription` | Per-read stall timeout for transcription model downloads. |
| `transcription.total_timeout_secs` | `integer` | `1800` | `1` to `86400` s | `Transcription` | Whole-request deadline for transcription model downloads. |
| `plugin_limits.wasm_fuel_per_call` | `integer` | `500000000` | `1000000` to `100000000000` instructions | `Plugins` | Wasmtime instruction fuel budget granted to each Tier 2 WASM plugin call before trapping. |
| `plugin_limits.wasm_memory_mib` | `integer` | `64` | `16` to `4096` MiB | `Plugins` | Linear memory ceiling per Tier 2 WASM plugin instance. |
| `providers.pexels` | `table` | unconfigured | `{ api_key = "..." }` or `{ api_key_env = "..." }` | `Provider keys` | Direct Pexels stock search API key. |
| `providers.pixabay` | `table` | unconfigured | `{ api_key = "..." }` or `{ api_key_env = "..." }` | `Provider keys` | Direct Pixabay stock search API key. |
| `providers.elevenlabs` | `table` | unconfigured | `{ api_key = "..." }` or `{ api_key_env = "..." }` | `Provider keys` | Direct ElevenLabs voice generation API key. |
| `providers.fal` | `table` | unconfigured | `{ api_key = "..." }` or `{ api_key_env = "..." }` | `Provider keys` | Direct fal.ai image and video generation API key. |
| `plugins.<key>` | `string` | unset | Non-empty key name and string value declared by a plugin's `[settings]` schema | Plugin section | Per-plugin string setting exposed to the declaring plugin via `SettingsHost`. |

## Schema version table (`[schema]`)

`config.toml` records a schema version for each of the eight persisted sections (`PersistedShape::all()` in `crates/aria-settings/src/schema.rs`). Any version below `oldest_migratable_version` or above `current_version` is rejected on load:

| Shape | Current version | Oldest migratable version |
| --- | --- | --- |
| `ai` | `2` | `1` |
| `appearance` | `1` | `1` |
| `cloud` | `2` | `1` |
| `plugin_limits` | `1` | `1` |
| `plugins` | `1` | `1` |
| `providers` | `1` | `1` |
| `storage` | `1` | `1` |
| `transcription` | `1` | `1` |

## UI settings (`ui-settings.json`)

`UiSettings` (`crates/aria-ui/src/settings.rs`) persists workspace geometry, view toggles, and keymap overrides in `<data dir>/ui-settings.json`. Values are clamped to their valid bounds on load:

| Key | Type | Default | Allowed values or range | Surface | Effect |
| --- | --- | --- | --- | --- | --- |
| `sidebarWidth` | `number` | `256.0` | `208.0` to `400.0` px | Project panel splitter | Width of the left sidebar column (`SIDEBAR_MIN..=SIDEBAR_MAX`). |
| `sidebarCollapsed` | `boolean` | `false` | `true`, `false` | Sidebar collapse button | Collapses or expands the left sidebar column. |
| `inspectorWidth` | `number` | `520.0` | `360.0` to `760.0` px | Inspector splitter | Width of the right Inspector column (`INSPECTOR_MIN..=INSPECTOR_MAX`). |
| `inspectorCollapsed` | `boolean` | `false` | `true`, `false` | Inspector collapse button | Collapses or expands the right Inspector column. |
| `timelineHeight` | `number` | `280.0` | `>= 120.0` px | Small-window splitter | Height of the Timeline under the Viewer in a window smaller than 1024 x 640 (`TIMELINE_MIN_HEIGHT`). |
| `outlineWidth` | `number` | `300.0` | `240.0` to `480.0` px | Timeline outline splitter | Width of the Timeline track-header and property outline column (`OUTLINE_MIN..=OUTLINE_MAX`). |
| `appearance` | `string` | `"system"` | `"system"`, `"light"`, `"dark"` | **Settings > Appearance** | Selects the dark or light surface palette (`AppearanceMode`). |
| `homeRail` | `string` | `"recents"` | `"new"`, `"open"`, `"recents"`, `"samples"`, `"learn"` | Home left rail | Last selected view on the Home route (`HomeRail`). |
| `motion` | `string` | `"full"` | `"full"`, `"brisk"`, `"relaxed"`, `"off"` | **Settings > Appearance** | Controls UI transition animation duration (`MotionPreference`). |
| `keys` | `object` | `{}` | Map of binding ID to chord string (`"Ctrl+Shift+P"`) or `null` (unbound) | **Settings > Keys** | Per-command keyboard shortcut overrides. |
| `keymap` | `string` | `null` | Keymap name in `<data dir>/keymaps` or `null` | **Settings > Keys** | Name of the custom keymap file last loaded or saved. |
| `workspace` | `object` | `null` | `PaneLayout` JSON object or `null` | **Settings > Workspaces** | Saved `Custom` workspace slot. |
| `dockTree` | `object` | `null` | `DockTree` (`version = 2`) or `null` | Workspace dock | Active dock frame tree restored on launch. |
| `workspaceTrees` | `object` | `{}` | Map of workspace ID to `DockTree` | Workspace switcher | Per-workspace customized dock trees saved via **Save Changes to this Workspace**. |
| `activeWorkspace` | `string` | `null` | Workspace ID or `null` | **Window > Workspaces** | Most recently applied workspace ID. |
| `poolView` | `string` | `"tiles"` | `"tiles"`, `"list"`, `"icons"`, `"freeform"` | Project > Media toolbar | Display mode of the Media pool (`PoolView`). |
| `logDroppedFrames` | `boolean` | `false` | `true`, `false` | **Settings > Playback** | Logs dropped presentation frames and their cause under the `aria::playback` tracing target. |
| `themeFile` | `string` | `null` | File stem in `<data dir>/themes` (without `.toml`) or `null` | **Settings > Appearance** | Applies a custom theme file over the active appearance. |
| `playbackResolution` | `string` | `"auto"` | `"full"`, `"half"`, `"quarter"`, `"eighth"`, `"auto"` | **View > Playback Resolution** | Viewer raster resolution while transport is playing (`PlaybackResolution`). |
| `pausedResolution` | `string` | `"full"` | `"full"`, `"half"` | **View > Paused Resolution** | Viewer raster resolution while transport is paused or scrubbing (`PausedResolution`). |
| `useProxies` | `boolean` | `true` | `true`, `false` | **View > Use Proxies** | Substitutes generated proxy media during Viewer decode when available. |
| `transparencyGrid` | `boolean` | `false` | `true`, `false` | **View > Transparency Grid** | Draws a transparency checkerboard where no layer covers the canvas. |
| `audioScrubbing` | `boolean` | `true` | `true`, `false` | **Settings > Playback** / **Sequence > Audio Scrubbing** | Plays a 40 ms audio grain at each new pointer position while scrubbing. |
| `timelineDisplay` | `object` | `{ "thumbnails": "continuous", "waveform": "logarithmic", "follow": "page" }` | `{ thumbnails, waveform, follow }` | **View > Timeline Display** | Composite timeline display preferences (`TimelineDisplay`), detailed in the three sub-keys below. |
| `timelineDisplay.thumbnails` | `string` | `"continuous"` | `"head"`, `"head_and_tail"`, `"continuous"` | **View > Timeline Display** | Controls filmstrip thumbnail cells on video clips (`Thumbnails`). |
| `timelineDisplay.waveform` | `string` | `"logarithmic"` | `"logarithmic"`, `"linear"`, `"hidden"` | **View > Timeline Display** | Maps audio sample peaks to clip bar heights (`WaveformScale`, floor `-48.0 dB` in logarithmic mode). |
| `timelineDisplay.follow` | `string` | `"page"` | `"page"`, `"smooth"` | **View > Timeline Display** | Controls how the timeline viewport follows the playhead during playback (`Follow`). |
| `waveform` | `string` | `"luma"` | `"luma"`, `"rgbParade"`, `"ycbcrParade"` | **View > Scopes** / Scopes panel chips | Selects the Scopes waveform monitor mode (`WaveformMode`). |
| `favouriteEffects` | `array` | `[]` | Array of `"kind:id"` strings (such as `"video:gaussian_blur"`) | Effects & Presets star button | Starred effect keys in the order they were favorited. |
| `windowPlacements` | `object` | `{}` | Map of display arrangement signature (`"x,y,wxH;..."`) to `WindowPlacement` | Window move/resize | Saved main window position, size (`>= 1024x640`), maximized state, and display index per monitor layout. |
| `lastDisplayArrangement` | `string` | `null` | Display arrangement signature string or `null` | Multi-monitor restore | Most recent monitor arrangement signature used for fallback display selection on launch. |

## Project canvas settings

The **Canvas** section of `aria.dialog.settings` (implemented in `crates/aria-ui/src/panels/settings_dialog.rs` and `crates/aria-ui/src/surfaces/settings_pick.rs`) edits the active project's sequence format through `EditCommand`:

- **Aspect ratio (`CanvasAspect::ALL`)**: `auto` (`Match footage`, derives from the largest video on the timeline), `16:9` (`1920x1080` at 1080p), `9:16` (`1080x1920`), `1:1` (`1080x1080`), `4:5` (`1080x1350`), or `21:9` (`2520x1080`).
- **Resolution (`CANVAS_BASELINES`)**: `Derived` (computed from the aspect ratio or largest video source) or an explicit short-edge baseline tier (`720` / 720p, `1080` / 1080p, `1440` / 1440p, `2160` / 4K, `4320` / 8K) or custom even pixel dimensions between `16` and `8192` (`CanvasSettings::MIN_DIM..=CanvasSettings::MAX_DIM`, applied via `EditCommand::SetCanvas`).
- **Frame rate (`RATES`)**: `23.976` (`24000/1001`), `24` (`24/1`), `25` (`25/1`), `29.97` (`30000/1001`), `30` (`30/1`), `50` (`50/1`), `59.94` (`60000/1001`), or `60` (`60/1`) fps (`EditCommand::SetSequenceRate`).
- **Audio channel layout (`AudioLayout::ALL`)**: `mono` (`Mono (1.0)`, `1` ch), `stereo` (`Stereo (2.0)`, `2` ch, default), or `5.1` (`5.1 Surround`, `6` ch) (`EditCommand::SetAudioLayout`).
- **Timecode display (`TimeDisplay::ALL`)**: `timecode` (`Timecode`, `HH:MM:SS:FF`, default), `frames` (`Frames`, integer frame count), or `feet_frames` (`Feet + Frames 35mm`, `16` frames per foot) (`EditCommand::SetTimeDisplay`).

## Config refusal reasons (`Refusal::ALL`)

When `Settings::from_document` encounters a field in `config.toml` that fails validation, the field keeps its default value and an entry is appended to `Settings::rejected` with one of four refusal categories (`Refusal::ALL` in `crates/aria-settings/src/read.rs`):

| Variant | Reason (`reason()`) |
| --- | --- |
| `WrongType` | `is not the kind of value this key takes` |
| `NotOneOf` | `is not one of the values this key accepts` |
| `NotAbsolute` | `is not an absolute path, so it would resolve differently depending on where the editor was started` |
| `OutOfRange` | `is outside the range this key accepts` |

## Storage cache registry (`CacheId::ALL`)

`CacheId::ALL` (`crates/aria-storage/src/cache_registry.rs`) enumerates the thirteen clearable caches displayed in **Settings > Caches**:

| Variant | Key (`as_str()`) | Label (`descriptor().label`) | Kind (`kind`) | Tier (`tier`) | Default relative path (`default_relative`) |
| --- | --- | --- | --- | --- | --- |
| `PreviewFrames` | `preview_frames` | `Preview frames` | `Memory` | `Disposable` | `none` |
| `LibraryThumbnails` | `library_thumbnails` | `Library thumbnails` | `Disk` | `Disposable` | `library-thumbnails` |
| `TimelineFilmstrips` | `timeline_filmstrips` | `Timeline filmstrips` | `Memory` | `Disposable` | `none` |
| `TimelineWaveforms` | `timeline_waveforms` | `Timeline waveforms` | `Memory` | `Disposable` | `none` |
| `Proxies` | `proxies` | `Proxies` | `Disk` | `Disposable` | `proxies` |
| `Analysis` | `analysis` | `Media analysis` | `Disk` | `Disposable` | `analysis` |
| `RenderFiles` | `render_files` | `Render files` | `Disk` | `Disposable` | `render-files` |
| `AiModels` | `ai_models` | `AI models` | `Disk` | `Redownloadable` | `ai-models` |
| `Download` | `download` | `Downloads` | `Disk` | `Redownloadable` | `download-cache` |
| `Catalog` | `catalog` | `Catalog` | `Disk` | `Redownloadable` | `catalog-cache` |
| `Luts` | `luts` | `LUTs` | `Disk` | `Redownloadable` | `luts` |
| `Lottie` | `lottie` | `Lottie assets` | `Disk` | `Redownloadable` | `lottie` |
| `Templates` | `templates` | `Templates` | `Disk` | `Redownloadable` | `templates` |

## Settings dialog text fields and numeric limits (`SettingsField::ALL`, `TimeoutField::ALL`, `WasmLimit::ALL`)

`SettingsField::ALL` (`crates/aria-ui/src/app/settings_form.rs`), `TimeoutField::ALL` (`crates/aria-ui/src/app/models.rs`), and `WasmLimit::ALL` (`crates/aria-ui/src/app/plugin_install.rs`) enumerate the twelve shipped text inputs and numeric limit rows in `aria.dialog.settings`:

| Variant | Element ID | Label |
| --- | --- | --- |
| `AiBaseUrl` | `ai-base-url` | `AI base URL` |
| `AiModel` | `ai-model` | `AI model` |
| `AiKey` | `ai-key` | `AI key` |
| `AiKeyEnv` | `ai-key-env` | `AI key variable` |
| `CloudBaseUrl` | `cloud-base-url` | `Cloud base URL` |
| `KeymapName` | `keymap-name` | `Save keys as` |
| `Filter` | `settings-filter` | `Filter` |
| `Connect` | `model-timeout-connect` | `Connect` |
| `Read` | `model-timeout-read` | `Read` |
| `Total` | `model-timeout-total` | `Total` |
| `Fuel` | `wasm-limit-fuel` | `WASM fuel per call` |
| `Memory` | `wasm-limit-memory` | `WASM memory` |

## Playback drop causes and decoder output modes (`DropCause::ALL` and `OutputMode::ALL`)

When `logDroppedFrames` is enabled in **Settings > Playback**, `DroppedFrames` classifies each dropped presentation frame into one of three causes (`DropCause::ALL` in `crates/aria-render/src/playback_meter.rs`), and video decoders deliver frames in one of two transfer modes (`OutputMode::ALL` in `crates/aria-decoder/src/lib.rs`):

| Variant | Label (`label()`) |
| --- | --- |
| `DecodeLate` | `decode late` |
| `RenderLate` | `render late` |
| `PresentLate` | `present late` |
| `Gpu` | `GPU surfaces` |
| `Cpu` | `CPU planes` |

## Render resolution levels (`Resolution::ALL`)

`Resolution::ALL` (`crates/aria-ui/src/resolution.rs`) defines the viewer and canvas downsampling divisors returned by `PlaybackResolution::resolve()` and `PausedResolution::resolve()`:

| Variant | Divisor (`divisor()`) | Label (`label()`) | Lower (`lower()`) | Higher (`higher()`) | Default |
| --- | --- | --- | --- | --- | --- |
| `Full` | `1` | `Full` | `Half` | `Full` | `yes` |
| `Half` | `2` | `1/2` | `Quarter` | `Full` | `no` |
| `Quarter` | `4` | `1/4` | `Eighth` | `Half` | `no` |
| `Eighth` | `8` | `1/8` | `Eighth` | `Quarter` | `no` |
