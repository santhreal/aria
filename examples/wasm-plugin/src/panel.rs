//! Declarative sidebar tools panel and safe-frame viewer overlay.

use std::sync::Mutex;

use serde_json::Value;

use crate::{
    DragPhase, HandleLook, OverlayColor, OverlayEvent, OverlayHandle, OverlayLine, OverlayRect,
    OverlayShape, OverlayText, PanelEvent, Point, UiButton, UiInput, UiNode, UiSlider,
    project_snapshot,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelMode {
    Normal,
    MalformedList,
    TrapView,
}

pub(crate) struct PanelModel {
    pub(crate) presses: u32,
    level: f64,
    note: String,
    mode: PanelMode,
}

impl Default for PanelModel {
    fn default() -> Self {
        Self {
            presses: 0,
            level: 0.5,
            note: "ready".to_owned(),
            mode: PanelMode::Normal,
        }
    }
}

pub(crate) static PANEL_STATE: Mutex<Option<PanelModel>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayMode {
    Normal,
    TrapScene,
}

pub(crate) struct OverlayModel {
    focus_x: f32,
    focus_y: f32,
    drags: u32,
    pub(crate) refreshes: u32,
    pub(crate) mode: OverlayMode,
}

impl Default for OverlayModel {
    fn default() -> Self {
        Self {
            focus_x: 960.0,
            focus_y: 540.0,
            drags: 0,
            refreshes: 0,
            mode: OverlayMode::Normal,
        }
    }
}

pub(crate) static OVERLAY_STATE: Mutex<Option<OverlayModel>> = Mutex::new(None);

pub(crate) fn panel_view(id: String) -> Vec<UiNode> {
    if id != "example.tools-panel" {
        return Vec::new();
    }
    let mut guard = PANEL_STATE.lock().expect("panel state lock");
    let state = guard.get_or_insert_with(PanelModel::default);
    match state.mode {
        PanelMode::TrapView => panic!("explicit panel-view trap"),
        PanelMode::MalformedList => {
            state.mode = PanelMode::Normal;
            return vec![UiNode::Column(vec![99])];
        }
        PanelMode::Normal => {}
    }

    vec![
        UiNode::Column(vec![1, 2, 3, 4]),
        UiNode::Heading(format!("Tools ({})", state.presses)),
        UiNode::Button(UiButton {
            id: "bump".to_owned(),
            label: format!("Count: {}", state.presses),
            disabled: false,
        }),
        UiNode::Slider(UiSlider {
            id: "level".to_owned(),
            label: "Level".to_owned(),
            value: state.level,
            min: 0.0,
            max: 1.0,
            step: 0.25,
        }),
        UiNode::Input(UiInput {
            id: "note".to_owned(),
            value: state.note.clone(),
            placeholder: "Enter note".to_owned(),
        }),
    ]
}

pub(crate) fn panel_event(id: String, event: PanelEvent) -> Result<(), String> {
    if id != "example.tools-panel" {
        return Err(format!("unknown panel: {id}"));
    }
    let mut guard = PANEL_STATE
        .lock()
        .map_err(|_| "panel state lock poisoned".to_owned())?;
    let state = guard.get_or_insert_with(PanelModel::default);
    match event {
        PanelEvent::Pressed(control_id) if control_id == "bump" => {
            state.presses += 1;
            Ok(())
        }
        PanelEvent::Slid(slid) if slid.id == "level" => {
            state.level = slid.value;
            Ok(())
        }
        PanelEvent::Submitted(sub) if sub.id == "note" => match sub.text.as_str() {
            "ERROR" => Err("note rejected by guest".to_owned()),
            "MALFORMED" => {
                state.mode = PanelMode::MalformedList;
                Ok(())
            }
            "TRAP_VIEW" => {
                state.mode = PanelMode::TrapView;
                Ok(())
            }
            "TRAP_EVENT" => panic!("explicit panel-event trap"),
            _ => {
                state.note = sub.text;
                Ok(())
            }
        },
        other => Err(format!("unhandled panel event: {other:?}")),
    }
}

pub(crate) fn overlay_scene(id: String) -> Vec<OverlayShape> {
    if id != "example.safe-frame" {
        return Vec::new();
    }
    let guard = OVERLAY_STATE.lock().expect("overlay state lock");
    let state = guard.as_ref().cloned().unwrap_or_default();
    drop(guard);
    if state.mode == OverlayMode::TrapScene {
        panic!("explicit overlay-scene trap");
    }
    let (frame_w, frame_h) = sequence_frame_size();
    let fx = state.focus_x;
    let fy = state.focus_y;
    vec![
        OverlayShape::Rect(OverlayRect {
            origin: Point {
                x: frame_w * 0.05,
                y: frame_h * 0.05,
            },
            size: Point {
                x: frame_w * 0.90,
                y: frame_h * 0.90,
            },
            color: OverlayColor::Muted,
            width: 1.0,
            fill: None,
        }),
        OverlayShape::Rect(OverlayRect {
            origin: Point {
                x: frame_w * 0.10,
                y: frame_h * 0.10,
            },
            size: Point {
                x: frame_w * 0.80,
                y: frame_h * 0.80,
            },
            color: OverlayColor::Text,
            width: 1.0,
            fill: None,
        }),
        OverlayShape::Line(OverlayLine {
            from: Point {
                x: fx - 24.0,
                y: fy,
            },
            to: Point {
                x: fx + 24.0,
                y: fy,
            },
            color: OverlayColor::Accent,
            width: 1.5,
        }),
        OverlayShape::Line(OverlayLine {
            from: Point {
                x: fx,
                y: fy - 24.0,
            },
            to: Point {
                x: fx,
                y: fy + 24.0,
            },
            color: OverlayColor::Accent,
            width: 1.5,
        }),
        OverlayShape::Text(OverlayText {
            at: Point {
                x: fx + 10.0,
                y: fy - 10.0,
            },
            text: format!(
                "focus ({fx:.0}, {fy:.0}) drags={} refreshes={}",
                state.drags, state.refreshes
            ),
            color: OverlayColor::Text,
            size: 12.0,
        }),
        OverlayShape::Handle(OverlayHandle {
            id: "focus".to_owned(),
            at: Point { x: fx, y: fy },
            color: OverlayColor::Accent,
            look: HandleLook::Cross,
        }),
    ]
}

pub(crate) fn overlay_event(id: String, event: OverlayEvent) -> Result<(), String> {
    if id != "example.safe-frame" {
        return Err(format!("unknown overlay: {id}"));
    }
    let mut guard = OVERLAY_STATE
        .lock()
        .map_err(|_| "overlay state lock poisoned".to_owned())?;
    let state = guard.get_or_insert_with(OverlayModel::default);
    match event {
        OverlayEvent::Drag(drag) if drag.handle == "focus" => {
            state.focus_x = drag.at.x;
            state.focus_y = drag.at.y;
            if drag.phase == DragPhase::End {
                state.drags += 1;
            }
            Ok(())
        }
        OverlayEvent::Drag(drag) => Err(format!("unknown overlay handle: {}", drag.handle)),
    }
}

fn sequence_frame_size() -> (f32, f32) {
    let Ok(snapshot_json) = project_snapshot() else {
        return (1920.0, 1080.0);
    };
    let Ok(doc) = serde_json::from_str::<Value>(&snapshot_json) else {
        return (1920.0, 1080.0);
    };
    if let Some(aspect) = doc
        .get("timeline")
        .and_then(|tl| tl.get("canvas"))
        .and_then(|c| c.get("aspect"))
        .and_then(Value::as_str)
    {
        match aspect {
            "16:9" => return (1920.0, 1080.0),
            "9:16" => return (1080.0, 1920.0),
            "1:1" => return (1080.0, 1080.0),
            "4:5" => return (1080.0, 1350.0),
            "21:9" => return (2520.0, 1080.0),
            _ => {}
        }
    }
    if let Some(media_entries) = doc.get("media").and_then(Value::as_array) {
        let mut best: Option<(u32, u32)> = None;
        for entry in media_entries {
            let item = entry.get(1).unwrap_or(entry);
            let width = item.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            let height = item.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
            if width > 0 && height > 0 {
                let area = u64::from(width) * u64::from(height);
                if best.is_none_or(|(bw, bh)| area > u64::from(bw) * u64::from(bh)) {
                    best = Some((width, height));
                }
            }
        }
        if let Some((w, h)) = best {
            return (w as f32, h as f32);
        }
    }
    (1920.0, 1080.0)
}

impl Clone for OverlayModel {
    fn clone(&self) -> Self {
        Self {
            focus_x: self.focus_x,
            focus_y: self.focus_y,
            drags: self.drags,
            refreshes: self.refreshes,
            mode: self.mode,
        }
    }
}
