//! The ghost buttons, chips and switches a test records: the debug selector
//! each control draws under, and what it draws in its frame.
//!
//! A test stands a [`NotingControls`], starts a record with
//! [`record_controls`], draws, and takes the record with [`drawn_controls`].
//! A control built while a [`NotingControls`] stands draws under the selector
//! [`control_selector`] states, so a test finds it by kind, id and label.
//!
//! Every labelled control drawn in a test passes [`note`], which rejects a
//! label that opens with a glyph for its verb, and a [`super::ghost_button`]
//! label in Title Case that is not a menu row.

use gpui::{Div, InteractiveElement as _, Pixels, SharedString, Stateful};

use crate::motion;

thread_local! {
    /// The controls built since [`record_controls`] while a
    /// [`NotingControls`] stands.
    static CONTROLS: std::cell::RefCell<Option<ControlRecord>> =
        const { std::cell::RefCell::new(None) };
    /// How many [`NotingControls`] stand. A surface stands one of its own
    /// while it builds, inside a test's, so the inner one dropping leaves the
    /// test's standing.
    static NOTING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The kind and id of each control built since the standing
    /// [`NotingControls`] began.
    static BUILT: std::cell::RefCell<std::collections::HashSet<(&'static str, motion::ControlId)>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// What [`record_controls`] has recorded.
#[derive(Default)]
struct ControlRecord {
    /// Each control built.
    built: Vec<DrawnControl>,
    /// The debug selector of each control built with the kind and id of an
    /// earlier control under the same [`NotingControls`].
    shared: Vec<String>,
}

/// What a recorded control draws in its frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Drawn {
    /// A ghost button or a chip: `label` at text size `size`, `inset` inside
    /// each side of the frame.
    Labelled {
        label: SharedString,
        inset: f32,
        size: Pixels,
    },
    /// A [`super::switch`], [`super::SWITCH_WIDTH`] wide.
    Switch,
}

/// A control built while a [`NotingControls`] stands: the debug selector the
/// control draws under, and what it draws.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrawnControl {
    pub selector: String,
    pub drawn: Drawn,
}

/// The ghost buttons, chips and switches built until this drops are the ones
/// [`drawn_controls`] returns, and no other surface's. Two of one kind built
/// with one id share one hover, so each lights whenever the pointer is on
/// either. One stood while another stands nests in it: controls stay noted
/// until the outermost drops.
pub(crate) struct NotingControls(());

impl NotingControls {
    pub(crate) fn start() -> Self {
        NOTING.set(NOTING.get() + 1);
        BUILT.with_borrow_mut(std::collections::HashSet::clear);
        Self(())
    }
}

impl Drop for NotingControls {
    fn drop(&mut self) {
        NOTING.set(NOTING.get() - 1);
    }
}

/// Start a fresh record of the controls built while a [`NotingControls`]
/// stands.
pub(crate) fn record_controls() {
    CONTROLS.set(Some(ControlRecord::default()));
}

/// Stop recording and return every control built since [`record_controls`],
/// once each, then the debug selector of each one built with the kind and id
/// of another under the same [`NotingControls`].
pub(crate) fn drawn_controls() -> (Vec<DrawnControl>, Vec<String>) {
    let ControlRecord {
        mut built,
        mut shared,
    } = CONTROLS.with_borrow_mut(|record| record.take().unwrap_or_default());
    built.sort_by(|a, b| a.selector.cmp(&b.selector));
    built.dedup_by(|a, b| a.selector == b.selector);
    shared.sort();
    shared.dedup();
    (built, shared)
}

/// `control`, the `kind` of control `id` drawing `drawn`, under its debug
/// selector while a [`NotingControls`] stands, and recorded while a record
/// stands. A control keeps its selector after [`drawn_controls`] stops the
/// record, so a later frame still draws it where a test can find it.
pub(super) fn note(
    control: Stateful<Div>,
    kind: &'static str,
    id: motion::ControlId,
    drawn: Drawn,
) -> Stateful<Div> {
    if let Drawn::Labelled { label, .. } = &drawn {
        assert!(
            !opens_with_glyph(label),
            "the {kind} `{label}` opens with a glyph for its verb: label it with the word"
        );
        if kind == "ghost" {
            assert!(
                ghost_sentence_case(label),
                "the ghost `{label}` is in Title Case: write it in sentence case, or match its menu row"
            );
        }
    }
    if NOTING.get() == 0 {
        return control;
    }
    let selector = control_selector(kind, id, &drawn);
    CONTROLS.with_borrow_mut(|record| {
        if let Some(record) = record.as_mut() {
            if !BUILT.with_borrow_mut(|built| built.insert((kind, id))) {
                record.shared.push(selector.clone());
            }
            record.built.push(DrawnControl {
                selector: selector.clone(),
                drawn,
            });
        }
    });
    control.debug_selector(move || selector)
}

/// Whether `label` opens with a glyph standing for its verb, as "+ Submix
/// bus" does: a mark, then a space. A button's label is a word. A sign on a
/// quantity, as in "+512 MiB", is part of the quantity.
pub(crate) fn opens_with_glyph(label: &str) -> bool {
    let mut chars = label.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(mark), Some(space)) if !mark.is_alphanumeric() && space.is_whitespace()
    )
}

/// Surface and slot names that keep their initial capital after the first word
/// of a [`super::ghost_button`] label.
const NAMES: [&str; 2] = ["Settings", crate::layouts::CUSTOM_NAME];

/// Whether `word`, after the first word of a [`super::ghost_button`] label,
/// may start with an uppercase letter: a name in [`NAMES`], a token with a
/// digit (such as `H.264`), or an acronym or unit with a later uppercase
/// letter (such as `LUT`, `CSV…` or `MiB`).
fn allowed_capital_word(word: &str) -> bool {
    NAMES.contains(&word)
        || word.chars().any(|ch| ch.is_ascii_digit())
        || word.chars().skip(1).any(char::is_uppercase)
}

/// Every row label in [`crate::chrome::menu_items`] across
/// [`crate::types::AppMenu::ALL`], and every catalog label in
/// [`crate::catalog_text::effect_entries`], which the Effects tab's Add list
/// draws as ghost buttons.
fn registry_labels() -> &'static std::collections::HashSet<String> {
    static LABELS: std::sync::LazyLock<std::collections::HashSet<String>> =
        std::sync::LazyLock::new(|| {
            let ctx = aria_plugin::PluginContext::new();
            let state =
                crate::app::AppState::new(ctx, crate::scratch_pool::scratch("noted-menu-labels"))
                    .expect("scratch state for menu labels");
            crate::types::AppMenu::ALL
                .into_iter()
                .flat_map(|menu| crate::chrome::menu_items::labels_for(menu, &state))
                .map(str::to_owned)
                .chain(
                    crate::catalog_text::effect_entries()
                        .iter()
                        .map(|entry| entry.label.to_string()),
                )
                .collect()
        });
    &LABELS
}

/// Whether a [`super::ghost_button`] `label` is in sentence case: every word
/// after the first starting in lower case unless it is an
/// [`allowed_capital_word`], or the label (without the ` (passthrough)` mark
/// the Effects tab adds when no GPU pass is mounted) is in [`registry_labels`].
pub(crate) fn ghost_sentence_case(label: &str) -> bool {
    label
        .split_whitespace()
        .skip(1)
        .all(|word| !word.starts_with(char::is_uppercase) || allowed_capital_word(word))
        || registry_labels().contains(label.strip_suffix(" (passthrough)").unwrap_or(label))
}

/// The debug selector a `kind` of control `id` drawing `drawn` draws under
/// while a [`NotingControls`] stands: `{kind}-{id:?}`, then `-{label}` for a
/// labelled one.
pub(crate) fn control_selector(kind: &str, id: motion::ControlId, drawn: &Drawn) -> String {
    match drawn {
        Drawn::Labelled { label, .. } => format!("{kind}-{id:?}-{label}"),
        Drawn::Switch => format!("{kind}-{id:?}"),
    }
}

mod tests {
    use gpui::div;

    use super::*;

    /// The class: a surface standing its own [`NotingControls`] while it
    /// builds, as the Inspector does on every frame, inside a test's, and
    /// ending the test's when it drops, so every control built after the
    /// surface in that frame went unrecorded and drew under no selector.
    /// Not covered: a guard leaked with `std::mem::forget`, which leaves
    /// noting on for the thread.
    #[test]
    fn a_noting_stood_inside_another_ends_with_the_outer_one() {
        let id = motion::ControlId::from("noted-nest");
        let build = || note(div().id("noted-nest"), "switch", id, Drawn::Switch);
        let outer = NotingControls::start();
        record_controls();
        drop(NotingControls::start());
        let _ = build();
        let (built, _) = drawn_controls();
        assert_eq!(
            built,
            vec![DrawnControl {
                selector: control_selector("switch", id, &Drawn::Switch),
                drawn: Drawn::Switch,
            }],
            "a control built after an inner noting dropped is recorded"
        );
        drop(outer);
        record_controls();
        let _ = build();
        assert_eq!(
            drawn_controls().0,
            Vec::new(),
            "no control is recorded once the outer noting drops"
        );
    }

    /// The class: a button that types its verb as a glyph ("+ Submix bus")
    /// beside buttons that state it ("Add at playhead"). Every labelled
    /// control a test draws passes [`note`], which rejects one. Not covered:
    /// a control no test draws.
    #[test]
    fn a_label_opening_with_a_glyph_and_a_space_is_rejected() {
        for (label, glyph) in [
            ("+ Submix bus", true),
            ("× Close", true),
            ("↺ Reset", true),
            ("+512 MiB", false),
            ("−512 MiB", false),
            ("Add audio effect", false),
            ("4 K", false),
            ("…", false),
            ("", false),
        ] {
            assert_eq!(opens_with_glyph(label), glyph, "`{label}`");
        }
    }

    /// The class: a [`super::super::ghost_button`] in Title Case ("Show All",
    /// "Open Folder", "Hide Log", "Relink All") beside buttons in sentence
    /// case ("Show all", "Open folder", "Hide log", "Reload", "Reset all").
    /// Every ghost button a test draws passes [`note`], which rejects a word
    /// after the first that starts in upper case unless it is in [`NAMES`],
    /// contains a digit, has a later uppercase letter, or the whole label is a
    /// menu row in [`crate::chrome::menu_items`] ("Relink All…", "Install
    /// Plugin…", "Open Data Folder", "Easy Ease"). Not covered: a ghost button
    /// no test draws, a solid [`super::super::button`], or a multi-word
    /// user-supplied keymap set name in Title Case.
    #[test]
    fn a_ghost_label_in_title_case_that_is_not_a_menu_row_is_rejected() {
        for (label, ok) in [
            ("Show all", true),
            ("Show All", false),
            ("Relink All…", true),
            ("Relink All", false),
            ("Install Plugin…", true),
            ("Open Data Folder", true),
            ("Easy Ease", true),
            ("Open folder", true),
            ("Open Folder", false),
            ("Hide log", true),
            ("Hide Log", false),
            ("Export frames…", true),
            ("Export Frames…", false),
            ("Export as…", true),
            ("Export As…", false),
            ("Driver", true),
            ("Remove driver", true),
            ("ƒ Driver", false),
            ("ƒ Remove", false),
            ("Open Settings", true),
            ("Save as Custom", true),
            ("Replace Custom", true),
            ("Clear LUT", true),
            ("Export CSV…", true),
            ("+512 MiB", true),
            ("Gaussian Blur (passthrough)", true),
            ("H.264", true),
            ("", true),
        ] {
            assert_eq!(ghost_sentence_case(label), ok, "`{label}`");
        }
    }
}
