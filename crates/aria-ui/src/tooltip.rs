//! What an icon-only control is called, and what key does the same thing.
//!
//! One owner, because a glyph is not a name: the tool strip, the transport, the
//! track headers, and the effect rows were all icon-only, so the only way to
//! learn what a control did was to press it. Every one of them now names itself
//! on hover, with its shortcut when it has one.
//!
//! The shortcut is not typed in twice. A tip names a binding id, and the chord is
//! read from the shipped binding table, so re-chording a key moves the tooltip
//! with it and a tip that names a binding which no longer exists is caught by the
//! test below rather than shown as a lie.

use gpui::{AnyView, App, AppContext, IntoElement, ParentElement, Render, Styled, Window, div, px};

use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::theme::Theme;

/// One named control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlTip {
    /// The control key: the literal a control passes when it asks for its own
    /// tip, which for most controls is also its element id. A row drawn once per
    /// item keys on its kind, because its element id carries the index.
    pub control: &'static str,
    /// What the control is called, in the words the docs use.
    pub name: &'static str,
    /// The binding whose chord this control repeats, if any.
    pub binding: Option<&'static str>,
}

/// Every icon-only control that names itself.
pub const CONTROL_TIPS: &[ControlTip] = &[
    // Tool strip.
    ControlTip {
        control: "tool-pointer",
        name: "Pointer",
        binding: Some("key-tool-pointer"),
    },
    ControlTip {
        control: "tool-razor",
        name: "Razor",
        binding: Some("key-tool-razor"),
    },
    ControlTip {
        control: "tool-hand",
        name: "Hand",
        binding: Some("key-tool-hand"),
    },
    ControlTip {
        control: "tool-zoom",
        name: "Zoom",
        binding: Some("key-tool-zoom"),
    },
    ControlTip {
        control: "tool-slip",
        name: "Slip — move the take inside the clip",
        binding: Some("key-tool-slip"),
    },
    ControlTip {
        control: "tool-slide",
        name: "Slide — move the clip, trim its neighbours",
        binding: Some("key-tool-slide"),
    },
    ControlTip {
        control: "tool-roll",
        name: "Rolling edit — move the cut",
        binding: Some("key-tool-roll"),
    },
    ControlTip {
        control: "tool-rate",
        name: "Rate stretch — retime without trimming",
        binding: Some("key-tool-rate"),
    },
    ControlTip {
        control: "tool-ripple",
        name: "Ripple trim — trim and close the gap",
        binding: Some("key-tool-ripple"),
    },
    ControlTip {
        control: "bin-new",
        name: "New bin in this folder",
        binding: None,
    },
    ControlTip {
        control: "bin-rename",
        name: "Rename this bin",
        binding: None,
    },
    ControlTip {
        control: "bin-delete",
        name: "Delete this bin; its contents move up",
        binding: None,
    },
    ControlTip {
        control: "snap",
        name: "Snapping",
        binding: None,
    },
    ControlTip {
        control: "toolbar-tc",
        name: "Playhead timecode",
        binding: None,
    },
    // Transport, in the tool strip and under the preview.
    ControlTip {
        control: "go-start",
        name: "Go to start",
        binding: Some("key-start"),
    },
    ControlTip {
        control: "prev-frame",
        name: "Step back one frame",
        binding: Some("key-step-back"),
    },
    ControlTip {
        control: "play",
        name: "Play",
        binding: Some("key-play"),
    },
    ControlTip {
        control: "pause",
        name: "Pause",
        binding: Some("key-play"),
    },
    ControlTip {
        control: "preview-shuttle",
        name: "Shuttle speed — silent above 1x, click for 1x",
        binding: Some("key-shuttle-forward"),
    },
    ControlTip {
        control: "next-frame",
        name: "Step forward one frame",
        binding: Some("key-step-forward"),
    },
    ControlTip {
        control: "go-end",
        name: "Go to end",
        binding: Some("key-end"),
    },
    // Preview view.
    ControlTip {
        control: "fit-timeline",
        name: "Fit the sequence to the window",
        binding: Some("key-fit"),
    },
    ControlTip {
        control: "preview-zoom-out",
        name: "Zoom the preview out",
        binding: None,
    },
    ControlTip {
        control: "preview-zoom-in",
        name: "Zoom the preview in",
        binding: None,
    },
    ControlTip {
        control: "preview-zoom-label",
        name: "Fit the frame to the pane",
        binding: None,
    },
    ControlTip {
        control: "preview-handles",
        name: "Cycle the on-canvas handles",
        binding: None,
    },
    ControlTip {
        control: "preview-meter",
        name: "Output peaks — click to clear the clip",
        binding: None,
    },
    // Source monitor.
    ControlTip {
        control: "source-close",
        name: "Back to the sequence",
        binding: Some("key-play"),
    },
    ControlTip {
        control: "source-mark-in",
        name: "Mark the source in point",
        binding: Some("key-mark-in"),
    },
    ControlTip {
        control: "source-mark-out",
        name: "Mark the source out point",
        binding: Some("key-mark-out"),
    },
    ControlTip {
        control: "source-mark-clear",
        name: "Clear both source marks",
        binding: None,
    },
    ControlTip {
        control: "source-insert",
        name: "Insert the marked range, rippling the lane",
        binding: Some("key-source-insert"),
    },
    ControlTip {
        control: "source-overwrite",
        name: "Overwrite with the marked range",
        binding: Some("key-source-overwrite"),
    },
    // Timeline.
    ControlTip {
        control: "add-track",
        name: "Add a track",
        binding: None,
    },
    // Multicam. A frame nudge and an audio sync are glyphs on a dense row of
    // camera strips, so the words live here.
    ControlTip {
        control: "nudge-multicam-back",
        name: "Nudge this angle one frame earlier",
        binding: None,
    },
    ControlTip {
        control: "nudge-multicam-forward",
        name: "Nudge this angle one frame later",
        binding: None,
    },
    ControlTip {
        control: "sync-multicam-group",
        name: "Sync these angles from their audio",
        binding: None,
    },
    // Inspector resets.
    ControlTip {
        control: "inspector-reset",
        name: "Reset these values",
        binding: None,
    },
    ControlTip {
        control: "inspector-wheel-reset",
        name: "Reset this colour wheel",
        binding: None,
    },
    // Effect rows.
    ControlTip {
        control: "inspector-fx-up",
        name: "Move this effect earlier",
        binding: None,
    },
    ControlTip {
        control: "inspector-fx-down",
        name: "Move this effect later",
        binding: None,
    },
    ControlTip {
        control: "inspector-fx-remove",
        name: "Remove this effect",
        binding: None,
    },
    // Audio insert rows, in the inspector's Audio block and on a mixer strip.
    ControlTip {
        control: "insert-up",
        name: "Move this audio effect earlier",
        binding: None,
    },
    ControlTip {
        control: "insert-down",
        name: "Move this audio effect later",
        binding: None,
    },
    ControlTip {
        control: "insert-remove",
        name: "Remove this audio effect",
        binding: None,
    },
    // The bar's own controls: the two rails, the settings dialog, and the three
    // window buttons. Each is a glyph with no word beside it.
    ControlTip {
        control: "rail-sidebar",
        name: "Media sidebar",
        binding: None,
    },
    ControlTip {
        control: "rail-inspector",
        name: "Inspector rail",
        binding: None,
    },
    ControlTip {
        control: "settings",
        name: "Settings",
        binding: Some("key-settings"),
    },
    ControlTip {
        control: "caption-min",
        name: "Minimise the window",
        binding: None,
    },
    ControlTip {
        control: "caption-max",
        name: "Maximise the window",
        binding: None,
    },
    ControlTip {
        control: "caption-close",
        name: "Close the window",
        binding: None,
    },
];

/// The tip for one control id, or nothing when the control is not named.
pub fn tip_for(control: &str) -> Option<ControlTip> {
    CONTROL_TIPS
        .iter()
        .copied()
        .find(|tip| tip.control == control)
}

/// How a tip's shortcut reads now, or nothing when it has none.
///
/// Read from the mounted registry, so a rebound key is named as the operator
/// bound it and a cleared key claims no chord at all. Falls back to the shipped
/// table when no shell has published a registry, which is what a unit test and a
/// headless render see.
pub fn chord_label(tip: &ControlTip, cx: &App) -> Option<String> {
    let id = tip.binding?;
    match crate::keys::mounted(cx) {
        Some(host) => host.chord_of(id).map(|chord| chord.label()),
        None => shipped_label(id),
    }
}

/// The chord a binding shipped with, ignoring any rebinding.
pub fn shipped_label(id: &str) -> Option<String> {
    SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.id == id)
        .map(|binding| binding.chord.label())
}

/// The hover card itself: the name, and the chord beside it.
pub struct Tip {
    name: &'static str,
    chord: Option<String>,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let chord = self.chord.clone();
        div()
            .flex()
            .items_center()
            .gap(px(Theme::SPACE_SM))
            .px(px(Theme::SPACE_SM))
            .py(px(Theme::SPACE_XS))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.surface_card)
            .border_1()
            .border_color(theme.border)
            .text_size(px(12.))
            .text_color(theme.text)
            .child(self.name)
            .children(chord.map(|chord| {
                div()
                    .font_family(theme.font_mono.clone())
                    .text_size(px(11.))
                    .text_color(theme.text_faint)
                    .child(chord)
            }))
    }
}

/// The tooltip builder for one control id, or nothing when it is not named.
///
/// Returns a builder rather than an element because GPUI asks for the view only
/// when the pointer has rested: an unhovered control costs nothing. The chord is
/// resolved inside the builder rather than captured, so a key rebound since the
/// button was drawn is named as it is now.
pub fn builder(control: &str) -> Option<impl Fn(&mut Window, &mut App) -> AnyView + use<>> {
    let tip = tip_for(control)?;
    Some(move |_: &mut Window, cx: &mut App| {
        let name = tip.name;
        let chord = chord_label(&tip, cx);
        cx.new(|_| Tip { name, chord }).into()
    })
}

/// A tooltip whose registry and keybinding work starts only after GPUI's hover
/// delay has elapsed.
pub fn deferred_builder(
    control: &'static str,
) -> impl Fn(&mut Window, &mut App) -> AnyView + use<> {
    move |_: &mut Window, cx: &mut App| {
        let tip = tip_for(control).unwrap_or_else(|| panic!("{control} has no registered tooltip"));
        let name = tip.name;
        let chord = chord_label(&tip, cx);
        cx.new(|_| Tip { name, chord }).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tip that names a binding which was renamed or dropped would show no
    /// chord at all, silently. The table is enumerated, so a rename breaks here
    /// rather than in a hover nobody tested.
    #[test]
    fn every_named_binding_exists() {
        for tip in CONTROL_TIPS {
            if let Some(id) = tip.binding {
                assert!(
                    SHIPPED_BINDINGS.iter().any(|binding| binding.id == id),
                    "{} names the binding {id}, which no longer exists",
                    tip.control
                );
                assert!(
                    shipped_label(id).is_some(),
                    "{} resolves no chord",
                    tip.control
                );
            }
        }
    }

    #[test]
    fn a_control_is_named_once_and_the_names_differ() {
        for tip in CONTROL_TIPS {
            let same: Vec<&str> = CONTROL_TIPS
                .iter()
                .filter(|other| other.control == tip.control)
                .map(|other| other.name)
                .collect();
            assert_eq!(same.len(), 1, "{} has {same:?}", tip.control);
            let sharing: Vec<&str> = CONTROL_TIPS
                .iter()
                .filter(|other| other.name == tip.name)
                .map(|other| other.control)
                .collect();
            assert_eq!(
                sharing.len(),
                1,
                "{:?} all call themselves {}",
                sharing,
                tip.name
            );
        }
    }

    #[test]
    fn a_name_is_a_name_and_not_a_glyph() {
        for tip in CONTROL_TIPS {
            assert!(
                tip.name.chars().count() > 2,
                "{} is called {:?}, which names nothing",
                tip.control,
                tip.name
            );
            assert!(
                tip.name
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_uppercase()),
                "{:?} does not start with a capital",
                tip.name
            );
        }
    }

    #[test]
    fn an_unnamed_control_has_no_tip_rather_than_an_empty_one() {
        assert!(tip_for("no-such-control").is_none());
        assert!(builder("no-such-control").is_none());
    }

    /// The fallback path, which is also what every headless render sees: no
    /// registry published, so the shipped chord is what a hover names.
    #[test]
    fn a_chord_reads_the_way_settings_shows_it() {
        let play = tip_for("play").expect("play is named");
        assert_eq!(
            shipped_label(play.binding.expect("play has a key")).as_deref(),
            Some("space")
        );
        let pointer = tip_for("tool-pointer").expect("the pointer tool is named");
        assert_eq!(
            shipped_label(pointer.binding.expect("the pointer tool has a key")).as_deref(),
            Some("v")
        );
        let add = tip_for("add-track").expect("add track is named");
        assert_eq!(add.binding, None, "it has no shortcut to claim");
        assert_eq!(shipped_label("key-no-such-binding"), None);
    }

    /// Every icon button ID and tooltip builder reference across all source files
    /// must exist in CONTROL_TIPS.
    ///
    /// WHY: the id is read by scanning forward from the call across whitespace and
    /// the optional tuple paren, not by looking at one line, because a call
    /// formatted over several lines used to leave its glyph nameless on hover
    /// while this census still passed.
    ///
    /// The needle is assembled with `concat!` so the literal `icon_button(`
    /// never appears in this file. Spelled whole, the scan matched its own
    /// needle, took the literal's closing quote for the opening quote of an id,
    /// and asserted on a slab of its own source: a census that can only fail
    /// reports nothing about the product. The found-ids floor is the other half
    /// of that: a scanner that matches nothing passes every `contains` check it
    /// never runs.
    #[test]
    fn every_icon_button_id_in_chrome_and_panels_is_registered_in_control_tips() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let tipped_ids: std::collections::HashSet<&str> =
            CONTROL_TIPS.iter().map(|tip| tip.control).collect();
        // Chrome wraps `icon_button` in `icon_act` / `tool_btn`. Those still
        // name the tooltip id in the first argument; scanning only the inner
        // helper misses the settings and transport rows.
        let needles = [
            concat!("icon_", "button("),
            concat!("icon_", "act("),
            concat!("tool_", "btn("),
        ];
        let mut found: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut stack = vec![src];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(current).expect("read dir") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_some_and(|ext| ext == "rs")
                    && !path.to_string_lossy().ends_with("_tests.rs")
                {
                    let content = std::fs::read_to_string(&path).expect("read file");
                    for needle in needles {
                        let mut cursor = 0;
                        while let Some(pos) = content[cursor..].find(needle) {
                            let after = cursor + pos + needle.len();
                            cursor = after;
                            let rest = content[after..]
                                .trim_start_matches(|c: char| c.is_whitespace() || c == '(');
                            let Some(quoted) = rest.strip_prefix('"') else {
                                continue;
                            };
                            let Some(id) = quoted.split('"').next() else {
                                continue;
                            };
                            if id.is_empty() || id.contains('{') || id.contains('$') {
                                continue;
                            }
                            assert!(
                                tipped_ids.contains(id),
                                "Icon button ID {:?} in {} has no entry in CONTROL_TIPS",
                                id,
                                path.display()
                            );
                            found.insert(id.to_string());
                        }
                    }
                }
            }
        }

        assert!(
            found.contains("settings"),
            "the scan reached no call site: {} ids found, and the settings button \
             in the tool strip was not one of them",
            found.len()
        );
    }
}
