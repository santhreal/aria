//! Plain Rust replacements for the Slint-generated view-model types.

use aria_models::{
    AudioRole, BlendMode, ClipId, CropRect, MarkerColor, MarkerKind, MaskKind, MaskMode,
    MatteSource, MediaId, MotionBlur, Rational, SlotMedia, StabilizeLevel, TrackId,
};
use gpui::SharedString;
use std::path::PathBuf;

pub fn clip_id(id: ClipId) -> u64 {
    id.raw()
}
pub fn track_id(id: TrackId) -> u64 {
    id.raw()
}
pub fn media_id(id: MediaId) -> u64 {
    id.raw()
}

// The view-model types live in sibling files, grouped by what they describe,
// and are re-exported here so `crate::types::Name` keeps resolving.
#[path = "types_clip.rs"]
mod clip;
pub use clip::*;
#[path = "types_clip_bar.rs"]
mod clip_bar;
pub use clip_bar::*;
#[path = "types_editor.rs"]
mod editor;
pub use editor::*;
#[path = "types_held.rs"]
mod held;
pub use held::*;
#[path = "types_pool.rs"]
mod pool;
pub use pool::*;
#[path = "types_lane.rs"]
mod lane;
pub use lane::*;
#[path = "types_sidebar.rs"]
mod sidebar;
pub use sidebar::*;
#[path = "types_target.rs"]
mod target;
pub use target::*;
