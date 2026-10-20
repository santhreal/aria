//! Visual effects, CPU effects, generators, transitions, clip types, driver
//! languages, presets, themes, and workspaces registered by WASM components.

use std::sync::Arc;

use aria_compositor::{
    CpuEffect, CpuFrameIn, CpuFrameOut, EffectDefinition, EffectParams, TransitionDefinition,
};
use aria_models::{
    ClipTypeSpec, EffectBounds, EffectChoiceOption, EffectCurvePoint, EffectGradientStop,
    EffectGroupSpec, EffectParamDisplay, EffectParamKind, EffectParamSpec, EffectSpec,
    EffectVisibilityCondition, GraphPass, MarkerColor, OutputSize, ParamValue, PassInput,
    PassStage, TransitionSpec,
};
use aria_plugin::{
    Analyzer, AnalyzerResult, AnalyzerRun, AnalyzerScope, AnalyzerSpec, DriverInput,
    DriverLanguage, DriverLanguageSpec, DriverProgram, DriverScope, PluginContext, WorkspaceFocus,
    WorkspacePanes, WorkspaceSpec, host_wire,
};
use parking_lot::Mutex;

use crate::bindings;
use crate::host::WasmPluginShared;

#[path = "clip_type_schema.rs"]
mod clip_type_schema;

pub(crate) use clip_type_schema::define_clip_type;

fn host_param_spec(p: bindings::EffectParamSpec) -> EffectParamSpec {
    EffectParamSpec {
        name: p.name,
        label: p.label,
        kind: match p.kind {
            bindings::EffectParamKind::Scalar => EffectParamKind::Scalar,
            bindings::EffectParamKind::Int => EffectParamKind::Int,
            bindings::EffectParamKind::Boolean => EffectParamKind::Bool,
            bindings::EffectParamKind::Choice => EffectParamKind::Choice,
            bindings::EffectParamKind::Color => EffectParamKind::Color,
            bindings::EffectParamKind::Vec2 => EffectParamKind::Vec2,
            bindings::EffectParamKind::Point3 => EffectParamKind::Point3,
            bindings::EffectParamKind::Size => EffectParamKind::Size,
            bindings::EffectParamKind::Angle => EffectParamKind::Angle,
            bindings::EffectParamKind::Curve => EffectParamKind::Curve,
            bindings::EffectParamKind::Gradient => EffectParamKind::Gradient,
            bindings::EffectParamKind::Text => EffectParamKind::Text,
            bindings::EffectParamKind::Font => EffectParamKind::Font,
            bindings::EffectParamKind::File => EffectParamKind::File,
            bindings::EffectParamKind::Layer => EffectParamKind::Layer,
        },
        default: p.default_value,
        min: p.min,
        max: p.max,
        default_color: [
            p.default_color.0,
            p.default_color.1,
            p.default_color.2,
            p.default_color.3,
        ],
        default_vec2: [p.default_vec2.0, p.default_vec2.1],
        display: Some(match p.display {
            bindings::EffectParamDisplay::Slider => EffectParamDisplay::Slider,
            bindings::EffectParamDisplay::AngleDial => EffectParamDisplay::AngleDial,
            bindings::EffectParamDisplay::Percentage => EffectParamDisplay::Percentage,
        }),
        soft_min: p.soft_min,
        soft_max: p.soft_max,
        unit: p.unit,
        tooltip: p.tooltip,
        group: p.group,
        visible_when: p.visible_when.map(|v| {
            let equals = v
                .equals_choice
                .or_else(|| v.equals_bool.map(|b| b.to_string()))
                .or_else(|| v.equals_int.map(|i| i.to_string()))
                .unwrap_or_default();
            EffectVisibilityCondition {
                param: v.param,
                equals,
            }
        }),
        choices: p
            .choices
            .into_iter()
            .map(|c| EffectChoiceOption {
                id: c.id,
                label: c.label,
            })
            .collect(),
        default_int: i64::from(p.default_int),
        int_min: p.min.round() as i64,
        int_max: p.max.round() as i64,
        default_bool: p.default_bool,
        default_choice: (!p.default_choice.is_empty()).then_some(p.default_choice),
        default_linear_color: None,
        default_vec3: [p.default_vec3.0, p.default_vec3.1, p.default_vec3.2],
        default_curve: p
            .default_curve
            .into_iter()
            .map(|pt| EffectCurvePoint { x: pt.x, y: pt.y })
            .collect(),
        default_gradient: p
            .default_gradient
            .into_iter()
            .map(|st| EffectGradientStop {
                offset: st.position,
                color: [st.color.0, st.color.1, st.color.2, st.color.3],
            })
            .collect(),
        default_text: p.default_text,
    }
}

fn host_group_spec(group: bindings::EffectGroupSpec) -> EffectGroupSpec {
    EffectGroupSpec {
        id: group.id,
        label: group.label,
        collapsed: group.collapsed,
    }
}

pub(crate) fn host_effect_spec(
    spec: bindings::EffectSpec,
    force_generator: Option<bool>,
) -> EffectSpec {
    let params = spec.params.into_iter().map(host_param_spec).collect();
    let groups = spec.groups.into_iter().map(host_group_spec).collect();
    EffectSpec {
        id: spec.id,
        category: spec.category,
        label: spec.label,
        params,
        groups,
        generator: force_generator.unwrap_or(spec.generator),
        stateful: spec.stateful,
        bounds: aria_models::EffectBounds::ZERO,
        reads_outside: false,
        time_dependent: false,
    }
}

pub(crate) fn register_effect(
    ctx: &PluginContext,
    spec: bindings::EffectSpec,
    passes: Vec<String>,
) -> Result<(), String> {
    let spec = host_effect_spec(spec, Some(false));
    let passes = passes.into_iter().map(GraphPass::fragment).collect();
    ctx.register_effect(EffectDefinition { spec, passes })
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(crate) fn register_generator(
    ctx: &PluginContext,
    spec: bindings::EffectSpec,
    passes: Vec<String>,
) -> Result<(), String> {
    let spec = host_effect_spec(spec, Some(true));
    let passes = passes.into_iter().map(GraphPass::fragment).collect();
    ctx.register_effect(EffectDefinition { spec, passes })
        .map(drop)
        .map_err(|err| err.to_string())
}

fn host_graph_pass(pass: bindings::GraphPass) -> GraphPass {
    GraphPass {
        name: pass.name,
        stage: match pass.stage {
            bindings::PassStage::Fragment => PassStage::Fragment,
            bindings::PassStage::Compute((x, y)) => PassStage::Compute { workgroup: [x, y] },
        },
        inputs: pass
            .inputs
            .into_iter()
            .map(|input| match input {
                bindings::PassInput::Source => PassInput::Source,
                bindings::PassInput::Pass(name) => PassInput::Pass(name),
                bindings::PassInput::Layer(param) => PassInput::Layer(param),
                bindings::PassInput::Texture(id) => PassInput::Texture(id),
            })
            .collect(),
        output: match pass.output {
            bindings::OutputSize::Same => OutputSize::Same,
            bindings::OutputSize::Scale((x, y)) => OutputSize::Scale([x, y]),
            bindings::OutputSize::Fixed((w, h)) => OutputSize::Fixed([w, h]),
        },
        source: pass.wgsl,
    }
}

pub(crate) fn register_effect_graph(
    ctx: &PluginContext,
    spec: bindings::EffectSpec,
    traits: bindings::EffectTraits,
    passes: Vec<bindings::GraphPass>,
) -> Result<(), String> {
    let (left, top, right, bottom) = traits.bounds;
    let spec = EffectSpec {
        bounds: EffectBounds::new(left, top, right, bottom),
        reads_outside: traits.reads_outside,
        time_dependent: traits.time_dependent,
        ..host_effect_spec(spec, None)
    };
    let passes = passes.into_iter().map(host_graph_pass).collect();
    ctx.register_effect(EffectDefinition { spec, passes })
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(crate) fn register_texture(
    ctx: &PluginContext,
    id: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<(), String> {
    host_wire::register_rgba_texture(ctx, id, width, height, pixels)
}

pub(crate) fn register_cpu_effect(
    ctx: &PluginContext,
    shared: Arc<WasmPluginShared>,
    spec: bindings::EffectSpec,
) -> Result<(), String> {
    if !shared.has_parity_exports() {
        return Err("component does not export cpu-effect-process".to_owned());
    }
    let spec = host_effect_spec(spec, None);
    let effect = WasmCpuEffect::new(shared, ctx, spec.id.clone());
    ctx.register_cpu_effect(spec, effect)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(crate) fn register_transition(
    ctx: &PluginContext,
    spec: bindings::TransitionSpec,
    wgsl: String,
) -> Result<(), String> {
    ctx.register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: spec.id,
            label: spec.label,
            params: spec.params.into_iter().map(host_param_spec).collect(),
        },
        shader: wgsl,
    })
    .map(drop)
    .map_err(|err| err.to_string())
}

pub(crate) fn register_clip_type(
    ctx: &PluginContext,
    spec: bindings::ClipTypeSpec,
) -> Result<(), String> {
    ctx.register_clip_type(host_clip_type_spec(spec))
        .map(drop)
        .map_err(|err| err.to_string())
}

fn host_clip_type_spec(spec: bindings::ClipTypeSpec) -> ClipTypeSpec {
    let colour = match spec.colour {
        bindings::MarkerColor::Teal => MarkerColor::Teal,
        bindings::MarkerColor::Blue => MarkerColor::Blue,
        bindings::MarkerColor::Purple => MarkerColor::Purple,
        bindings::MarkerColor::Pink => MarkerColor::Pink,
        bindings::MarkerColor::Red => MarkerColor::Red,
        bindings::MarkerColor::Orange => MarkerColor::Orange,
        bindings::MarkerColor::Yellow => MarkerColor::Yellow,
        bindings::MarkerColor::Green => MarkerColor::Green,
    };
    let icon = host_wire::clip_type_icon(spec.icon_svg);
    ClipTypeSpec {
        id: spec.id,
        label: spec.label,
        generator: spec.generator,
        colour,
        icon,
        handles: spec.handles,
    }
}

pub(crate) fn register_driver_language(
    ctx: &PluginContext,
    shared: Arc<WasmPluginShared>,
    spec: bindings::DriverLanguageSpec,
) -> Result<(), String> {
    if !shared.has_parity_exports() {
        return Err("component does not export driver-evaluate".to_owned());
    }
    let language = WasmDriverLanguage {
        id: Arc::from(spec.id.as_str()),
        shared,
        ctx: ctx.clone(),
    };
    ctx.register_driver_language(
        DriverLanguageSpec {
            id: spec.id,
            name: spec.name,
        },
        language,
    )
    .map(drop)
    .map_err(|err| err.to_string())
}

pub(crate) fn register_preset(ctx: &PluginContext, preset_json: &str) -> Result<(), String> {
    host_wire::register_preset_json(ctx, preset_json)
}

pub(crate) fn register_theme(ctx: &PluginContext, id: &str, toml: &str) -> Result<(), String> {
    host_wire::register_theme_toml(ctx, id, toml)
}

pub(crate) fn register_workspace(
    ctx: &PluginContext,
    spec: bindings::WorkspaceSpec,
) -> Result<(), String> {
    let focus = spec.panes.focus.map(|f| match f {
        bindings::WorkspaceFocus::Media => WorkspaceFocus::Media,
        bindings::WorkspaceFocus::Preview => WorkspaceFocus::Preview,
        bindings::WorkspaceFocus::Timeline => WorkspaceFocus::Timeline,
        bindings::WorkspaceFocus::Inspector => WorkspaceFocus::Inspector,
        bindings::WorkspaceFocus::Assistant => WorkspaceFocus::Assistant,
        bindings::WorkspaceFocus::Mixer => WorkspaceFocus::Mixer,
    });
    ctx.register_workspace(WorkspaceSpec {
        id: spec.id,
        name: spec.name,
        panes: WorkspacePanes {
            sidebar_width: spec.panes.sidebar_width,
            sidebar_collapsed: spec.panes.sidebar_collapsed,
            inspector_width: spec.panes.inspector_width,
            inspector_collapsed: spec.panes.inspector_collapsed,
            timeline_height: spec.panes.timeline_height,
            scopes: spec.panes.scopes,
            focus,
            tree: None,
        },
    })
    .map(drop)
    .map_err(|err| err.to_string())
}

pub(crate) fn register_analyzer(
    ctx: &PluginContext,
    shared: Arc<WasmPluginShared>,
    spec: bindings::AnalyzerSpec,
) -> Result<(), String> {
    if !shared.has_parity_exports() {
        return Err(
            "component does not export analyzer-open/analyzer-push/analyzer-finish/analyzer-drop"
                .to_owned(),
        );
    }
    let scope = match spec.scope {
        bindings::AnalyzerScope::Clip => AnalyzerScope::Clip,
        bindings::AnalyzerScope::Sequence => AnalyzerScope::Sequence,
        bindings::AnalyzerScope::InOut => AnalyzerScope::InOut,
    };
    let host_spec = AnalyzerSpec {
        id: spec.id,
        label: spec.label,
        category: spec.category,
        scope,
    };
    let analyzer = WasmAnalyzer {
        id: Arc::from(host_spec.id.as_str()),
        shared,
        ctx: ctx.clone(),
    };
    ctx.register_analyzer(host_spec, analyzer)
        .map(drop)
        .map_err(|err| err.to_string())
}

struct WasmCpuEffect {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
    frame: Mutex<bindings::VideoFrame>,
}

impl WasmCpuEffect {
    /// The effect `id` drawn by the component's `cpu-effect-process`.
    fn new(shared: Arc<WasmPluginShared>, ctx: &PluginContext, id: String) -> Self {
        Self {
            shared,
            ctx: ctx.clone(),
            id,
            frame: Mutex::new(bindings::VideoFrame {
                width: 0,
                height: 0,
                time: 0.0,
                pixels: Vec::new(),
            }),
        }
    }
}

impl CpuEffect for WasmCpuEffect {
    fn process(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String> {
        let width = input.width();
        let height = input.height();
        let row_elems = (width as usize) * 4;
        let expected_len = (height as usize) * row_elems;
        let out_frame = {
            let mut held = self.frame.lock();
            held.width = width;
            held.height = height;
            held.time = time;
            host_wire::pack_cpu_frame_in(input, &mut held.pixels);
            self.shared
                .call_cpu_effect_process(&self.ctx, &self.id, params.values(), &held)?
        };
        if out_frame.pixels.len() != expected_len {
            return Err(format!(
                "CPU effect {} returned {} pixel channels; its {width}x{height} frame requires {expected_len} channels",
                self.id,
                out_frame.pixels.len()
            ));
        }
        host_wire::unpack_cpu_frame_out(&out_frame.pixels, output);
        Ok(())
    }
}

struct WasmDriverLanguage {
    id: Arc<str>,
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
}

impl DriverLanguage for WasmDriverLanguage {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        host_wire::validate_driver_source(source)?;
        Ok(Arc::new(WasmDriverProgram {
            language_id: Arc::clone(&self.id),
            expr: Arc::from(source),
            shared: Arc::clone(&self.shared),
            ctx: self.ctx.clone(),
        }))
    }
}

struct WasmDriverProgram {
    language_id: Arc<str>,
    expr: Arc<str>,
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
}

impl DriverProgram for WasmDriverProgram {
    fn inputs(&self) -> &[DriverInput] {
        &[]
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        let [canvas_width, canvas_height] = scope.canvas();
        let vars = bindings::DriverVars {
            time: scope.time(),
            clip_time: scope.clip_time(),
            frame_rate: scope.frame_rate(),
            duration: scope.duration(),
            canvas_width,
            canvas_height,
            value: to_wit_param_value(scope.value()),
            seed: scope.seed(),
        };
        let out =
            self.shared
                .call_driver_evaluate(&self.ctx, &self.language_id, &self.expr, vars)?;
        Ok(from_wit_param_value(out))
    }
}

fn to_wit_param_value(value: ParamValue) -> bindings::ParamValue {
    match value {
        ParamValue::Scalar(v) => bindings::ParamValue::Scalar(v),
        ParamValue::Vec2([x, y]) => bindings::ParamValue::Vec2((x, y)),
        ParamValue::Vec3([x, y, z]) => bindings::ParamValue::Vec3((x, y, z)),
        ParamValue::Color([r, g, b, a]) => bindings::ParamValue::Color((r, g, b, a)),
        ParamValue::Rect([x, y, w, h]) => bindings::ParamValue::Rect((x, y, w, h)),
    }
}

fn from_wit_param_value(value: bindings::ParamValue) -> ParamValue {
    match value {
        bindings::ParamValue::Scalar(v) => ParamValue::Scalar(v),
        bindings::ParamValue::Vec2((x, y)) => ParamValue::Vec2([x, y]),
        bindings::ParamValue::Vec3((x, y, z)) => ParamValue::Vec3([x, y, z]),
        bindings::ParamValue::Color((r, g, b, a)) => ParamValue::Color([r, g, b, a]),
        bindings::ParamValue::Rect((x, y, w, h)) => ParamValue::Rect([x, y, w, h]),
    }
}

struct WasmAnalyzer {
    id: Arc<str>,
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
}

impl Analyzer for WasmAnalyzer {
    fn begin(&self, sample_rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        match self
            .shared
            .call_analyzer_open(&self.ctx, &self.id, sample_rate, channels)
        {
            Ok(instance) => Box::new(WasmAnalyzerRun {
                instance: Some(instance),
                error: None,
                finished: false,
                shared: Arc::clone(&self.shared),
                ctx: self.ctx.clone(),
            }),
            Err(err) => Box::new(WasmAnalyzerRun {
                instance: None,
                error: Some(err),
                finished: true,
                shared: Arc::clone(&self.shared),
                ctx: self.ctx.clone(),
            }),
        }
    }
}

struct WasmAnalyzerRun {
    instance: Option<u64>,
    error: Option<String>,
    finished: bool,
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
}

impl AnalyzerRun for WasmAnalyzerRun {
    fn block(&mut self, _first_frame: i64, samples: &[f32]) -> std::ops::ControlFlow<()> {
        if self.error.is_some() {
            return std::ops::ControlFlow::Break(());
        }
        let Some(instance) = self.instance else {
            return std::ops::ControlFlow::Break(());
        };
        match self.shared.call_analyzer_push(&self.ctx, instance, samples) {
            Ok(()) => std::ops::ControlFlow::Continue(()),
            Err(err) => {
                self.error = Some(err);
                std::ops::ControlFlow::Break(())
            }
        }
    }

    fn finish(mut self: Box<Self>) -> Result<AnalyzerResult, String> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        self.finished = true;
        let Some(instance) = self.instance else {
            return Err("analyzer session was not opened".to_owned());
        };
        let json = self.shared.call_analyzer_finish(&self.ctx, instance)?;
        host_wire::parse_analyzer_result(&json)
    }
}

impl Drop for WasmAnalyzerRun {
    fn drop(&mut self) {
        if !self.finished
            && let Some(instance) = self.instance
        {
            self.shared.call_analyzer_drop(&self.ctx, instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_wit_param(name: &str, kind: bindings::EffectParamKind) -> bindings::EffectParamSpec {
        bindings::EffectParamSpec {
            name: name.into(),
            label: name.into(),
            kind,
            default_value: 0.5,
            min: 0.0,
            max: 1.0,
            default_color: (10, 20, 30, 255),
            default_vec2: (0.1, 0.2),
            display: bindings::EffectParamDisplay::Slider,
            soft_min: Some(0.1),
            soft_max: Some(0.9),
            unit: Some("px".into()),
            tooltip: Some("tip".into()),
            group: Some("g1".into()),
            visible_when: Some(bindings::EffectVisibilityCondition {
                param: "flag".into(),
                equals_bool: Some(true),
                equals_int: None,
                equals_choice: None,
            }),
            choices: vec![bindings::EffectChoiceOption {
                id: "a".into(),
                label: "A".into(),
            }],
            default_int: 4,
            default_bool: true,
            default_choice: "a".into(),
            default_vec3: (0.1, 0.2, 0.3),
            default_curve: vec![
                bindings::EffectCurvePoint { x: 0.0, y: 0.0 },
                bindings::EffectCurvePoint { x: 1.0, y: 1.0 },
            ],
            default_gradient: vec![
                bindings::EffectGradientStop {
                    position: 0.0,
                    color: (0.0, 0.0, 0.0, 1.0),
                },
                bindings::EffectGradientStop {
                    position: 1.0,
                    color: (1.0, 1.0, 1.0, 1.0),
                },
            ],
            default_text: "hello".into(),
        }
    }

    #[test]
    fn wit_effect_spec_converts_all_15_param_kinds_and_groups() {
        let kinds = [
            (bindings::EffectParamKind::Scalar, EffectParamKind::Scalar),
            (bindings::EffectParamKind::Int, EffectParamKind::Int),
            (bindings::EffectParamKind::Boolean, EffectParamKind::Bool),
            (bindings::EffectParamKind::Choice, EffectParamKind::Choice),
            (bindings::EffectParamKind::Color, EffectParamKind::Color),
            (bindings::EffectParamKind::Vec2, EffectParamKind::Vec2),
            (bindings::EffectParamKind::Point3, EffectParamKind::Point3),
            (bindings::EffectParamKind::Size, EffectParamKind::Size),
            (bindings::EffectParamKind::Angle, EffectParamKind::Angle),
            (bindings::EffectParamKind::Curve, EffectParamKind::Curve),
            (
                bindings::EffectParamKind::Gradient,
                EffectParamKind::Gradient,
            ),
            (bindings::EffectParamKind::Text, EffectParamKind::Text),
            (bindings::EffectParamKind::Font, EffectParamKind::Font),
            (bindings::EffectParamKind::File, EffectParamKind::File),
            (bindings::EffectParamKind::Layer, EffectParamKind::Layer),
        ];
        let params = kinds
            .iter()
            .enumerate()
            .map(|(i, (wk, _))| base_wit_param(&format!("p{i}"), *wk))
            .collect();
        let host = host_effect_spec(
            bindings::EffectSpec {
                id: "wasm.test.v2".into(),
                label: "WASM V2".into(),
                category: "Stylize".into(),
                params,
                groups: vec![bindings::EffectGroupSpec {
                    id: "g1".into(),
                    label: "Group 1".into(),
                    collapsed: true,
                }],
                generator: false,
                stateful: false,
            },
            None,
        );
        assert_eq!(host.params.len(), 15);
        for ((_, expected), actual) in kinds.iter().zip(&host.params) {
            assert_eq!(actual.kind, *expected);
            assert_eq!(
                actual
                    .visible_when
                    .as_ref()
                    .map(|v| (v.param.as_str(), v.equals.as_str())),
                Some(("flag", "true"))
            );
        }
        assert_eq!(host.groups.len(), 1);
        assert!(host.groups[0].collapsed);
    }
}
