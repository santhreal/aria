//! Hover fades: the store, its listener, and the colour blend it feeds.
//!
//! A hover is not a one-shot transition — the pointer can leave halfway — so it
//! is a value chased per control rather than a [`gpui::Animation`]. One store
//! holds them all, keyed by [`MotionKey`], so every strip in the app fades at
//! one rate and a control that stopped being drawn stops costing anything.

use super::*;

pub fn set_hover(key: MotionKey, hovered: bool, reduced: bool) {
    HOVER_FADES.with(|fades| {
        fades
            .borrow_mut()
            .set_at(key, hovered, reduced, Instant::now())
    });
}

pub fn hover_listener(key: MotionKey) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    move |hovered, window, cx| {
        set_hover(key, *hovered, reduced_motion(cx));
        window.refresh();
    }
}

pub fn hover_fades_active() -> bool {
    HOVER_FADES.with(|fades| fades.borrow_mut().tick_at(Instant::now()))
}

/// Blend two colours in premultiplied space, so a fade out of a transparent
/// colour does not travel through that colour's hue on the way.
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    let (f, g) = (Rgba::from(from), Rgba::from(to));
    let a = lerp(f.a, g.a, t);
    if a <= f32::EPSILON {
        return Hsla::from(Rgba { a: 0.0, ..g });
    }
    Hsla::from(Rgba {
        r: lerp(f.r * f.a, g.r * g.a, t) / a,
        g: lerp(f.g * f.a, g.g * g.a, t) / a,
        b: lerp(f.b * f.a, g.b * g.a, t) / a,
        a,
    })
}

pub fn hover_blend(key: MotionKey, rest: Hsla, hover: Hsla) -> Hsla {
    mix(rest, hover, hover_t(key))
}
