//! Track-matte block of the Look tab.
//!
//! Source buttons come from `MatteSource::ALL`, so a new read channel in the
//! model reaches the inspector without a second list here. The stencil is not
//! picked in this block: the renderer reads whatever layer sits directly above,
//! so the block states whether anything is up there rather than offering a lane
//! chooser that cannot bind.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use aria_models::MatteSource;

use crate::motion::MotionKey;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::{
    Chip, Columns, chip_row, choice_chip, label_note, param_rows, pick_button, switch_line,
};

use crate::components::section_label;

pub fn matte_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let matte = clip.matte;
    let stencil_ready = state.read(cx).matte_stencil_ready();

    let mut sources: Vec<_> = MatteSource::ALL
        .iter()
        .map(|source| {
            let source = *source;
            let on = matte.map(|m| m.source) == Some(source);
            choice_chip(
                ("matte-source", source.id()),
                source.label(),
                on,
                theme,
                cx,
                move |state| inspector_pick::apply(state, Pick::Matte(source)),
            )
        })
        .collect();
    if matte.is_some() {
        sources.push(Chip::other(pick_button(
            "matte-clear",
            "Remove matte",
            theme,
            cx,
            |state| inspector_pick::apply(state, Pick::ClearMatte),
        )));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Track matte"))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "matte-source"),
            "Channel",
            Columns::Keyed,
            matte.map_or("Off", |matte| matte.source.label()),
            sources,
        ))
        .when(matte.is_some() && !stencil_ready, |this| {
            this.child(label_note(
                Columns::Keyed,
                theme.warning,
                "Nothing above this clip to cut with",
            ))
        })
        .when_some(matte, |this, matte| {
            this.children(param_rows(theme, "matte", state, cx))
                .child(switch_line(
                    theme,
                    MotionKey::new("insp.row", "matte-inv"),
                    "Invert",
                    Columns::Keyed,
                    "matte-invert",
                    Some(matte.invert),
                    cx,
                    |state| inspector_pick::apply(state, Pick::ToggleMatteInvert),
                ))
        })
}
