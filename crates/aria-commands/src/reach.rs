//! How an operator reaches each command.
//!
//! The class closed here is "a command is added, dispatched, serialised, tested
//! at the wire, and never reachable by a person". A command that no surface
//! sends is not a feature, and nothing about its own definition says so.
//!
//! Reach is a column of the command registry, so a new variant cannot compile
//! until its row states which it is. A variant with no operator path declares
//! the reason in that row, beside its label and its undo, rather than being
//! discovered later by diffing identifiers across crates.

/// Whether a command is something a person can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// A shipped operator surface sends this command, and this says which: a
    /// palette row, a menu item, a chord, a panel control, or a timeline
    /// gesture.
    ///
    /// The surface is written down because most of them cannot be enumerated.
    /// A palette row, a menu row and a chord live in tables a test can drive,
    /// and the editor's census does drive them; an inspector block, a
    /// context-menu item and an on-canvas handle are drawn gpui elements with
    /// their action inside a closure, so no test can walk them. Naming the
    /// surface is what makes the difference visible: a command whose row says
    /// nothing is a command nobody checked for a control.
    Operator(&'static str),
    /// Nothing sends it yet, deliberately, and this says what is missing.
    ///
    /// A command whose surface is blocked on work elsewhere is not a feature,
    /// and shipping it as `Operator` claims a control that is not there. The
    /// text names the missing piece and the ledger row that owns it.
    Planned(&'static str),
    /// No operator surface sends it, and this is the reason. Either another
    /// command covers the same intent with more cases handled, or the command
    /// exists for a non-operator caller (the assistant, a plugin, the CLI, a
    /// migration).
    Internal(&'static str),
}

impl Reach {
    /// Whether an operator can ask for this.
    pub fn is_operator(self) -> bool {
        matches!(self, Reach::Operator(_))
    }

    /// The surface an operator reaches it through, when there is one.
    pub fn surface(self) -> Option<&'static str> {
        match self {
            Reach::Operator(surface) => Some(surface),
            Reach::Planned(_) | Reach::Internal(_) => None,
        }
    }

    /// The recorded reason a command has no operator path.
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Reach::Operator(_) => None,
            Reach::Planned(reason) | Reach::Internal(reason) => Some(reason),
        }
    }
}

#[cfg(test)]
mod tests;
