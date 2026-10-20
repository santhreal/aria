//! Host tools: what the assistant can do that is not an edit.
//!
//! `aria-ai`'s edit vocabulary is closed and validated — every timeline change
//! goes through it. This is the other half: senses and app control, which the
//! agent loop treats as opaque calls and which only the desktop can implement.
//! Until these existed the assistant was blind (it could change a frame it had
//! never seen) and the `Autonomy` setting gated nothing, because nothing needed
//! authorizing.
//!
//! Three kinds of tool, split by where they can run:
//!
//! - **Senses** run on the job thread from a cloned project and a headless
//!   renderer, the same way an export does. Nothing about them needs the UI.
//! - **App control** cannot run there: saving a project or moving the playhead is
//!   `AppState`, which lives on the UI thread. Those send a `HostRequest` up
//!   the beat channel and block on the answer, so the work happens where the
//!   state is and the model still sees one synchronous tool result.
//! - **Plugin tools** run on the job thread through
//!   [`PluginContext::call_ai_tool`], listed and authorized by the tier the
//!   plugin registered.
//! - **Destructive tools** additionally prompt for approval. With
//!   [`Autonomy::Ask`] the panel shows the call and the job thread waits for
//!   Approve or Deny; with [`Autonomy::Full`] it does not ask. A cancelled turn
//!   releases the wait, so Stop is never blocked by a question nobody answered.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use aria_ai::tools::{HostToolSpec, ToolHost, ToolOutput, ToolTier};
use aria_engine::Engine;
use aria_models::RationalTime;
use aria_plugin::{AI_TOOL_PREFIX, AiToolTier, PluginContext};
use aria_settings::Autonomy;
use crossbeam_channel::Sender;
use parking_lot::Mutex;

use super::assistant::Beat;

/// Longest picture the model is handed, in pixels.
///
/// A screenshot is evidence, not a deliverable: the canvas is scaled down to fit
/// this box (never up) because a 4K frame costs the request thirty times the
/// bytes and tells the model nothing more about where a cut lands.
const SCREENSHOT_FIT: (u32, u32) = (960, 540);

/// How often a blocked host call checks whether the turn was stopped.
const WAIT_TICK: Duration = Duration::from_millis(100);

/// Work only the UI thread can do, sent up the beat channel by a host tool.
///
/// Deliberately a small closed set rather than a callback: a host tool runs on
/// the job thread, and a closure that captured `&mut AppState` could not exist
/// there at all.
pub(crate) enum HostRequest {
    /// Save the project to its current path.
    SaveProject,
    /// Move the playhead, in seconds from the start of the sequence.
    Seek { seconds: f64 },
    /// Empty one registered cache directory.
    ClearCache { key: String },
    /// Start a render of one clip, to be decoded in its place when written.
    RenderClip { clip: aria_models::ClipId },
}

impl HostRequest {
    /// The line the transcript shows for this request.
    pub(crate) fn summary(&self) -> String {
        match self {
            Self::SaveProject => "save the project".to_string(),
            Self::Seek { seconds } => format!("move the playhead to {seconds:.2}s"),
            Self::ClearCache { key } => format!("empty the {key} cache"),
            Self::RenderClip { clip } => format!("render clip {}", clip.raw()),
        }
    }
}

/// A destructive call waiting on approval.
///
/// Holds the reply channel the job thread is blocked on, so answering it is what
/// unblocks the turn. Dropping it without answering denies the call, which is the
/// safe reading: a question that vanished was never approved.
#[derive(Debug)]
pub struct PendingAsk {
    pub name: String,
    pub summary: String,
    pub reply: Sender<bool>,
}

/// The desktop's tool surface for one turn.
pub(crate) struct ShellHost {
    /// The same engine the timeline reads, so a screenshot shows the project as
    /// it stands after the edits this turn already made.
    engine: Arc<Mutex<Engine>>,
    /// Where requests and questions go: the panel's own beat channel, so a host
    /// call appears in the transcript in the order it happened.
    beats: crate::handoff::Wake<Beat>,
    autonomy: Autonomy,
    /// The process's device, or `None` where none mounted. Handed in rather
    /// than found, because this runs on a job thread with no plugin context.
    gpu: Option<aria_compositor::GpuContext>,
    /// Built on first use and kept for the turn. Building pipelines costs tens
    /// of milliseconds, and a model that looks three times would otherwise pay
    /// for three.
    renderer: Option<aria_render::Renderer>,
    /// The context whose plugins' AI tools are offered beside this host's own.
    plugins: PluginContext,
    /// The plugin tool the last [`authorize`](ToolHost::authorize) allowed,
    /// and at which tier. [`call`](ToolHost::call) runs it at no more trust
    /// than that.
    granted: Option<(String, AiToolTier)>,
}

impl ShellHost {
    pub(crate) fn new(
        engine: Arc<Mutex<Engine>>,
        beats: crate::handoff::Wake<Beat>,
        autonomy: Autonomy,
        gpu: Option<aria_compositor::GpuContext>,
        plugins: PluginContext,
    ) -> Self {
        Self {
            engine,
            beats,
            autonomy,
            gpu,
            renderer: None,
            plugins,
            granted: None,
        }
    }

    /// Run the plugin tool `name` at the tier the last authorization allowed
    /// it, and hand the model its text and pictures.
    fn plugin_tool(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<ToolOutput, String> {
        let granted = match self.granted.take() {
            Some((allowed, tier)) if allowed == name => tier,
            _ => AiToolTier::ReadOnly,
        };
        let output = self
            .plugins
            .call_ai_tool(name, arguments, granted, cancel)
            .map_err(|error| error.to_string())?;
        Ok(ToolOutput {
            text: output.text,
            images: output
                .images
                .into_iter()
                .map(|image| aria_ai::ImagePart::png(image.png, image.label))
                .collect(),
        })
    }

    /// Hand one request to the UI thread and wait for its answer.
    fn on_ui_thread(&self, request: HostRequest, cancel: &AtomicBool) -> Result<String, String> {
        on_ui_thread(&self.beats, request, cancel)
    }

    /// Render the timeline at `seconds` and hand the model the picture.
    fn screenshot(&mut self, seconds: f64, cancel: &AtomicBool) -> Result<ToolOutput, String> {
        let project = self.engine.lock().snapshot();
        if cancel.load(Ordering::Relaxed) {
            return Err("the turn was stopped".to_string());
        }
        let rate = project.timeline().frame_rate;
        let tick = (seconds.max(0.0) * f64::from(rate.num) / f64::from(rate.den)).round() as i64;
        let at = RationalTime::new(tick, rate);
        if self.renderer.is_none() {
            self.renderer = Some(crate::app::render_device::renderer_on(
                self.gpu.as_ref().map(aria_compositor::GpuContext::share),
                "a screenshot",
            )?);
        }
        let renderer = self
            .renderer
            .as_mut()
            .expect("the renderer was just created");
        let image = renderer
            .render_frame_fit(&project, at, SCREENSHOT_FIT.0, SCREENSHOT_FIT.1)
            .map_err(|err| err.to_string())?;
        let (width, height) = (image.width, image.height);
        let bytes = aria_render::encode_png(&image).map_err(|err| err.to_string())?;
        let label = format!("timeline at {seconds:.2}s");
        Ok(ToolOutput {
            text: format!("the timeline at {seconds:.2}s, rendered {width}x{height}"),
            images: vec![aria_ai::ImagePart::png(bytes, label)],
        })
    }

    /// Read a media file's real metadata, rather than guessing from its name.
    fn probe(path: &str) -> Result<ToolOutput, String> {
        let probe =
            aria_decoder::probe(std::path::Path::new(path)).map_err(|err| err.to_string())?;
        let mut text = format!("{path}: ");
        if probe.is_image {
            text.push_str(&format!("still image, {}x{}", probe.width, probe.height));
            return Ok(ToolOutput::text(text));
        }
        text.push_str(&format!(
            "{}x{} at {}/{} fps, {} frame(s)",
            probe.width,
            probe.height,
            probe.frame_rate.num,
            probe.frame_rate.den,
            probe.frame_count
        ));
        match &probe.video_codec {
            Some(codec) => text.push_str(&format!(", video {codec}")),
            None => text.push_str(", no video track"),
        }
        if probe.has_audio {
            match &probe.audio_codec {
                Some(codec) => text.push_str(&format!(", audio {codec}")),
                None => text.push_str(", audio track of an unnamed codec"),
            }
        } else {
            text.push_str(", no audio");
        }
        Ok(ToolOutput::text(text))
    }

    /// One required string argument, or a model-readable complaint.
    fn string_arg(arguments: &serde_json::Value, key: &str) -> Result<String, String> {
        arguments
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("'{key}' is required and must be a string"))
    }
}

/// Every tool this host offers, in the order a panel should list them.
///
/// A free function so the Settings dialog can state the capability without
/// building a host for a turn that is not running.
pub fn host_tool_specs() -> Vec<HostToolSpec> {
    vec![
        HostToolSpec {
            name: "media_screenshot_timeline".to_string(),
            description: "Render the timeline at a given time and look at it. \
                 Use this to check what a frame actually shows before changing it."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "at_seconds": {
                        "type": "number",
                        "description": "Seconds from the start of the sequence.",
                    },
                },
                "required": ["at_seconds"],
            }),
            tier: ToolTier::ReadOnly,
        },
        HostToolSpec {
            name: "media_probe".to_string(),
            description: "Read a media file's size, frame rate, length, and codecs \
                 from the file itself."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the media file.",
                    },
                },
                "required": ["path"],
            }),
            tier: ToolTier::ReadOnly,
        },
        HostToolSpec {
            name: "app_seek".to_string(),
            description: "Move the playhead, so the editor shows the moment being \
                 discussed."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "at_seconds": {
                        "type": "number",
                        "description": "Seconds from the start of the sequence.",
                    },
                },
                "required": ["at_seconds"],
            }),
            tier: ToolTier::Workspace,
        },
        HostToolSpec {
            name: "app_save_project".to_string(),
            description: "Save the project to its current path.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
            tier: ToolTier::Workspace,
        },
        HostToolSpec {
            name: "system_clear_cache".to_string(),
            description: "Empty one cache directory on disk. Destructive: the files \
                 are deleted and rebuilt on demand."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "cache": {
                        "type": "string",
                        "description": "Cache id, e.g. 'preview', 'proxies', 'thumbnails'.",
                    },
                },
                "required": ["cache"],
            }),
            tier: ToolTier::System,
        },
    ]
}

/// The AI tools the plugins mounted on `plugins` offer, in registration order.
pub fn plugin_tool_specs(plugins: &PluginContext) -> Vec<HostToolSpec> {
    plugins
        .ai_tools()
        .into_iter()
        .map(|tool| HostToolSpec {
            description: format!(
                "{} (from the {} plugin)",
                tool.spec.description, tool.plugin_name
            ),
            parameters: tool.spec.parameters.clone(),
            tier: tool_tier(tool.spec.tier),
            name: tool.call_name,
        })
        .collect()
}

fn tool_tier(tier: AiToolTier) -> ToolTier {
    match tier {
        AiToolTier::ReadOnly => ToolTier::ReadOnly,
        AiToolTier::Workspace => ToolTier::Workspace,
        AiToolTier::System => ToolTier::System,
    }
}

fn plugin_tier(tier: ToolTier) -> AiToolTier {
    match tier {
        ToolTier::ReadOnly => AiToolTier::ReadOnly,
        ToolTier::Workspace => AiToolTier::Workspace,
        ToolTier::System => AiToolTier::System,
    }
}

impl ToolHost for ShellHost {
    fn tools(&self) -> Vec<HostToolSpec> {
        let mut specs = host_tool_specs();
        specs.extend(plugin_tool_specs(&self.plugins));
        specs
    }

    fn authorize(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        tier: ToolTier,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        self.granted = None;
        // A plugin tool is authorized at the tier its plugin registers now,
        // not the one listed when the turn began: a reload may have raised it.
        let plugin = if name.starts_with(AI_TOOL_PREFIX) {
            let tool = self
                .plugins
                .ai_tool(name)
                .ok_or_else(|| format!("no plugin offers {name} any more"))?;
            Some((tool.spec.tier.max(plugin_tier(tier)), tool.plugin_name))
        } else {
            None
        };
        let tier = plugin
            .as_ref()
            .map_or(tier, |(current, _)| tool_tier(*current));
        let allowed = self.allow(name, arguments, tier, plugin.as_ref(), cancel);
        if allowed.is_ok()
            && let Some((current, _)) = plugin
        {
            self.granted = Some((name.to_owned(), current));
        }
        allowed
    }

    fn call(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<ToolOutput, String> {
        match name {
            "media_screenshot_timeline" => {
                let seconds = arguments
                    .get("at_seconds")
                    .and_then(|value| value.as_f64())
                    .ok_or_else(|| "'at_seconds' is required and must be a number".to_string())?;
                self.screenshot(seconds, cancel)
            }
            "media_probe" => Self::probe(&Self::string_arg(arguments, "path")?),
            "app_seek" => {
                let seconds = arguments
                    .get("at_seconds")
                    .and_then(|value| value.as_f64())
                    .ok_or_else(|| "'at_seconds' is required and must be a number".to_string())?;
                self.on_ui_thread(HostRequest::Seek { seconds }, cancel)
                    .map(ToolOutput::text)
            }
            "app_save_project" => self
                .on_ui_thread(HostRequest::SaveProject, cancel)
                .map(ToolOutput::text),
            "system_clear_cache" => {
                let key = Self::string_arg(arguments, "cache")?;
                self.on_ui_thread(HostRequest::ClearCache { key }, cancel)
                    .map(ToolOutput::text)
            }
            plugin if plugin.starts_with(AI_TOOL_PREFIX) => {
                self.plugin_tool(plugin, arguments, cancel)
            }
            other => Err(format!("unknown tool '{other}'")),
        }
    }
}

impl ShellHost {
    /// Allow one call at `tier`, prompting for approval first for a System call
    /// under [`Autonomy::Ask`]. `plugin` is the current tier and plugin name
    /// of a plugin tool.
    fn allow(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        tier: ToolTier,
        plugin: Option<&(AiToolTier, Arc<str>)>,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        match tier {
            ToolTier::ReadOnly | ToolTier::Workspace => Ok(()),
            ToolTier::System => match self.autonomy {
                Autonomy::Full => Ok(()),
                Autonomy::Ask => {
                    let summary = match (plugin, Self::string_arg(arguments, "cache")) {
                        (Some((_, plugin)), _) => {
                            format!("run the {plugin} plugin's tool with {arguments}")
                        }
                        (None, Ok(cache)) => format!("empty the {cache} cache"),
                        (None, Err(_)) => arguments.to_string(),
                    };
                    let (tx, rx) = crossbeam_channel::bounded(1);
                    if !self.beats.wake(Beat::Ask(PendingAsk {
                        name: name.to_string(),
                        summary,
                        reply: tx,
                    })) {
                        return Err("the editor is gone".to_string());
                    }
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            return Err("the turn was stopped before you answered".to_string());
                        }
                        match rx.recv_timeout(WAIT_TICK) {
                            Ok(true) => return Ok(()),
                            Ok(false) => return Err("you did not allow this".to_string()),
                            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                            // The question went away unanswered. Unanswered is
                            // not approved.
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                                return Err("the question was dismissed".to_string());
                            }
                        }
                    }
                }
            },
        }
    }
}

/// Hand one request to the UI thread and wait for its answer.
///
/// Free function because two callers need it and neither owns the other: a host
/// tool, and the edit bridge when a tool call lowers to work only the shell can
/// start. One owner of the wait, so a turn stopped mid-request is released the
/// same way whichever asked.
pub(crate) fn on_ui_thread(
    beats: &crate::handoff::Wake<Beat>,
    request: HostRequest,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    if !beats.wake(Beat::Apply { request, reply: tx }) {
        return Err("the editor is gone".to_string());
    }
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("the turn was stopped".to_string());
        }
        match rx.recv_timeout(WAIT_TICK) {
            Ok(result) => return result,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            // The panel dropped the request without answering: the window is
            // closing, and reporting that beats waiting forever.
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err("the editor did not answer".to_string());
            }
        }
    }
}
