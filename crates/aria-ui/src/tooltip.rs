//! What an icon-only control is called, and what key does the same thing.
//!
//! One owner, because a glyph is not a name: the tool bar, the transport, the
//! track headers, and the effect rows were all icon-only, so the only way to
//! learn what a control did was to press it. Every one of them now names itself
//! on hover, with its shortcut when it has one.
//!
//! The shortcut is not typed in twice. A tip names a binding id, and the chord is
//! read from the shipped binding table, so re-chording a key moves the tooltip
//! with it and a tip that names a binding which no longer exists is caught by the
//! test below rather than shown as a lie.

use std::time::Duration;

use gpui::{
    AnyView, App, AppContext, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};

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
    // Tool bar.
    ControlTip {
        control: "tool-pointer",
        name: "Pointer",
        binding: Some("key-tool-pointer"),
    },
    ControlTip {
        control: "tool-track-forward",
        name: "Track Select Forward — select from here on",
        binding: Some("key-tool-track-forward"),
    },
    ControlTip {
        control: "tool-track-backward",
        name: "Track Select Backward — select up to here",
        binding: Some("key-tool-track-backward"),
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
        control: "tool-pen",
        name: "Pen — add keyframes or draw masks",
        binding: Some("key-tool-pen"),
    },
    ControlTip {
        control: "tool-type",
        name: "Type — create or edit text in the Viewer",
        binding: Some("key-tool-type"),
    },
    ControlTip {
        control: "bin-new",
        name: "New bin",
        binding: None,
    },
    // The Project panel's pool bars: the filter's Import and Audio switch, the
    // views, and the multicam maker.
    ControlTip {
        control: "media-import",
        name: "Import",
        binding: None,
    },
    ControlTip {
        control: "pool-audio-only",
        name: "Audio only",
        binding: None,
    },
    ControlTip {
        control: "pool-view-tiles",
        name: "Tiles",
        binding: None,
    },
    ControlTip {
        control: "pool-view-list",
        name: "List",
        binding: None,
    },
    ControlTip {
        control: "pool-view-icons",
        name: "Icons",
        binding: None,
    },
    ControlTip {
        control: "pool-view-freeform",
        name: "Freeform",
        binding: None,
    },
    ControlTip {
        control: "create-multicam-group",
        name: "New multicam",
        binding: None,
    },
    ControlTip {
        control: "pool-sort",
        name: "Sort order",
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
        control: "edit-mode",
        name: "Drop mode — Ripple pushes later clips right, Overwrite covers them",
        binding: None,
    },
    ControlTip {
        control: "fit-timeline",
        name: "Fit the sequence to the window",
        binding: Some("key-fit"),
    },
    // The Viewer's control row: the playhead field and the transport.
    ControlTip {
        control: "playhead-tc",
        name: "Playhead timecode",
        binding: None,
    },
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
        control: "preview-loop",
        name: "Loop playback",
        binding: Some("key-loop"),
    },
    ControlTip {
        control: "preview-shuttle",
        name: "Shuttle speed — silent above 4x, click for 1x",
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
    ControlTip {
        control: "program-mark-in",
        name: "Mark the sequence in point",
        binding: Some("key-mark-in"),
    },
    ControlTip {
        control: "program-mark-out",
        name: "Mark the sequence out point",
        binding: Some("key-mark-out"),
    },
    // Preview view.
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
        control: "preview-handles-transform",
        name: "Transform handles",
        binding: None,
    },
    ControlTip {
        control: "preview-handles-crop",
        name: "Crop handles",
        binding: None,
    },
    ControlTip {
        control: "preview-handles-mask",
        name: "Mask handles",
        binding: None,
    },
    ControlTip {
        control: "preview-scopes-toggle",
        name: "Scope strip",
        binding: None,
    },
    ControlTip {
        control: "preview-resolution",
        name: "Playback and paused resolution",
        binding: None,
    },
    ControlTip {
        control: "preview-proxies-toggle",
        name: "Play proxies or the original media",
        binding: None,
    },
    ControlTip {
        control: "preview-meter",
        name: "Output peaks — click to clear the clip",
        binding: None,
    },
    ControlTip {
        control: "viewer-tab-program",
        name: "Show the sequence",
        binding: Some("key-pane-viewer"),
    },
    ControlTip {
        control: "viewer-tab-source",
        name: "Show this source; middle-click to close it",
        binding: Some("key-pane-viewer"),
    },
    ControlTip {
        control: "viewer-tab-close",
        name: "Close this Source tab",
        binding: Some("key-pane-close"),
    },
    ControlTip {
        control: "viewer-display-settings",
        name: "Viewer display settings",
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
    // Timeline. The header repeats the tool bar's toggles beside the
    // timecode, each under its own id so the two light on their own.
    ControlTip {
        control: "add-track",
        name: "Add a track",
        binding: None,
    },
    ControlTip {
        control: "timeline-tc",
        name: "Sequence timecode",
        binding: None,
    },
    ControlTip {
        control: "timeline-snap",
        name: "Snapping",
        binding: Some("key-snap"),
    },
    ControlTip {
        control: "timeline-linked",
        name: "Linked selection",
        binding: None,
    },
    ControlTip {
        control: "timeline-marker",
        name: "Add a marker at the playhead",
        binding: Some("key-marker-add"),
    },
    ControlTip {
        control: "timeline-display-settings",
        name: "Timeline display settings",
        binding: None,
    },
    ControlTip {
        control: "timeline-graph",
        name: "Graph editor",
        binding: Some("key-graph-editor"),
    },
    ControlTip {
        control: "property-search-clear",
        name: "Clear the property search",
        binding: None,
    },
    ControlTip {
        control: "effects-query-clear",
        name: "Clear the effects search",
        binding: None,
    },
    ControlTip {
        control: "graph-btn-easy-ease",
        name: "Easy Ease the selected keyframes",
        binding: Some("key-easy-ease"),
    },
    ControlTip {
        control: "graph-btn-hold",
        name: "Hold the selected keyframes",
        binding: Some("key-toggle-hold"),
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
    // Driver source under a driven property.
    ControlTip {
        control: "inspector-driver-whip",
        name: "Pick whip: click a property to read it in this driver",
        binding: None,
    },
    // Parent row in the Transform tab.
    ControlTip {
        control: "inspector-parent-whip",
        name: "Pick whip: drag onto a timeline clip, or click here and then the clip, to parent this clip to it. Hold Shift to snap",
        binding: None,
    },
    // Effect rows.
    ControlTip {
        control: "inspector-fx-bypass",
        name: "Switch this effect off, or back on",
        binding: None,
    },
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
        control: "inspector-fx-reset",
        name: "Put this effect's parameters back at their defaults",
        binding: None,
    },
    ControlTip {
        control: "inspector-fx-menu",
        name: "Rename, duplicate, copy, paste or save this effect as a preset",
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
    // The property rows in the timeline and in Effect Controls, and the Path
    // rows: a stopwatch, a keyframe navigator and a reset, each a glyph. A row
    // is drawn once per property, so the key is the control's kind, and each
    // control is named once for every row that draws it.
    ControlTip {
        control: "prop-watch",
        name: "Stopwatch: animate this property, or stop animating it and keep the value at the playhead. Alt+click adds or removes a driver",
        binding: None,
    },
    ControlTip {
        control: "prop-reset",
        name: "Reset this property to its default, as a keyframe when it is animated",
        binding: None,
    },
    ControlTip {
        control: "path-watch",
        name: "Stopwatch — keyframe the mask path",
        binding: None,
    },
    ControlTip {
        control: "kf-prev",
        name: "Go to the previous keyframe",
        binding: None,
    },
    ControlTip {
        control: "kf-at",
        name: "Add or remove a keyframe at the playhead",
        binding: None,
    },
    ControlTip {
        control: "kf-next",
        name: "Go to the next keyframe",
        binding: None,
    },
    // The three window buttons. Each is a glyph with no word beside it.
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
    // A dock frame's tab row.
    ControlTip {
        control: "dock-tab-more",
        name: "More panels",
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
    name: gpui::SharedString,
    chord: Option<String>,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let chord = self.chord.clone();
        div()
            .debug_selector(|| "tooltip".into())
            .flex()
            .items_center()
            .gap(px(Theme::SPACE_SM))
            .px(px(Theme::SPACE_SM))
            .py(px(Theme::SPACE_XS))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.surface_card)
            .border_1()
            .border_color(theme.border)
            .text_size(Theme::TEXT_TITLE)
            .text_color(theme.text)
            .child(self.name.clone())
            .children(chord.map(|chord| {
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(chord)
            }))
    }
}

/// How long the pointer rests on a control before its tip shows.
pub const SHOW_DELAY: Duration = Duration::from_millis(400);

/// A tip attached with the crate's rest delay.
///
/// Every tip in the crate is attached through [`WithTip::with_tip`], so
/// [`SHOW_DELAY`] is the rest delay for all of them, and the census below fails
/// on a tip attached any other way. A press hides the tip and holds it back
/// until the pointer leaves the control, so a tip never opens over the menu or
/// popover the press opened.
pub trait WithTip: StatefulInteractiveElement {
    /// The element with `build` as its tip, shown after [`SHOW_DELAY`].
    fn with_tip(self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self
    where
        Self: Sized,
    {
        self.tooltip(build).tooltip_show_delay(SHOW_DELAY)
    }
}

impl<E: StatefulInteractiveElement> WithTip for E {}

/// The tooltip builder for one control id, or nothing when it is not named.
///
/// Returns a builder rather than an element because GPUI asks for the view only
/// when the pointer has rested: an unhovered control costs nothing. The chord is
/// resolved inside the builder rather than captured, so a key rebound since the
/// button was drawn is named as it is now.
pub fn builder(control: &str) -> Option<impl Fn(&mut Window, &mut App) -> AnyView + use<>> {
    let tip = tip_for(control)?;
    Some(move |_: &mut Window, cx: &mut App| {
        let name = gpui::SharedString::new_static(tip.name);
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
        let name = gpui::SharedString::new_static(tip.name);
        let chord = chord_label(&tip, cx);
        cx.new(|_| Tip { name, chord }).into()
    }
}

/// A tooltip showing a dynamic text label (such as a plugin's display name).
pub fn text_builder<T: Into<gpui::SharedString>>(
    text: T,
) -> impl Fn(&mut Window, &mut App) -> AnyView + use<T> {
    let name = text.into();
    move |_: &mut Window, cx: &mut App| {
        let name = name.clone();
        cx.new(|_| Tip { name, chord: None }).into()
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

    /// WHY: two controls with one name read as one action in their tooltips
    /// while they do different things. Closes "a new control named after an
    /// existing one": every name belongs to one control, so a second control
    /// called the same goes red. Does not cover two names for one action.
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
                .filter(|&other| other != tip.control)
                .collect();
            assert!(
                sharing.is_empty(),
                "{} and {:?} all call themselves {}",
                tip.control,
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

    /// WHY: GPUI shows a tip after its own 500 ms default unless the element
    /// sets a delay, so the table can name a control whose tip still shows
    /// late, and a tip that opens after a press covers the menu the press
    /// opened. Hovers the Viewer's go-to-start button in a drawn shell: nothing
    /// a millisecond before [`SHOW_DELAY`], the tip a millisecond after it,
    /// nothing once the button is pressed or after a press made during the
    /// delay, however long the pointer rests or moves on the button, and the
    /// tip again once the pointer has left and come back. Does not cover a tip
    /// attached without [`WithTip::with_tip`]; the census below covers that.
    #[gpui::test]
    fn a_tip_shows_after_the_rest_delay_and_a_press_holds_it_back(cx: &mut gpui::TestAppContext) {
        use gpui::{Modifiers, MouseButton};
        let executor = cx.executor();
        let (_state, visual) = crate::test_support::drawn_shell(cx, "tooltip-rest");
        visual.run_until_parked();
        let bounds = visual
            .debug_bounds("go-start")
            .expect("the Viewer draws its go-to-start button");
        let button = bounds.center();
        let elsewhere = button + gpui::point(gpui::px(1.), gpui::px(0.));
        let away = gpui::point(gpui::px(1.), gpui::px(1.));
        let none = Modifiers::none();
        let rest = |visual: &mut gpui::VisualTestContext| {
            executor.advance_clock(SHOW_DELAY * 2);
            visual.run_until_parked();
            visual.debug_bounds("tooltip").is_some()
        };
        visual.simulate_mouse_move(button, None, none);
        let step = Duration::from_millis(1);
        executor.advance_clock(SHOW_DELAY - step);
        visual.run_until_parked();
        assert_eq!(
            visual.debug_bounds("tooltip"),
            None,
            "the tip showed before the pointer rested {SHOW_DELAY:?}"
        );
        executor.advance_clock(step * 2);
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("tooltip").is_some(),
            "no tip once the pointer rested {SHOW_DELAY:?}"
        );
        visual.simulate_mouse_down(button, MouseButton::Left, none);
        visual.run_until_parked();
        assert_eq!(
            visual.debug_bounds("tooltip"),
            None,
            "the tip stayed up through a press"
        );
        visual.simulate_mouse_up(button, MouseButton::Left, none);
        assert!(!rest(visual), "the tip came back with the pointer at rest");
        visual.simulate_mouse_move(elsewhere, None, none);
        assert!(
            !rest(visual),
            "a move on the pressed button brought its tip back"
        );
        visual.simulate_mouse_move(away, None, none);
        visual.simulate_mouse_move(button, None, none);
        assert!(
            rest(visual),
            "no tip after the pointer left the button and came back"
        );
        visual.simulate_mouse_move(away, None, none);
        visual.simulate_mouse_move(button, None, none);
        visual.simulate_mouse_down(button, MouseButton::Left, none);
        visual.simulate_mouse_up(button, MouseButton::Left, none);
        assert!(
            !rest(visual),
            "a press during the rest delay let the tip open after it"
        );
    }

    /// WHY: a tip attached with the raw GPUI call rests for GPUI's default
    /// rather than [`SHOW_DELAY`]. Every source file under `src` is read, so a
    /// file added later is covered on the run that adds it, and the only raw
    /// call allowed is the one in [`WithTip::with_tip`]. The needles are
    /// assembled with `concat!` so this test does not match itself. The floor
    /// on attached tips fails a scan that reads nothing.
    #[test]
    fn every_tip_is_attached_with_the_rest_delay() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        crate::test_support::collect_rs(&src, &mut files);
        let raw_needles = [concat!(".tool", "tip("), concat!("hoverable_", "tooltip(")];
        let owner = src.join("tooltip.rs");
        let mut attached = 0;
        for path in &files {
            let text = std::fs::read_to_string(path).expect("read a source file");
            attached += text.matches(concat!(".with_", "tip(")).count();
            let raw: usize = raw_needles
                .iter()
                .map(|needle| text.matches(needle).count())
                .sum();
            let allowed = usize::from(*path == owner);
            assert_eq!(
                raw,
                allowed,
                "{} attaches a tip without crate::tooltip::WithTip, so it shows after \
                 GPUI's default delay rather than SHOW_DELAY",
                path.display()
            );
        }
        assert!(
            attached >= 40,
            "the scan found {attached} tips attached with the rest delay"
        );
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
            Some("Space")
        );
        let pointer = tip_for("tool-pointer").expect("the pointer tool is named");
        assert_eq!(
            shipped_label(pointer.binding.expect("the pointer tool has a key")).as_deref(),
            Some("V")
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
        // The Timeline header wraps `icon_button` in `tool_btn` and its display
        // button in `display_btn`, the Project panel's and the Viewer's toggles
        // in `lit_icon`, and the Viewer wraps it in `jump_btn` and `step_btn`
        // and draws its marks through `mark_btn`. Each names the tooltip id in
        // its first argument; scanning only the inner helper misses them.
        let needles = [
            concat!("icon_", "button("),
            concat!("tool_", "btn("),
            concat!("lit_", "icon("),
            concat!("display_", "btn("),
            concat!("jump_", "btn("),
            concat!("step_", "btn("),
            concat!("mark_", "btn("),
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
            [
                "timeline-snap",
                "go-start",
                "next-frame",
                "program-mark-in",
                "viewer-display-settings"
            ]
            .iter()
            .all(|id| found.contains(*id)),
            "the scan reached no call site: {} ids found, and the Timeline \
             header's snap toggle, the Viewer's go-to-start or In button, or its \
             display button was not one of them",
            found.len()
        );
    }
}
