//! Plugin panels: a declared [`UiNode`] tree drawn with the shell's own controls.
//!
//! Each node is drawn by the control the rest of the editor uses for the same
//! job, so a plugin panel reads as a built-in one. A control shows the value in
//! the tree, except the text box holding the keyboard, which shows its draft,
//! and the slider being dragged, which shows the value under the pointer.

use crate::components::Wraps as _;
use std::sync::Arc;

use aria_plugin::{PanelEvent, PanelLocation, PanelSpec, PanelState, Tone, UiNode, WaitingPanel};
use gpui::{
    AnyElement, CursorStyle, Entity, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ObjectFit, ParentElement, SharedString, Styled, StyledImage, canvas, div, img,
    px, relative,
};

use crate::app::AppState;
use crate::app::plugin_panels::{PanelImages, PanelInput, PanelSlide, PluginPanels, image_of};
use crate::components::{self, ChipSize};
use crate::fluent::FluentBuilder;
use crate::motion::{self, MotionKey};
use crate::shell::Shell;
use crate::theme::Theme;

/// What every node of one panel is drawn against.
struct Draw<'a> {
    theme: &'a Theme,
    spec: &'a Arc<PanelSpec>,
    /// The panel's own key; each control's is this with the control's id.
    key: MotionKey,
    input: Option<&'a PanelInput>,
    slide: Option<&'a PanelSlide>,
    images: &'a PanelImages,
    state: &'a Entity<AppState>,
}

/// The plugin panels docked at `location`, in registration order, then the
/// panels of packages waiting to load.
pub(crate) fn render_at(
    location: PanelLocation,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
) -> Vec<AnyElement> {
    let (shown, waiting, input, slide, images) = {
        let panels = &state.read(cx).plugin_panels;
        if !panels.any_at(location) {
            return Vec::new();
        }
        (
            Arc::clone(panels.shown()),
            Arc::clone(panels.waiting()),
            panels.input.clone(),
            panels.slide.clone(),
            Arc::clone(panels.images()),
        )
    };
    let theme = Theme::of(cx);
    let mut cards: Vec<AnyElement> = shown
        .iter()
        .filter(|panel| panel.spec.location == location)
        .map(|panel| {
            let draw = Draw {
                theme: &theme,
                spec: &panel.spec,
                key: MotionKey::new("plugin-panel", &panel.spec.id),
                input: input.as_ref(),
                slide: slide.as_ref(),
                images: &images,
                state,
            };
            card(&draw, panel, cx)
        })
        .collect();
    cards.extend(
        waiting
            .iter()
            .filter(|panel| panel.spec.location == location)
            .map(|panel| waiting_card(&theme, panel, cx)),
    );
    cards
}

/// Render a single declared or waiting plugin panel by its id, for a dock
/// frame whose active tab is that plugin panel.
pub(crate) fn render_one(
    panel: &str,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
) -> Option<AnyElement> {
    let (shown, waiting, input, slide, images) = {
        let panels = &state.read(cx).plugin_panels;
        (
            Arc::clone(panels.shown()),
            Arc::clone(panels.waiting()),
            panels.input.clone(),
            panels.slide.clone(),
            Arc::clone(panels.images()),
        )
    };
    let theme = Theme::of(cx);
    if let Some(found) = shown.iter().find(|held| held.spec.id == panel) {
        let draw = Draw {
            theme: &theme,
            spec: &found.spec,
            key: MotionKey::new("plugin-panel", &found.spec.id),
            input: input.as_ref(),
            slide: slide.as_ref(),
            images: &images,
            state,
        };
        return Some(card(&draw, found, cx));
    }
    if let Some(found) = waiting.iter().find(|held| held.spec.id == panel) {
        return Some(waiting_card(&theme, found, cx));
    }
    None
}

/// A waiting package's panel: its title, the package that draws it, and Open,
/// which loads the package.
fn waiting_card(theme: &Theme, panel: &WaitingPanel, cx: &mut gpui::Context<Shell>) -> AnyElement {
    let key = MotionKey::new("plugin-panel", &panel.spec.id);
    let id = panel.spec.id.clone();
    let open = components::button(
        ("plugin-panel-open", key.with("open")),
        SharedString::new_static("Open"),
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_waiting_panel(&id));
            cx.notify();
        }),
    );
    motion::fade_in(
        key.with("card"),
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
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text_muted)
                    .child(SharedString::from(&panel.spec.title)),
            )
            .child(line(
                theme,
                SharedString::from(format!("{} loads when opened", panel.plugin_name)),
                Tone::Muted,
            ))
            .child(div().flex().child(open)),
    )
    .into_any_element()
}

fn card(draw: &Draw<'_>, panel: &PanelState, cx: &mut gpui::Context<Shell>) -> AnyElement {
    let theme = draw.theme;
    let body = match (&panel.tree, &panel.refused) {
        (Some(tree), _) => node(draw, tree, cx),
        (None, Some(reason)) => line(theme, SharedString::from(reason), Tone::Danger),
        (None, None) => line(theme, SharedString::new_static("Loading"), Tone::Muted),
    };
    // A refused rebuild keeps the tree before it on screen, with the reason.
    let stale = panel.tree.as_ref().and(panel.refused.as_ref());
    motion::fade_in(
        draw.key.with("card"),
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
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text_muted)
                    .child(SharedString::from(&draw.spec.title)),
            )
            .child(body)
            .when_some(stale, |el, reason| {
                el.child(line(theme, SharedString::from(reason), Tone::Danger))
            })
            .when_some(panel.event_error.as_ref(), |el, error| {
                el.child(line(theme, SharedString::from(error), Tone::Danger))
            }),
    )
    .into_any_element()
}

fn tone_color(theme: &Theme, tone: Tone) -> gpui::Hsla {
    match tone {
        Tone::Normal => theme.text,
        Tone::Muted => theme.text_muted,
        Tone::Success => theme.success,
        Tone::Warning => theme.warning,
        Tone::Danger => theme.danger,
    }
}

fn line(theme: &Theme, text: SharedString, tone: Tone) -> AnyElement {
    div()
        .text_size(Theme::TEXT_TITLE)
        .text_color(tone_color(theme, tone))
        .child(text)
        .into_any_element()
}

fn label(theme: &Theme, text: &Arc<str>) -> gpui::Div {
    div()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        .child(SharedString::from(text))
}

/// One node and everything under it. `UiNode::check` held the depth to
/// `MAX_TREE_DEPTH` before the tree reached the window.
fn node(draw: &Draw<'_>, node_: &UiNode, cx: &mut gpui::Context<Shell>) -> AnyElement {
    let theme = draw.theme;
    match node_ {
        UiNode::Column { children } => div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .children(children.iter().map(|child| node(draw, child, cx)))
            .into_any_element(),
        UiNode::Row { children } => div()
            .flex()
            .flex_row()
            .wraps()
            .items_center()
            .gap(px(Theme::SPACE_SM))
            .children(children.iter().map(|child| node(draw, child, cx)))
            .into_any_element(),
        UiNode::Heading { text } => div()
            .text_size(Theme::TEXT_UI)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .child(SharedString::from(text))
            .into_any_element(),
        UiNode::Text { text, tone } => line(theme, SharedString::from(text), *tone),
        UiNode::Field { label: name, value } => div()
            .flex()
            .justify_between()
            .gap(px(Theme::SPACE_SM))
            .text_size(Theme::TEXT_TITLE)
            .child(
                div()
                    .text_color(theme.text_muted)
                    .child(SharedString::from(name)),
            )
            .child(
                div()
                    .text_color(theme.text)
                    .child(SharedString::from(value)),
            )
            .into_any_element(),
        UiNode::Separator => div().h(px(1.)).w_full().bg(theme.border).into_any_element(),
        UiNode::Button {
            id,
            label: text,
            disabled,
        } => button(draw, id, text, *disabled, cx),
        UiNode::Toggle {
            id,
            label: text,
            on,
        } => toggle(draw, id, text, *on, cx),
        UiNode::Slider {
            id,
            label: text,
            value,
            min,
            max,
            step,
        } => slider(
            draw,
            draw.key.with(&**id),
            id,
            text,
            [*value, *min, *max, *step],
            cx,
        ),
        UiNode::Input {
            id,
            value,
            placeholder,
        } => input(draw, id, value, placeholder, cx),
        UiNode::Choice {
            id,
            label: text,
            options,
            selected,
        } => choice(draw, id, text, options, *selected, cx),
        UiNode::Progress {
            label: text,
            fraction,
        } => div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XXS))
            .when(!text.is_empty(), |el| el.child(label(theme, text)))
            .child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded_full()
                    .bg(theme.border)
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(theme.accent),
                    ),
            )
            .into_any_element(),
        UiNode::Image {
            width,
            height,
            rgba,
        } => match image_of(draw.images, *width, *height, rgba) {
            Some(image) => crate::components::picture(
                img(image)
                    .w_full()
                    .max_w(px(*width as f32))
                    .aspect_ratio(*width as f32 / *height as f32)
                    .object_fit(ObjectFit::Contain),
            )
            .into_any_element(),
            // Not decoded until the next read; it draws then.
            None => div().into_any_element(),
        },
    }
}

/// Send `event` from a listener, as the shell's state.
fn send(this: &mut Shell, spec: &PanelSpec, event: PanelEvent, cx: &mut gpui::Context<Shell>) {
    this.state
        .update(cx, |state, _| state.panel_event(&spec.id, event));
    cx.notify();
}

fn button(
    draw: &Draw<'_>,
    id: &Arc<str>,
    text: &Arc<str>,
    disabled: bool,
    cx: &mut gpui::Context<Shell>,
) -> AnyElement {
    let control = components::button(
        ("plugin-ui-button", draw.key.with(&**id)),
        SharedString::from(text),
        draw.theme,
    );
    if disabled {
        // Drawn, and takes no press: the host would refuse one anyway.
        return control
            .opacity(0.45)
            .cursor(CursorStyle::Arrow)
            .into_any_element();
    }
    let spec = Arc::clone(draw.spec);
    let id = Arc::clone(id);
    control
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let event = PanelEvent::Pressed {
                    id: Arc::clone(&id),
                };
                send(this, &spec, event, cx);
            }),
        )
        .into_any_element()
}

fn toggle(
    draw: &Draw<'_>,
    id: &Arc<str>,
    text: &Arc<str>,
    on: bool,
    cx: &mut gpui::Context<Shell>,
) -> AnyElement {
    let theme = draw.theme;
    let spec = Arc::clone(draw.spec);
    let id = Arc::clone(id);
    components::chip(
        ("plugin-ui-toggle", draw.key.with(&*id)),
        SharedString::from(text),
        on,
        ChipSize::Regular,
        theme,
    )
    .flex()
    .items_center()
    .gap(px(Theme::SPACE_XS))
    .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(if on {
        theme.accent
    } else {
        theme.border
    }))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            let event = PanelEvent::Toggled {
                id: Arc::clone(&id),
                on: !on,
            };
            send(this, &spec, event, cx);
        }),
    )
    .into_any_element()
}

/// `[value, min, max, step]` as the tree holds them. `key` is the slider's
/// identity; `control` is the id its events name.
fn slider(
    draw: &Draw<'_>,
    key: MotionKey,
    control: &Arc<str>,
    text: &Arc<str>,
    [value, min, max, step]: [f64; 4],
    cx: &mut gpui::Context<Shell>,
) -> AnyElement {
    let theme = draw.theme;
    let held = draw
        .slide
        .filter(|slide| *slide.panel == *draw.spec.id && slide.control == *control);
    let value = held.map_or(value, |slide| slide.value);
    let frac = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let shown = if step >= 1.0 && step.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    let measured = PluginPanels::slider_key(&draw.spec.id, control);
    let spec = Arc::clone(draw.spec);
    let control = Arc::clone(control);
    let track = div()
        .id(key.element_id("plugin-slider"))
        .relative()
        .flex_1()
        .h(px(16.))
        .flex()
        .items_center()
        .cursor(CursorStyle::ResizeLeftRight)
        .child(
            div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(theme.border)
                .child(
                    div()
                        .h_full()
                        .w(relative(frac))
                        .rounded_full()
                        .bg(if held.is_some() {
                            theme.accent
                        } else {
                            theme.text_muted
                        }),
                ),
        )
        .child(measure(draw.state, measured))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let at = f32::from(event.position.x);
                let grabbed = this.state.update(cx, |state, _| {
                    state.grab_panel_slider(&spec.id, &control, at)
                });
                if grabbed {
                    cx.notify();
                }
            }),
        );
    div()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(label(theme, text).w(px(72.)))
        .child(track)
        .child(
            div()
                .w(px(48.))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(SharedString::from(shown)),
        )
        .into_any_element()
}

/// Records a slider track's rectangle every paint, so a press can be turned
/// into a value. Draws nothing, and asks for a frame only when the rectangle
/// moved; anything else is a repaint loop.
fn measure(state: &Entity<AppState>, key: MotionKey) -> impl IntoElement {
    let state = Entity::clone(state);
    canvas(
        move |bounds, _, cx| {
            let track = [f32::from(bounds.origin.x), f32::from(bounds.size.width)];
            state.update(cx, |state, cx| {
                if state.plugin_panels.record_track(key, track) {
                    cx.notify();
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn input(
    draw: &Draw<'_>,
    id: &Arc<str>,
    value: &Arc<str>,
    placeholder: &Arc<str>,
    cx: &mut gpui::Context<Shell>,
) -> AnyElement {
    let typing = draw
        .input
        .filter(|input| *input.panel == *draw.spec.id && input.control == *id);
    let text = match typing {
        Some(input) => input.draft.clone(),
        None => SharedString::from(value),
    };
    let spec = Arc::clone(draw.spec);
    let control = Arc::clone(id);
    components::text_box(
        ("plugin-ui-input", draw.key.with(&**id)),
        text,
        SharedString::from(placeholder),
        typing.is_some(),
        draw.theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.focus_panel_input(&spec.id, &control));
            cx.notify();
        }),
    )
    .into_any_element()
}

fn choice(
    draw: &Draw<'_>,
    id: &Arc<str>,
    text: &Arc<str>,
    options: &[Arc<str>],
    selected: usize,
    cx: &mut gpui::Context<Shell>,
) -> AnyElement {
    let theme = draw.theme;
    let key = draw.key.with(&**id);
    let chips: Vec<AnyElement> = options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let spec = Arc::clone(draw.spec);
            let id = Arc::clone(id);
            components::chip(
                ("plugin-ui-choice", key.with(index)),
                SharedString::from(option),
                index == selected,
                ChipSize::Compact,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    let event = PanelEvent::Chose {
                        id: Arc::clone(&id),
                        index,
                    };
                    send(this, &spec, event, cx);
                }),
            )
            .into_any_element()
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .when(!text.is_empty(), |el| el.child(label(theme, text)))
        .child(
            div()
                .flex()
                .flex_row()
                .wraps()
                .gap(px(Theme::SPACE_XXS))
                .children(chips),
        )
        .into_any_element()
}
