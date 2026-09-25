//! The layer list one render walks, borrowed rather than materialized.
//!
//! [`Compositor::render`](super::Compositor::render) is handed plain
//! [`CompositeLayer`]s; the full path is handed [`CompositorLayer`]s that can
//! also be canvas passes, transitions, and mattes. The plain entry point used
//! to promote its slice by collecting a `Vec<CompositorLayer>` — a heap
//! allocation and free per rendered frame, on the hottest path in the crate,
//! for values that are nothing but a pointer and a tag.
//!
//! [`Scene`] is that promotion done lazily: it borrows either slice and wraps
//! each element as the render visits it.

use crate::layer::{CompositeLayer, CompositorLayer};

/// A borrowed scene: either list shape the compositor accepts.
#[derive(Clone, Copy)]
pub(super) enum Scene<'a> {
    /// Plain layers, promoted to [`CompositorLayer::Layer`] on visit.
    Plain(&'a [CompositeLayer<'a>]),
    /// Layers that may already be passes, transitions, or mattes.
    Full(&'a [CompositorLayer<'a>]),
}

impl<'a> Scene<'a> {
    /// Walk the scene bottom-to-top.
    pub(super) fn iter(&self) -> SceneIter<'a> {
        SceneIter {
            scene: *self,
            next: 0,
        }
    }
}

/// [`Scene::iter`]'s iterator. Hand-written because the two variants yield the
/// same item type by different routes, and boxing the iterator would put back
/// the per-frame allocation this type exists to remove.
pub(super) struct SceneIter<'a> {
    scene: Scene<'a>,
    next: usize,
}

impl<'a> Iterator for SceneIter<'a> {
    type Item = CompositorLayer<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.next;
        self.next += 1;
        match self.scene {
            Scene::Plain(layers) => layers.get(index).map(CompositorLayer::layer),
            Scene::Full(layers) => layers.get(index).copied(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len();
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for SceneIter<'_> {
    fn len(&self) -> usize {
        let total = match self.scene {
            Scene::Plain(layers) => layers.len(),
            Scene::Full(layers) => layers.len(),
        };
        total.saturating_sub(self.next)
    }
}

#[cfg(test)]
mod tests {
    use super::Scene;
    use crate::layer::{CompositeLayer, CompositorConfig, CompositorLayer, LayerPlacement};
    use aria_core::RgbaImage;

    /// Both scene shapes must present the same sequence to the render loop:
    /// the plain path promotes on visit, so a promotion that dropped or
    /// reordered layers would silently change what gets composited.
    #[test]
    fn plain_and_full_scenes_walk_the_same_layers() {
        let a = RgbaImage::new(1, 1, vec![1, 2, 3, 4]);
        let b = RgbaImage::new(1, 1, vec![5, 6, 7, 8]);
        let config = CompositorConfig::new(4, 4);
        let plain = [
            CompositeLayer::rgba(&a, LayerPlacement::full_canvas(&config)),
            CompositeLayer::rgba(&b, LayerPlacement::full_canvas(&config)),
        ];
        let full = [
            CompositorLayer::layer(&plain[0]),
            CompositorLayer::layer(&plain[1]),
        ];

        assert_eq!(
            layer_ptrs(Scene::Plain(&plain)),
            layer_ptrs(Scene::Full(&full))
        );
        assert_eq!(layer_ptrs(Scene::Plain(&plain)).len(), 2);
        assert_eq!(Scene::Full(&full).iter().len(), 2);
    }

    /// Identity of each visited layer, so a promotion that dropped, duplicated,
    /// or reordered one is visible.
    fn layer_ptrs(scene: Scene<'_>) -> Vec<usize> {
        scene
            .iter()
            .filter_map(|item| match item {
                CompositorLayer::Layer(layer) => Some(layer as *const CompositeLayer<'_> as usize),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_empty_scene_yields_nothing() {
        assert_eq!(Scene::Plain(&[]).iter().count(), 0);
        assert_eq!(Scene::Full(&[]).iter().count(), 0);
    }
}
