//! A repeated frame hashes no bitmap, for every painting topology alone, under
//! each mask family, and as either side of a pair.
//!
//! The compositor finds a bitmap's texture by a digest of its pixels. A bitmap
//! a renderer cache holds across frames reaches it shared, so the digest is
//! taken on the first draw and read back by allocation on every later one. The
//! class this closes: a cached bitmap passed borrowed, or a cache that returns
//! a copy per request, which hashes the whole picture again on every frame. For
//! a 12 MP still that is 48 MB of SHA-256 per frame.
//!
//! The sweep takes its topologies from the capability table, as the paint
//! sweep does, so a new painting topology is covered when it ships.
//!
//! What this does not catch: a bitmap rebuilt every frame, such as the card
//! behind animated text, which holds new pixels and is hashed; and the cost of
//! the first draw.

use crate::scene::{Nested, SceneLayer};

use super::harness;
use super::masks::{lone, masks, painting_layers, pairs_of};

/// Every painting topology draws its second identical frame without hashing a
/// bitmap.
#[test]
fn a_repeated_frame_hashes_no_bitmap() {
    let (fx, mut renderer) = harness_or_skip!();
    let mut first = 0;
    for (kind, layer) in painting_layers(&fx, &mut renderer) {
        let mut cases = vec![(kind.clone(), layer.clone())];
        for (mask_kind, mask) in masks() {
            cases.push((
                format!("{kind} under a {mask_kind}"),
                SceneLayer {
                    masks: vec![mask],
                    ..layer.clone()
                },
            ));
        }
        for (what, layer) in cases {
            let mut scenes = vec![(what.clone(), lone(layer.clone()))];
            if !matches!(layer.source.spec().nested, Nested::Refuses) {
                scenes.extend(
                    pairs_of(&layer)
                        .into_iter()
                        .map(|(pair, wrapper)| (format!("{what} as {pair}"), lone(wrapper))),
                );
            }
            for (what, scene) in scenes {
                renderer
                    .render_scene(&fx.project, &scene)
                    .unwrap_or_else(|e| panic!("{what}: first frame: {e}"));
                first += renderer.compositor.bitmaps_hashed();
                renderer
                    .render_scene(&fx.project, &scene)
                    .unwrap_or_else(|e| panic!("{what}: repeated frame: {e}"));
                assert_eq!(
                    renderer.compositor.bitmaps_hashed(),
                    0,
                    "{what}: the repeated frame hashed a bitmap"
                );
            }
        }
    }
    assert!(
        first > 0,
        "no first frame hashed a bitmap, so the repeats prove nothing"
    );
}
