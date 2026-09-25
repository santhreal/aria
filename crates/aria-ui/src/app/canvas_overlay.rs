//! On-canvas handle geometry for the preview: where a crop or mask handle sits,
//! and what a drag to a point means.
//!
//! Framing and masking were editable only as steppers, so the operator set a
//! window they could not see. The rules here are pure arithmetic over fractions
//! and window pixels: the panel converts pointer positions with them and never
//! does geometry of its own, and every rule is testable without a window.
//!
//! Two coordinate spaces meet here:
//!
//! - **Window pixels**, what a pointer event carries.
//! - **Layer fractions**, `0.0..1.0` across the clip's own quad, which is what a
//!   crop rect and a mask center/size are expressed in.
//!
//! [`FrameBox`] is the bridge. It is the letterboxed rectangle the preview draws
//! the canvas into, computed the same way `ObjectFit::Contain` does, because the
//! handles have to land on the picture rather than on the black bars beside it.

use aria_models::{CropRect, MIN_CROP_FRACTION};

/// A rectangle in window pixels: where something is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// The point at fractions `[fx, fy]` across this rectangle.
    pub fn at(&self, [fx, fy]: [f32; 2]) -> [f32; 2] {
        [self.x + self.w * fx, self.y + self.h * fy]
    }

    /// Where `point` falls across this rectangle, as fractions clamped to it.
    /// A drag that leaves the picture stops at the edge rather than running away.
    pub fn fraction_of(&self, [px, py]: [f32; 2]) -> [f32; 2] {
        let fx = if self.w > 0.0 {
            (px - self.x) / self.w
        } else {
            0.0
        };
        let fy = if self.h > 0.0 {
            (py - self.y) / self.h
        } else {
            0.0
        };
        [fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0)]
    }
}

/// The letterboxed picture area inside the preview panel.
///
/// Held rather than recomputed because a pointer event carries a window position
/// and nothing else: the panel records the drawn rectangle while it paints, and
/// the drag rules read it back.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameBox {
    /// The whole picture area of the panel in window pixels: what the handles are
    /// positioned relative to, since an absolutely placed element is offset from
    /// its parent rather than from the window.
    pub container: Option<Rect>,
    /// The canvas rectangle in window pixels, or `None` before the first paint.
    pub canvas: Option<Rect>,
    /// The selected clip's own quad in window pixels, when it has one.
    pub layer: Option<Rect>,
}

impl FrameBox {
    /// A window point as an offset inside the picture area, which is what an
    /// absolutely positioned handle needs.
    pub fn local(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        match self.container {
            Some(container) => [x - container.x, y - container.y],
            None => [x, y],
        }
    }
}

/// Fit `canvas` inside `container` the way `ObjectFit::Contain` does: the whole
/// canvas visible, centered, aspect kept.
pub fn fit_frame(container: Rect, canvas: [u32; 2]) -> Rect {
    let (cw, ch) = (canvas[0] as f32, canvas[1] as f32);
    if cw <= 0.0 || ch <= 0.0 || container.w <= 0.0 || container.h <= 0.0 {
        return container;
    }
    let scale = (container.w / cw).min(container.h / ch);
    let (w, h) = (cw * scale, ch * scale);
    Rect::new(
        container.x + (container.w - w) * 0.5,
        container.y + (container.h - h) * 0.5,
        w,
        h,
    )
}

/// A clip's quad inside the canvas rectangle, from the resolved layer's center
/// and size in canvas pixels.
pub fn layer_rect(canvas_rect: Rect, canvas: [u32; 2], center: [f32; 2], size: [f32; 2]) -> Rect {
    let (cw, ch) = (canvas[0] as f32, canvas[1] as f32);
    if cw <= 0.0 || ch <= 0.0 {
        return canvas_rect;
    }
    let sx = canvas_rect.w / cw;
    let sy = canvas_rect.h / ch;
    Rect::new(
        canvas_rect.x + (center[0] - size[0] * 0.5) * sx,
        canvas_rect.y + (center[1] - size[1] * 0.5) * sy,
        size[0] * sx,
        size[1] * sy,
    )
}

/// The eight crop handles: four edges and four corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropHandle {
    Left,
    Top,
    Right,
    Bottom,
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

impl CropHandle {
    pub const ALL: [CropHandle; 8] = [
        CropHandle::Left,
        CropHandle::Top,
        CropHandle::Right,
        CropHandle::Bottom,
        CropHandle::TopLeft,
        CropHandle::TopRight,
        CropHandle::BottomRight,
        CropHandle::BottomLeft,
    ];

    /// Stable element id, also the drag identity.
    pub fn id(self) -> &'static str {
        match self {
            CropHandle::Left => "crop-handle-left",
            CropHandle::Top => "crop-handle-top",
            CropHandle::Right => "crop-handle-right",
            CropHandle::Bottom => "crop-handle-bottom",
            CropHandle::TopLeft => "crop-handle-top-left",
            CropHandle::TopRight => "crop-handle-top-right",
            CropHandle::BottomRight => "crop-handle-bottom-right",
            CropHandle::BottomLeft => "crop-handle-bottom-left",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|handle| handle.id() == id)
    }

    /// Whether this handle moves the left/top edge, on each axis.
    fn moves(self) -> (Option<bool>, Option<bool>) {
        // `Some(true)` = the near edge (left / top), `Some(false)` = the far one.
        match self {
            CropHandle::Left => (Some(true), None),
            CropHandle::Right => (Some(false), None),
            CropHandle::Top => (None, Some(true)),
            CropHandle::Bottom => (None, Some(false)),
            CropHandle::TopLeft => (Some(true), Some(true)),
            CropHandle::TopRight => (Some(false), Some(true)),
            CropHandle::BottomRight => (Some(false), Some(false)),
            CropHandle::BottomLeft => (Some(true), Some(false)),
        }
    }
}

/// Where a handle sits for `crop`, as layer fractions. An edge handle sits at the
/// middle of its edge; a corner handle at the corner.
pub fn crop_handle_anchor(crop: CropRect, handle: CropHandle) -> [f32; 2] {
    let (mx, my) = handle.moves();
    let x = match mx {
        Some(true) => crop.x,
        Some(false) => crop.x + crop.w,
        None => crop.x + crop.w * 0.5,
    };
    let y = match my {
        Some(true) => crop.y,
        Some(false) => crop.y + crop.h,
        None => crop.y + crop.h * 0.5,
    };
    [x, y]
}

/// Smallest window a drag will leave, a hair above the model's floor: `w` is
/// derived by subtraction, and landing exactly on the floor can round an ulp under
/// it and be refused.
const MIN_WINDOW: f32 = MIN_CROP_FRACTION + 1e-4;

/// `crop` after dragging `handle` to layer fraction `to`.
///
/// The dragged edge moves and the opposite one stays put, which is what a crop
/// handle means; the extent floor stops the window from collapsing, so a drag past
/// the far edge stalls rather than inverting the rect.
pub fn drag_crop(crop: CropRect, handle: CropHandle, [tx, ty]: [f32; 2]) -> CropRect {
    let (mx, my) = handle.moves();
    let mut out = crop;
    if let Some(near) = mx {
        let far = (crop.x + crop.w).min(1.0);
        if near {
            let x = tx.clamp(0.0, (far - MIN_WINDOW).max(0.0));
            out.w = (far - x).max(MIN_WINDOW);
            out.x = (far - out.w).max(0.0);
        } else {
            let right = tx.clamp((crop.x + MIN_WINDOW).min(1.0), 1.0);
            out.w = (right - crop.x).max(MIN_WINDOW).min(1.0 - crop.x);
        }
    }
    if let Some(near) = my {
        let far = (crop.y + crop.h).min(1.0);
        if near {
            let y = ty.clamp(0.0, (far - MIN_WINDOW).max(0.0));
            out.h = (far - y).max(MIN_WINDOW);
            out.y = (far - out.h).max(0.0);
        } else {
            let bottom = ty.clamp((crop.y + MIN_WINDOW).min(1.0), 1.0);
            out.h = (bottom - crop.y).max(MIN_WINDOW).min(1.0 - crop.y);
        }
    }
    out
}

/// The three mask handles: move, resize, rotate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskHandle {
    Center,
    Size,
    Rotate,
}

impl MaskHandle {
    pub const ALL: [MaskHandle; 3] = [MaskHandle::Center, MaskHandle::Size, MaskHandle::Rotate];

    pub fn id(self) -> &'static str {
        match self {
            MaskHandle::Center => "mask-handle-center",
            MaskHandle::Size => "mask-handle-size",
            MaskHandle::Rotate => "mask-handle-rotate",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|handle| handle.id() == id)
    }
}

/// Smallest mask extent, as a fraction of the layer: a mask smaller than this is
/// invisible and its handles would sit on top of each other.
pub const MIN_MASK_FRACTION: f32 = 0.02;

/// A mask's outline in layer fractions: `[x, y, w, h]` of the unrotated box.
///
/// The model stores the center as an offset from the layer center and the size as
/// a fraction of the layer, so the outline is derived rather than stored.
pub fn mask_box(center: [f32; 2], size: [f32; 2]) -> [f32; 4] {
    let w = size[0].max(MIN_MASK_FRACTION);
    let h = size[1].max(MIN_MASK_FRACTION);
    [0.5 + center[0] - w * 0.5, 0.5 + center[1] - h * 0.5, w, h]
}

/// Where a mask handle sits, as layer fractions: the center handle at the mask
/// center, the size handle at its bottom-right corner, the rotate handle above
/// its top edge.
pub fn mask_handle_anchor(center: [f32; 2], size: [f32; 2], handle: MaskHandle) -> [f32; 2] {
    let [x, y, w, h] = mask_box(center, size);
    match handle {
        MaskHandle::Center => [x + w * 0.5, y + h * 0.5],
        MaskHandle::Size => [x + w, y + h],
        MaskHandle::Rotate => [x + w * 0.5, (y - h * 0.25).max(0.0)],
    }
}

/// The mask center after dragging it to layer fraction `to`. The center is stored
/// as an offset from the layer center, and is allowed outside the layer: a mask
/// sliding off the edge is a normal reveal.
pub fn drag_mask_center([tx, ty]: [f32; 2]) -> [f32; 2] {
    [tx - 0.5, ty - 0.5]
}

/// The mask size after dragging the size handle to layer fraction `to`: the corner
/// follows the pointer while the center stays put, so the box grows symmetrically
/// like every other center-anchored shape in the app.
pub fn drag_mask_size(center: [f32; 2], [tx, ty]: [f32; 2]) -> [f32; 2] {
    let cx = 0.5 + center[0];
    let cy = 0.5 + center[1];
    [
        ((tx - cx) * 2.0).abs().max(MIN_MASK_FRACTION),
        ((ty - cy) * 2.0).abs().max(MIN_MASK_FRACTION),
    ]
}

/// The mask rotation in degrees after dragging the rotate handle to layer fraction
/// `to`, given the layer's pixel aspect so a circle drag feels circular.
///
/// Zero is the handle's rest position straight above the center, and the angle
/// grows clockwise, matching the model's clockwise degrees.
pub fn drag_mask_rotation(center: [f32; 2], [tx, ty]: [f32; 2], layer: Rect) -> f32 {
    let cx = 0.5 + center[0];
    let cy = 0.5 + center[1];
    // Fractions are per-axis, so scale back to pixels before taking an angle.
    let dx = (tx - cx) * layer.w.max(1.0);
    let dy = (ty - cy) * layer.h.max(1.0);
    if dx == 0.0 && dy == 0.0 {
        return 0.0;
    }
    let degrees = dx.atan2(-dy).to_degrees();
    if degrees < 0.0 {
        degrees + 360.0
    } else {
        degrees
    }
}

/// The four corners a scale drag can grab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformHandle {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

impl TransformHandle {
    pub const ALL: [TransformHandle; 4] = [
        TransformHandle::TopLeft,
        TransformHandle::TopRight,
        TransformHandle::BottomRight,
        TransformHandle::BottomLeft,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TransformHandle::TopLeft => "scale-handle-top-left",
            TransformHandle::TopRight => "scale-handle-top-right",
            TransformHandle::BottomRight => "scale-handle-bottom-right",
            TransformHandle::BottomLeft => "scale-handle-bottom-left",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|handle| handle.id() == id)
    }

    /// Where the handle sits on the layer quad, as layer fractions.
    pub fn anchor(self) -> [f32; 2] {
        match self {
            TransformHandle::TopLeft => [0.0, 0.0],
            TransformHandle::TopRight => [1.0, 0.0],
            TransformHandle::BottomRight => [1.0, 1.0],
            TransformHandle::BottomLeft => [0.0, 1.0],
        }
    }
}

/// Smallest and largest scale a corner drag will produce. The floor keeps a layer
/// grabbable — a zero-scale quad has no corners to grab back — and the ceiling
/// stops a flick near the center from throwing the picture off the canvas.
const MIN_SCALE: f32 = 0.02;
const MAX_SCALE: f32 = 20.0;

/// The position after dragging the picture from `grab` to `to`, both in window
/// pixels.
///
/// Position is an offset from the canvas center normalized to the canvas, so the
/// pointer delta is divided by the drawn canvas rather than by the layer: moving
/// the pointer across half the canvas moves the picture by 0.5 whatever the layer
/// is scaled to. Unclamped, because a picture pushed off frame is a normal move.
pub fn drag_position(start: [f32; 2], grab: [f32; 2], to: [f32; 2], canvas: Rect) -> [f32; 2] {
    if canvas.w <= 0.0 || canvas.h <= 0.0 {
        return start;
    }
    [
        start[0] + (to[0] - grab[0]) / canvas.w,
        start[1] + (to[1] - grab[1]) / canvas.h,
    ]
}

/// The scale after dragging a corner from `grab` to `to`, both in window pixels.
///
/// The factor is how much further the pointer is from the layer's center than it
/// was when the drag began, so the corner tracks the pointer and the opposite
/// side moves with it: the layer scales about its own center, which is what the
/// model does. Uniform on purpose — a corner that skewed the aspect would need a
/// modifier to get the common case back.
pub fn drag_scale(start: [f32; 2], layer: Rect, grab: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    let center = [layer.x + layer.w * 0.5, layer.y + layer.h * 0.5];
    let from = ((grab[0] - center[0]).powi(2) + (grab[1] - center[1]).powi(2)).sqrt();
    let now = ((to[0] - center[0]).powi(2) + (to[1] - center[1]).powi(2)).sqrt();
    // A grab on the center has no distance to compare against, so the drag is
    // inert rather than infinite.
    if from <= 1.0 {
        return start;
    }
    let factor = now / from;
    [
        (start[0] * factor).clamp(MIN_SCALE, MAX_SCALE),
        (start[1] * factor).clamp(MIN_SCALE, MAX_SCALE),
    ]
}

/// What an on-canvas drag is moving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayDrag {
    /// A crop handle, with the crop the drag started from. The start rect is kept
    /// so the commit sends one edit for the whole gesture rather than one per
    /// pointer move.
    Crop { handle: CropHandle, draft: CropRect },
    /// A mask handle, with the mask geometry the drag is building.
    Mask {
        handle: MaskHandle,
        center: [f32; 2],
        size: [f32; 2],
        rotation: f32,
    },
    /// The picture itself: a corner scales it, no corner moves it. The transform
    /// the gesture started from is kept so every move is measured from the press
    /// rather than accumulating rounding, and `grab` is where the press landed.
    Transform {
        handle: Option<TransformHandle>,
        start: crate::types::UiClipTransform,
        grab: [f32; 2],
        draft: crate::types::UiClipTransform,
    },
}

use super::AppState;
use aria_engine::EngineError;

// Further methods live in sibling files; inherent methods keep their paths.
#[path = "canvas_overlay_handles.rs"]
mod canvas_overlay_handles;

// The selected layer's quad now comes from the projection, which resolves the
// scene once per change instead of once per element per frame; see
// `crate::app::selected_detail`.
