//! Held tool keys, edge modifier tool switching, tool variants popover state,
//! Pen tool timeline clip-line keyframing, and Type tool canvas clip creation.

use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, ClipParam, Easing, Generator, ParamValue, RationalTime};

use super::AppState;
use crate::app::canvas_overlay::Rect;
use crate::types::{EditorTool, PenVariant};

/// Hold duration (ms) that distinguishes a temporary tool hold (or a tool-strip
/// press-and-hold that opens the variants popover) from a tap (`docs/spec/layout.md`).
pub const TOOL_HOLD_MS: u64 = 300;

/// One selectable entry in a tool button's variants popover (`docs/spec/layout.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVariant {
    Trim(EditorTool),
    Pen(PenVariant),
}

impl ToolVariant {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Trim(EditorTool::Ripple) => "Ripple Edit",
            Self::Trim(EditorTool::Roll) => "Rolling Edit",
            Self::Trim(EditorTool::Rate) => "Rate Stretch",
            Self::Trim(EditorTool::Slip) => "Slip",
            Self::Trim(EditorTool::Slide) => "Slide",
            Self::Trim(_) => "Tool",
            Self::Pen(variant) => variant.label(),
        }
    }

    #[must_use]
    pub const fn shortcut(self) -> &'static str {
        match self {
            Self::Trim(EditorTool::Ripple) => "B",
            Self::Trim(EditorTool::Roll) => "N",
            Self::Trim(EditorTool::Rate) => "R",
            Self::Trim(EditorTool::Slip) => "Y",
            Self::Trim(EditorTool::Slide) => "U",
            Self::Trim(_) => "",
            Self::Pen(PenVariant::Standard) => "P",
            Self::Pen(_) => "",
        }
    }
}

const TRIM_TOOL_VARIANTS: &[ToolVariant] = &[
    ToolVariant::Trim(EditorTool::Ripple),
    ToolVariant::Trim(EditorTool::Roll),
    ToolVariant::Trim(EditorTool::Rate),
    ToolVariant::Trim(EditorTool::Slip),
    ToolVariant::Trim(EditorTool::Slide),
];

const PEN_TOOL_VARIANTS: &[ToolVariant] = &[
    ToolVariant::Pen(PenVariant::Standard),
    ToolVariant::Pen(PenVariant::AddVertex),
    ToolVariant::Pen(PenVariant::DeleteVertex),
    ToolVariant::Pen(PenVariant::ConvertVertex),
];

impl EditorTool {
    /// Effective tool when hovering or dragging a clip edge with `ctrl` and `alt`
    /// held (`docs/spec/editing.md`). With the Selection tool (`Pointer`), `Ctrl`
    /// switches to Ripple Edit and `Ctrl+Alt` switches to Rolling Edit while held.
    #[must_use]
    pub const fn edge_tool(self, ctrl: bool, alt: bool) -> Self {
        if self.selects() {
            if ctrl && alt {
                return Self::Roll;
            }
            if ctrl {
                return Self::Ripple;
            }
        }
        self
    }

    /// Variants offered in the tool-strip popover for this tool (`docs/spec/layout.md`).
    #[must_use]
    pub const fn variants(self) -> &'static [ToolVariant] {
        match self {
            Self::Ripple | Self::Roll | Self::Rate | Self::Slip | Self::Slide => TRIM_TOOL_VARIANTS,
            Self::Pen => PEN_TOOL_VARIANTS,
            _ => &[],
        }
    }
}

/// Active key-hold record for a tool shortcut (`docs/spec/editing.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolHold {
    pub id: &'static str,
    pub key: &'static str,
    pub prev_tool: EditorTool,
    pub target_tool: EditorTool,
    pub pressed_at: Instant,
    pub held: bool,
}

/// Session state for held tool keys, edge modifier overrides, tool variants,
/// and Pen/Type canvas interactions.
#[derive(Debug, Clone, Default)]
pub struct ToolSession {
    pub hold: Option<ToolHold>,
    pub edge_hover: bool,
    pub edge_mod_prev: Option<EditorTool>,
    pub pen_variant: PenVariant,
    pub variants_open: Option<EditorTool>,
    pub press_target: Option<(EditorTool, u64)>,
    pub press_seq: u64,
    /// A title being typed into with the Type tool.
    pub text_edit: Option<super::text_caret::TextEdit>,
    /// A mask path the Pen is drawing, or a press on a mask path in flight.
    pub mask_path: Option<crate::app::mask_path::MaskPathEdit>,
    /// The mask the Viewer handles, the Pen, and the tracker edit, as an index
    /// into that clip's mask stack. Another clip's entry reads as its first
    /// mask.
    pub active_mask: Option<(aria_models::ClipId, u32)>,
}

impl AppState {
    #[must_use]
    pub fn pen_variant(&self) -> PenVariant {
        self.tool_session.pen_variant
    }

    pub fn set_pen_variant(&mut self, variant: PenVariant) {
        self.tool_session.pen_variant = variant;
    }

    #[must_use]
    pub fn tool_variants_open(&self) -> Option<EditorTool> {
        self.tool_session.variants_open
    }

    pub fn open_tool_variants(&mut self, tool: EditorTool) {
        if tool.has_variants() {
            self.tool_session.variants_open = Some(tool);
        }
    }

    pub fn close_tool_variants(&mut self) {
        self.tool_session.variants_open = None;
    }

    pub fn select_tool_variant(&mut self, variant: ToolVariant) {
        match variant {
            ToolVariant::Trim(tool) => {
                self.tool = tool;
            }
            ToolVariant::Pen(pv) => {
                self.tool = EditorTool::Pen;
                self.tool_session.pen_variant = pv;
            }
        }
        self.tool_session.variants_open = None;
    }

    /// Record the start of a left mouse-down on a tool-strip button and return
    /// its press sequence token for the 300 ms hold timer.
    pub fn begin_tool_press(&mut self, tool: EditorTool) -> u64 {
        self.tool_session.press_seq = self.tool_session.press_seq.wrapping_add(1);
        let seq = self.tool_session.press_seq;
        self.tool_session.press_target = Some((tool, seq));
        seq
    }

    /// Clear any in-flight tool-strip button press when the mouse is released.
    pub fn end_tool_press(&mut self) {
        self.tool_session.press_target = None;
    }

    /// Open the variants popover for `tool` if press `seq` is still held after 300 ms.
    pub fn hold_open_tool_variants(&mut self, tool: EditorTool, seq: u64) -> bool {
        if self.tool_session.press_target == Some((tool, seq)) && tool.has_variants() {
            self.tool_session.press_target = None;
            self.tool_session.variants_open = Some(tool);
            return true;
        }
        false
    }

    /// Handle a tool-key press (`is_held` is true on OS key auto-repeat).
    /// Holding a tool's key switches to it while down and returns to the
    /// previous tool on release; a quick tap switches permanently.
    pub fn tool_key_down(
        &mut self,
        id: &'static str,
        key: &'static str,
        is_held: bool,
        run: fn(&mut AppState),
    ) {
        if let Some(mut active) = self.tool_session.hold
            && active.id == id
        {
            active.held = true;
            self.tool_session.hold = Some(active);
            self.tool = active.target_tool;
            return;
        }
        let prev = self.tool_session.edge_mod_prev.take().unwrap_or(self.tool);
        run(self);
        self.tool_session.hold = Some(ToolHold {
            id,
            key,
            prev_tool: prev,
            target_tool: self.tool,
            pressed_at: Instant::now(),
            held: is_held,
        });
    }

    /// Mark an active tool-key press as having exceeded `TOOL_HOLD_MS`.
    pub fn mark_tool_hold_elapsed(&mut self) {
        if let Some(mut active) = self.tool_session.hold {
            active.pressed_at = Instant::now()
                .checked_sub(Duration::from_millis(TOOL_HOLD_MS + 50))
                .unwrap_or(active.pressed_at);
            active.held = true;
            self.tool_session.hold = Some(active);
        }
    }

    /// Handle a key-up event for a potentially held tool key. Returns `true`
    /// when releasing a held tool key restored the previous tool.
    pub fn tool_key_up(&mut self, id: Option<&str>, key: &str) -> bool {
        let Some(hold) = self.tool_session.hold else {
            return false;
        };
        let matches = id == Some(hold.id) || hold.key.eq_ignore_ascii_case(key);
        if !matches {
            return false;
        }
        self.tool_session.hold = None;
        let held_long_enough =
            hold.held || hold.pressed_at.elapsed() >= Duration::from_millis(TOOL_HOLD_MS);
        if held_long_enough && self.tool == hold.target_tool && hold.prev_tool != hold.target_tool {
            self.tool = hold.prev_tool;
            return true;
        }
        false
    }

    /// Update Selection-tool edge modifier override (`Ctrl` over an edge
    /// switches to Ripple Edit and `Ctrl+Alt` to Rolling Edit while held).
    pub fn update_edge_modifiers(&mut self, over_edge: bool, ctrl: bool, alt: bool) -> bool {
        self.tool_session.edge_hover = over_edge;
        let base = self.tool_session.edge_mod_prev.unwrap_or(self.tool);
        if over_edge && base.selects() && ctrl {
            let next = base.edge_tool(ctrl, alt);
            self.tool_session.edge_mod_prev = Some(base);
            if self.tool != next {
                self.tool = next;
                return true;
            }
            return false;
        }
        if let Some(prev) = self.tool_session.edge_mod_prev.take()
            && self.tool != prev
        {
            self.tool = prev;
            return true;
        }
        false
    }

    /// Pin `param`'s sampled value on `clip` at absolute timeline `tick`.
    pub fn add_param_keyframe_at(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        tick: i64,
    ) -> Result<(), EngineError> {
        self.select_clip(clip);
        let Some(model) = self.selected_model() else {
            return Ok(());
        };
        let clamped_tick = tick.clamp(
            model.timeline.start.value,
            (model.timeline.end_tick() - 1).max(model.timeline.start.value),
        );
        let Some(row) = self.key_param(param) else {
            return Ok(());
        };
        let Some(value) = row.spec.sample(model, model.param_tick(clamped_tick)) else {
            return Ok(());
        };
        let easing = row
            .spec
            .easing_at(model, model.param_tick(clamped_tick))
            .unwrap_or(Easing::Linear);
        let at = RationalTime::new(clamped_tick, self.store.sequence.frame_rate);
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at,
            value,
            easing,
            tangents: None,
        }))?;
        Ok(())
    }
    /// Whether a `Ctrl`+click at `at` on the Viewer's motion path may insert a
    /// keyframe (`docs/spec/animation.md`: only with the Pen tool).
    #[must_use]
    pub fn can_add_key_on_path(&self, ctrl: bool, at: [f32; 2]) -> bool {
        ctrl && self.tool.pens() && self.path_hit(at).is_some()
    }

    /// Add a keyframe on the property `clip`'s bar draws as its line (Volume
    /// on an audio lane and Opacity elsewhere, unless the clip's badge menu
    /// chose another) at absolute timeline `tick`. `None` when the bar draws
    /// no line.
    pub fn add_clip_line_keyframe(
        &mut self,
        clip: ClipId,
        tick: i64,
    ) -> Result<Option<ClipParam>, EngineError> {
        let Some(param) = self
            .ui_clip_ref(clip)
            .and_then(|ui| ui.line.as_ref())
            .map(|line| line.param)
        else {
            return Ok(None);
        };
        self.add_param_keyframe_at(clip, param, tick)?;
        Ok(Some(param))
    }

    /// Create a text clip at `point` (window pixels inside the Viewer canvas, or
    /// canvas pixels when no Viewer frame has been painted yet), select it, and
    /// start typing into it with its placeholder text selected.
    pub fn create_text_at(&mut self, point: [f32; 2]) -> Result<ClipId, EngineError> {
        const PLACEHOLDER: &str = "Title";
        let [cw, ch] = self.canvas_size();
        let canvas = self
            .overlay_frame
            .canvas
            .unwrap_or_else(|| Rect::new(0.0, 0.0, cw.max(1) as f32, ch.max(1) as f32));
        let [fx, fy] = canvas.fraction_of(point);
        let position = [fx - 0.5, fy - 0.5];
        let mut created = None;
        self.one_step(|state| {
            let clip = state.place_generated(
                Generator::text(PLACEHOLDER),
                None,
                state.playhead.value.max(0),
            )?;
            if position != [0.0, 0.0] {
                state.apply_command(Command::Edit(EditCommand::SetParamConstant {
                    clip,
                    param: ClipParam::Position,
                    value: ParamValue::Vec2(position),
                }))?;
            }
            created = Some(clip);
            Ok(())
        })?;
        let clip = created.expect("one_step created clip");
        self.select_clip(clip);
        self.tool_session.text_edit = Some(super::text_caret::TextEdit {
            anchor: 0,
            ..super::text_caret::TextEdit::at(clip, PLACEHOLDER.chars().count())
        });
        Ok(clip)
    }
}

#[cfg(test)]
#[path = "tool_hold_tests.rs"]
mod tests;
