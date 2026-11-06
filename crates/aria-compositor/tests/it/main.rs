//! Consolidated integration test binary for `aria-compositor`.
#![allow(clippy::duplicate_mod)]

#[path = "../adjust.rs"]
mod adjust;
#[path = "../blend.rs"]
mod blend;
#[path = "../cpu_effect_readback.rs"]
mod cpu_effect_readback;
#[path = "../effect_graph.rs"]
mod effect_graph;
#[path = "../effects.rs"]
mod effects;
#[path = "../generators.rs"]
mod generators;
#[path = "../glyphs.rs"]
mod glyphs;
#[path = "../mask_stacks.rs"]
mod mask_stacks;
#[path = "../masks.rs"]
mod masks;
#[path = "../matte.rs"]
mod matte;
#[path = "../motion_blur.rs"]
mod motion_blur;
#[path = "../nests.rs"]
mod nests;
#[path = "../pair_sides.rs"]
mod pair_sides;
#[path = "../registered_passes.rs"]
mod registered_passes;
#[path = "../render.rs"]
mod render;
#[path = "../scopes.rs"]
mod scopes;
#[path = "../stencils.rs"]
mod stencils;
#[path = "../styles.rs"]
mod styles;
#[path = "../transitions.rs"]
mod transitions;
