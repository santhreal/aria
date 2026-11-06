//! A control that cannot act now.
//!
//! The look spec draws every disabled control at 40 % opacity with the arrow
//! cursor. [`blocked`] is the single definition of that state: a button, a
//! chip or a row passes through it with the condition that blocks it, and its
//! press handler stays attached so a press still reports the reason on the
//! status line. The press itself draws nothing: a blocked control holds its
//! opacity and plate while held, where a live one dims and darkens.

use gpui::{CursorStyle, StatefulInteractiveElement, Styled};

/// The opacity of a control that cannot act now.
pub const BLOCKED_OPACITY: f32 = 0.4;

/// `control` as drawn while `blocked`: at [`BLOCKED_OPACITY`], with the arrow
/// cursor, and without the control's press style. Unchanged while it can act.
pub fn blocked<E: Styled + StatefulInteractiveElement>(control: E, blocked: bool) -> E {
    if blocked {
        control
            .opacity(BLOCKED_OPACITY)
            .cursor(CursorStyle::Arrow)
            .active(|style| style)
    } else {
        control
    }
}
