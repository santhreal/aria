//! A control that cannot act now.
//!
//! The look spec draws every disabled control at 40 % opacity with the arrow
//! cursor. [`blocked`] is the single definition of that state: a button, a
//! chip or a row passes through it with the condition that blocks it, and its
//! press handler stays attached so a press still reports the reason on the
//! status line.

use gpui::{CursorStyle, Styled};

/// The opacity of a control that cannot act now.
pub const BLOCKED_OPACITY: f32 = 0.4;

/// `control` as drawn while `blocked`: at [`BLOCKED_OPACITY`], with the arrow
/// cursor. Unchanged while it can act.
pub fn blocked<E: Styled>(control: E, blocked: bool) -> E {
    if blocked {
        control.opacity(BLOCKED_OPACITY).cursor(CursorStyle::Arrow)
    } else {
        control
    }
}
