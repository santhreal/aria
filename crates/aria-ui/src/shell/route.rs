//! The route transition: how far the route being drawn has arrived.
//!
//! Home and the editor are two layouts with nothing in common, so a route change
//! used to be a repaint into a different application. Each end then grew its own
//! entrance — Home a one-shot fade, the editor its staged assembly — at two
//! different rates and neither cancellable, so bouncing between them replayed a
//! transition from zero every time.
//!
//! One chased value replaces both. Every frame the shell reports which route it
//! is drawing, both routes' values travel toward their targets, and the shell
//! draws the incoming body at the value it has reached. Chased rather than
//! played, so a route change during a route change continues from where the
//! screen actually is.
//!
//! What this is not: a true cross-fade. The outgoing body is unmounted on the
//! frame the route changes — a panel's listeners are attached to the tree that
//! holds them, and keeping two editors' worth of live listeners alive to fade
//! one out costs more than the transition is worth. The incoming body rises out
//! of the shell's own background, which is the surface both routes sit on.

use super::*;

/// How far the body for `route` has arrived, in `0.0..=1.0`.
///
/// Both routes' values are advanced on every call, not only the one being drawn:
/// a value nothing asks for is evicted by the store's frame sweep, and an evicted
/// value has no history to travel from, which is what made the old one-shots
/// restart. Two hash lookups a frame is the price of a transition that can be
/// interrupted.
pub(crate) fn fade(route: UiRoute, reduced: bool) -> f32 {
    let home = travel("route-home", route == UiRoute::Home, reduced);
    let editor = travel("route-editor", route != UiRoute::Home, reduced);
    if route == UiRoute::Home { home } else { editor }
}

fn travel(key: &'static str, incoming: bool, reduced: bool) -> f32 {
    glide::glide(
        motion::MotionKey::new(key, ""),
        if incoming { 1.0 } else { 0.0 },
        glide::ROUTE,
        reduced,
    )
    .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
