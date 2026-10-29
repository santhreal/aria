//! The render host: frames of a project for plugins.
//!
//! [`RenderHost`] renders a sequence, or one clip of it, at a time and a size,
//! as an image or into a texture on the process's GPU device. It renders with a
//! renderer of its own, built on the first request, so a plugin render neither
//! waits on nor disturbs the preview. A plugin takes the project from
//! [`ProjectHost::snapshot`](crate::ProjectHost::snapshot).

use std::borrow::Cow;
use std::ops::ControlFlow;

use aria_compositor::{GpuContext, wgpu};
use aria_core::{RationalTime, RgbaImage, TimeRange};
use aria_models::{ClipId, ModelError, Project, SequenceId};
use aria_render::{LayerSource, RenderError, Renderer, Scene, SceneLayer};
use parking_lot::Mutex;

/// What a frame shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSource {
    /// The sequence open for editing, which the viewer shows.
    Open,
    /// The root sequence (`None`) or a nested sequence, whichever is open.
    Sequence(Option<SequenceId>),
    /// One clip of any sequence, as it is placed on that sequence's canvas,
    /// with its effects, over transparency. `time` is on that sequence. A clip
    /// in a transition or a track matte draws without the other clip.
    Clip(ClipId),
}

/// The pixel size of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSize {
    /// The sequence canvas.
    Canvas,
    /// The canvas scaled down to fit the box, aspect kept, never enlarged.
    Fit { max_width: u32, max_height: u32 },
    /// Exactly this size: the canvas scaled to fit and centred, the remainder
    /// filled with the background.
    Exact { width: u32, height: u32 },
}

/// A frame request: what to show, how large, and whether transforms blur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRequest {
    pub source: FrameSource,
    pub size: FrameSize,
    /// Supersample moving transforms as an export does. Off for a preview.
    pub motion_blur: bool,
}

impl FrameRequest {
    /// The open sequence at canvas size, without motion blur.
    pub fn open() -> Self {
        Self {
            source: FrameSource::Open,
            size: FrameSize::Canvas,
            motion_blur: false,
        }
    }

    /// The root sequence (`None`) or a nested one, at canvas size.
    pub fn sequence(sequence: Option<SequenceId>) -> Self {
        Self {
            source: FrameSource::Sequence(sequence),
            ..Self::open()
        }
    }

    /// One clip at canvas size, without motion blur.
    pub fn clip(clip: ClipId) -> Self {
        Self {
            source: FrameSource::Clip(clip),
            ..Self::open()
        }
    }

    pub fn size(self, size: FrameSize) -> Self {
        Self { size, ..self }
    }

    pub fn motion_blur(self, motion_blur: bool) -> Self {
        Self {
            motion_blur,
            ..self
        }
    }
}

/// Renders frames of a project on the process's GPU device.
pub struct RenderHost {
    gpu: GpuContext,
    renderer: Mutex<Option<Renderer>>,
}

impl std::fmt::Debug for RenderHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderHost")
            .field("gpu", &self.gpu)
            .finish_non_exhaustive()
    }
}

impl RenderHost {
    /// A host that renders on `gpu`. Builds no pipeline until the first frame.
    pub fn new(gpu: GpuContext) -> Self {
        Self {
            gpu,
            renderer: Mutex::new(None),
        }
    }

    /// The device every texture this host writes lives on.
    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// The width and height a frame of `request` has.
    pub fn frame_size(
        &self,
        project: &Project,
        request: &FrameRequest,
    ) -> Result<(u32, u32), RenderError> {
        let target = Target::of(project, request.source)?;
        let (width, height) = aria_render::canvas_size(&target.project);
        Ok(match request.size {
            FrameSize::Canvas => (width, height),
            FrameSize::Fit {
                max_width,
                max_height,
            } => {
                let mut scene = Scene::empty(width, height, [0; 4]);
                scene.fit_within(max_width, max_height);
                (scene.width, scene.height)
            }
            FrameSize::Exact { width, height } => (width, height),
        })
    }

    /// Render the frame at `time` as a straight-alpha image.
    pub fn frame(
        &self,
        project: &Project,
        time: RationalTime,
        request: &FrameRequest,
    ) -> Result<RgbaImage, RenderError> {
        let target = Target::of(project, request.source)?;
        self.image(&target, time, request)
    }

    /// Render one frame per tick of `range`, at the range's rate, and hand each
    /// to `each` in order until it breaks. The renderer is released between
    /// frames, so another plugin's request interleaves rather than waits for
    /// the whole range.
    pub fn frames(
        &self,
        project: &Project,
        range: TimeRange,
        request: &FrameRequest,
        mut each: impl FnMut(RationalTime, RgbaImage) -> ControlFlow<()>,
    ) -> Result<(), RenderError> {
        let target = Target::of(project, request.source)?;
        for tick in range.start.value..range.end_tick() {
            let time = RationalTime::new(tick, range.start.rate);
            let image = self.image(&target, time, request)?;
            if each(time, image).is_break() {
                break;
            }
        }
        Ok(())
    }

    /// Render the frame at `time` into `texture`, premultiplied, with no copy
    /// to the CPU. `texture` is an `Rgba8Unorm` texture on [`Self::gpu`] with
    /// `COPY_DST` usage and the size [`Self::frame_size`] reports; any other
    /// texture fails with [`aria_compositor::CompositorError::FrameTarget`]
    /// before anything is drawn.
    pub fn frame_to_texture(
        &self,
        project: &Project,
        time: RationalTime,
        request: &FrameRequest,
        texture: &wgpu::Texture,
    ) -> Result<(), RenderError> {
        let target = Target::of(project, request.source)?;
        self.with_renderer(|renderer| {
            let scene = target.scene(renderer, time, request)?;
            renderer.render_scene_to_texture(&target.project, &scene, texture)
        })
    }

    fn image(
        &self,
        target: &Target<'_>,
        time: RationalTime,
        request: &FrameRequest,
    ) -> Result<RgbaImage, RenderError> {
        self.with_renderer(|renderer| {
            let scene = target.scene(renderer, time, request)?;
            let mut image = renderer.render_scene(&target.project, &scene)?;
            image.unpremultiply();
            Ok(image)
        })
    }

    fn with_renderer<T>(
        &self,
        body: impl FnOnce(&mut Renderer) -> Result<T, RenderError>,
    ) -> Result<T, RenderError> {
        let mut slot = self.renderer.lock();
        let renderer = slot.get_or_insert_with(|| Renderer::with_gpu(self.gpu.share()));
        body(renderer)
    }
}

/// The project as a request draws it: with the asked-for sequence open, and
/// the one clip to keep, if any.
struct Target<'p> {
    project: Cow<'p, Project>,
    clip: Option<ClipId>,
}

impl<'p> Target<'p> {
    fn of(project: &'p Project, source: FrameSource) -> Result<Self, ModelError> {
        let (sequence, clip) = match source {
            FrameSource::Open => (project.active_sequence(), None),
            FrameSource::Sequence(sequence) => (sequence, None),
            FrameSource::Clip(clip) => (sequence_holding(project, clip)?, Some(clip)),
        };
        let project = if sequence == project.active_sequence() {
            Cow::Borrowed(project)
        } else {
            let mut opened = project.clone();
            opened.set_active_sequence(sequence)?;
            Cow::Owned(opened)
        };
        Ok(Self { project, clip })
    }

    /// The scene at `time`: resolved, narrowed to the clip when there is one,
    /// and sized.
    fn scene(
        &self,
        renderer: &mut Renderer,
        time: RationalTime,
        request: &FrameRequest,
    ) -> Result<Scene, RenderError> {
        renderer.set_apply_motion_blur(request.motion_blur);
        let mut scene = renderer.resolve_frame(&self.project, time)?;
        if let Some(clip) = self.clip {
            scene.layers = std::mem::take(&mut scene.layers)
                .into_iter()
                .filter_map(|layer| clip_alone(layer, clip))
                .collect();
            scene.background = [0; 4];
        }
        match request.size {
            FrameSize::Canvas => {}
            FrameSize::Fit {
                max_width,
                max_height,
            } => scene.fit_within(max_width, max_height),
            FrameSize::Exact { width, height } => scene.fit_into(width, height),
        }
        Ok(scene)
    }
}

/// The layer that draws `clip` and nothing else: `layer` itself, or the side of
/// a transition or a track matte that is `clip`, without the other side. A
/// nested sequence's clip is on the sequence's own timeline, which
/// [`Target::of`] opens, so a nest is kept or dropped whole.
fn clip_alone(layer: SceneLayer, clip: ClipId) -> Option<SceneLayer> {
    let sides = match layer.source {
        LayerSource::Transition {
            outgoing, incoming, ..
        } => [*outgoing, *incoming],
        LayerSource::Matte { layers, .. } => *layers,
        LayerSource::Media { .. }
        | LayerSource::Still { .. }
        | LayerSource::Sticker { .. }
        | LayerSource::Lottie { .. }
        | LayerSource::Text { .. }
        | LayerSource::Solid(_)
        | LayerSource::CanvasPass
        | LayerSource::Shape { .. }
        | LayerSource::PathShape { .. }
        | LayerSource::Nest { .. } => return (layer.clip == Some(clip)).then_some(layer),
    };
    sides.into_iter().find_map(|side| clip_alone(side, clip))
}

/// The sequence whose timeline holds `clip`: the open one first, then the
/// root, then every nested one.
pub(crate) fn sequence_holding(
    project: &Project,
    clip: ClipId,
) -> Result<Option<SequenceId>, ModelError> {
    if project.timeline().track_of(clip).is_some() {
        return Ok(project.active_sequence());
    }
    if project.root_timeline().track_of(clip).is_some() {
        return Ok(None);
    }
    project
        .sequences()
        .find(|sequence| sequence.timeline.track_of(clip).is_some())
        .map(|sequence| Some(sequence.id))
        .ok_or(ModelError::UnknownClip(clip))
}
