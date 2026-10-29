//! Motion: one live specimen per primitive, labelled with its name and its
//! duration so two curves can be compared side by side.

use aria_ui::components;
use aria_ui::motion;
use aria_ui::theme::Theme;
use gpui::motion::{Easing, MotionSpec, loaders, presets};
use gpui::{
    AnimationExt as _, AnyElement, Context, EntityId, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::gallery_kit::{caption, group, meter, section, shelf, specimen};
use crate::{Gallery, GalleryState};

/// The one hover specimen's identity. A function rather than a const because
/// [`motion::MotionKey`] is a hash of its parts, and hashing is not const.
fn hover_demo_key() -> motion::MotionKey {
    motion::MotionKey::new("gallery", "hover-demo")
}

pub(crate) fn motion_section(
    theme: &Theme,
    state: GalleryState,
    view: EntityId,
    cx: &mut Context<Gallery>,
) -> AnyElement {
    // Two clocks: the 2.4s pulse and the 750ms gradient spin. Both are the
    // shipped specs, so what is on screen is what the app runs.
    let pulse = motion::pulse_delta(&presets::ZERON_PULSE, view, cx);
    let spin = motion::pulse_delta(&presets::GRADIENT_SPIN, view, cx);
    let policy = cx.motion_policy();
    section(
        theme,
        "Motion",
        "entry animations replay on click; the continuous ones run on their own clock",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(entries(theme, state, cx))
            .child(open_stages(theme, state))
            .child(curves(theme))
            .child(specs(theme))
            .child(continuous(theme, pulse, spin))
            .child(stagger(theme, pulse))
            .child(hover_fade(theme))
            .child(caption(
                theme,
                format!(
                    "duration scale = {:.2} ({}); reduced = {}. \
                     Every primitive above reads the motion policy itself.",
                    policy.duration_scale(),
                    motion::SCALE_ENV,
                    policy.reduced()
                ),
            ))
            .into_any_element(),
    )
    .into_any_element()
}

/// Entry and exit animations. A gpui animation runs once per element id, so the
/// replay counter is spliced into every id: a fresh id is a fresh animation.
fn entries(theme: &Theme, state: GalleryState, cx: &mut Context<Gallery>) -> AnyElement {
    let n = state.replay;
    // `menu_out` takes its progress from the caller rather than the animation,
    // so it is driven from the demo clock instead of the replay button.
    let out_t = if state.replay.is_multiple_of(2) {
        0.0
    } else {
        0.85
    };
    group(
        theme,
        "entry / exit — click Replay to re-run every specimen",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .child(
                components::button("motion-replay", format!("Replay ({n})"), theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this: &mut Gallery, _: &MouseDownEvent, _, cx| {
                        this.state.replay = this.state.replay.wrapping_add(1);
                        cx.notify();
                    }),
                ),
            )
            .child(shelf(vec![
                specimen(
                    theme,
                    &spec_label("fade_in", presets::FADE_IN),
                    motion::fade_in(replay_id("fade-in", n), plate(theme, "fade_in")),
                ),
                specimen(
                    theme,
                    &spec_label("fade_quick", presets::FADE_QUICK),
                    motion::fade_quick(replay_id("fade-quick", n), plate(theme, "fade_quick")),
                ),
                specimen(
                    theme,
                    &spec_label("menu_in", presets::MENU_IN),
                    motion::menu_in(replay_id("menu-in", n), plate(theme, "menu_in")),
                ),
                specimen(
                    theme,
                    &spec_label("menu_out", presets::MENU_OUT),
                    motion::menu_out(replay_id("menu-out", n), out_t, plate(theme, "menu_out")),
                ),
                specimen(
                    theme,
                    &spec_label("dialog_in", presets::DIALOG_IN),
                    motion::dialog_in(replay_id("dialog-in", n), plate(theme, "dialog_in")),
                ),
                specimen(
                    theme,
                    &spec_label("splash_out", presets::SPLASH_OUT),
                    motion::splash_out(replay_id("splash-out", n), plate(theme, "splash_out")),
                ),
            ]))
            .child(caption(
                theme,
                format!(
                    "menu_out progress is supplied by the caller, not the animation: \
                     t = {out_t:.2} this frame, and Replay toggles it."
                ),
            )),
    )
}

fn replay_id(kind: &'static str, replay: usize) -> motion::ControlId {
    motion::MotionKey::new(kind, replay).into()
}

/// `MotionSpec` is the whole label: name, duration, delay, and the curve it
/// rides. Two specs with the same duration and different curves are not the
/// same motion, which is what the plots below are for.
fn spec_label(name: &str, spec: MotionSpec) -> String {
    if spec.delay_ms == 0 {
        format!("{name} · {}ms", spec.duration_ms)
    } else {
        format!("{name} · {}ms +{}ms", spec.duration_ms, spec.delay_ms)
    }
}

fn plate(theme: &Theme, label: &str) -> gpui::Div {
    div()
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface_overlay)
        .text_size(px(11.))
        .text_color(theme.text)
        .child(label.to_string())
}

/// The app-open, as the shell draws it: one route value on `DIALOG_IN`, with
/// the docked panels and the status strip [`motion::OPEN_STAGE_LAG`] of it
/// behind the bar.
///
/// The shell's own `opening` is private to `aria-ui`, so the two lines it applies
/// are repeated here against the same spec and [`motion::open_stage`]. What is on
/// screen is the editor arriving.
fn open_stages(theme: &Theme, state: GalleryState) -> AnyElement {
    let n = state.replay;
    group(
        theme,
        &format!(
            "Opening · DIALOG_IN {}ms, later stages {}% of the route behind",
            presets::DIALOG_IN.duration_ms,
            (motion::OPEN_STAGE_LAG * 100.0).round()
        ),
        div().flex().flex_col().gap(px(Theme::SPACE_XS)).children(
            [("bar", 0), ("docked panels", 1), ("status strip", 1)]
                .into_iter()
                .map(move |(label, stage)| {
                    plate(theme, label).with_animation(
                        gpui::ElementId::from(format!("m-open-{label}-{n}")),
                        motion::animation(presets::DIALOG_IN),
                        move |el, arrived| {
                            let t = motion::open_stage(arrived, stage);
                            el.opacity(t).relative().top(px(6.0 * (1.0 - t)))
                        },
                    )
                }),
        ),
    )
}

/// Every named easing curve, sampled and drawn. This is the only place the
/// difference between `ease` and `ease_out_expo` is visible without a stopwatch.
///
/// The list is [`Easing::NAMED`], so a curve added to `gpui::motion` gets a
/// specimen here without anyone remembering to add one.
fn curves(theme: &Theme) -> AnyElement {
    group(
        theme,
        "Easing: 32 samples of eval(x), left to right",
        shelf(
            Easing::NAMED
                .into_iter()
                .map(|easing| curve(theme, easing))
                .collect(),
        ),
    )
}

fn curve(theme: &Theme, curve: Easing) -> AnyElement {
    let bar = theme.accent;
    let track = theme.border;
    specimen(
        theme,
        curve.name(),
        div()
            .flex()
            .flex_row()
            .items_end()
            .gap(px(1.))
            .h(px(44.))
            .children((0..32).map(move |i| {
                let x = i as f32 / 31.0;
                let y = curve.eval(x);
                div()
                    .w(px(3.))
                    .h(px((y * 44.0).max(1.0)))
                    .bg(if y >= 0.999 { track } else { bar })
            })),
    )
}

/// Every `MotionSpec` the design system ships, plotted through `progress` so the
/// delay is part of the shape.
///
/// The rows come from [`motion::catalog::CATALOG`], not from a list here: a spec
/// added to `motion.rs` is on screen the moment it has a catalog row, and the
/// gallery cannot fall one behind the design system the way it did with
/// `CARET_BLINK`. Several of these have no helper of their own: RESIZE,
/// TAB_SLIDE, COLLAPSE, CHEVRON, SCROLL_GLIDE, FLING and LOADER_WAIT are applied at
/// their call sites. A duration and a curve nobody can compare is a token
/// nobody can review.
fn specs(theme: &Theme) -> AnyElement {
    group(
        theme,
        "MotionSpec: 24 samples of progress(x), delay included, one per catalog row",
        shelf(
            motion::catalog::CATALOG
                .iter()
                .map(|row| {
                    specimen(
                        theme,
                        &spec_label(row.name, row.spec),
                        plot(theme, row.spec),
                    )
                })
                .collect(),
        ),
    )
}

fn plot(theme: &Theme, spec: MotionSpec) -> AnyElement {
    let bar = theme.accent;
    let track = theme.border;
    div()
        .flex()
        .flex_row()
        .items_end()
        .gap(px(1.))
        // Taller than the 36px the samples are scaled to, so a spec that passes
        // its target has somewhere to put the overshoot: FLING peaks at 1.098,
        // and a box exactly 36px high would have hidden the one thing its
        // specimen exists to show.
        .h(px(44.))
        .children((0..24).map(move |i| {
            let y = spec.progress(i as f32 / 23.0);
            div()
                .w(px(3.))
                .h(px((y * 36.0).max(1.0)))
                .bg(if y >= 0.999 { track } else { bar })
        }))
        .into_any_element()
}

/// The continuous primitives, all reading the same two clocks.
fn continuous(theme: &Theme, pulse: f32, spin: f32) -> AnyElement {
    let wave = loaders::pulse_wave(pulse);
    group(
        theme,
        "continuous: pulse_delta / pulse_wave / pulse_opacity / pulse_scale / \
         matrix_wave / gspin / mark_phase / lerp",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .child(meter(
                theme,
                &format!(
                    "pulse_delta ZERON_PULSE {}ms",
                    presets::ZERON_PULSE.duration_ms
                ),
                pulse,
            ))
            .child(meter(
                theme,
                &format!(
                    "pulse_delta GRADIENT_SPIN {}ms",
                    presets::GRADIENT_SPIN.duration_ms
                ),
                spin,
            ))
            .child(meter(theme, "pulse_wave(delta)", wave))
            .child(meter(
                theme,
                "pulse_opacity(delta)",
                loaders::pulse_opacity(pulse),
            ))
            .child(meter(
                theme,
                "pulse_scale(delta)",
                loaders::pulse_scale(pulse),
            ))
            .child(meter(
                theme,
                "lerp(0.2, 0.9, pulse_wave)",
                gpui::motion::lerp(0.2, 0.9, wave),
            ))
            .child(meter(
                theme,
                "FADE_IN.progress(delta)",
                presets::FADE_IN.progress(pulse),
            ))
            .child(shelf(vec![
                specimen(theme, "matrix_wave(i, 5)", matrix_row(theme, pulse)),
                specimen(theme, "gspin_cell_phase 3x3", gspin_grid(theme, spin)),
                specimen(theme, "mark_phase (MARK_CELLS)", mark_row(theme, pulse)),
                specimen(
                    theme,
                    "mix(accent, danger, wave)",
                    div()
                        .w(px(64.))
                        .h(px(28.))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .bg(motion::mix(theme.accent, theme.danger, wave)),
                ),
            ])),
    )
}

fn matrix_row(theme: &Theme, delta: f32) -> AnyElement {
    let color = theme.text;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .children((0..5).map(move |i| {
            let w = loaders::matrix_wave(delta, i, 5);
            div()
                .w(px(10.))
                .h(px(24.))
                .rounded(px(2.))
                .bg(color)
                .opacity(gpui::motion::lerp(0.1, 1.0, w))
        }))
        .into_any_element()
}

fn gspin_grid(theme: &Theme, delta: f32) -> AnyElement {
    let color = theme.text;
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .children((0..loaders::MATRIX_SIDE).map(move |row| {
            div()
                .flex()
                .flex_row()
                .gap(px(3.))
                .children((0..loaders::MATRIX_SIDE).map(move |col| {
                    let phase = loaders::gspin_cell_phase(row, col);
                    div()
                        .w(px(8.))
                        .h(px(8.))
                        .rounded(px(4.))
                        .bg(color)
                        .opacity(loaders::gspin_opacity(delta + phase, loaders::GSPIN_DIM))
                }))
        }))
        .into_any_element()
}

fn mark_row(theme: &Theme, delta: f32) -> AnyElement {
    let color = theme.text;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.))
        .children(loaders::MARK_CELLS.iter().take(10).map(move |&(x, y)| {
            let phase = loaders::mark_phase(delta, x, y);
            div()
                .w(px(8.))
                .h(px(8.))
                .rounded(px(2.))
                .bg(color)
                .opacity(loaders::pulse_opacity(phase))
        }))
        .into_any_element()
}

/// Eight cells on one clock with a fixed offset each. The phase offset is the
/// whole point, so the raw value is printed under every cell.
fn stagger(theme: &Theme, delta: f32) -> AnyElement {
    let color = theme.accent;
    let cells: Vec<AnyElement> = (0..8usize)
        .map(|i| {
            let phase = loaders::staggered_phase(delta, i, loaders::PULSE_STAGGER);
            specimen(
                theme,
                &format!("{i} · {phase:.2}"),
                div()
                    .w(px(20.))
                    .h(px(20.))
                    .rounded(px(4.))
                    .bg(color)
                    .opacity(loaders::pulse_opacity(phase)),
            )
        })
        .collect();
    group(
        theme,
        &format!(
            "staggered_phase(delta, index, PULSE_STAGGER = {:.4})",
            loaders::PULSE_STAGGER
        ),
        shelf(cells),
    )
}

/// The hover fade, driven the way a component drives it, on an element the
/// gallery owns. `on_hover` panics on a second registration, so this is a bare
/// div rather than a `components::` control that already has a listener.
fn hover_fade(theme: &Theme) -> AnyElement {
    let key = hover_demo_key();
    let t = motion::hover_t(key);
    group(
        theme,
        &format!(
            "hover_listener / hover_t / hover_blend · HOVER_FADE {}ms",
            presets::HOVER_FADE.duration_ms
        ),
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .child(shelf(vec![specimen(
                theme,
                "hover me",
                div()
                    .id("m-hover-demo")
                    .w(px(120.))
                    .h(px(32.))
                    .rounded(px(Theme::CONTROL_RADIUS))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .bg(motion::hover_blend(key, theme.surface_raised, theme.accent))
                    .text_color(motion::hover_blend(key, theme.text_muted, theme.on_accent))
                    .on_hover(motion::hover_listener(key))
                    .child("hover_blend"),
            )]))
            .child(meter(theme, "hover_t(key)", t))
            .child(caption(
                theme,
                "The fade completes because the gallery root ends each motion frame, \
                 which requests the next frame while a hover is still moving.",
            )),
    )
}
