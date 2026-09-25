//! The duck tuner: how deep and how fast the next dip goes.
//!
//! `DuckForm` shipped complete — an amount, an attack, a release, five choices
//! each, a label for every one, and the name of the lane the voice is read from —
//! and nothing drew it. The dip itself was reachable (the palette starts the job),
//! so the operator could duck at whatever depth the default happened to be and
//! had no way to change it. This is the view that was missing, not new behaviour.
//!
//! It shows itself. There is no toggle and no dialog state: the panel is
//! registered with `duck_precheck().is_ok()` as its visibility, so it appears
//! exactly when a duck could run — music selected, a lane marked as the voice —
//! and is absent the rest of the time. The refusal sentence stays where it was, on
//! the job.
//!
//! Both motion primitives are the shared ones. `motion::fade_in` brings the panel
//! in when the selection makes it available, so it does not pop into the inspector
//! column, and each chip's background is `motion::hover_blend` on its own key, so
//! the fade is per chip rather than per panel. Both respect `cx.reduce_motion()`
//! inside the helpers.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::duck::DuckForm;
use crate::motion;
use crate::theme::Theme;

/// What one chip cycles, and what it currently reads.
struct Chip {
    id: &'static str,
    label: &'static str,
    value: String,
    /// Which of the choices is showing, and how many there are. Read from the
    /// form's own tables so the dots cannot disagree with what a click does.
    at: Option<usize>,
    count: usize,
    cycle: fn(&mut AppState),
}

/// The nearest choice to `value` in `choices`, so the dots mark the live one.
fn position<T>(choices: &[T], value: f64) -> Option<usize>
where
    T: Copy + Into<f64>,
{
    choices
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let a: f64 = (**a).into();
            let b: f64 = (**b).into();
            (a - value)
                .abs()
                .partial_cmp(&(b - value).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(at, _)| at)
}

fn chips(form: DuckForm) -> [Chip; 3] {
    [
        Chip {
            id: "duck-amount",
            label: "Amount",
            value: form.amount_label(),
            at: position(&DuckForm::AMOUNTS, f64::from(form.amount)),
            count: DuckForm::AMOUNTS.len(),
            cycle: AppState::cycle_duck_amount,
        },
        Chip {
            id: "duck-attack",
            label: "Attack",
            value: form.attack_label(),
            at: position(&DuckForm::TIMES, form.attack_seconds),
            count: DuckForm::TIMES.len(),
            cycle: AppState::cycle_duck_attack,
        },
        Chip {
            id: "duck-release",
            label: "Release",
            value: form.release_label(),
            at: position(&DuckForm::TIMES, form.release_seconds),
            count: DuckForm::TIMES.len(),
            cycle: AppState::cycle_duck_release,
        },
    ]
}

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let form = state.read(cx).duck;
    let voice = state.read(cx).duck_source_name();
    let heading = match voice {
        Some(lane) => format!("Duck under {lane}"),
        // The panel is only visible when a duck could run, so this is the
        // narrow case of a lane that lost its name, not the unset one.
        None => "Duck under voice".to_string(),
    };
    let rows = chips(form).map(|chip| row(&theme, chip, cx));

    motion::fade_in(
        "duck-panel",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .p(px(Theme::SPACE_SM))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.surface_raised)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child(SharedString::from(heading)),
            )
            .children(rows),
    )
    .into_any_element()
}

fn row(theme: &Theme, chip: Chip, cx: &mut gpui::Context<crate::shell::Shell>) -> gpui::AnyElement {
    // One fade key per chip instance. The id is unique within the panel and the
    // panel is drawn once, so no two chips share a fade.
    let key = motion::MotionKey::new("duck", chip.id);
    let background = motion::hover_blend(key, theme.input_bg, theme.surface_raised_hover);
    let cycle = chip.cycle;
    div()
        .id(chip.id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(background)
        .text_size(px(13.))
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| cycle(state));
                cx.notify();
            }),
        )
        .child(div().text_color(theme.text_muted).child(chip.label))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(div().child(SharedString::from(chip.value)))
                .child(dots(theme, chip.at, chip.count)),
        )
        .into_any_element()
}

/// One dot per choice, the live one filled. Built from the count the form
/// reported, so a choice added to `AMOUNTS` or `TIMES` shows up here with no edit.
fn dots(theme: &Theme, at: Option<usize>, count: usize) -> gpui::AnyElement {
    let mut strip = div().flex().items_center().gap(px(Theme::SPACE_XXS));
    for index in 0..count {
        let live = at == Some(index);
        strip = strip.child(div().w(px(4.)).h(px(4.)).rounded(px(2.)).bg(if live {
            theme.accent
        } else {
            theme.border
        }));
    }
    strip.into_any_element()
}
