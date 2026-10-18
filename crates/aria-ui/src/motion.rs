//! The editor's motion state over [`gpui::motion`].
//!
//! Curves, specs, presets, tweens, colour mixing, loader waveforms, and
//! release velocity are `gpui::motion`. This module holds what the editor adds:
//! the identity every animated control is keyed by, one hover store and one
//! glide store shared by every element of the window, the frame those stores
//! sample, the element entrances, the loader pulse clock, the motion choice in
//! Settings, and the catalog of the specs the editor spends.
//!
//! The entrances are ported from Comet (<https://github.com/zeronsh/comet>),
//! MIT License, see LICENSES/comet-MIT.txt.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use gpui::motion::{
    Easing, FrameInstant, MotionDriver, MotionFrame, MotionPolicy, MotionSpec, Timestamp as _,
    TweenStore, loaders, presets,
};
use gpui::{
    Animation, AnimationElement, AnimationExt as _, App, EntityId, Global, IntoElement, Styled,
    Window, px,
};

const PULSE_TICK: Duration = Duration::from_millis(33);
const PULSE_LEASE: Duration = Duration::from_millis(300);

#[derive(Default)]
struct PulseClock {
    /// The instant the first pulse was drawn. Every loader reads its phase from
    /// this one epoch, so two loaders on screen stay phase-locked.
    epoch: Option<FrameInstant>,
    /// The instant each view last drew a pulse.
    leases: HashMap<EntityId, FrameInstant>,
    running: bool,
}

impl Global for PulseClock {}

/// What one pulse tick did.
#[derive(Debug, PartialEq, Eq)]
enum Tick {
    /// How many views were repainted: the ones still holding a lease.
    Repainted(usize),
    /// The last lease expired, so the clock stopped. An idle window runs no
    /// animation frames at all.
    Parked,
}

/// Drops expired leases, repaints the views that still hold one, and reports
/// whether any view is left to repaint.
///
/// A tick notifies the leased views and no others, so a pulsing record button
/// does not redraw the timeline. The clock parks when the last lease expires,
/// so an idle window runs no 33 ms timer.
fn pulse_tick(cx: &mut App) -> Tick {
    let now = cx.frame_instant();
    let lease = PULSE_LEASE.as_secs_f32();
    let clock = cx.default_global::<PulseClock>();
    clock
        .leases
        .retain(|_, drawn| now.seconds_since(*drawn) < lease);
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

/// The phase in `[0, 1)` of the repeating `spec` on the shared pulse clock, for
/// `view`, which the clock repaints for `PULSE_LEASE` after this call. The
/// period is the spec's authored total: a loader or a caret keeps its rate
/// under every duration scale. Returns 0 and takes no lease under reduced
/// motion.
pub fn pulse_delta(spec: &MotionSpec, view: EntityId, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.0;
    }
    // A pulse drawn in a cached panel body repaints that body, not the window.
    let view = building_view().unwrap_or(view);
    let now = cx.frame_instant();
    let clock = cx.default_global::<PulseClock>();
    clock.leases.insert(view, now);
    let epoch = *clock.epoch.get_or_insert(now);
    let phase = loaders::cycle_phase(now.seconds_since(epoch), spec.total().as_secs_f32());
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

/// The motion state every element of the window shares: one hover store, one
/// glide store, the frame they sample, and the cached views drawn in it.
#[derive(Default)]
struct Chrome {
    driver: MotionDriver,
    /// The frame opened by the latest [`begin_frame`]. It stays open after
    /// [`end_frame`], so an element painted after the driver samples the same
    /// instant as the elements painted before it.
    frame: Option<MotionFrame>,
    hover: TweenStore<MotionKey, FrameInstant>,
    glide: TweenStore<MotionKey, FrameInstant>,
    /// Keys whose hover toggle started a transition at the stored instant.
    hover_started: HashMap<MotionKey, FrameInstant>,
    /// Set by [`hold_for_playback`] while playback runs.
    held: bool,
    views: Views,
}

/// The cached views of the open frame, and where its moving values were read.
///
/// A cached view that is not notified replays its last drawing and reads no
/// value. So a frame that replays one is partial: the stores are not advanced,
/// because advancing drops every key no element read, and the replayed view's
/// keys would restart on its next build. A value still moving in a built view
/// repaints that view alone on the next frame; one moving anywhere else
/// repaints the window.
#[derive(Default)]
struct Views {
    /// The cached view whose body is being built.
    building: Option<EntityId>,
    /// The empty view whose notification redraws the window chrome alone.
    chrome: Option<EntityId>,
    /// The cached views mounted in this frame.
    mounted: Vec<EntityId>,
    /// The cached views built in this frame.
    built: Vec<EntityId>,
    /// The cached views a moving value was read in.
    moving: Vec<EntityId>,
    /// Whether a moving value was read outside every cached view.
    moving_root: bool,
}

impl Views {
    /// Records that a value read now is still moving.
    fn moving(&mut self) {
        match self.building {
            Some(view) if !self.moving.contains(&view) => self.moving.push(view),
            Some(_) => {}
            None => self.moving_root = true,
        }
    }
}

impl Chrome {
    /// The policy hover fades and glides run under: `policy`, or reduced while
    /// playback holds them.
    fn policy(&self, policy: MotionPolicy) -> MotionPolicy {
        policy.with_reduced(policy.reduced() || self.held)
    }
}

thread_local! {
    static CHROME: RefCell<Chrome> = RefCell::new(Chrome::default());
}

const NO_FRAME: &str = "a motion value was read before any motion frame was opened: \
     the window root calls motion::begin_frame first in its render";

/// Runs `f` against the shared stores and the open frame.
///
/// # Panics
///
/// Panics when no frame was ever opened on this thread: the window root did not
/// call [`begin_frame`] before building its elements.
fn with_frame<R>(f: impl FnOnce(&mut Chrome, MotionFrame) -> R) -> R {
    CHROME.with(|chrome| {
        let chrome = &mut *chrome.borrow_mut();
        let frame = chrome.frame.expect(NO_FRAME);
        f(chrome, frame)
    })
}

/// The frames this thread has opened, and a ceiling a test sets on them.
///
/// A window that leaves itself dirty at the end of every draw is redrawn by
/// gpui's test executor inside one flush, without end. With a ceiling set, the
/// frame past it panics with the count, so such a test fails where it would
/// otherwise hang.
#[cfg(test)]
pub(crate) mod frames {
    use std::cell::Cell;

    thread_local! {
        static OPENED: Cell<usize> = const { Cell::new(0) };
        static CEILING: Cell<Option<usize>> = const { Cell::new(None) };
    }

    /// Fail the frame after `more` further frames, or never with `None`.
    pub(crate) fn allow(more: Option<usize>) {
        CEILING.set(more.map(|more| OPENED.get() + more));
    }

    pub(super) fn open() {
        let opened = OPENED.get() + 1;
        OPENED.set(opened);
        if let Some(ceiling) = CEILING.get()
            && opened > ceiling
        {
            CEILING.set(None);
            panic!("a draw loop: frame {opened} opened past the ceiling of {ceiling}");
        }
    }
}

/// Opens the frame every hover blend, glide, and entrance of this draw
/// samples: the current [`App::frame_instant`] under the current
/// [`App::motion_policy`]. The window root calls this first in its render.
pub fn begin_frame(cx: &App) {
    #[cfg(test)]
    frames::open();
    CHROME.with(|chrome| {
        let chrome = &mut *chrome.borrow_mut();
        chrome.frame = Some(chrome.driver.begin(cx));
        let now = chrome.frame.expect(NO_FRAME).now();
        chrome.hover_started.retain(|_, at| *at == now);
        chrome.views.chrome = None;
        let views = &mut chrome.views;
        views.mounted.clear();
        views.built.clear();
        views.moving.clear();
        views.moving_root = false;
    });
}

/// Records that cached view `view` is mounted in the open frame, built or
/// replayed.
pub(crate) fn mount_view(view: EntityId) {
    CHROME.with(|chrome| chrome.borrow_mut().views.mounted.push(view));
}
/// Records that `view` redraws the window chrome of the open frame without
/// notifying its cached panel bodies.
pub(crate) fn mount_chrome(view: EntityId) {
    CHROME.with(|chrome| chrome.borrow_mut().views.chrome = Some(view));
}

/// The view that redraws the window chrome of the open frame, if any.
pub(crate) fn chrome_view() -> Option<EntityId> {
    CHROME.with(|chrome| chrome.borrow().views.chrome)
}

/// Builds the body of cached view `view` in `build`: a value read there that
/// is still moving repaints `view` alone on the next frame.
pub(crate) fn build_view<R>(view: EntityId, build: impl FnOnce() -> R) -> R {
    let outer = CHROME.with(|chrome| {
        let views = &mut chrome.borrow_mut().views;
        views.built.push(view);
        views.building.replace(view)
    });
    let built = build();
    CHROME.with(|chrome| chrome.borrow_mut().views.building = outer);
    built
}

/// The cached view whose body is being built, if any.
pub(crate) fn building_view() -> Option<EntityId> {
    CHROME.with(|chrome| chrome.borrow().views.building)
}

/// Draws every hover fade and glide at its end state while `playing`, so a
/// frame during playback spends its budget on the picture and the playhead
/// stands on the frame being shown instead of chasing it. The window root calls
/// this after [`begin_frame`]; one-shot entrances are not held.
pub fn hold_for_playback(playing: bool) {
    CHROME.with(|chrome| chrome.borrow_mut().held = playing);
}

/// The frame opened by the latest [`begin_frame`]: its instant and its motion
/// policy, for timing that is not a store value, such as how long an overlay
/// keeps drawing after it closed.
///
/// # Panics
///
/// Panics when no frame was ever opened on this thread.
pub(crate) fn frame() -> MotionFrame {
    with_frame(|_, frame| frame)
}

/// Ends the frame opened by [`begin_frame`]: drops the hover and glide keys no
/// element read during the previous full frame, and requests the next animation
/// frame while a value is still moving. Returns whether a value is still
/// moving.
///
/// A frame that replayed a cached view advances neither store (see `Views`).
/// A value moving only inside built cached views requests the next frame for
/// those views; any other moving value requests it for the window.
///
/// Called once per draw, during prepaint, after the elements that read the
/// stores. A second call in one draw advances the stores' frame counter twice
/// and drops the values the draw had just read.
pub fn end_frame(window: &mut Window) -> bool {
    let (root, chrome_view, mut views) = CHROME.with(|chrome| {
        let chrome = &mut *chrome.borrow_mut();
        let frame = chrome.frame.expect(NO_FRAME);
        let views = &mut chrome.views;
        let partial = views.mounted.iter().any(|view| !views.built.contains(view));
        let active = !partial && {
            let hover = chrome.hover.end_frame(frame.policy(), frame.now());
            let glide = chrome.glide.end_frame(frame.policy(), frame.now());
            hover || glide
        };
        let root = views.moving_root || (active && views.moving.is_empty());
        (root, views.chrome, std::mem::take(&mut views.moving))
    });
    if root {
        match chrome_view {
            Some(chrome) => {
                if !views.contains(&chrome) {
                    views.push(chrome);
                }
            }
            None => {
                window.request_animation_frame();
                return true;
            }
        }
    }
    let moving = !views.is_empty();
    for view in views {
        window.on_next_frame(move |_, cx| cx.notify(view));
    }
    moving
}

/// A one-shot [`Animation`] over `spec`: its total scaled by the motion policy
/// of the open frame, eased by [`MotionSpec::progress`]. Reduced motion is
/// applied by [`gpui::AnimationExt::with_animation`], which draws the end
/// state.
pub fn animation(spec: MotionSpec) -> Animation {
    let policy = with_frame(|_, frame| frame.policy());
    Animation::new(policy.scale(spec.total())).with_easing(move |raw| spec.progress(raw))
}

/// The span a wait must outlast before a loader appears, spent by
/// `glide::WAIT`: only the span is read, never the curve.
pub const LOADER_WAIT: MotionSpec = MotionSpec::new(320, Easing::EaseOutExpo);
/// How far the editor's arrival travels before each later stage of the layout
/// starts to arrive, as a fraction of the route value: the bar arrives with
/// the route, the docked panels a quarter behind it.
pub const OPEN_STAGE_LAG: f32 = 0.25;

/// How far stage `stage` of the editor layout has arrived when the route has
/// arrived `arrived` of the way, in `0.0..=1.0`.
///
/// The stages are read off the one route value rather than run as their own
/// one-shots, so the opening ends when the route lands, a route change during
/// it continues from the screen, and reduced motion, which lands the route on
/// its first frame, draws every stage at rest.
pub fn open_stage(arrived: f32, stage: usize) -> f32 {
    let lag = (OPEN_STAGE_LAG * stage as f32).min(0.9);
    ((arrived - lag) / (1.0 - lag)).clamp(0.0, 1.0)
}
/// A pan the hand let go of. The length of [`presets::SCROLL_GLIDE`], on the
/// one curve that overshoots, so the offset carries the momentum of the release
/// and settles instead of stopping dead under the finger that left.
pub const FLING: MotionSpec =
    MotionSpec::new(presets::SCROLL_GLIDE.duration_ms, Easing::EaseOutBack);
/// A caret marking time for someone who is typing. The curve is not spent: a
/// caret is a square wave, and [`pulse_delta`] reads the raw phase.
pub const CARET_BLINK: MotionSpec = MotionSpec::new(1000, Easing::Standard);
/// The offset between neighbours in a list that arrived at once, so the order
/// is legible, in milliseconds: one frame at 60 Hz. Multiplied by the row's
/// place in the list.
pub const ROW_STAGGER_MS: u64 = 16;

pub fn fade_in<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("fade-in"),
        animation(presets::FADE_IN),
        |el, t| el.relative().opacity(t).top(px(4.0 * (1.0 - t))),
    )
}

pub fn fade_quick<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("fade-quick"),
        animation(presets::FADE_QUICK),
        |el, t| el.opacity(t),
    )
}

pub fn menu_in<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("menu-in"),
        animation(presets::MENU_IN),
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
        animation(presets::MENU_OUT),
        move |el, _| el.relative().opacity(1.0 - t).top(px(-2.0 * t)),
    )
}

pub fn dialog_in<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("dialog-in"),
        animation(presets::DIALOG_IN),
        |el, t| el.relative().opacity(t).top(px(2.0 * (1.0 - t))),
    )
}

pub fn splash_out<E>(id: impl Into<ControlId>, element: E) -> AnimationElement<E>
where
    E: Styled + IntoElement + 'static,
{
    element.with_animation(
        id.into().element("splash-out"),
        animation(presets::SPLASH_OUT),
        |el, t| el.opacity(1.0 - t).top(px(-6.0 * t)),
    )
}

/// Motion frames for tests: a headless app whose clock moves only when the
/// test moves it, with a frame open at the app's current instant, and one
/// window whose draw ends frames the way a window root's driver does.
#[cfg(test)]
pub(crate) struct TestFrames {
    cx: gpui::TestAppContext,
    window: gpui::AnyWindowHandle,
    ending: std::rc::Rc<FrameEnding>,
}

/// The hand-off between [`TestFrames::end`] and the draw of its window.
/// `armed` makes the next draw end the open frame; `moving` holds what that
/// draw's [`end_frame`] returned. Draws the test did not request, such as the
/// one that opens the window, leave the frame open.
#[cfg(test)]
#[derive(Default)]
struct FrameEnding {
    armed: std::cell::Cell<bool>,
    moving: std::cell::Cell<Option<bool>>,
}

/// The root of the [`TestFrames`] window: a zero-size canvas that ends the
/// open frame during prepaint while [`FrameEnding::armed`] is set.
/// [`gpui::Window::request_animation_frame`] reads the view being drawn, so
/// the frame can end only inside a draw.
#[cfg(test)]
struct FrameEndRoot(std::rc::Rc<FrameEnding>);

#[cfg(test)]
impl gpui::Render for FrameEndRoot {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let ending = self.0.clone();
        gpui::canvas(
            move |_, window, _| {
                if ending.armed.replace(false) {
                    ending.moving.set(Some(end_frame(window)));
                }
            },
            |_, _, _, _| {},
        )
    }
}

#[cfg(test)]
impl TestFrames {
    pub(crate) fn new() -> Self {
        let mut cx = gpui::TestAppContext::single();
        let ending = std::rc::Rc::new(FrameEnding::default());
        let root = ending.clone();
        let window = cx.add_window(move |_, _| FrameEndRoot(root)).into();
        cx.update(|cx| begin_frame(cx));
        Self { cx, window, ending }
    }

    /// Moves the clock by `by` and opens a frame at the new instant.
    pub(crate) fn advance(&self, by: Duration) {
        self.cx.executor().advance_clock(by);
        self.cx.update(|cx| begin_frame(cx));
    }

    /// Runs `f` against the app, then opens a frame under the policy `f` left.
    pub(crate) fn update<R>(&self, f: impl FnOnce(&mut App) -> R) -> R {
        self.cx.update(|cx| {
            let result = f(cx);
            begin_frame(cx);
            result
        })
    }

    /// Draws the test window, whose prepaint ends the open frame as the window
    /// root's driver does. Returns whether a value is still moving.
    ///
    /// # Panics
    ///
    /// Panics when the test window was closed.
    pub(crate) fn end(&self) -> bool {
        self.ending.armed.set(true);
        self.cx.update(|cx| {
            self.window
                .update(cx, |_, window, cx| window.draw(cx).clear(cx))
                .expect("the test window is open");
        });
        self.ending
            .moving
            .take()
            .expect("the test window's draw ends the open frame")
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;

// What a running animation costs the window around it.
#[cfg(test)]
#[path = "motion_scoped_repaint_tests.rs"]
mod scoped_repaint;

// The release contract the timeline pan spends from `gpui::motion::fling`.
#[cfg(test)]
#[path = "motion_fling_tests.rs"]
mod fling;

// The hover store and the colour blend it feeds.
#[path = "motion_hover.rs"]
mod hover;
pub use hover::*;

// The identity every animated control is keyed by.
#[path = "motion_key.rs"]
mod key;
pub use key::*;

// The motion choice in Settings.
#[path = "motion_pref.rs"]
mod pref;
pub use pref::*;

// The catalog as a reviewable table: purpose, timing, and call site per spec.
#[path = "motion_catalog.rs"]
pub mod catalog;

// The glide store: the values that chase a target instead of running one fixed
// transition.
#[path = "motion_glide.rs"]
pub(crate) mod glide;
