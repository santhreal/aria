//! The accent family table, held to the rules every family must meet.
//!
//! The class closed here is a family that reads well in the appearance
//! someone looked at and fails in the other, or a choice that drifts back to
//! violet. Every [`ThemeChoice::ALL`] member is checked in both appearances,
//! so a new family is covered the moment it is added. Not caught: whether a
//! family is pleasant, or whether a panel paints the accent where it should.

use super::*;

fn every_theme() -> impl Iterator<Item = (ThemeChoice, Appearance, Theme)> {
    ThemeChoice::ALL.into_iter().flat_map(|choice| {
        [Appearance::Dark, Appearance::Light]
            .into_iter()
            .map(move |appearance| (choice, appearance, Theme::for_choice(appearance, choice)))
    })
}

fn hue_degrees(color: Hsla) -> f32 {
    color.h * 360.0
}

/// The HSL hue band that reads as indigo, violet, or purple. The old indigo
/// accent sat at 236 degrees and its blue retint at 245.
const VIOLET: std::ops::RangeInclusive<f32> = 232.0..=335.0;

#[test]
fn every_family_meets_the_contrast_rules() {
    for (choice, appearance, t) in every_theme() {
        let at = format!("{choice:?} {appearance:?}");
        for (name, surface) in [
            ("bg", t.bg),
            ("surface", t.surface),
            ("surface_header", t.surface_header),
        ] {
            let ratio = contrast_ratio(t.accent, surface);
            assert!(
                ratio >= 4.5,
                "{at}: accent text on {name} {ratio:.2}:1 < 4.5:1"
            );
            let ratio = contrast_ratio(t.playhead, surface);
            assert!(ratio >= 3.0, "{at}: playhead on {name} {ratio:.2}:1 < 3:1");
        }
        let ratio = contrast_ratio(t.accent, t.surface_raised);
        assert!(
            ratio >= 3.0,
            "{at}: accent on surface_raised {ratio:.2}:1 < 3:1"
        );
        for (name, fill) in [("accent", t.accent), ("accent_strong", t.accent_strong)] {
            let ratio = contrast_ratio(t.on_accent, fill);
            assert!(
                ratio >= 4.5,
                "{at}: on_accent on {name} {ratio:.2}:1 < 4.5:1"
            );
        }
        let field = flatten(t.input_bg, t.surface);
        let ratio = contrast_ratio(t.caret, field);
        assert!(ratio >= 3.0, "{at}: caret in a field {ratio:.2}:1 < 3:1");
        let selected = flatten(t.selection, t.surface);
        let ratio = contrast_ratio(t.text, selected);
        assert!(
            ratio >= 4.5,
            "{at}: text on the selection {ratio:.2}:1 < 4.5:1"
        );
    }
}

#[test]
fn no_accent_token_is_violet() {
    for (choice, appearance, t) in every_theme() {
        for (name, color) in [
            ("accent", t.accent),
            ("accent_strong", t.accent_strong),
            ("caret", t.caret),
            ("selection", t.selection),
            ("playhead", t.playhead),
            ("value", t.value),
            ("track_video", t.track_video),
            ("track_audio", t.track_audio),
            ("track_text", t.track_text),
            ("track_generator", t.track_generator),
        ] {
            let hue = hue_degrees(color);
            assert!(
                color.s < 0.05 || !VIOLET.contains(&hue),
                "{choice:?} {appearance:?}: {name} is violet (hue {hue:.0}, saturation {:.2})",
                color.s
            );
        }
    }
}

#[test]
fn default_clip_labels_are_not_violet() {
    let t = Theme::dark();
    for (kind, label) in [
        ("video", aria_models::LabelColor::VIDEO),
        ("audio", aria_models::LabelColor::AUDIO),
        ("generator", aria_models::LabelColor::GENERATOR),
        ("nested", aria_models::LabelColor::NESTED),
        ("adjustment", aria_models::LabelColor::ADJUSTMENT),
    ] {
        let fill = t.label(label, false);
        let hue = hue_degrees(fill);
        assert!(
            !VIOLET.contains(&hue),
            "a {kind} clip defaults to {label:?}, which is violet (hue {hue:.0})"
        );
    }
}

#[test]
fn a_choice_moves_only_the_accent_family() {
    for appearance in [Appearance::Dark, Appearance::Light] {
        let base = Theme::for_appearance(appearance);
        let shipped = base.clone().with_choice(ThemeChoice::default());
        assert_eq!(
            shipped.accent, base.accent,
            "{appearance:?}: the shipped accent is not the default choice"
        );
        assert_eq!(shipped.playhead, base.playhead, "{appearance:?}");
        for choice in ThemeChoice::ALL {
            let t = base.clone().with_choice(choice);
            for (name, got, want) in [
                ("bg", t.bg, base.bg),
                ("text", t.text, base.text),
                ("value", t.value, base.value),
                ("danger", t.danger, base.danger),
                ("warning", t.warning, base.warning),
                ("success", t.success, base.success),
                ("track_video", t.track_video, base.track_video),
            ] {
                assert_eq!(got, want, "{choice:?} {appearance:?} moved {name}");
            }
        }
    }
}

#[test]
fn every_family_has_its_own_accent() {
    for appearance in [Appearance::Dark, Appearance::Light] {
        let accents: Vec<_> = ThemeChoice::ALL
            .into_iter()
            .map(|choice| (choice, Theme::for_choice(appearance, choice).accent))
            .collect();
        for (i, (a, ca)) in accents.iter().enumerate() {
            for (b, cb) in &accents[i + 1..] {
                assert_ne!(
                    ca, cb,
                    "{appearance:?}: {a:?} and {b:?} paint the same accent"
                );
            }
        }
    }
}
