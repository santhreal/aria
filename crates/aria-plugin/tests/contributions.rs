//! What a host lists for each plugin: every contribution it registered, by
//! kind and id.
//!
//! Its own test binary: the effect, transition and audio effect catalogs are
//! process-wide, and every id here is used by no other test.
//!
//! The class this closes: a registration path that adds something without
//! labelling it, so the Plugins list shows a plugin providing nothing while its
//! command sits in the palette. The plugin below registers one of every
//! [`ContributionKind::ALL`] through an exhaustive match, so a new kind does not
//! compile until it is registered here. It does not catch a second, unlabelled
//! registration path added beside an existing one.

use std::io::Write;
use std::path::Path;

use aria_plugin::{
    AiToolOutput, AiToolSpec, AiToolTier, AnalyzerResult, AnalyzerRun, AnalyzerScope, AnalyzerSpec,
    AudioBlock, AudioEffectSpec, AudioProcessor, Chord, ClipTypeSpec, CommandSpec, Contribution,
    ContributionKind, DecodeError, DecoderSpec, DriverInput, DriverLanguage, DriverLanguageSpec,
    DriverProgram, DriverScope, EffectParamKind, EffectParamSpec, EffectSpec, EncodeError, Encoder,
    EncoderConfig, EncoderSpec, Exporter, ExporterSpec, ExternalDecoder, Importer, ImporterSpec,
    InterchangeReport, KeyDefault, KeyRoute, MarkerColor, MediaProbe, MenuCommand, MenuPath,
    OverlayEvent, OverlayScene, OverlaySpec, PanelEvent, PanelLocation, PanelSpec, PanelView,
    ParamValue, Plugin, PluginContext, PluginError, PluginManifest, Preset, Project, RgbaImage,
    ScopeFrame, ScopeImage, ScopeSpec, SettingKind, SettingSpec, SettingsSchema, SettingsSection,
    TransactError, Transaction, TransitionSpec, UiNode, VideoDecoder, VideoEncoder, VideoFrame,
    ViewerOverlay, WorkspacePanes, WorkspaceSpec,
};

const PLUGIN: &str = "test.contributions";

struct Quiet;

impl AudioProcessor for Quiet {
    fn process(&mut self, _block: &mut AudioBlock<'_>) {}

    fn reset(&mut self) {}
}

struct NullAnalyzerRun;

impl AnalyzerRun for NullAnalyzerRun {
    fn block(&mut self, _first_frame: i64, _samples: &[f32]) -> std::ops::ControlFlow<()> {
        std::ops::ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        Ok(AnalyzerResult::default())
    }
}

struct Lanes;

impl Importer for Lanes {
    fn import(
        &self,
        _file: &Path,
        _tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        Ok(InterchangeReport::default())
    }
}

impl Exporter for Lanes {
    fn export(
        &self,
        _project: &Project,
        _out: &mut dyn Write,
    ) -> std::io::Result<InterchangeReport> {
        Ok(InterchangeReport::default())
    }
}

/// A driver language whose every program returns the property's own value.
struct Constant;

impl DriverLanguage for Constant {
    fn compile(&self, _source: &str) -> Result<std::sync::Arc<dyn DriverProgram>, String> {
        Ok(std::sync::Arc::new(Constant))
    }
}

impl DriverProgram for Constant {
    fn inputs(&self) -> &[DriverInput] {
        &[]
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        Ok(scope.value())
    }
}

struct NullEncoder;

impl VideoEncoder for NullEncoder {
    fn push(&mut self, _frame: &VideoFrame) -> Result<(), EncodeError> {
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }

    fn choice(&self) -> aria_core::EncoderChoice {
        aria_core::EncoderChoice::only("null", None)
    }
}

impl Encoder for NullEncoder {
    fn open(
        &self,
        _path: &Path,
        _config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        Ok(Box::new(NullEncoder))
    }
}

struct NullDecoder;

impl ExternalDecoder for NullDecoder {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        Err(DecodeError::unsupported("reads nothing"))
    }

    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        Err(DecodeError::unsupported("reads nothing"))
    }
}

struct Blank;

impl PanelView for Blank {
    fn view(&self, _: &PluginContext) -> UiNode {
        UiNode::heading("Blank")
    }

    fn event(&self, _: &PluginContext, _: &PanelEvent) -> Result<(), String> {
        Ok(())
    }
}

impl ViewerOverlay for Blank {
    fn scene(&self, _: &PluginContext) -> OverlayScene {
        OverlayScene::default()
    }

    fn event(&self, _: &PluginContext, _: &OverlayEvent) -> Result<(), String> {
        Ok(())
    }
}

fn gain() -> EffectParamSpec {
    EffectParamSpec {
        name: "gain".into(),
        label: "Gain".into(),
        kind: EffectParamKind::Scalar,
        default: 1.0,
        min: 0.0,
        max: 2.0,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
        ..Default::default()
    }
}

/// The id this test registers `kind` under.
fn id_of(kind: ContributionKind) -> String {
    format!("{PLUGIN}.{}", kind.label().replace(' ', "_"))
}

/// What the Plugins list shows for the `kind` [`register`] adds: a menu row or
/// a key is listed by its command and where it is offered.
fn listed_as(kind: ContributionKind) -> String {
    match kind {
        ContributionKind::Menu => format!("{} in Sequence/Tools", id_of(kind)),
        ContributionKind::Key => format!("Ctrl+Alt+L for {}", id_of(kind)),
        ContributionKind::AiTool => "plugin_contributions_ai_tool".to_owned(),
        _ => id_of(kind),
    }
}

/// Register one contribution of `kind` under [`id_of`].
fn register(ctx: &PluginContext, kind: ContributionKind) -> Result<(), PluginError> {
    let id = id_of(kind);
    match kind {
        ContributionKind::Command => {
            ctx.register_command(CommandSpec::new(id, "Command"), |_: &PluginContext| Ok(()))?;
        }
        ContributionKind::Menu => {
            ctx.register_menu_entry(MenuCommand {
                command: id,
                path: MenuPath::parse("Sequence/Tools").expect("a menu path"),
            })?;
        }
        ContributionKind::Key => {
            ctx.register_key_default(KeyDefault {
                command: id,
                chord: Chord::ctrl_alt("l"),
                route: KeyRoute::Editor,
            })?;
        }
        ContributionKind::Settings => {
            ctx.register_settings(SettingsSection {
                plugin: PLUGIN.into(),
                title: id.clone(),
                fields: vec![SettingSpec {
                    key: format!("{id}.token"),
                    label: "Token".into(),
                    placeholder: String::new(),
                    kind: SettingKind::Secret,
                    default: None,
                }],
            })?;
        }
        ContributionKind::Panel => {
            ctx.register_panel(
                PanelSpec {
                    id,
                    title: "Panel".into(),
                    location: PanelLocation::Sidebar,
                },
                Blank,
            )?;
        }
        ContributionKind::Overlay => {
            ctx.register_viewer_overlay(
                OverlaySpec {
                    id,
                    title: "Overlay".into(),
                },
                Blank,
            )?;
        }
        ContributionKind::Importer => {
            ctx.register_importer(
                ImporterSpec {
                    id,
                    name: "Lanes".into(),
                    extensions: vec!["lanes".into()],
                },
                Lanes,
            )?;
        }
        ContributionKind::Exporter => {
            ctx.register_exporter(
                ExporterSpec {
                    id,
                    name: "Lanes".into(),
                    extension: "lanes".into(),
                },
                Lanes,
            )?;
        }
        ContributionKind::Encoder => {
            ctx.register_encoder(
                EncoderSpec {
                    id,
                    name: "Null".into(),
                    extension: "null".into(),
                    audio: true,
                },
                NullEncoder,
            )?;
        }
        ContributionKind::Decoder => {
            ctx.register_decoder(
                DecoderSpec {
                    id,
                    name: "Null".into(),
                    extensions: vec!["nulldecoded".into()],
                    priority: 0,
                },
                NullDecoder,
            )?;
        }
        ContributionKind::DriverLanguage => {
            ctx.register_driver_language(
                DriverLanguageSpec {
                    id,
                    name: "Constant".into(),
                },
                Constant,
            )?;
        }
        ContributionKind::Effect => {
            ctx.register_effect(aria_compositor::EffectDefinition {
                spec: EffectSpec {
                    id,
                    category: String::new(),
                    label: "Gain".into(),
                    params: vec![gain()],
                    generator: false,
                    stateful: false,
                    ..Default::default()
                },
                passes: vec![
                    "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv) * param_gain();
}"
                    .into(),
                ],
            })?;
        }
        ContributionKind::Transition => {
            ctx.register_transition(aria_compositor::TransitionDefinition {
                spec: TransitionSpec {
                    id,
                    label: "Mix".into(),
                    ..Default::default()
                },
                shader: "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    return mix(a, b, aria.progress);
}"
                .into(),
            })?;
        }
        ContributionKind::AudioEffect => {
            ctx.register_audio_effect(
                AudioEffectSpec {
                    id,
                    category: String::new(),
                    label: "Quiet".into(),
                    params: vec![gain()],
                    latency: 0,
                    sidechain: false,
                },
                |_rate: u32| -> Box<dyn AudioProcessor> { Box::new(Quiet) },
            )?;
        }
        ContributionKind::Analyzer => {
            ctx.register_analyzer(
                AnalyzerSpec::new(id, "Analyzer", AnalyzerScope::Clip),
                |_rate: u32, _channels: u16| -> Box<dyn AnalyzerRun> { Box::new(NullAnalyzerRun) },
            )?;
        }
        ContributionKind::Preset => {
            ctx.register_preset(Preset::new(id, Vec::new(), Vec::new(), Vec::new()))?;
        }
        ContributionKind::Theme => {
            ctx.register_theme(&id, "[dark]\naccent = \"#336699\"\n")?;
        }
        ContributionKind::AiTool => {
            ctx.register_ai_tool(
                AiToolSpec {
                    name: "contributions_ai_tool".to_owned(),
                    description: "Does nothing.".to_owned(),
                    parameters: serde_json::json!({ "type": "object" }),
                    tier: AiToolTier::ReadOnly,
                },
                |_: &PluginContext,
                 _: &serde_json::Value,
                 _: &std::sync::atomic::AtomicBool|
                 -> Result<AiToolOutput, String> { Ok(AiToolOutput::default()) },
            )?;
        }
        ContributionKind::Scope => {
            ctx.register_scope(
                ScopeSpec {
                    id,
                    name: "Scope".into(),
                    width: 8,
                    height: 8,
                },
                |_: &PluginContext, _: &ScopeFrame<'_>, _: &mut ScopeImage| -> Result<(), String> {
                    Ok(())
                },
            )?;
        }
        ContributionKind::Workspace => {
            ctx.register_workspace(WorkspaceSpec {
                id,
                name: "Workspace".into(),
                panes: WorkspacePanes::default(),
            })?;
        }
        ContributionKind::ClipType => {
            ctx.register_clip_type(ClipTypeSpec::new(
                id,
                "Clip type",
                id_of(ContributionKind::Effect),
                MarkerColor::Teal,
            ))?;
        }
        ContributionKind::Texture => {
            ctx.register_texture(id, &RgbaImage::new(1, 1, vec![255; 4]))?;
        }
    }
    Ok(())
}

/// Registers one of every kind while it mounts.
struct Everything(PluginManifest);

impl Plugin for Everything {
    fn manifest(&self) -> &PluginManifest {
        &self.0
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        for kind in ContributionKind::ALL {
            register(ctx, kind)?;
        }
        Ok(())
    }
}

#[test]
fn a_mounted_plugin_lists_every_contribution_it_registered_in_order() {
    let ctx = PluginContext::new();
    let schema = std::sync::Arc::new(SettingsSchema::default());
    let _settings_host = ctx.register_arc(std::sync::Arc::clone(&schema));
    ctx.mount(&Everything(PluginManifest::new(
        PLUGIN,
        "Everything",
        "1.0.0",
        &[],
        &[],
    )))
    .expect("mounts");
    let mounted = ctx.mounted();
    let info = mounted
        .iter()
        .find(|info| info.manifest.id == PLUGIN)
        .expect("listed");
    let expected: Vec<Contribution> = ContributionKind::ALL
        .into_iter()
        .map(|kind| Contribution {
            kind,
            id: listed_as(kind),
        })
        .collect();
    assert_eq!(info.contributions, expected);
    assert!(
        info.services.is_empty(),
        "a contribution is not a service: {:?}",
        info.services
    );

    ctx.unmount(PLUGIN).expect("unmounts");
    assert!(ctx.mounted().iter().all(|info| info.manifest.id != PLUGIN));
    assert!(ctx.commands().is_empty(), "the command outlived its plugin");
    assert!(ctx.panels().is_empty(), "the panel outlived its plugin");
    assert!(ctx.encoders().is_empty(), "the encoder outlived its plugin");
    assert!(ctx.decoders().is_empty(), "the decoder outlived its plugin");
    assert!(
        ctx.driver_languages().is_empty(),
        "the driver language outlived its plugin"
    );
    assert!(
        !aria_render::driver_languages().contains(&id_of(ContributionKind::DriverLanguage)),
        "the driver language is still installed"
    );
    assert!(
        !aria_decoder::external_decoder_extensions().contains(&"nulldecoded".to_owned()),
        "the decoder still claims its extension"
    );
    assert!(
        ctx.menu_entries().is_empty(),
        "the menu row outlived its plugin"
    );
    assert!(ctx.key_defaults().is_empty(), "the key outlived its plugin");
    assert!(ctx.ai_tools().is_empty(), "the AI tool outlived its plugin");
    assert!(
        ctx.analyzers().is_empty(),
        "the analyzer outlived its plugin"
    );
    assert!(
        schema.sections().is_empty(),
        "the settings outlived their plugin"
    );
}

/// Outside a mount the caller holds the registration, and it says what it holds.
#[test]
fn a_held_registration_names_its_contribution() {
    let ctx = PluginContext::new();
    let held = ctx
        .register_command(
            CommandSpec::new("test.contributions.held", "Held"),
            |_: &PluginContext| Ok(()),
        )
        .expect("registers");
    assert_eq!(
        held.contribution(),
        Some(&Contribution {
            kind: ContributionKind::Command,
            id: "test.contributions.held".into(),
        })
    );
    assert_eq!(held.service_name(), None);
    assert_eq!(
        held.contribution().map(ToString::to_string).as_deref(),
        Some("command test.contributions.held")
    );
}

#[test]
fn every_kind_has_a_distinct_label() {
    let mut labels: Vec<&str> = ContributionKind::ALL
        .iter()
        .map(|kind| kind.label())
        .collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), ContributionKind::ALL.len());
}

#[test]
fn cpu_effect_and_generator_plugin_lifecycle_fault_panic_and_unmount() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};

    use aria_compositor::{
        CompositeLayer, Compositor, CompositorConfig, LayerPlacement, PassInstance, pass_id,
        renders_effect,
    };
    use aria_plugin::{CpuFrameIn, CpuFrameOut, EffectParams, GpuContext, PluginFault};

    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping cpu_effect_and_generator_plugin_lifecycle: no GPU adapter");
        return;
    };
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(32, 8).with_background([0, 0, 0, 255]);

    // 0 = invert, 1 = return Err, 2 = panic
    let mode = Arc::new(AtomicU8::new(0));

    struct CpuPlugin {
        manifest: PluginManifest,
        mode: Arc<AtomicU8>,
    }

    impl Plugin for CpuPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }

        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            let mode = Arc::clone(&self.mode);
            ctx.register_cpu_effect(
                EffectSpec {
                    id: "test.cpu_plugin.invert".into(),
                    category: String::new(),
                    label: "Plugin CPU Invert".into(),
                    params: Vec::new(),
                    generator: false,
                    stateful: false,
                    ..Default::default()
                },
                move |input: CpuFrameIn<'_>,
                      output: &mut CpuFrameOut<'_>,
                      _params: &EffectParams<'_>,
                      _time: f32| {
                    match mode.load(Ordering::Acquire) {
                        1 => return Err("kernel step failed".into()),
                        2 => panic!("cpu callback crashed"),
                        _ => {}
                    }
                    for y in 0..input.height() {
                        for x in 0..input.width() {
                            let [r, g, b, a] = input.pixel_f32(x, y);
                            output.set_pixel_f32(x, y, [1.0 - r, 1.0 - g, 1.0 - b, a]);
                        }
                    }
                    Ok(())
                },
            )?;
            ctx.register_cpu_effect(
                EffectSpec {
                    id: "test.cpu_plugin.gen".into(),
                    category: String::new(),
                    label: "Plugin CPU Generator".into(),
                    params: Vec::new(),
                    generator: true,
                    stateful: false,
                    ..Default::default()
                },
                |input: CpuFrameIn<'_>,
                 output: &mut CpuFrameOut<'_>,
                 _params: &EffectParams<'_>,
                 _time: f32| {
                    for y in 0..input.height() {
                        for x in 0..input.width() {
                            output.set_pixel_f32(
                                x,
                                y,
                                [20.0 / 255.0, 140.0 / 255.0, 220.0 / 255.0, 1.0],
                            );
                        }
                    }
                    Ok(())
                },
            )?;
            Ok(())
        }
    }

    let ctx = PluginContext::new();
    let plugin = CpuPlugin {
        manifest: PluginManifest::new("test.cpu_plugin", "CPU Plugin", "1.0.0", &[], &[]),
        mode: Arc::clone(&mode),
    };
    ctx.mount(&plugin).expect("mounts cpu plugin");

    // 1. CPU effect inverts pixels; CPU generator draws on a transparent clip.
    let invert_fx = [PassInstance::new(
        pass_id("test.cpu_plugin.invert").expect("invert pass"),
        Vec::new(),
    )];
    let layer = CompositeLayer::solid([200, 100, 50, 255], LayerPlacement::full_canvas(&config))
        .with_effects(&invert_fx);
    let img = comp
        .render(&gpu, &config, std::slice::from_ref(&layer))
        .expect("renders cpu invert");
    assert_eq!(img.pixel(16, 4), [55, 155, 205, 255]);

    let gen_fx = [PassInstance::new(
        pass_id("test.cpu_plugin.gen").expect("gen pass"),
        Vec::new(),
    )];
    let gen_layer = CompositeLayer::solid([0, 0, 0, 0], LayerPlacement::full_canvas(&config))
        .with_effects(&gen_fx);
    let gen_img = comp
        .render(&gpu, &config, &[gen_layer])
        .expect("renders cpu generator");
    assert_eq!(gen_img.pixel(16, 4), [20, 140, 220, 255]);

    // 2. Callback returning Err passes the frame through and reports a fault.
    mode.store(1, Ordering::Release);
    let err_img = comp
        .render(&gpu, &config, std::slice::from_ref(&layer))
        .expect("renders passthrough on Err");
    assert_eq!(err_img.pixel(16, 4), [200, 100, 50, 255]);
    assert_eq!(
        ctx.faults(),
        vec![PluginFault {
            plugin: "test.cpu_plugin".into(),
            message: "effect test.cpu_plugin.invert failed: kernel step failed".into(),
        }]
    );
    assert!(ctx.plugins().iter().any(|id| id == "test.cpu_plugin"));

    // 3. Unmounting removes the effect and generator from the catalog and passes frames through.
    mode.store(0, Ordering::Release);
    ctx.unmount("test.cpu_plugin").expect("unmounts cpu plugin");
    assert!(!renders_effect("test.cpu_plugin.invert"));
    assert!(!renders_effect("test.cpu_plugin.gen"));
    assert!(aria_models::effect_spec("test.cpu_plugin.invert").is_none());
    assert!(aria_models::effect_spec("test.cpu_plugin.gen").is_none());
    let unmounted_img = comp
        .render(&gpu, &config, std::slice::from_ref(&layer))
        .expect("renders passthrough after unmount");
    assert_eq!(unmounted_img.pixel(16, 4), [200, 100, 50, 255]);

    // 4. Re-mounting and triggering a panic catches the panic, fails the plugin
    // for the session, removes its registrations, and keeps the frame rendering.
    ctx.mount(&plugin).expect("re-mounts cpu plugin");
    mode.store(2, Ordering::Release);
    let fresh_fx = [PassInstance::new(
        pass_id("test.cpu_plugin.invert").expect("re-mounted invert pass"),
        Vec::new(),
    )];
    let fresh_layer =
        CompositeLayer::solid([200, 100, 50, 255], LayerPlacement::full_canvas(&config))
            .with_effects(&fresh_fx);
    let panic_img = comp
        .render(&gpu, &config, &[fresh_layer])
        .expect("renders passthrough on panic");
    assert_eq!(panic_img.pixel(16, 4), [200, 100, 50, 255]);
    assert!(!ctx.plugins().iter().any(|id| id == "test.cpu_plugin"));
    assert!(!renders_effect("test.cpu_plugin.invert"));
    assert!(!renders_effect("test.cpu_plugin.gen"));
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, "test.cpu_plugin");
    assert_eq!(failures[0].name, "CPU Plugin");
    assert_eq!(
        failures[0].reason,
        "effect test.cpu_plugin.invert panicked: cpu callback crashed"
    );
}
/// WHY: Rendering a plugin effect through the real compositor must record the
/// plugin's per-frame cost without mixing two plugins' timings, and an effect
/// whose pass exceeds the sequence/preview frame budget (`1 / fps`) must
/// report a `PluginFault` with its measured cost while staying mounted.
#[test]
fn cpu_and_wgsl_effects_record_frame_cost_and_report_missed_frame_deadline() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use aria_compositor::{
        CompositeLayer, Compositor, CompositorConfig, CpuFrameIn, CpuFrameOut, EffectDefinition,
        EffectParams, LayerPlacement, PassInstance, pass_id,
    };
    use aria_plugin::GpuContext;

    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping cpu_and_wgsl_effects_record_frame_cost: no GPU adapter");
        return;
    };
    let mut comp = Compositor::new(&gpu);
    let sleep_ms = Arc::new(AtomicU64::new(15));

    struct SlowCpuPlugin {
        manifest: PluginManifest,
        sleep_ms: Arc<AtomicU64>,
    }

    impl Plugin for SlowCpuPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }

        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            let sleep_ms = Arc::clone(&self.sleep_ms);
            ctx.register_cpu_effect(
                EffectSpec {
                    id: "test.cost_cpu.slow".into(),
                    category: String::new(),
                    label: "Slow CPU".into(),
                    params: Vec::new(),
                    generator: false,
                    stateful: false,
                    ..Default::default()
                },
                move |input: CpuFrameIn<'_>,
                      output: &mut CpuFrameOut<'_>,
                      _params: &EffectParams<'_>,
                      _time: f32| {
                    let delay = sleep_ms.load(Ordering::Acquire);
                    if delay > 0 {
                        std::thread::sleep(Duration::from_millis(delay));
                    }
                    for y in 0..input.height() {
                        for x in 0..input.width() {
                            output.set_pixel(x, y, input.pixel(x, y));
                        }
                    }
                    Ok(())
                },
            )?;
            Ok(())
        }
    }

    struct FastWgslPlugin {
        manifest: PluginManifest,
    }

    impl Plugin for FastWgslPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }

        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            ctx.register_effect(EffectDefinition {
                spec: EffectSpec {
                    id: "test.cost_wgsl.tint".into(),
                    category: String::new(),
                    label: "Fast WGSL".into(),
                    params: Vec::new(),
                    generator: false,
                    stateful: false,
                    ..Default::default()
                },
                passes: vec![
                    "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return textureSample(input_tex, aria_sampler, in.uv);\n}".into(),
                ],
            })?;
            Ok(())
        }
    }

    let ctx = PluginContext::new();
    let cpu_plugin = SlowCpuPlugin {
        manifest: PluginManifest::new("test.cost_cpu", "Slow CPU Plugin", "1.0.0", &[], &[]),
        sleep_ms: Arc::clone(&sleep_ms),
    };
    let wgsl_plugin = FastWgslPlugin {
        manifest: PluginManifest::new("test.cost_wgsl", "Fast WGSL Plugin", "1.0.0", &[], &[]),
    };
    ctx.mount(&cpu_plugin).expect("mounts cpu plugin");
    ctx.mount(&wgsl_plugin).expect("mounts wgsl plugin");

    assert!(
        ctx.cost_of("test.cost_cpu")
            .expect("cpu cost")
            .frame
            .is_empty()
    );
    assert!(
        ctx.cost_of("test.cost_wgsl")
            .expect("wgsl cost")
            .frame
            .is_empty()
    );

    // 1. Within a 50 ms frame budget, the 15 ms CPU effect and the WGSL effect
    // both record their frame costs and raise no fault.
    let ok_config = CompositorConfig::new(32, 8).with_frame_budget(Duration::from_millis(50));
    let chain = [
        PassInstance::new(pass_id("test.cost_cpu.slow").expect("cpu pass"), Vec::new()),
        PassInstance::new(
            pass_id("test.cost_wgsl.tint").expect("wgsl pass"),
            Vec::new(),
        ),
    ];
    let layer = CompositeLayer::solid([120, 80, 40, 255], LayerPlacement::full_canvas(&ok_config))
        .with_effects(&chain);
    let img = comp
        .render(&gpu, &ok_config, std::slice::from_ref(&layer))
        .expect("renders within budget");
    assert_eq!(img.pixel(16, 4), [120, 80, 40, 255]);
    assert!(
        ctx.faults().is_empty(),
        "unexpected fault: {:?}",
        ctx.faults()
    );

    let cpu_cost = ctx.cost_of("test.cost_cpu").expect("cpu cost").frame;
    assert_eq!(cpu_cost.calls, 1);
    assert!(
        cpu_cost.last >= Duration::from_millis(14),
        "expected CPU effect frame cost >= 14 ms, got {:?}",
        cpu_cost.last
    );
    assert_eq!(cpu_cost.mean, cpu_cost.last);
    assert_eq!(cpu_cost.max, cpu_cost.last);

    let wgsl_cost = ctx.cost_of("test.cost_wgsl").expect("wgsl cost").frame;
    assert_eq!(wgsl_cost.calls, 1);
    assert!(wgsl_cost.last > Duration::ZERO);
    assert!(
        wgsl_cost.last < cpu_cost.last,
        "WGSL encode cost ({:?}) must not mix with CPU sleep cost ({:?})",
        wgsl_cost.last,
        cpu_cost.last
    );

    // 2. Exceeding a 10 ms frame budget reports a fault with the measured cost
    // while leaving the plugin mounted and keeping only the latest fault.
    sleep_ms.store(25, Ordering::Release);
    let tight_config = CompositorConfig::new(32, 8).with_frame_budget(Duration::from_millis(10));
    comp.render(&gpu, &tight_config, std::slice::from_ref(&layer))
        .expect("renders over budget");
    comp.render(&gpu, &tight_config, std::slice::from_ref(&layer))
        .expect("renders second frame over budget");

    assert!(ctx.is_mounted("test.cost_cpu"));
    let cpu_cost_after = ctx.cost_of("test.cost_cpu").expect("cpu cost").frame;
    assert_eq!(cpu_cost_after.calls, 3);
    assert!(cpu_cost_after.max >= Duration::from_millis(24));

    let faults = ctx.faults();
    assert_eq!(faults.len(), 1, "expected one latest fault, got {faults:?}");
    assert_eq!(faults[0].plugin, "test.cost_cpu");
    let msg = &faults[0].message;
    let measured_str = msg
        .strip_prefix("effect test.cost_cpu.slow took ")
        .and_then(|rest| rest.strip_suffix(" ms, over the 10.0 ms frame budget"))
        .unwrap_or_else(|| panic!("unexpected deadline fault message: {msg}"));
    let measured_ms: f64 = measured_str.parse().expect("measured ms is a float");
    assert!(
        measured_ms >= 24.0,
        "expected measured fault cost >= 24.0 ms, got {measured_ms} in {msg:?}"
    );

    // 3. Unmounting clears the plugin's cost and fault.
    ctx.unmount("test.cost_cpu").expect("unmounts cpu plugin");
    ctx.unmount("test.cost_wgsl").expect("unmounts wgsl plugin");
    assert!(ctx.cost_of("test.cost_cpu").is_none());
    assert!(ctx.faults().is_empty());
}
