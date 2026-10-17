//! Typography, Icons, and the theme tokens.
//!
//! The icon section is self-checking: every constant is drawn with its name,
//! every constant is loaded through the shipped `AssetSource` so a wrong path
//! shows up as a red row, and the asset source is enumerated so an icon that
//! exists in `aria-ui` but not in this list shows up as an unlisted row.

use aria_ui::components;
use aria_ui::icons;
use aria_ui::theme::Theme;
use gpui::{AnyElement, AssetSource, IntoElement, ParentElement, Styled, div, px};

use crate::gallery_kit::{caption, group, section, shelf, specimen, swatch};

/// Every `pub const` in `aria_ui::icons`.
///
/// `icons.rs` exposes no iterable list, and `Assets::list` hands back
/// `SharedString` while `icon_button` demands `&'static str`, so the paths
/// cannot round-trip through the asset source. The cross-check below is what
/// keeps this list honest instead.
const ICONS: [&str; 32] = [
    icons::PLAY,
    icons::PAUSE,
    icons::PLUS,
    icons::FOLDER_OPEN,
    icons::CLAPPERBOARD,
    icons::EYE,
    icons::EYE_OFF,
    icons::LOCK,
    icons::LOCK_OPEN,
    icons::VOLUME,
    icons::VOLUME_X,
    icons::UPLOAD,
    icons::SETTINGS,
    icons::TRASH,
    icons::FIT,
    icons::SCISSORS,
    icons::ZOOM_IN,
    icons::ZOOM_OUT,
    icons::MAGNET,
    icons::MINUS,
    icons::SQUARE,
    icons::CLOSE,
    icons::SLIP,
    icons::SLIDE,
    icons::ROLL,
    icons::RATE,
    icons::RIPPLE,
    icons::SKIP_BACK,
    icons::SKIP_FORWARD,
    icons::CHEVRON_LEFT,
    icons::CHEVRON_RIGHT,
    icons::CHEVRON_DOWN,
];

/// The raster rows: art the theme does not tint, drawn at a size instead of a
/// text colour. Listed separately from the glyphs because they are drawn by a
/// different element, and cross-checked the same way so a raster asset added
/// without a row here shows up as unlisted.
const RASTER: [&str; 1] = [icons::BRAND_MARK];

pub fn icons_section(theme: &Theme) -> AnyElement {
    let listed = icons::Assets.list("icon/").unwrap_or_default();
    let unlisted: Vec<String> = listed
        .iter()
        .filter(|path| {
            !ICONS
                .iter()
                .chain(RASTER.iter())
                .any(|known| *known == path.as_str())
        })
        .map(|path| path.to_string())
        .collect();
    let specimens: Vec<AnyElement> = ICONS.into_iter().map(|path| glyph(theme, path)).collect();
    section(
        theme,
        "Icons",
        "every icons:: constant, drawn through icon_button and checked against the asset source",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(shelf(specimens))
            .child(group(
                theme,
                "icons::brand_mark(size_px)",
                shelf(
                    RASTER
                        .into_iter()
                        .flat_map(|path| {
                            [24.0f32, 72.0].map(move |size| {
                                specimen(
                                    theme,
                                    stem(path),
                                    icons::brand_mark(size).into_any_element(),
                                )
                            })
                        })
                        .collect(),
                ),
            ))
            .child(caption(
                theme,
                format!(
                    "{} constants drawn; the asset source lists {} paths under icon/.",
                    ICONS.len(),
                    listed.len()
                ),
            ))
            .child(if unlisted.is_empty() {
                caption(theme, "No unlisted icons: the gallery covers every asset.")
            } else {
                div()
                    .p(px(Theme::SPACE_SM))
                    .rounded(px(Theme::CONTROL_RADIUS))
                    .border_1()
                    .border_color(theme.danger)
                    .text_size(px(11.))
                    .text_color(theme.danger)
                    .child(format!(
                        "In aria-ui but not in this gallery: {}",
                        unlisted.join(", ")
                    ))
                    .into_any_element()
            })
            .child(caption(
                theme,
                "A `pub const ALL: &[(&str, &str)]` in icons.rs would make this \
                 section self-maintaining; today the list above is manual and only \
                 the cross-check catches drift.",
            ))
            .into_any_element(),
    )
    .into_any_element()
}

/// One icon: the control, its name, and — when the asset will not load — the
/// path that failed, in danger colour.
fn glyph(theme: &Theme, path: &'static str) -> AnyElement {
    let name = stem(path);
    let present = matches!(icons::Assets.load(path), Ok(Some(_)));
    if present {
        specimen(theme, name, components::icon_button(name, path, theme))
    } else {
        specimen(
            theme,
            name,
            div()
                .text_size(px(9.))
                .text_color(theme.danger)
                .child(format!("missing {path}")),
        )
    }
}

/// `icon/eye-off.svg` -> `eye-off`, `icon/aria-icon.png` -> `aria-icon`,
/// without allocating.
fn stem(path: &'static str) -> &'static str {
    let file = match path.rsplit_once('/') {
        Some((_, file)) => file,
        None => path,
    };
    match file.rsplit_once('.') {
        Some((name, _)) => name,
        None => file,
    }
}

pub fn typography(theme: &Theme) -> AnyElement {
    section(
        theme,
        "Typography",
        "the font and text tokens, at the sizes the components use",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(group(
                theme,
                "families",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XS))
                    .child(sample(theme, theme.font_sans.clone(), "font_sans"))
                    .child(sample(theme, theme.font_mono.clone(), "font_mono"))
                    .child(sample(
                        theme,
                        theme.font_sans_fallback.clone(),
                        "font_sans_fallback",
                    ))
                    .child(sample(
                        theme,
                        theme.font_mono_fallback.clone(),
                        "font_mono_fallback",
                    )),
            ))
            .child(group(
                theme,
                "sizes",
                div().flex().flex_col().gap(px(Theme::SPACE_XS)).children(
                    [9.0f32, 10.0, 11.0, 12.0, 14.0, 18.0, 28.0].map(|size| {
                        div()
                            .text_size(px(size))
                            .text_color(theme.text)
                            .child(format!("{size:.0}px — Aria timeline 00:00:12:04"))
                    }),
                ),
            ))
            .child(group(
                theme,
                "weights",
                shelf(vec![
                    weight(theme, "NORMAL", gpui::FontWeight::NORMAL),
                    weight(theme, "MEDIUM", gpui::FontWeight::MEDIUM),
                    weight(theme, "SEMIBOLD", gpui::FontWeight::SEMIBOLD),
                    weight(theme, "BOLD", gpui::FontWeight::BOLD),
                ]),
            ))
            .child(group(
                theme,
                "text colour tokens",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XS))
                    .child(ink(theme, "text", theme.text))
                    .child(ink(theme, "text_muted", theme.text_muted))
                    .child(ink(theme, "text_faint", theme.text_faint))
                    .child(ink(theme, "text_dim", theme.text_dim))
                    .child(ink(theme, "accent", theme.accent))
                    .child(ink(theme, "danger", theme.danger))
                    .child(ink(theme, "warning", theme.warning))
                    .child(ink(theme, "success", theme.success)),
            ))
            .child(caption(
                theme,
                "Geist is embedded in aria-ui and registered by a private \
                 `register_fonts`, so this crate loads the same .ttf bytes itself; \
                 if a family here renders as the system face, that registration \
                 failed.",
            ))
            .into_any_element(),
    )
    .into_any_element()
}

fn sample(theme: &Theme, family: gpui::SharedString, label: &str) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(150.))
                .text_size(px(10.))
                .text_color(theme.text_faint)
                .child(format!("{label} = {family}")),
        )
        .child(
            div()
                .font_family(family)
                .text_size(px(14.))
                .text_color(theme.text)
                .child("Aria 0123 — ABCabc gjpqy il1 O0"),
        )
        .into_any_element()
}

fn weight(theme: &Theme, label: &str, w: gpui::FontWeight) -> AnyElement {
    specimen(
        theme,
        label,
        div()
            .font_weight(w)
            .text_size(px(14.))
            .text_color(theme.text)
            .child("Render"),
    )
}

fn ink(theme: &Theme, label: &str, color: gpui::Hsla) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(120.))
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_faint)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(color)
                .child("The quick brown fox jumps over the lazy dog"),
        )
        .into_any_element()
}

/// Every colour token on the theme, so a palette change is reviewable in one
/// place rather than hunted through panels.
pub fn theme_tokens(theme: &Theme) -> AnyElement {
    section(
        theme,
        "Theme",
        "every colour token, in the appearance selected in the header",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(group(
                theme,
                "surfaces",
                shelf(vec![
                    swatch(theme, "bg", theme.bg),
                    swatch(theme, "surface", theme.surface),
                    swatch(theme, "surface_header", theme.surface_header),
                    swatch(theme, "surface_raised", theme.surface_raised),
                    swatch(theme, "surface_raised_hover", theme.surface_raised_hover),
                    swatch(theme, "surface_card", theme.surface_card),
                    swatch(theme, "surface_dialog", theme.surface_dialog),
                    swatch(theme, "surface_overlay", theme.surface_overlay),
                    swatch(theme, "element_hover", theme.element_hover),
                    swatch(theme, "element_active", theme.element_active),
                    swatch(theme, "band", theme.band),
                    swatch(theme, "input_bg", theme.input_bg),
                ]),
            ))
            .child(group(
                theme,
                "lines and text",
                shelf(vec![
                    swatch(theme, "border", theme.border),
                    swatch(theme, "border_strong", theme.border_strong),
                    swatch(theme, "hairline", theme.hairline),
                    swatch(theme, "text", theme.text),
                    swatch(theme, "text_muted", theme.text_muted),
                    swatch(theme, "text_faint", theme.text_faint),
                    swatch(theme, "text_dim", theme.text_dim),
                    swatch(theme, "value", theme.value),
                    swatch(theme, "selection", theme.selection),
                    swatch(theme, "cursor", theme.cursor),
                    swatch(theme, "caret", theme.caret),
                ]),
            ))
            .child(group(
                theme,
                "intent",
                shelf(vec![
                    swatch(theme, "solid", theme.solid),
                    swatch(theme, "on_solid", theme.on_solid),
                    swatch(theme, "accent", theme.accent),
                    swatch(theme, "accent_strong", theme.accent_strong),
                    swatch(theme, "on_accent", theme.on_accent),
                    swatch(theme, "danger", theme.danger),
                    swatch(theme, "danger_strong", theme.danger_strong),
                    swatch(theme, "danger_muted", theme.danger_muted),
                    swatch(theme, "warning", theme.warning),
                    swatch(theme, "warning_muted", theme.warning_muted),
                    swatch(theme, "success", theme.success),
                    swatch(theme, "success_muted", theme.success_muted),
                    swatch(theme, "busy", theme.busy),
                ]),
            ))
            .child(group(
                theme,
                "timeline",
                shelf(vec![
                    swatch(theme, "track_video", theme.track_video),
                    swatch(theme, "track_audio", theme.track_audio),
                    swatch(theme, "track_text", theme.track_text),
                    swatch(theme, "track_generator", theme.track_generator),
                    swatch(theme, "playhead", theme.playhead),
                    swatch(theme, "snap_guide", theme.snap_guide),
                    swatch(theme, "waveform_peak", theme.waveform_peak),
                    swatch(theme, "waveform_rms", theme.waveform_rms),
                    swatch(theme, "recording_dot", theme.recording_dot),
                ]),
            ))
            .child(group(
                theme,
                "ramps — wash(a) / ink(a) / hairline(a)",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_SM))
                    .child(ramp(theme, "wash", |t, a| t.wash(a)))
                    .child(ramp(theme, "ink", |t, a| t.ink(a)))
                    .child(ramp(theme, "hairline", |t, a| t.hairline(a))),
            ))
            .into_any_element(),
    )
    .into_any_element()
}

fn ramp(theme: &Theme, label: &str, f: impl Fn(&Theme, f32) -> gpui::Hsla) -> AnyElement {
    let alphas = [0.02f32, 0.04, 0.08, 0.14, 0.24, 0.40, 0.70, 1.0];
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(72.))
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_faint)
                .child(label.to_string()),
        )
        .children(alphas.map(|a| {
            div()
                .relative()
                .w(px(52.))
                .h(px(32.))
                .overflow_hidden()
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_card)
                .border_1()
                .border_color(theme.border)
                .child(div().absolute().inset_0().bg(f(theme, a)))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .text_color(theme.text)
                        .child(format!("{a:.2}")),
                )
        }))
        .into_any_element()
}
