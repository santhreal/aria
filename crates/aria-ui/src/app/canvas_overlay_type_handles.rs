//! `AppState` overlay: the selected clip's clip type handles. Each one moves a
//! vec2 parameter of the type's generator, and a drag is one keyframe edit at
//! release, like the inspector's vec2 field.

use super::*;
use crate::types::UiTypeHandle;

impl AppState {
    /// Where each of the selected clip's clip type handles draws, as fractions
    /// of the layer: the drafted value for the handle a drag holds, the
    /// projected value for the rest. Empty with no selection, no type, or a type
    /// whose generator is not in the chain.
    pub fn overlay_type_handles(&self) -> Vec<[f32; 2]> {
        let Some(clip) = self.selected_ui_clip_ref() else {
            return Vec::new();
        };
        let Some(clip_type) = &clip.clip_type else {
            return Vec::new();
        };
        clip_type
            .handles
            .iter()
            .enumerate()
            .filter_map(|(index, handle)| match self.overlay_drag {
                Some(OverlayDrag::TypeHandle {
                    index: held, draft, ..
                }) if held == index => Some(draft),
                _ => {
                    let effect = clip.effects.get(handle.effect)?;
                    effect
                        .vec2(handle.param)
                        .or_else(|| effect.point3(handle.param).map(|[x, y, _]| [x, y]))
                }
            })
            .collect()
    }

    /// Take hold of the clip type handle at `index`. Inert with no selection,
    /// no such handle, or a generator slot holding no vec2.
    pub fn begin_type_handle_drag(&mut self, index: usize) {
        let Some(clip) = self.selected_clip() else {
            return;
        };
        let Some(ui) = self.selected_ui_clip_ref() else {
            return;
        };
        let Some(handle) = ui
            .clip_type
            .as_ref()
            .and_then(|clip_type| clip_type.handles.get(index))
            .copied()
        else {
            return;
        };
        let Some(start) = ui.effects.get(handle.effect).and_then(|effect| {
            effect
                .vec2(handle.param)
                .or_else(|| effect.point3(handle.param).map(|[x, y, _]| [x, y]))
        }) else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::TypeHandle {
            index,
            handle,
            start,
            draft: start,
        });
        self.engine.lock().set_effect_vec2_override(Some((
            clip,
            handle.effect,
            handle.param,
            start,
        )));
    }

    /// Hold `draft` as the dragged handle's value and show it in the preview
    /// without editing the project.
    pub(super) fn draft_type_handle(
        &mut self,
        index: usize,
        handle: UiTypeHandle,
        start: [f32; 2],
        draft: [f32; 2],
    ) {
        self.overlay_drag = Some(OverlayDrag::TypeHandle {
            index,
            handle,
            start,
            draft,
        });
        let Some(clip) = self.selected_clip() else {
            return;
        };
        self.engine.lock().set_effect_vec2_override(Some((
            clip,
            handle.effect,
            handle.param,
            draft,
        )));
    }
}
