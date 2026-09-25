//! Render and replace: the two calls that turn a clip into a file and back.

use super::*;

/// Render one clip to a file and decode that file in its place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderAndReplace {
    /// The clip to render.
    pub clip: u64,
}

/// Put back the clip a render replaced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreUnrendered {
    /// The rendered clip to put back.
    pub clip: u64,
}
