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
use crate::types::EditorTool;

/// One named control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlTip {
    /// The control key: the literal a control passes when it asks for its own
    /// tip, which for most controls is also its element id. A row drawn once per
    /// item keys on its kind, because its element id carries the index.
    pub control: &'static str,
    /// What the control is called, in the words the docs use.
    pub name: &'static str,
    /// What the control does beyond its name, on a line under it, if anything.
    pub detail: Option<&'static str>,
    /// The binding whose chord this control repeats, if any.
    pub binding: Option<&'static str>,
}

impl ControlTip {
    /// The control `control`, called `name`, with no key and no detail.
    const fn named(control: &'static str, name: &'static str) -> Self {
        Self {
            control,
            name,
            detail: None,
            binding: None,
        }
    }

    /// The tool bar button of `tool`, called what the tool table calls the
    /// tool, so the tip, the status line and the variants popover agree.
    const fn tool(tool: EditorTool) -> Self {
        let spec = tool.spec();
        Self::named(spec.id, spec.label)
    }

    /// This tip, repeating the chord of the binding `id`.
    const fn key(self, id: &'static str) -> Self {
        Self {
            binding: Some(id),
            ..self
        }
    }

    /// This tip, with `detail` on a line under its name.
    const fn detail(self, detail: &'static str) -> Self {
        Self {
            detail: Some(detail),
            ..self
        }
    }
}

/// Every icon-only control that names itself.
pub const CONTROL_TIPS: &[ControlTip] = &[
    // Tool bar.
    ControlTip::tool(EditorTool::Pointer).key("key-tool-pointer"),
    ControlTip::tool(EditorTool::TrackSelectForward).key("key-tool-track-forward").detail("Select from here on"),
    ControlTip::tool(EditorTool::TrackSelectBackward).key("key-tool-track-backward").detail("Select up to here"),
    ControlTip::tool(EditorTool::Razor).key("key-tool-razor"),
    ControlTip::tool(EditorTool::Hand).key("key-tool-hand"),
    ControlTip::tool(EditorTool::Zoom).key("key-tool-zoom"),
    ControlTip::tool(EditorTool::Slip).key("key-tool-slip").detail("Move the take inside the clip"),
    ControlTip::tool(EditorTool::Slide).key("key-tool-slide").detail("Move the clip and trim its neighbors"),
    ControlTip::tool(EditorTool::Roll).key("key-tool-roll").detail("Move the cut"),
    ControlTip::tool(EditorTool::Rate).key("key-tool-rate").detail("Retime without trimming"),
    ControlTip::tool(EditorTool::Ripple).key("key-tool-ripple").detail("Trim and close the gap"),
    ControlTip::tool(EditorTool::Pen).key("key-tool-pen").detail("Add keyframes or draw masks"),
    ControlTip::tool(EditorTool::Type).key("key-tool-type").detail("Create or edit text in the Viewer"),
    ControlTip::named("bin-new", "New bin"),
    // The Project panel's pool bars: the filter's Import and Audio switch, the
    // views, and the multicam maker.
    ControlTip::named("media-import", "Import").key("key-import"),
    ControlTip::named("pool-audio-only", "Audio only"),
    ControlTip::named("pool-view-tiles", "Tiles"),
    ControlTip::named("pool-view-list", "List"),
    ControlTip::named("pool-view-icons", "Icons"),
    ControlTip::named("pool-view-freeform", "Freeform"),
    ControlTip::named("create-multicam-group", "New multicam"),
    ControlTip::named("pool-sort", "Sort order"),
    ControlTip::named("bin-rename", "Rename this bin"),
    ControlTip::named("bin-delete", "Delete this bin").detail("Its contents move up"),
    ControlTip::named("edit-mode", "Drop mode").detail("Ripple pushes later clips right; Overwrite covers them"),
    ControlTip::named("fit-timeline", "Fit the sequence to the window").key("key-fit"),
    // The Viewer's control row: the playhead field and the transport.
    ControlTip::named("playhead-tc", "Playhead timecode"),
    ControlTip::named("go-start", "Go to start").key("key-start"),
    ControlTip::named("prev-frame", "Step back one frame").key("key-step-back"),
    ControlTip::named("play", "Play").key("key-play"),
    ControlTip::named("pause", "Pause").key("key-play"),
    ControlTip::named("preview-loop", "Loop playback").key("key-loop"),
    ControlTip::named("preview-shuttle", "Shuttle speed").key("key-shuttle-forward").detail("Silent above 4x; click for 1x"),
    ControlTip::named("next-frame", "Step forward one frame").key("key-step-forward"),
    ControlTip::named("go-end", "Go to end").key("key-end"),
    ControlTip::named("program-mark-in", "Mark the sequence in point").key("key-mark-in"),
    ControlTip::named("program-mark-out", "Mark the sequence out point").key("key-mark-out"),
    // Preview view.
    ControlTip::named("preview-zoom-out", "Zoom the preview out"),
    ControlTip::named("preview-zoom-in", "Zoom the preview in"),
    ControlTip::named("preview-zoom", "Zoom level"),
    ControlTip::named("preview-handles-transform", "Transform handles"),
    ControlTip::named("preview-handles-crop", "Crop handles"),
    ControlTip::named("preview-handles-mask", "Mask handles"),
    ControlTip::named("preview-scopes-toggle", "Scope strip"),
    ControlTip::named("preview-resolution", "Playback and paused resolution"),
    ControlTip::named("preview-proxies-toggle", "Play proxies or the original media"),
    ControlTip::named("preview-meter", "Output peaks").detail("Click to clear the clip"),
    ControlTip::named("viewer-tab-program", "Show the sequence").key("key-pane-viewer"),
    ControlTip::named("viewer-tab-source", "Show this source").key("key-pane-viewer").detail("Middle-click to close it"),
    ControlTip::named("viewer-tab-close", "Close this Source tab").key("key-pane-close"),
    ControlTip::named("viewer-display-settings", "Viewer display settings"),
    // Source monitor.
    ControlTip::named("source-close", "Back to the sequence").key("key-play"),
    ControlTip::named("source-mark-in", "Mark the source in point").key("key-mark-in"),
    ControlTip::named("source-mark-out", "Mark the source out point").key("key-mark-out"),
    ControlTip::named("source-mark-clear", "Clear both source marks"),
    ControlTip::named("source-insert", "Insert the marked range, rippling the lane").key("key-source-insert"),
    ControlTip::named("source-overwrite", "Overwrite with the marked range").key("key-source-overwrite"),
    // Timeline. The header repeats the tool bar's toggles beside the
    // timecode, each under its own id so the two light on their own.
    ControlTip::named("add-track", "Add a track"),
    ControlTip::named("timeline-tc", "Sequence timecode"),
    ControlTip::named("timeline-snap", "Snapping").key("key-snap"),
    ControlTip::named("timeline-linked", "Linked selection"),
    ControlTip::named("timeline-marker", "Add a marker at the playhead").key("key-marker-add"),
    ControlTip::named("timeline-display-settings", "Timeline display settings"),
    ControlTip::named("timeline-graph", "Graph editor").key("key-graph-editor"),
    ControlTip::named("property-search-clear", "Clear the property search"),
    ControlTip::named("effects-query-clear", "Clear the effects search"),
    ControlTip::named("graph-btn-easy-ease", "Easy Ease the selected keyframes").key("key-easy-ease"),
    ControlTip::named("graph-btn-hold", "Hold the selected keyframes").key("key-toggle-hold"),
    // Multicam. A frame nudge and an audio sync are glyphs on a dense row of
    // camera strips, so the words live here.
    ControlTip::named("nudge-multicam-back", "Nudge this angle one frame earlier"),
    ControlTip::named("nudge-multicam-forward", "Nudge this angle one frame later"),
    ControlTip::named("sync-multicam-group", "Sync these angles from their audio"),
    // Inspector resets.
    ControlTip::named("inspector-reset", "Reset these values"),
    ControlTip::named("crop-clear", "Reset the crop and mirroring"),
    ControlTip::named("grade-clear", "Reset the grade"),
    ControlTip::named("inspector-wheel-reset", "Reset this color wheel"),
    // Driver source under a driven property.
    ControlTip::named("inspector-driver-whip", "Driver pick whip").detail("Click a property to read it in this driver"),
    // Parent row in the Transform tab.
    ControlTip::named("inspector-parent-whip", "Parent pick whip").detail("Drag onto a timeline clip, or click here and then the clip, to parent this clip to it. Hold Shift to snap."),
    // Effect rows.
    ControlTip::named("inspector-fx-bypass", "Switch this effect off, or back on"),
    ControlTip::named("inspector-fx-up", "Move this effect earlier"),
    ControlTip::named("inspector-fx-down", "Move this effect later"),
    ControlTip::named("inspector-fx-reset", "Put this effect's parameters back at their defaults"),
    ControlTip::named("inspector-fx-menu", "Rename, duplicate, copy, paste or save this effect as a preset"),
    ControlTip::named("inspector-fx-remove", "Remove this effect"),
    // Audio insert rows, in the inspector's Audio block and on a mixer strip.
    ControlTip::named("insert-up", "Move this audio effect earlier"),
    ControlTip::named("insert-down", "Move this audio effect later"),
    ControlTip::named("insert-remove", "Remove this audio effect"),
    ControlTip::named("insert-bypass", "Switch this audio effect off, or back on"),
    // A plugin row in Settings.
    ControlTip::named("plugin-toggle", "Turn this plugin off, or back on"),
    // The property rows in the timeline and in Effect Controls, and the Path
    // rows: a stopwatch, a keyframe navigator and a reset, each a glyph. A row
    // is drawn once per property, so the key is the control's kind, and each
    // control is named once for every row that draws it.
    ControlTip::named("prop-watch", "Stopwatch").detail("Animate this property, or stop animating it and keep the value at the playhead. Alt+click adds or removes a driver."),
    ControlTip::named("prop-reset", "Reset this property to its default, as a keyframe when it is animated"),
    ControlTip::named("path-watch", "Path stopwatch").detail("Keyframe the mask path"),
    ControlTip::named("kf-prev", "Go to the previous keyframe"),
    ControlTip::named("kf-at", "Add or remove a keyframe at the playhead"),
    ControlTip::named("kf-next", "Go to the next keyframe"),
    // The three window buttons. Each is a glyph with no word beside it.
    ControlTip::named("caption-min", "Minimize the window"),
    ControlTip::named("caption-max", "Maximize the window"),
    ControlTip::named("caption-close", "Close the window"),
    // A dock frame's tab row.
    ControlTip::named("dock-tab-more", "More panels"),
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
/// Read from the mounted registry, so a rebound key is named as rebound and a
/// cleared key claims no chord at all. Falls back to the shipped table when no
/// shell has published a registry, which is what a unit test and a headless
/// render see.
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

/// The widest a tip draws: a longer detail wraps under the name.
const MAX_W: f32 = 280.0;

/// The hover card itself: the name with the chord beside it, and the detail,
/// if any, on a line under them.
pub struct Tip {
    name: gpui::SharedString,
    chord: Option<String>,
    detail: Option<gpui::SharedString>,
}

impl Tip {
    /// The card for `tip`, its chord read as it is bound now.
    fn of(tip: &ControlTip, cx: &App) -> Self {
        Self {
            name: gpui::SharedString::new_static(tip.name),
            chord: chord_label(tip, cx),
            detail: tip.detail.map(gpui::SharedString::new_static),
        }
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let chord = self.chord.clone();
        div()
            .debug_selector(|| "tooltip".into())
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XXS))
            .max_w(px(MAX_W))
            .px(px(Theme::SPACE_SM))
            .py(px(Theme::SPACE_XS))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.surface_card)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(Theme::SPACE_SM))
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text)
                    .child(self.name.clone())
                    .children(chord.map(|chord| {
                        div()
                            .text_size(Theme::TEXT_UI)
                            .text_color(theme.text_faint)
                            .child(chord)
                    })),
            )
            .children(self.detail.clone().map(|detail| {
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .child(detail)
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
        let card = Tip::of(&tip, cx);
        cx.new(|_| card).into()
    })
}

/// A tooltip whose registry and keybinding work starts only after GPUI's hover
/// delay has elapsed.
pub fn deferred_builder(
    control: &'static str,
) -> impl Fn(&mut Window, &mut App) -> AnyView + use<> {
    move |_: &mut Window, cx: &mut App| {
        let tip = tip_for(control).unwrap_or_else(|| panic!("{control} has no registered tooltip"));
        let card = Tip::of(&tip, cx);
        cx.new(|_| card).into()
    }
}

/// A tooltip showing a dynamic text label (such as a plugin's display name).
pub fn text_builder<T: Into<gpui::SharedString>>(
    text: T,
) -> impl Fn(&mut Window, &mut App) -> AnyView + use<T> {
    let name = text.into();
    move |_: &mut Window, cx: &mut App| {
        let name = name.clone();
        cx.new(|_| Tip {
            name,
            chord: None,
            detail: None,
        })
        .into()
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

    /// WHY: a tip that packs what its control does into its name behind a
    /// separator, as `Slip — move the take inside the clip` and `Pick whip:
    /// click a property to read it in this driver` did, reads as one long name
    /// whose second half starts in lower case. For every row of the table, a
    /// name holds no dash, colon or semicolon, and a detail starts with a
    /// capital. Does not judge the words themselves.
    #[test]
    fn a_name_holds_no_description_and_a_detail_reads_as_a_sentence() {
        for tip in CONTROL_TIPS {
            assert!(
                !tip.name.contains(['—', ':', ';']),
                "{} packs a description into its name {:?}; set it as the detail",
                tip.control,
                tip.name
            );
            if let Some(detail) = tip.detail {
                assert_eq!(
                    crate::app::status::lowercase_start(detail),
                    None,
                    "{}: the detail {detail:?} starts in lower case",
                    tip.control
                );
            }
        }
    }

    /// WHY: a tool was called three things: `Pointer` on hover and `Select` on
    /// the status line, `Ripple trim` on hover, `Ripple Edit` in the variants
    /// popover and `Ripple Trim` in the tool table. Every tool, read from the
    /// tool table when the test runs, is called what its row calls it in its
    /// tip and in the popover.
    #[test]
    fn a_tool_is_called_what_its_row_calls_it() {
        use crate::app::clip_text::tool_hold::ToolVariant;
        for tool in EditorTool::all() {
            let tip = tip_for(tool.control_id()).unwrap_or_else(|| panic!("{tool:?} has no tip"));
            assert_eq!(tip.name, tool.label(), "the tip of {tool:?}");
            assert_eq!(
                ToolVariant::Tool(tool).label(),
                tool.label(),
                "the popover row of {tool:?}"
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
