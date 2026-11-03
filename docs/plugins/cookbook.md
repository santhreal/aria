# Plugin cookbook

Complete plugin recipes combining `plugin.toml` declarations with WebAssembly
(`plugin.wit`), native (`aria-sdk`), and in-process (`aria-plugin`) host calls.
See [Capabilities across surfaces](capabilities.md) for the full host call and
permission matrix and [Worked example](worked-example.md) for the shipped
`examples/wasm-plugin` package.

## Recipe: Contact sheet and playhead jump panel (WebAssembly)

This WebAssembly package registers a sidebar panel and a command that renders a
`160x90` thumbnail at the midpoint of each selected clip via `render-frame`,
stores the rendered frame dimensions in package storage, displays the thumbnail
in `UiNode::Image`, and jumps the playhead (`editor-seek`) when a clip button is
clicked.

### `plugin.toml`

```toml
[plugin]
id = "com.example.contact-sheet"
name = "Contact Sheet"
version = "0.1.0"
api = 1
description = "Clip midpoint thumbnails with playhead seek buttons"

[permissions]
project = "read-write"

[[commands]]
id = "capture-selected"
title = "Contact Sheet: Capture Selected Clip"
enablement = "selection"

[[panels]]
id = "sheet"
title = "Contact Sheet"
location = "sidebar"
```

### `src/lib.rs`

```rust
wit_bindgen::generate!({
    path: "../../crates/aria-plugin-wasm/wit",
    world: "plugin",
});

use std::sync::Mutex;

struct ShotEntry {
    clip_id: u64,
    name: String,
    midpoint: RationalTime,
    width: u32,
    height: u32,
    data_url: String,
}

static SHOTS: Mutex<Vec<ShotEntry>> = Mutex::new(Vec::new());

struct ContactSheetPlugin;

impl Guest for ContactSheetPlugin {
    fn mount() -> Result<(), String> {
        subscribe("selection-changed")
    }

    fn unmount() -> Result<(), String> {
        Ok(())
    }

    fn command_run(id: String, _args_json: String) -> Result<(), String> {
        if id == "com.example.contact-sheet.capture-selected" {
            capture_selected_clips()?;
            refresh_panel("com.example.contact-sheet.sheet")?;
        }
        Ok(())
    }

    fn event(kind: String, _event_json: String) -> Result<Option<String>, String> {
        if kind == "selection-changed" {
            let _ = capture_selected_clips();
            let _ = refresh_panel("com.example.contact-sheet.sheet");
        }
        Ok(None)
    }

    fn panel_view(_id: String) -> Vec<UiNode> {
        let shots = SHOTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut nodes = Vec::new();
        let mut children = Vec::new();

        for shot in shots.iter() {
            let img_idx = nodes.len() as u32 + 1;
            let btn_idx = img_idx + 1;
            let row_idx = btn_idx + 1;
            children.push(row_idx);
        }

        nodes.push(UiNode::Column(UiGroup { children }));
        for shot in shots.iter() {
            let img_idx = nodes.len() as u32;
            nodes.push(UiNode::Image(UiImage {
                src: shot.data_url.clone(),
                width: Some(shot.width as f32),
                height: Some(shot.height as f32),
                alt: shot.name.clone(),
            }));
            let btn_idx = nodes.len() as u32;
            nodes.push(UiNode::Button(UiButton {
                id: format!("seek:{}", shot.clip_id),
                label: format!("Jump to {}", shot.name),
            }));
            nodes.push(UiNode::Row(UiGroup {
                children: vec![img_idx, btn_idx],
            }));
        }
        nodes
    }

    fn panel_event(_id: String, event: PanelEvent) -> Result<(), String> {
        if let PanelEvent::Clicked(control_id) = event
            && let Some(rest) = control_id.strip_prefix("seek:")
            && let Ok(clip_id) = rest.parse::<u64>()
        {
            let target = SHOTS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|s| s.clip_id == clip_id)
                .map(|s| s.midpoint);
            if let Some(midpoint) = target {
                editor_seek(midpoint)?;
            }
        }
        Ok(())
    }

    fn import(_id: String, _path: String) -> Result<ImportResult, String> {
        Err("unused".into())
    }
    fn export(_id: String, _project: String, _path: String) -> Result<Report, String> {
        Err("unused".into())
    }
    fn audio_create(_id: String, _rate: u32) -> Result<u64, String> {
        Err("unused".into())
    }
    fn audio_process(_i: u64, _p: Vec<AudioParam>, _b: AudioBlock) -> Result<StereoBuffers, String> {
        Err("unused".into())
    }
    fn audio_reset(_i: u64) -> Result<(), String> {
        Ok(())
    }
    fn audio_drop(_i: u64) -> Result<(), String> {
        Ok(())
    }
    fn overlay_scene(_id: String) -> Vec<OverlayShape> {
        Vec::new()
    }
    fn overlay_event(_id: String, _event: OverlayEvent) -> Result<(), String> {
        Ok(())
    }
    fn encoder_open(_id: String, _path: String, _c: EncoderConfig) -> Result<u64, String> {
        Err("unused".into())
    }
    fn encoder_push_frame(_i: u64, _f: EncoderFrame) -> Result<(), String> {
        Err("unused".into())
    }
    fn encoder_push_audio(_i: u64, _a: EncoderAudio) -> Result<(), String> {
        Err("unused".into())
    }
    fn encoder_finish(_i: u64) -> Result<(), String> {
        Err("unused".into())
    }
    fn encoder_drop(_i: u64) -> Result<(), String> {
        Ok(())
    }
    fn job_step(_job: u64) -> Result<JobStep, String> {
        Err("unused".into())
    }
    fn job_end(_job: u64, _cancelled: bool) {}
    fn decoder_probe(_id: String, _path: String) -> Result<MediaProbe, DecodeError> {
        Err(DecodeError::Unsupported("unused".into()))
    }
    fn decoder_open_video(_id: String, _path: String) -> Result<OpenedVideo, DecodeError> {
        Err(DecodeError::Unsupported("unused".into()))
    }
    fn decoder_seek(_i: u64, _pts: i64) -> Result<(), String> {
        Err("unused".into())
    }
    fn decoder_next_frame(_i: u64) -> Result<Option<DecodedFrame>, String> {
        Err("unused".into())
    }
    fn decoder_open_audio(
        _id: String,
        _path: String,
        _rate: u32,
        _ch: u16,
    ) -> Result<u64, DecodeError> {
        Err(DecodeError::Unsupported("unused".into()))
    }
    fn decoder_read_audio(_i: u64, _max: u32) -> Result<Vec<f32>, String> {
        Err("unused".into())
    }
    fn decoder_seek_audio(_i: u64, _frame: i64) -> Result<(), String> {
        Err("unused".into())
    }
    fn decoder_close(_i: u64) {}
    fn ai_tool_call(_name: String, _args: String) -> Result<AiToolOutput, String> {
        Err("unused".into())
    }
    fn scope_measure(_id: String, _frame: ScopeFrame) -> Result<ScopeDrawing, String> {
        Err("unused".into())
    }
    fn cpu_effect_process(
        _id: String,
        _params: Vec<f32>,
        _frame: VideoFrame,
    ) -> Result<VideoFrame, String> {
        Err("unused".into())
    }
    fn driver_evaluate(_id: String, _expr: String, _vars: DriverVars) -> Result<ParamValue, String> {
        Err("unused".into())
    }
    fn analyzer_open(_id: String, _rate: u32, _ch: u16) -> Result<u64, String> {
        Err("unused".into())
    }
    fn analyzer_push(_i: u64, _samples: Vec<f32>) -> Result<(), String> {
        Err("unused".into())
    }
    fn analyzer_finish(_i: u64) -> Result<String, String> {
        Err("unused".into())
    }
    fn analyzer_drop(_i: u64) {}
}

fn capture_selected_clips() -> Result<(), String> {
    let clips = selected_clips()?;
    let mut next = Vec::with_capacity(clips.len());
    for clip in clips {
        let midpoint = RationalTime {
            value: clip.start.value + (clip.end.value - clip.start.value) / 2,
            rate_num: clip.start.rate_num,
            rate_den: clip.start.rate_den,
        };
        let rendered = render_frame(
            FrameSource::Clip(clip.id),
            midpoint,
            FrameSize::Fit((160, 90)),
        )?;
        next.push(ShotEntry {
            clip_id: clip.id,
            name: clip.name,
            midpoint,
            width: rendered.width,
            height: rendered.height,
            data_url: format!("data:image/png;base64,{}", encode_rgba_png(&rendered)),
        });
    }
    *SHOTS.lock().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(())
}

fn encode_rgba_png(_frame: &RenderedFrame) -> String {
    String::new()
}

export!(ContactSheetPlugin);
```

## Recipe: Marker shuttle and review transport (`aria-sdk`)

This native plugin subscribes to `PlayheadMoved`, `PlaybackStarted`, and
`PlaybackStopped`, jumps the playhead to the next ruler marker with
`Host::markers_in_range` and `Host::editor_seek`, and starts or stops review
playback with `Host::editor_play` and `Host::editor_stop`.

### `plugin.toml`

```toml
[plugin]
id = "com.example.review-shuttle"
name = "Review Shuttle"
version = "0.1.0"
api = 1
description = "Jump to the next marker and run double-speed review playback"

[permissions]
project = "read-write"

[[commands]]
id = "next-marker-play"
title = "Review: Play From Next Marker"

[[commands]]
id = "halt"
title = "Review: Stop Playback"
enablement = "playing"
```

### `src/lib.rs`

```rust
use aria_sdk::{
    EventResponse, Host, LogLevel, Plugin, Rational, RationalTime, TimeRange, serde_json,
};

#[derive(Default)]
struct ReviewShuttle;

impl Plugin for ReviewShuttle {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.subscribe("playhead-moved")?;
        host.subscribe("playback-started")?;
        host.subscribe("playback-stopped")?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            "com.example.review-shuttle.next-marker-play" => {
                let view: serde_json::Value = host.editor_view()?;
                let current_val = view["playhead"]["value"].as_i64().unwrap_or(0);
                let rate_num = view["playhead"]["rate"]["num"].as_i32().unwrap_or(24);
                let rate_den = view["playhead"]["rate"]["den"].as_i32().unwrap_or(1);
                let rate = Rational::new(rate_num, rate_den);
                let search_range = TimeRange::new(
                    RationalTime::new(current_val + 1, rate),
                    RationalTime::new(i64::from(rate_num) * 3600, rate),
                );
                let markers = host.markers_in_range(None, search_range)?;
                if let Some(first) = markers.first() {
                    host.editor_seek(first.tick)?;
                }
                host.editor_play(2)
            }
            "com.example.review-shuttle.halt" => host.editor_stop(),
            _ => Err(format!("unknown command `{id}`")),
        }
    }

    fn event(&self, host: &Host, kind: &str, event_json: &str) -> Result<EventResponse, String> {
        host.log(LogLevel::Debug, &format!("{kind}: {event_json}"));
        Ok(EventResponse::Pass)
    }
}

aria_sdk::export!(ReviewShuttle);
```

## Recipe: Project checkpoint and session rotation (`aria-sdk` and in-process)

This native plugin saves the active project (`Host::project_save` or
`Host::project_save_as`) before opening a reference project
(`Host::project_open`), relying on `fs = ["project"]` in `plugin.toml` so paths
inside the active project folder pass `PackageGrants::check_fs_read` and
`PackageGrants::check_fs_write`.

### `plugin.toml`

```toml
[plugin]
id = "com.example.checkpoint"
name = "Project Checkpoint"
version = "0.1.0"
api = 1
description = "Save checkpoints and switch between project files"

[permissions]
project = "read-write"
fs = ["project"]

[[commands]]
id = "save-checkpoint"
title = "Checkpoint: Save Copy"

[[commands]]
id = "switch-project"
title = "Checkpoint: Save and Open Target"
```

### Native (`aria-sdk`)

```rust
use aria_sdk::{Host, LogLevel, Plugin, serde_json};

#[derive(Default)]
struct CheckpointPlugin;

impl Plugin for CheckpointPlugin {
    fn mount(&self, _host: &Host) -> Result<(), String> {
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, args_json: &str) -> Result<(), String> {
        match id {
            "com.example.checkpoint.save-checkpoint" => {
                let args: serde_json::Value =
                    serde_json::from_str(args_json).map_err(|e| e.to_string())?;
                if let Some(target) = args["path"].as_str() {
                    let saved = host.project_save_as(target)?;
                    host.log(LogLevel::Info, &format!("saved checkpoint to {saved}"));
                } else {
                    let saved = host.project_save()?;
                    host.log(LogLevel::Info, &format!("saved project to {saved}"));
                }
                Ok(())
            }
            "com.example.checkpoint.switch-project" => {
                let args: serde_json::Value =
                    serde_json::from_str(args_json).map_err(|e| e.to_string())?;
                let target = args["path"]
                    .as_str()
                    .ok_or_else(|| "missing `path` in command arguments".to_string())?;
                // Save unsaved edits first so `project_open` does not fail on a dirty session.
                let _ = host.project_save();
                host.project_open(target)
            }
            _ => Err(format!("unknown command `{id}`")),
        }
    }
}

aria_sdk::export!(CheckpointPlugin);
```

### In-process (`aria-plugin`)

```rust
use std::path::Path;
use aria_plugin::{Origin, PluginContext, ProjectHost};

pub fn save_and_open_in_process(
    ctx: &PluginContext,
    plugin_id: &'static str,
    next_project: &Path,
    fallback_save_path: &Path,
) -> Result<(), String> {
    let host = ctx.get::<ProjectHost>();
    let origin = Origin::plugin(plugin_id);
    if host.is_dirty() {
        if host.project_path().is_some() {
            host.save(&origin).map_err(|err| err.to_string())?;
        } else {
            host.save_as(&origin, fallback_save_path)
                .map_err(|err| err.to_string())?;
        }
    }
    host.open(&origin, next_project)
        .map_err(|err| err.to_string())
}
```
