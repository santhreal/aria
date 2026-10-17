//! The rows the fx badge on a clip bar opens.
//!
//! The menu lists every property of the clip the timeline's property rows
//! list (the transform, each effect's parameters, each mask's, the audio), and
//! choosing one makes it the property the bar draws as its line, which a drag
//! on the bar writes and whose keyframes the bar dots. The properties are data,
//! so they are generated from the clip rather than listed here; what each asks
//! for is [`draw_property`].

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Clip, ClipId, ClipParam, ParamSpec, default_line_param};
use gpui::SharedString;

use crate::app::AppState;
use crate::app::property_rows::PropertyGroup;
use crate::types::{ActivePanel, UiTrackKind};

use super::{MenuItem, MenuRow, always};

/// One property the badge menu offers.
#[derive(Clone, Debug, PartialEq)]
pub struct BadgeProperty {
    pub param: ClipParam,
    /// The row's text: the block the property is in, then the property, as a
    /// grouped context menu row reads (`Transform: Opacity`).
    pub label: SharedString,
}

/// The clip facts a badge menu row reads, gathered once when the menu opens.
#[derive(Clone, Debug, PartialEq)]
pub struct BadgeMenu {
    pub clip: u64,
    pub track: u64,
    /// The clip sits on an audio lane, whose bars draw Volume by default.
    pub audio: bool,
    /// The property the bar draws now.
    pub drawn: Option<ClipParam>,
    /// A property was chosen for the bar, so the lane's default is not drawn.
    pub chosen: bool,
    /// Every property the bar can draw, in property-row order.
    pub properties: Arc<[BadgeProperty]>,
}

impl BadgeMenu {
    /// Read the facts a menu would gather when it opens on the badge of
    /// `clip`, or `None` when the clip is no longer projected.
    pub fn of(state: &AppState, clip: u64) -> Option<Self> {
        let (track, audio) = state.store.sequence.tracks.iter().find_map(|lane| {
            lane.clips
                .iter()
                .any(|c| c.id == clip)
                .then_some((lane.id, lane.kind == UiTrackKind::Audio))
        })?;
        let drawn = state
            .ui_clip_ref(ClipId::from_raw(clip))?
            .line
            .as_ref()
            .map(|line| line.param);
        let engine = state.engine().lock();
        let model = engine.project().clip(ClipId::from_raw(clip))?;
        let properties = model
            .line_rows(audio)
            .into_iter()
            .filter_map(|spec| {
                let block = block_label(model, &spec)?;
                Some(BadgeProperty {
                    param: spec.param,
                    label: format!("{block}: {}", spec.label).into(),
                })
            })
            .collect();
        Some(Self {
            clip,
            track,
            audio,
            drawn,
            chosen: model.line_param.is_some(),
            properties,
        })
    }

    fn clip(&self) -> ClipId {
        ClipId::from_raw(self.clip)
    }
}

/// The block a property row is listed under, or `None` for a property the
/// timeline's property rows do not list.
fn block_label(clip: &Clip, spec: &ParamSpec) -> Option<String> {
    let group = PropertyGroup::of(spec)?;
    Some(match (group, spec.param) {
        (PropertyGroup::Effects, ClipParam::Effect { effect, .. }) => {
            let instance = clip.effects.get(usize::try_from(effect).ok()?)?;
            instance.name.clone().unwrap_or_else(|| {
                aria_models::effect_spec(&instance.effect_id)
                    .map_or_else(|| instance.effect_id.clone(), |spec| spec.label.clone())
            })
        }
        (PropertyGroup::Masks, ClipParam::Mask { mask, .. }) => format!("Mask {}", mask + 1),
        (PropertyGroup::Transform, _) => "Transform".to_owned(),
        (PropertyGroup::Audio, _) => "Audio".to_owned(),
        (PropertyGroup::Effects | PropertyGroup::Masks, _) => return None,
    })
}

/// Make `param` the property the bar of `menu`'s clip draws.
pub fn draw_property(
    state: &mut AppState,
    menu: &BadgeMenu,
    param: ClipParam,
) -> Result<(), EngineError> {
    // The lane's default is stored as no choice, so the bar follows the lane
    // when the clip moves between kinds of lane.
    let param = (param != default_line_param(menu.audio)).then_some(param);
    state
        .apply_command(Command::Edit(EditCommand::SetClipLineParam {
            clip: menu.clip(),
            param,
        }))
        .map(drop)
}

/// A badge menu, in the order it draws.
pub const BADGE_ITEMS: &[MenuItem<BadgeMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-badge-controls",
        label: |_| "Show in Effect Controls",
        when: always(),
        run: |state, menu| {
            state.select_clip(menu.clip());
            state.focus_pane(ActivePanel::Inspector);
            Ok(())
        },
    }),
    MenuItem::Properties,
];
