//! Inspector rows generated from the property catalog: the stepper rows and
//! colour swatch rows of one group, and rows whose caller owns the write.

use crate::app::AppState;
use crate::components::Wraps as _;
use crate::motion;
use crate::surfaces::clip_pick;
use crate::surfaces::param_units;
use crate::theme::Theme;
use aria_engine::EngineError;
use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::keys::{key_controls, stopwatch_dot};
use super::{
    MIXED, Readout, Stopwatch, Travel, Watching, color_name, color_stopwatch, row_label,
    stepper_row, swatch, value_cell,
};

/// A row whose caller owns the write and knows nothing about the property's
/// range: effect knobs, ramp points, crop insets, animation knobs.
pub fn audio_row(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    readout: Readout,
    write: impl Fn(&mut AppState, f32) -> Result<(), EngineError> + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    stepper_row(theme, state, key, label, readout, None, None, write, cx)
}

/// Every stepper row of one property group, generated from the catalog.
///
/// A block states which group it draws and gets its rows; it keeps no list of
/// the properties it owns, and it does not decide how an edit is written. That
/// is what `AppState::nudge_param` is for, and it is the reason a slider here
/// no longer erases the keyframes of the block it belongs to.
///
/// It is also why the whole catalog animates at once: the range, the keyframe
/// state and the value all arrive on the row, so [`stepper_row`] draws a track
/// and a stopwatch for every property the catalog holds without any group
/// opting in, and a property added to the catalog arrives animated.
///
/// A property the model stores in units nobody reads it in carries an entry in
/// [`crate::surfaces::param_units`], and that row is drawn in the units it
/// declares, with its own step and its own write. Such a row keeps its stopwatch
/// and loses its track: the value no longer travels along the range the catalog
/// declared, and a track drawn from that range would put the thumb in the wrong
/// place.
///
/// The state entity is passed in rather than read back off the shell: a panel
/// draws inside an update of the shell entity, so reading that entity from here
/// panics with "cannot read Shell while it is already being updated".
pub fn param_rows(
    theme: &Theme,
    group: &str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let mut elements = Vec::new();
    catalog_rows(
        theme,
        "insp.param",
        group,
        state,
        cx,
        true,
        |_| true,
        |_, element| elements.push(element),
    );
    elements
}

/// [`param_rows`] for a surface other than the inspector: each drawn row is
/// handed to `place` with its catalog id, so a panel can sort rows into sections
/// of its own. `keep` reads every row's id first, and a row it rejects is not
/// built. `surface` prefixes the rows' motion keys, so a row drawn here and in
/// the inspector at once does not share hover and scrub state. The rows draw no
/// time area: that is Effect Controls'.
pub fn param_rows_into(
    theme: &Theme,
    surface: &'static str,
    group: &str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
    keep: impl FnMut(&gpui::SharedString) -> bool,
    place: impl FnMut(&gpui::SharedString, gpui::AnyElement),
) {
    catalog_rows(theme, surface, group, state, cx, false, keep, place);
}

/// The rows of one catalog group, each with its time area when `strips`.
#[allow(clippy::too_many_arguments)]
fn catalog_rows(
    theme: &Theme,
    surface: &'static str,
    group: &str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
    strips: bool,
    mut keep: impl FnMut(&gpui::SharedString) -> bool,
    mut place: impl FnMut(&gpui::SharedString, gpui::AnyElement),
) {
    let state = gpui::Entity::clone(state);
    let (rows, watching) = {
        let state = state.read(cx);
        let rows: Vec<_> = state
            .param_rows(group)
            .into_iter()
            .filter(|row| keep(&row.id))
            .map(|row| {
                let strip = if strips {
                    state.key_strip(row.param)
                } else {
                    None
                };
                (row, strip)
            })
            .collect();
        (rows, Watching::read(state))
    };
    for (row, strip) in rows {
        let param = row.param;
        let component = row.component;
        let stopwatch = watching.stopwatch(&row, strip);
        let key = motion::MotionKey::new(surface, &row.id);
        if let Some(units) = param_units::of_param(group, param) {
            let element = stepper_row(
                theme,
                &state,
                key,
                row.label.clone(),
                Readout::printed(
                    (units.read)(row.value),
                    (units.shown)(row.value),
                    units.decimals,
                    units.unit,
                )
                .dashed(row.mixed),
                None,
                Some(stopwatch),
                units.write,
                cx,
            )
            .into_any_element();
            place(&row.id, element);
            continue;
        }
        let step = row.step();
        let travel = Travel {
            value: row.value,
            step,
            range: match (row.range.min, row.range.max) {
                (Some(min), Some(max)) => Some((min, max)),
                _ => None,
            },
        };
        let element = stepper_row(
            theme,
            &state,
            key,
            row.label.clone(),
            Readout::number(row.value, row.decimals(), row.unit()).dashed(row.mixed),
            Some(travel),
            Some(stopwatch),
            move |state, delta| state.nudge_shared_param(param, component, delta),
            cx,
        )
        .into_any_element();
        place(&row.id, element);
    }
}

/// Every colour property of one group, as a row of swatches.
///
/// The colour twin of [`param_rows`]: the block names its group, the catalog
/// supplies the properties, and a swatch writes through
/// `AppState::set_param_color`, which keys an animated colour at the playhead
/// and sets a constant one.
pub fn color_rows(
    theme: &Theme,
    group: &str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rows: Vec<_> = {
        let state = state.read(cx);
        state
            .param_colors(group)
            .into_iter()
            .map(|row| {
                let strip = state.key_strip(row.param);
                (row, strip)
            })
            .collect()
    };
    let mut elements = Vec::with_capacity(rows.len());
    for (row, strip) in rows {
        let key = motion::MotionKey::new("insp.color", &row.id);
        let param = row.param;
        let watch = color_stopwatch(&row, strip);
        elements.push(swatch_row(
            theme,
            key,
            row.label,
            (!row.mixed).then_some(row.value),
            Some(watch),
            move |state, rgba| clip_pick::apply(state, clip_pick::Pick::ParamColor { param, rgba }),
            cx,
        ));
    }
    elements
}

/// One colour row: the stopwatch, the label, the colour's name, the keyframe
/// navigator, the reset button and the time area on one line, laid out as a
/// number row's, and the palette on the line under it. `pick` writes the
/// swatch pressed. `value` is `None` where the selected clips read different
/// colours: the row names none and marks no swatch. The stopwatch is the dot a
/// stepper row draws, so a colour is keyed the same way a number is; a colour
/// with no `watch` cannot be animated and draws its label, its name and its
/// palette.
pub fn swatch_row(
    theme: &Theme,
    key: motion::MotionKey,
    label: SharedString,
    value: Option<[u8; 4]>,
    watch: Option<Stopwatch>,
    pick: impl Fn(&mut AppState, [u8; 4]) -> Result<(), EngineError> + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let pick = std::rc::Rc::new(pick);
    let swatches: Vec<_> = crate::app::clip_effects::PARAM_COLORS
        .iter()
        .map(|(name, _, rgba)| {
            let rgba = *rgba;
            let pick = std::rc::Rc::clone(&pick);
            div()
                .id(key.with(name).element_id("insp-swatch"))
                .size(px(16.))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(swatch(rgba))
                .border_1()
                .border_color(if Some(rgba) == value {
                    theme.accent
                } else {
                    theme.border
                })
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = pick(state, rgba) {
                                state.report_error(&err);
                            }
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                )
                .into_any_element()
        })
        .collect();
    let column = watch
        .as_ref()
        .filter(|watch| watch.strip.is_some())
        .map(|watch| watch.param);
    let timed = column.is_some();
    let mut controls = None;
    let mut strip = None;
    let mut dot = None;
    if let Some(mut watch) = watch {
        let (navigator, time_area) = key_controls(theme, key, &mut watch, cx);
        controls = Some(navigator);
        strip = time_area;
        dot = Some(stopwatch_dot(theme, key, watch, cx));
    }
    // The line above the palette is a number row's line with the colour's name
    // for its value, so its time area starts and ends where theirs do.
    let head = div()
        .flex()
        .flex_row()
        .when(!timed, |head| head.wraps())
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .children(dot)
        .child(row_label(key, label, column, theme.text_muted))
        .child(
            value_cell(column)
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(value.map_or(SharedString::new_static(MIXED), color_name)),
        )
        .children(controls)
        .children(strip);
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XS))
        .child(head)
        .child(
            div()
                .flex()
                .flex_row()
                .wraps()
                .gap(px(Theme::SPACE_XXS))
                .children(swatches),
        )
        .into_any_element()
}
