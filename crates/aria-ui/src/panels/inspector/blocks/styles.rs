//! Layer-style and LUT blocks of the Look tab.
//!
//! One block per style, from `app::clip_styles::STYLES`; the color and numeric
//! rows inside it are generated from the property catalog, whose group name *is*
//! the style id (`shadow`, `glow`, `outline`, `background`). So the panel names
//! no property, and a swatch or a knob writes that one property, keyed at the
//! playhead when it is animated, rather than rebuilding the block.

use gpui::{IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::app::clip_styles::{STYLES, style_enabled};
use crate::cloud::CloudFeed;
use crate::motion::MotionKey;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::{
    Chip, Columns, chip_row, color_rows, ghosts_under_labels, param_rows, pick_button, switch_line,
};

use crate::components::section_label;

pub fn styles_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let styles = clip.styles;
    let any_on = STYLES.iter().any(|(id, _)| style_enabled(&styles, id));

    let blocks: Vec<_> = STYLES
        .iter()
        .map(|(id, label)| {
            let style = *id;
            let on = style_enabled(&styles, style);
            let colors = color_rows(theme, style, state, cx);
            let knobs = param_rows(theme, style, state, cx);

            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(switch_line(
                    theme,
                    (style, *label),
                    ("style-toggle", style),
                    on,
                    cx,
                    move |state| inspector_pick::apply(state, Pick::Style(style)),
                ))
                .when(on, |this| this.children(colors).children(knobs))
                .into_any_element()
        })
        .collect();
    let clear = any_on.then(|| {
        pick_button("styles-clear", "Clear styles", theme, cx, |state| {
            inspector_pick::apply(state, Pick::ClearStyles)
        })
        .into_any_element()
    });

    let lut = clip.lut.clone();
    let mut files = vec![Chip::other(pick_button(
        "lut-load",
        "Load .cube…",
        theme,
        cx,
        |state| {
            state.pick_and_load_lut();
            Ok(())
        },
    ))];
    if lut.is_some() {
        files.push(Chip::other(pick_button(
            "lut-clear",
            "Remove LUT",
            theme,
            cx,
            |state| inspector_pick::apply(state, Pick::ClearLut),
        )));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Layer styles"))
        .children(blocks)
        .when_some(clear, |this, clear| {
            this.child(ghosts_under_labels(Columns::Keyed, [clear]))
        })
        .child(section_label(theme, "LUT"))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "lut-file"),
            "File",
            Columns::Keyed,
            lut.as_ref()
                .map_or(SharedString::new_static("None"), |lut| {
                    SharedString::from(lut.name().to_string())
                }),
            files,
        ))
        .when(lut.is_some(), |this| {
            this.children(param_rows(theme, "lut", state, cx))
        })
        .child(section_label(theme, "Cloud LUTs"))
        // The block's lines start one ghost inset in, where its Load button's
        // label does, so it hangs to stand them on the label column.
        .child(ghosts_under_labels(
            Columns::Keyed,
            [div()
                .w_full()
                .child(crate::panels::cloud::cloud_catalog_block(
                    theme,
                    CloudFeed::Luts,
                    state,
                    cx,
                ))
                .into_any_element()],
        ))
}
