//! Motion catalog — ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt
//!
//! Animation kit — the Aria motion catalog as reusable helpers over gpui
//! [`Animation`]/[`AnimationExt`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationElement, App, ElementId, EntityId, Global, Hsla, IntoElement, Rgba, Styled,
    Window, px,
};

pub use gpui::AnimationExt;

const PULSE_TICK: Duration = Duration::from_millis(33);
const PULSE_LEASE: Duration = Duration::from_millis(300);

struct PulseClock {
    epoch: Instant,
    leases: HashMap<EntityId, Instant>,
    running: bool,
}

impl Global for PulseClock {}

impl Default for PulseClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            leases: HashMap::new(),
            running: false,
        }
    }
}

/// What one pulse tick did.
#[derive(Debug, PartialEq, Eq)]
enum Tick {
    /// How many views were repainted: the ones still holding a lease.
    Repainted(usize),
    /// The last lease expired, so the clock stopped. An idle window runs no
    /// animation frames at all.
    Parked,
}

/// Drop expired leases, repaint what is still animating, and report whether
/// anything is left to repaint.
///
/// The scope of a pulse repaint is this function's whole business, which is why
/// the tick body is here and not inside the spawn: it notifies the leased views
/// and nothing else, so a pulsing record button does not redraw the timeline,
/// and it parks the moment the last lease expires rather than running a 33 ms
/// clock over an idle window.
fn pulse_tick(cx: &mut App) -> Tick {
    let clock = cx.default_global::<PulseClock>();
    let now = Instant::now();
    clock.leases.retain(|_, until| *until > now);
    if clock.leases.is_empty() {
        clock.running = false;
        return Tick::Parked;
    }
    let views: Vec<EntityId> = clock.leases.keys().copied().collect();
    let repainted = views.len();
    for view in views {
        cx.notify(view);
    }
    Tick::Repainted(repainted)
}

pub fn pulse_delta(spec: &MotionSpec, view: EntityId, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.0;
    }
    let clock = cx.default_global::<PulseClock>();
    clock.leases.insert(view, Instant::now() + PULSE_LEASE);
    let period = spec.total().as_secs_f32();
    let phase = (clock.epoch.elapsed().as_secs_f32() / period).fract();
    if !clock.running {
        clock.running = true;
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(PULSE_TICK).await;
                if cx.update(pulse_tick) == Tick::Parked {
                    break;
                }
            }
        })
        .detach();
    }
    phase
}

// Cubic beziers and the named curves: what a spec's feel is made of.
#[path = "motion_curve.rs"]
mod curve;
pub use curve::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSpec {
    duration_ms: u64,
    delay_ms: u64,
    curve: CubicBezier,
}

impl MotionSpec {
    /// A spec is one step on the duration scale and one named curve. Both
    /// arguments are types this module mints, so a spec cannot carry a timing
    /// or a feel that nobody chose.
    pub const fn new(duration: Beat, curve: CubicBezier) -> Self {
        Self {
            duration_ms: duration.ms(),
            delay_ms: 0,
            curve,
        }
    }

    /// What this spec waits for before it starts. A stagger is
    /// [`Beat::times`], so a queue of rows is still on the scale's grid.
    pub const fn with_delay(mut self, delay: Beat) -> Self {
        self.delay_ms = delay.ms();
        self
    }

    pub const fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub const fn delay_ms(&self) -> u64 {
        self.delay_ms
    }

    pub const fn curve(&self) -> CubicBezier {
        self.curve
    }

    /// Delay plus duration: the whole span, which is what a phase of 0 to 1
    /// covers. Const, so a surface timed as a fraction of one animation can
    /// derive that fraction from the scale instead of writing it down.
    pub const fn total_ms(&self) -> u64 {
        self.delay_ms + self.duration_ms
    }

    pub const fn total(&self) -> Duration {
        Duration::from_millis(self.total_ms())
    }

    pub fn progress(&self, raw_delta: f32) -> f32 {
        let total = self.total_ms() as f32;
        if total <= 0.0 || self.duration_ms == 0 {
            return 1.0;
        }
        let t =
            (raw_delta.clamp(0.0, 1.0) * total - self.delay_ms as f32) / self.duration_ms as f32;
        self.curve.eval(t.clamp(0.0, 1.0))
    }

    pub fn animation(&self) -> Animation {
        let spec = *self;
        Animation::new(spec.total().mul_f32(speed_scale())).with_easing(move |d| spec.progress(d))
    }
}

pub const FADE_IN: MotionSpec = MotionSpec::new(GLIDE, EASE_OUT_EXPO);
pub const FADE_QUICK: MotionSpec = MotionSpec::new(QUICK, EASE);
pub const MENU_IN: MotionSpec = MotionSpec::new(QUICK, EASE);
pub const MENU_OUT: MotionSpec = MotionSpec::new(FLICK, EASE);
pub const DIALOG_IN: MotionSpec = MotionSpec::new(SNAP, EASE);
pub const SPLASH_OUT: MotionSpec = MotionSpec::new(GLIDE, EASE).with_delay(QUICK);
/// One stage of the editor opening. Fast and decelerating: the window is already
/// on screen, so this is the contents arriving, not a curtain.
pub const OPEN: MotionSpec = MotionSpec::new(SETTLE, EASE_OUT_EXPO);
/// What each later stage waits for the one before it. Three stages of chrome,
/// rails and timeline fit inside the splash's own exit.
pub const OPEN_STAGE: Beat = TAP;
pub const RESIZE: MotionSpec = MotionSpec::new(SNAP, EASE_OUT);
pub const TAB_SLIDE: MotionSpec = MotionSpec::new(QUICK, EASE_OUT);
pub const COLLAPSE: MotionSpec = MotionSpec::new(SNAP, EASE_OUT);
pub const CHEVRON: MotionSpec = MotionSpec::new(SNAP, EASE);
pub const SCROLL_GLIDE: MotionSpec = MotionSpec::new(GLIDE, EASE_IN_OUT);
/// A pan the hand let go of. The same length as a scroll the editor performed,
/// on the one curve that overshoots, so the offset carries the momentum of the
/// release and settles instead of stopping dead under the finger that left.
pub const FLING: MotionSpec = MotionSpec::new(GLIDE, SPRING);
pub const HOVER_FADE: MotionSpec = MotionSpec::new(QUICK, EASE_STANDARD);
pub const ZERON_PULSE: MotionSpec = MotionSpec::new(BREATH, EASE);
pub const GRADIENT_SPIN: MotionSpec = MotionSpec::new(CYCLE, EASE);
/// A caret marking time for someone who is typing. The curve is not spent: a
/// caret is a square wave, and [`pulse_delta`] reads the raw phase.
pub const CARET_BLINK: MotionSpec = MotionSpec::new(BLINK, EASE_STANDARD);
/// The offset between neighbours in a list that arrived at once, so the order
/// is legible. Multiplied by the row's place in the list.
pub const ROW_STAGGER: Beat = FRAME;

pub fn fade_in<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("fade-in"),
        FADE_IN.animation(),
        |el, t| el.relative().opacity(t).top(px(4.0 * (1.0 - t))),
    )
}

pub fn fade_quick<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("fade-quick"),
        FADE_QUICK.animation(),
        |el, t| el.opacity(t),
    )
}

pub fn menu_in<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("menu-in"),
        MENU_IN.animation(),
        |el, t| {
            el.relative()
                .opacity(0.3 + 0.7 * t)
                .top(px(-2.0 * (1.0 - t)))
        },
    )
}

pub fn menu_out<E>(id: impl Into<ControlId>, t: f32, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("menu-out"),
        MENU_OUT.animation(),
        move |el, _| el.relative().opacity(1.0 - t).top(px(-2.0 * t)),
    )
}

pub fn dialog_in<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("dialog-in"),
        DIALOG_IN.animation(),
        |el, t| el.relative().opacity(t).top(px(2.0 * (1.0 - t))),
    )
}

pub fn splash_out<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("splash-out"),
        SPLASH_OUT.animation(),
        |el, t| el.opacity(1.0 - t).top(px(-6.0 * t)),
    )
}

#[derive(Debug, Clone, Copy)]
struct FadeEntry {
    origin: f32,
    target: f32,
    started: Instant,
    seen: u64,
}

impl FadeEntry {
    fn value(&self, now: Instant, duration: Duration) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        if duration.is_zero() || elapsed >= duration {
            return self.target;
        }
        let raw = elapsed.as_secs_f32() / duration.as_secs_f32();
        lerp(self.origin, self.target, HOVER_FADE.curve.eval(raw))
    }

    fn settled(&self, now: Instant, duration: Duration) -> bool {
        self.origin == self.target || now.saturating_duration_since(self.started) >= duration
    }
}

#[derive(Default)]
pub struct HoverFades {
    entries: HashMap<MotionKey, FadeEntry>,
    frame: u64,
}

impl HoverFades {
    fn duration() -> Duration {
        HOVER_FADE.total().mul_f32(speed_scale())
    }

    pub fn set_at(&mut self, key: MotionKey, hovered: bool, reduced: bool, now: Instant) {
        let target = if hovered { 1.0 } else { 0.0 };
        let duration = Self::duration();
        let current = self
            .entries
            .get(&key)
            .map(|e| e.value(now, duration))
            .unwrap_or(0.0);
        if target == 0.0 && !self.entries.contains_key(&key) {
            return;
        }
        let origin = if reduced { target } else { current };
        let seen = self.frame;
        self.entries.insert(
            key,
            FadeEntry {
                origin,
                target,
                started: now,
                seen,
            },
        );
    }

    pub fn value_at(&mut self, key: MotionKey, now: Instant) -> f32 {
        let frame = self.frame;
        match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.seen = frame;
                entry.value(now, Self::duration())
            }
            None => 0.0,
        }
    }

    pub fn tick_at(&mut self, now: Instant) -> bool {
        self.frame += 1;
        let frame = self.frame;
        let duration = Self::duration();
        let mut active = false;
        self.entries.retain(|_, entry| {
            if entry.seen + 1 < frame {
                return false;
            }
            let settled = entry.settled(now, duration);
            if !settled {
                active = true;
            }
            !(settled && entry.target == 0.0)
        });
        active
    }
}

thread_local! {
    static HOVER_FADES: RefCell<HoverFades> = RefCell::new(HoverFades::default());
}

pub fn hover_t(key: MotionKey) -> f32 {
    HOVER_FADES.with(|fades| fades.borrow_mut().value_at(key, Instant::now()))
}

/// Bounds on the multiplier. Zero would leave every transition finished before
/// it drew, and an unbounded value would leave one running long after the click
/// that started it.
pub const MIN_SCALE: f32 = 0.01;
pub const MAX_SCALE: f32 = 100.0;

/// The multiplier every duration in the catalog passes through.
///
/// Seeded from `ARIA_MOTION_SCALE` so a launch can set it without a settings
/// file, then owned by the settings choice, which is why it is a cell rather
/// than a constant: the operator changes it while the app is running and every
/// transition after that frame has to use the new value. Stored as bits because
/// there is no atomic `f32`.
static MOTION_SCALE: LazyLock<AtomicU32> = LazyLock::new(|| {
    let seed = std::env::var("ARIA_MOTION_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|s| s.is_finite())
        .map(|s| s.clamp(MIN_SCALE, MAX_SCALE))
        .unwrap_or(1.0);
    AtomicU32::new(seed.to_bits())
});

/// Whether motion is off, readable without an `App`.
///
/// The setting itself belongs to gpui: [`gpui::AnimationExt::with_animation`]
/// reads it, so every one-shot primitive in the catalog honours it without
/// being asked to. What cannot ask gpui is the eased-geometry store, which is
/// driven from a canvas callback and is handed a target rather than a context.
/// Threading the flag through every caller of that store is the same defect one
/// forgotten argument at a time — a pane that keeps sliding for an operator who
/// turned motion off — so the setter mirrors it here and the store reads it.
static REDUCED: AtomicBool = AtomicBool::new(false);

/// Whether the operator has motion off. Prefer [`reduced_motion`] where an
/// `App` is in hand; this exists for the surfaces that have none.
pub fn reduced_motion_now() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// Keep the mirror in step. Written by [`set_reduced_motion`], which is the
/// only place the app changes the setting; the tests use it to drive the
/// context-free surfaces without a window.
pub(crate) fn set_reduced_mirror(reduced: bool) {
    REDUCED.store(reduced, Ordering::Relaxed);
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;

// What a running animation costs the window around it.
#[cfg(test)]
#[path = "motion_scoped_repaint_tests.rs"]
mod scoped_repaint;

// The duration scale: the only lengths an animation may be.
#[path = "motion_scale.rs"]
mod scale;
pub use scale::*;

// The hover-fade store and the colour blend it feeds, in a sibling file to stay
// under the file ceiling; re-exported so every existing path keeps resolving.
#[path = "motion_hover.rs"]
mod hover;
pub use hover::*;

// Moved to a sibling file to stay under the file ceiling; re-exported so
// every existing path keeps resolving.
#[path = "motion_key.rs"]
mod key;
pub use key::*;

// The operator-facing half of the catalog: how much of it runs at all.
#[path = "motion_pref.rs"]
mod pref;
pub use pref::*;

// The pulse and spin arithmetic, moved to a sibling file to stay under the
// file ceiling; re-exported so every existing path keeps resolving.
#[path = "motion_pulse.rs"]
mod pulse;
pub use pulse::*;

// What a released drag is worth: the velocity it carried and the distance that
// buys. Kept out of the shell because it is arithmetic, and out of the glide
// store because a store holds values, not gestures.
#[path = "motion_fling.rs"]
mod fling;
pub(crate) use fling::*;

// The catalog as a reviewable table: purpose, timing, and call site per spec.
#[path = "motion_catalog.rs"]
pub mod catalog;

// The eased-geometry store: the values that chase a target instead of running
// one fixed transition. It lived under `panels::timeline` and every non-panel
// that animates a number had to reach into a panel for it, which pointed the
// design system at the timeline. The store is motion, so it lives here; the
// element that drives its frames belongs to the shell that mounts it.
#[path = "motion_glide.rs"]
pub(crate) mod glide;
