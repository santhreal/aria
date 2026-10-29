//! The ghost buttons, chips and switches a test records: the debug selector
//! each control draws under, and what it draws in its frame.
//!
//! A test stands a [`NotingControls`], starts a record with
//! [`record_controls`], draws, and takes the record with [`drawn_controls`].
//! A control built while a [`NotingControls`] stands draws under the selector
//! [`control_selector`] states, so a test finds it by kind, id and label.

use gpui::{Div, InteractiveElement as _, Pixels, SharedString, Stateful};

use crate::motion;

thread_local! {
    /// The controls built since [`record_controls`] while a
    /// [`NotingControls`] stands.
    static CONTROLS: std::cell::RefCell<Option<ControlRecord>> =
        const { std::cell::RefCell::new(None) };
    /// Whether a [`NotingControls`] stands.
    static NOTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
/// either.
pub(crate) struct NotingControls(());

impl NotingControls {
    pub(crate) fn start() -> Self {
        NOTING.set(true);
        BUILT.with_borrow_mut(std::collections::HashSet::clear);
        Self(())
    }
}

impl Drop for NotingControls {
    fn drop(&mut self) {
        NOTING.set(false);
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
    if !NOTING.get() {
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

/// The debug selector a `kind` of control `id` drawing `drawn` draws under
/// while a [`NotingControls`] stands: `{kind}-{id:?}`, then `-{label}` for a
/// labelled one.
pub(crate) fn control_selector(kind: &str, id: motion::ControlId, drawn: &Drawn) -> String {
    match drawn {
        Drawn::Labelled { label, .. } => format!("{kind}-{id:?}-{label}"),
        Drawn::Switch => format!("{kind}-{id:?}"),
    }
}
