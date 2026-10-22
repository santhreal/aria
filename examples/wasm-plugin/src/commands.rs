//! Command execution, event handling, and `.mrk` marker import and export.

use serde_json::{Value, json};

use crate::jobs::start_counting_job;
use crate::panel::{OVERLAY_STATE, OverlayMode, OverlayModel, PANEL_STATE, PanelModel};
use crate::{
    DEFAULT_MARKER_NAME, ImportResult, OverlaySpec, RationalTime, Report, TimeRange, clip,
    clips_at, clips_in_range, editor_view, fs_read, fs_write, in_out, markers_in_range,
    project_snapshot, project_transact, project_undo, refresh_panel, refresh_viewer_overlay,
    register_command, register_viewer_overlay, selected_clips, setting_get, storage_delete,
    storage_get, storage_keys, storage_set,
};

pub(crate) fn command_run(id: String, args_json: String) -> Result<(), String> {
    match id.as_str() {
        "example.add-marker" => run_add_marker(&args_json),
        "example.inspect-and-undo" => run_inspect_and_undo(),
        "example.query-project" => run_query_project(),
        "example.storage-roundtrip" => run_storage_roundtrip(),
        "example.infinite-loop" => spin_forever(),
        "example.eat-memory" => exhaust_memory(),
        "example.trap" => panic!("explicit guest trap requested"),
        "example.late-register" => register_command("example.illegal", "Illegal Late Registration"),
        "example.read-unpermitted-path" => {
            let target = if args_json.trim().is_empty() || args_json.trim() == "{}" {
                "/etc/hosts".to_owned()
            } else {
                serde_json::from_str::<String>(&args_json)
                    .unwrap_or_else(|_| args_json.trim().to_owned())
            };
            let _ = fs_read(&target)?;
            Ok(())
        }
        "example.refresh-panel" => {
            let mut guard = PANEL_STATE
                .lock()
                .map_err(|_| "panel state lock poisoned".to_owned())?;
            let state = guard.get_or_insert_with(PanelModel::default);
            state.presses += 10;
            drop(guard);
            refresh_panel("example.tools-panel")
        }
        "example.refresh-overlay" => {
            let mut guard = OVERLAY_STATE
                .lock()
                .map_err(|_| "overlay state lock poisoned".to_owned())?;
            let state = guard.get_or_insert_with(OverlayModel::default);
            state.refreshes += 1;
            drop(guard);
            refresh_viewer_overlay("example.safe-frame")
        }
        "example.late-register-overlay" => register_viewer_overlay(&OverlaySpec {
            id: "example.illegal-overlay".to_owned(),
            title: "Illegal Late Overlay".to_owned(),
        }),
        "example.trap-overlay-scene" => {
            let mut guard = OVERLAY_STATE
                .lock()
                .map_err(|_| "overlay state lock poisoned".to_owned())?;
            let state = guard.get_or_insert_with(OverlayModel::default);
            state.mode = OverlayMode::TrapScene;
            drop(guard);
            refresh_viewer_overlay("example.safe-frame")
        }
        "example.count-job" => start_counting_job(false),
        "example.endless-job" => start_counting_job(true),
        other => Err(format!("unknown command: {other}")),
    }
}

pub(crate) fn event(kind: String, event_json: String) -> Result<Option<String>, String> {
    let current = match storage_get("event-count")? {
        Some(bytes) => String::from_utf8_lossy(&bytes)
            .trim()
            .parse::<u64>()
            .unwrap_or(0),
        None => 0,
    };
    let next = current + 1;
    storage_set("event-count", next.to_string().as_bytes())?;
    storage_set(&format!("event-{kind}.json"), event_json.as_bytes())?;

    if kind == "command-proposed" && event_json.contains("veto-by-wasm") {
        return Ok(Some(json!({ "veto": "vetoed by wasm plugin" }).to_string()));
    }
    if (kind == "resolve-dropped" || kind == "resolve-dropped-path") && event_json.contains(".mrk")
    {
        return Ok(Some(json!("ImportMedia").to_string()));
    }
    if (kind == "resolve-dropped" || kind == "resolve-dropped-path")
        && event_json.contains("https://")
    {
        return Ok(Some(json!("Handled").to_string()));
    }
    Ok(None)
}

pub(crate) fn import(importer_id: String, path: String) -> Result<ImportResult, String> {
    if importer_id != "example.markers" {
        return Err(format!("unknown importer: {importer_id}"));
    }
    let bytes = fs_read(&path)?;
    let text = String::from_utf8(bytes).map_err(|err| format!("invalid UTF-8: {err}"))?;

    let mut commands = Vec::new();
    let mut skipped = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "INFINITE_LOOP" {
            spin_forever()?;
        }
        let mut parts = line.splitn(3, char::is_whitespace);
        let frame_str = parts.next().unwrap_or("");
        let color_str = parts.next().unwrap_or("");
        let name_str = parts.next().unwrap_or("").trim();

        let Ok(frame) = frame_str.parse::<i64>() else {
            skipped.push(line.to_owned());
            continue;
        };
        if !is_valid_marker_color(color_str) || name_str.is_empty() {
            skipped.push(line.to_owned());
            continue;
        }
        commands.push(json!({
            "type": "AddMarker",
            "at": {
                "value": frame,
                "rate": { "num": 24, "den": 1 }
            },
            "name": name_str,
            "color": color_str,
        }));
    }

    let commands_json = serde_json::to_string(&commands).map_err(|err| err.to_string())?;
    Ok(ImportResult {
        commands_json,
        skipped,
    })
}

pub(crate) fn export(
    exporter_id: String,
    project_json: String,
    path: String,
) -> Result<Report, String> {
    if exporter_id != "example.markers" {
        return Err(format!("unknown exporter: {exporter_id}"));
    }
    let project: Value = serde_json::from_str(&project_json).map_err(|err| err.to_string())?;
    if project.get("name").and_then(Value::as_str) == Some("INFINITE_LOOP") {
        spin_forever()?;
    }

    let mut out = String::new();
    if let Some(markers) = project
        .get("timeline")
        .and_then(|tl| tl.get("markers"))
        .and_then(Value::as_array)
    {
        for marker in markers {
            let frame = marker
                .get("tick")
                .and_then(|t| t.get("value"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let color = marker
                .get("color")
                .and_then(Value::as_str)
                .unwrap_or("teal");
            let name = marker.get("name").and_then(Value::as_str).unwrap_or("");
            out.push_str(&format!("{frame} {color} {name}\n"));
        }
    }

    let mut skipped = Vec::new();
    if let Some(tracks) = project
        .get("timeline")
        .and_then(|tl| tl.get("tracks"))
        .and_then(Value::as_array)
    {
        if !tracks.is_empty() {
            skipped.push(format!("{} tracks omitted by marker export", tracks.len()));
        }
    }

    fs_write(&path, out.as_bytes())?;
    Ok(Report { skipped })
}

pub(crate) fn run_add_marker(args_json: &str) -> Result<(), String> {
    let configured_color = match setting_get("example.marker_color")? {
        Some(raw) => serde_json::from_str::<String>(&raw).unwrap_or(raw),
        None => "teal".to_owned(),
    };
    let color = if is_valid_marker_color(&configured_color) {
        configured_color
    } else {
        "teal".to_owned()
    };

    let parsed_args: Value = if args_json.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(args_json).unwrap_or(Value::Null)
    };

    let frame = parsed_args
        .get("frame")
        .and_then(Value::as_i64)
        .unwrap_or(24);
    let name = parsed_args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_MARKER_NAME);

    let commands = json!([{
        "type": "AddMarker",
        "at": {
            "value": frame,
            "rate": { "num": 24, "den": 1 }
        },
        "name": name,
        "color": color,
    }]);
    let commands_json = serde_json::to_string(&commands).map_err(|err| err.to_string())?;
    let receipt = project_transact("Add WASM Marker", &commands_json)?;
    storage_set("last-receipt.json", receipt.as_bytes())?;
    Ok(())
}

fn run_inspect_and_undo() -> Result<(), String> {
    let snapshot_json = project_snapshot()?;
    let snapshot: Value = serde_json::from_str(&snapshot_json).map_err(|err| err.to_string())?;
    let view_json = editor_view()?;
    let view: Value = serde_json::from_str(&view_json).map_err(|err| err.to_string())?;
    let undid = project_undo()?;

    let summary = json!({
        "project_name": snapshot.get("name").and_then(Value::as_str).unwrap_or(""),
        "playhead": view.get("playhead"),
        "undid": undid,
    });
    storage_set("last-inspection.json", summary.to_string().as_bytes())?;
    Ok(())
}

fn run_query_project() -> Result<(), String> {
    let t0 = RationalTime {
        value: 0,
        rate_num: 24,
        rate_den: 1,
    };
    let t48 = RationalTime {
        value: 48,
        rate_num: 24,
        rate_den: 1,
    };
    let range = TimeRange {
        start: t0,
        duration: t48,
    };
    let in_range = clips_in_range(None, range)?;
    let first_by_id = if let Some(first) = in_range.first() {
        clip(first.id)?
    } else {
        None
    };
    let missing = clip(999_999)?;
    let at_12 = clips_at(
        None,
        RationalTime {
            value: 12,
            rate_num: 24,
            rate_den: 1,
        },
    )?;
    let at_48 = clips_at(None, t48)?;
    let selected = selected_clips()?;
    let markers = markers_in_range(None, range)?;
    let io = in_out()?;
    let summary = json!({
        "in_range_ids": in_range.iter().map(|c| c.id).collect::<Vec<_>>(),
        "first_by_id": first_by_id.as_ref().map(|c| json!({
            "id": c.id,
            "track": c.track,
            "start": c.start.value,
            "end": c.end.value,
            "kind": c.kind,
            "name": c.name,
        })),
        "missing_is_none": missing.is_none(),
        "at_12_ids": at_12.iter().map(|c| c.id).collect::<Vec<_>>(),
        "at_48_ids": at_48.iter().map(|c| c.id).collect::<Vec<_>>(),
        "selected_ids": selected.iter().map(|c| c.id).collect::<Vec<_>>(),
        "marker_names": markers.iter().map(|m| &m.name).collect::<Vec<_>>(),
        "mark_in": io.mark_in.map(|t| t.value),
        "mark_out": io.mark_out.map(|t| t.value),
    });
    storage_set("last-queries.json", summary.to_string().as_bytes())?;
    Ok(())
}

fn run_storage_roundtrip() -> Result<(), String> {
    storage_set("alpha", b"one")?;
    storage_set("beta", b"two")?;
    let alpha = storage_get("alpha")?.ok_or_else(|| "missing key alpha".to_owned())?;
    if alpha != b"one" {
        return Err(format!("unexpected alpha value: {alpha:?}"));
    }
    let keys = storage_keys()?;
    storage_delete("alpha")?;
    let deleted = storage_get("alpha")?;
    if deleted.is_some() {
        return Err("alpha still present after storage_delete".to_owned());
    }
    storage_set(
        "storage-check.json",
        json!({ "keys_before_delete": keys }).to_string().as_bytes(),
    )?;
    Ok(())
}

fn spin_forever() -> Result<(), String> {
    let mut acc = 1u64;
    loop {
        acc = acc.wrapping_mul(6364136223846793005).wrapping_add(1);
        std::hint::black_box(acc);
    }
}

fn exhaust_memory() -> Result<(), String> {
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for i in 0..64u8 {
        let block = vec![i ^ 0xA5; 1024 * 1024];
        std::hint::black_box(&block);
        chunks.push(block);
    }
    std::hint::black_box(&chunks);
    Ok(())
}

fn is_valid_marker_color(color: &str) -> bool {
    matches!(
        color,
        "teal" | "blue" | "purple" | "pink" | "red" | "orange" | "yellow" | "green"
    )
}
