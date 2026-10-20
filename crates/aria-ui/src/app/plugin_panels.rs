//! Plugin panels as the window holds them.
//!
//! A plugin declares a panel as a [`UiNode`] tree through
//! [`aria_plugin::PluginContext::register_panel`], and the panel's code runs on
//! the event delivery thread. This module holds the last tree the window read of
//! every panel, read again when the context's panel revision moves, and the two
//! pieces of state a declared control has on this side: the draft of the text
//! box holding the keyboard, and the value of the slider being dragged. A click
//! is sent as a [`PanelEvent`] through the host's context, which holds it to the
//! tree before the plugin receives it.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use aria_plugin::{
    Activation, MAX_TEXT_BYTES, PanelEvent, PanelLocation, PanelState, UiNode, WaitingPanel, snap,
};
use gpui::{Context, RenderImage};

use super::AppState;
use crate::motion::MotionKey;

/// A declared text box holding the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelInput {
    pub panel: Arc<str>,
    pub control: Arc<str>,
    /// Rebuilt per keystroke, so drawing it is a reference count.
    pub draft: gpui::SharedString,
}

/// A declared slider being dragged. Its value is drawn in place of the tree's
/// until the release sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelSlide {
    pub panel: Arc<str>,
    pub control: Arc<str>,
    pub value: f64,
    min: f64,
    max: f64,
    step: f64,
    /// The track's left edge and width in window pixels, as last painted.
    track: [f32; 2],
}

/// An image's identity: the address of its pixels and its size. The cache entry
/// holds the pixels, so the address cannot be reused while it is a key.
type ImageKey = (usize, u32, u32);

/// Decoded panel images, shared with the frame that draws them.
pub type PanelImages = HashMap<ImageKey, (Arc<[u8]>, Arc<RenderImage>)>;

/// Every plugin panel's last read state.
#[derive(Default)]
pub struct PluginPanels {
    /// The panel revision `shown` was read at; `None` before the first read.
    revision: Option<u64>,
    shown: Arc<[PanelState]>,
    /// The panels of packages waiting to load, drawn as a card that loads
    /// the package when opened.
    waiting: Arc<[WaitingPanel]>,
    pub(crate) input: Option<PanelInput>,
    pub(crate) slide: Option<PanelSlide>,
    /// Each drawn slider's track, `[left, width]` in window pixels, recorded
    /// during paint because a pointer event carries a window position only.
    tracks: HashMap<MotionKey, [f32; 2]>,
    images: Arc<PanelImages>,
    /// Plugin panel IDs already registered into `dock_tree` as tabs.
    known_tabs: std::collections::HashSet<&'static str>,
}

impl std::fmt::Debug for PluginPanels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginPanels")
            .field("revision", &self.revision)
            .field("panels", &self.shown.len())
            .field("waiting", &self.waiting.len())
            .field("input", &self.input)
            .field("slide", &self.slide)
            .field("tracks", &self.tracks.len())
            .field("images", &self.images.len())
            .finish()
    }
}

impl PluginPanels {
    /// Every panel, in registration order.
    pub fn shown(&self) -> &Arc<[PanelState]> {
        &self.shown
    }

    /// The panels docked at `location`, in registration order.
    pub fn at(&self, location: PanelLocation) -> impl Iterator<Item = &PanelState> {
        self.shown
            .iter()
            .filter(move |state| state.spec.location == location)
    }

    /// Every waiting package's declared panel, in the order it was deferred.
    pub fn waiting(&self) -> &Arc<[WaitingPanel]> {
        &self.waiting
    }

    /// Whether anything docks at `location`: a panel, or a waiting one.
    pub fn any_at(&self, location: PanelLocation) -> bool {
        self.at(location).next().is_some()
            || self
                .waiting
                .iter()
                .any(|panel| panel.spec.location == location)
    }

    /// The decoded images, for a frame to clone the handle of.
    pub fn images(&self) -> &Arc<PanelImages> {
        &self.images
    }

    /// The key a slider's track is measured under.
    pub fn slider_key(panel: &str, control: &str) -> MotionKey {
        MotionKey::new("plugin-slider", panel).with(control)
    }

    /// Record where a slider's track painted. Returns whether it moved, so a
    /// paint that measured the same rectangle asks for no second frame.
    pub fn record_track(&mut self, key: MotionKey, track: [f32; 2]) -> bool {
        self.tracks.insert(key, track) != Some(track)
    }
}

/// The decoded image for an `Image` node's pixels, once a read has decoded it.
pub fn image_of(
    images: &PanelImages,
    width: u32,
    height: u32,
    rgba: &Arc<[u8]>,
) -> Option<Arc<RenderImage>> {
    images
        .get(&image_key(width, height, rgba))
        .map(|(_, image)| Arc::clone(image))
}

fn image_key(width: u32, height: u32, rgba: &Arc<[u8]>) -> ImageKey {
    (Arc::as_ptr(rgba).cast::<u8>() as usize, width, height)
}

/// Control `control` of panel `panel`, in the tree the window last read.
fn control_in<'a>(shown: &'a [PanelState], panel: &str, control: &str) -> Option<&'a UiNode> {
    shown
        .iter()
        .find(|state| state.spec.id == panel)?
        .tree
        .as_deref()?
        .find(control)
}

/// Every node of `tree`, parents before children.
fn walk<'a>(tree: &'a UiNode, mut visit: impl FnMut(&'a UiNode)) {
    let mut stack = vec![tree];
    while let Some(node) = stack.pop() {
        visit(node);
        if let UiNode::Column { children } | UiNode::Row { children } = node {
            stack.extend(children.iter().rev());
        }
    }
}

/// The value under window x `at` on a track at `[left, width]`.
fn slider_value(track: [f32; 2], at: f32, min: f64, max: f64, step: f64) -> f64 {
    let [left, width] = track;
    let frac = if width > 0.0 {
        ((at - left) / width).clamp(0.0, 1.0)
    } else {
        0.0
    };
    snap(min + f64::from(frac) * (max - min), min, max, step)
}

impl AppState {
    /// Read the plugin panels again whenever their revision moves. The context
    /// calls the waker from whichever thread made the change and a task on the
    /// window's thread reads. Calling this again replaces the waker, and the
    /// earlier task ends with it.
    pub fn watch_plugin_panels(&mut self, cx: &mut Context<Self>) {
        let (wake, wakes) = crate::handoff::wakes::<()>();
        self.ctx.set_panel_waker(move || {
            wake.wake(());
        });
        cx.spawn(async move |this, cx| {
            while let Some(()) = wakes.next_from_a_thread(cx.background_executor()).await {
                while wakes.try_next().is_some() {}
                if this
                    .update(cx, |state, cx| state.resync_plugin_panels(cx))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
        self.resync_plugin_panels(cx);
    }

    fn resync_plugin_panels(&mut self, cx: &mut Context<Self>) {
        let Some(evicted) = self.sync_plugin_panels() else {
            return;
        };
        for image in evicted {
            cx.drop_image(image, None);
        }
        cx.notify();
    }

    /// Read every panel again when the revision moved since the last read.
    ///
    /// Returns `None` when nothing moved, else the images no tree shows any
    /// more, for the caller to release from the window's atlas. A draft or a
    /// drag whose control is gone is dropped with it. The first read of a
    /// panel queues its first build, which moves the revision when it lands.
    pub fn sync_plugin_panels(&mut self) -> Option<Vec<Arc<RenderImage>>> {
        // Read before the panels: a build landing between the two moves the
        // revision again, and the waker brings this back for it.
        let revision = self.ctx.panel_revision();
        if self.plugin_panels.revision == Some(revision) {
            return None;
        }
        let shown: Arc<[PanelState]> = self
            .ctx
            .panels()
            .iter()
            .filter_map(|spec| self.ctx.panel(&spec.id))
            .collect();
        let waiting: Arc<[WaitingPanel]> = self.ctx.waiting_panels().into();
        let panels = &mut self.plugin_panels;
        panels.revision = Some(revision);
        panels.waiting = waiting;

        let typing = panels.input.as_ref().is_some_and(|input| {
            matches!(
                control_in(&shown, &input.panel, &input.control),
                Some(UiNode::Input { .. })
            )
        });
        if !typing {
            panels.input = None;
        }
        let slider = panels.slide.as_ref().and_then(|slide| {
            match control_in(&shown, &slide.panel, &slide.control) {
                Some(UiNode::Slider { min, max, step, .. }) => Some((*min, *max, *step)),
                _ => None,
            }
        });
        match (slider, panels.slide.as_mut()) {
            (Some((min, max, step)), Some(slide)) => {
                slide.min = min;
                slide.max = max;
                slide.step = step;
                slide.value = snap(slide.value, min, max, step);
            }
            _ => panels.slide = None,
        }

        let mut old = Arc::try_unwrap(std::mem::take(&mut panels.images))
            .unwrap_or_else(|shared| (*shared).clone());
        let mut images = PanelImages::with_capacity(old.len());
        let mut sliders = Vec::new();
        for state in shown.iter() {
            let Some(tree) = state.tree.as_deref() else {
                continue;
            };
            walk(tree, |node| match node {
                UiNode::Slider { id, .. } => {
                    sliders.push(PluginPanels::slider_key(&state.spec.id, id));
                }
                UiNode::Image {
                    width,
                    height,
                    rgba,
                } => {
                    let key = image_key(*width, *height, rgba);
                    if let Some(entry) = old.remove(&key) {
                        images.insert(key, entry);
                    } else if let Entry::Vacant(e) = images.entry(key) {
                        // `UiNode::check` held the size to the bytes, so this
                        // only fails on an empty image, which draws nothing.
                        if let Some(decoded) =
                            crate::frame::rgba_bytes_to_render(*width, *height, rgba)
                        {
                            e.insert((Arc::clone(rgba), decoded));
                        }
                    }
                }
                _ => {}
            });
        }
        panels.tracks.retain(|key, _| sliders.contains(key));
        panels.images = Arc::new(images);
        panels.shown = shown;
        let mut current_tabs = std::collections::HashSet::new();
        let mut newly_added: Vec<(&'static str, PanelLocation)> = Vec::new();
        for state in panels.shown.iter() {
            let id = crate::interned::intern(&state.spec.id);
            current_tabs.insert(id);
            if panels.known_tabs.insert(id) {
                newly_added.push((id, state.spec.location));
            }
        }
        for wait in panels.waiting.iter() {
            let id = crate::interned::intern(&wait.spec.id);
            current_tabs.insert(id);
            if panels.known_tabs.insert(id) {
                newly_added.push((id, wait.spec.location));
            }
        }
        let evicted: Vec<&'static str> = panels
            .known_tabs
            .iter()
            .copied()
            .filter(|id| !current_tabs.contains(id))
            .collect();
        for id in &evicted {
            panels.known_tabs.remove(id);
        }
        let out: Vec<Arc<RenderImage>> = old.into_values().map(|(_, image)| image).collect();
        // A plugin tab opened over the Scopes tab, or closed from in front of
        // it, changes what the scopes have to measure.
        let retabbed = !newly_added.is_empty() || !evicted.is_empty();
        for (id, location) in newly_added {
            if let Some(spec) = crate::plugins::panel_host::DockSpec::for_location(location) {
                self.dock_tree.open_in(id, spec.default_frame);
            }
        }
        for id in evicted {
            self.dock_tree.close(id);
        }
        if retabbed {
            self.sync_scope_demand();
        }
        Some(out)
    }

    /// Load the package that declares waiting panel `panel`, by its `panel:`
    /// event. Reported when the package does not load, or loads and registers
    /// no panel under that id.
    pub fn open_waiting_panel(&mut self, panel: &str) {
        let Some(waiting) = self
            .plugin_panels
            .waiting
            .iter()
            .find(|waiting| waiting.spec.id == panel)
            .cloned()
        else {
            return;
        };
        let activated = self.ctx.activate(&Activation::Panel(panel.to_owned()));
        let loaded = activated.iter().any(|one| one.result.is_ok());
        self.report_activations(activated);
        if loaded && !self.ctx.panels().iter().any(|spec| spec.id == panel) {
            self.report_error(format!(
                "{} did not register panel {panel}",
                waiting.plugin_name
            ));
        }
    }

    /// Send `event` to panel `panel`. A refusal (a control that is gone, a
    /// value the control cannot produce) is reported like a failed action.
    pub fn panel_event(&mut self, panel: &str, event: PanelEvent) {
        if let Err(err) = self.ctx.send_panel_event(panel, event) {
            self.report_error(err.to_string());
        }
    }

    /// Put the keyboard in panel `panel`'s text box `control`, starting from
    /// the value its tree shows. Returns `false` when the tree has no such box.
    pub fn focus_panel_input(&mut self, panel: &str, control: &Arc<str>) -> bool {
        let Some(UiNode::Input { value, .. }) =
            control_in(&self.plugin_panels.shown, panel, control)
        else {
            return false;
        };
        let draft = gpui::SharedString::from(value);
        self.plugin_panels.input = Some(PanelInput {
            panel: Arc::from(panel),
            control: Arc::clone(control),
            draft,
        });
        true
    }

    /// The focused panel box's draft, or nothing.
    pub fn panel_input_text(&self) -> String {
        self.plugin_panels
            .input
            .as_ref()
            .map(|input| input.draft.to_string())
            .unwrap_or_default()
    }

    /// Type one character. A control character is not text, and the draft
    /// stops at the longest text a panel event carries.
    pub fn panel_input_push(&mut self, ch: char) {
        let Some(input) = self.plugin_panels.input.as_mut() else {
            return;
        };
        if ch.is_control() || input.draft.len() + ch.len_utf8() > MAX_TEXT_BYTES {
            return;
        }
        let mut text = input.draft.to_string();
        text.push(ch);
        input.draft = text.into();
    }

    pub fn panel_input_pop(&mut self) {
        if let Some(input) = self.plugin_panels.input.as_mut() {
            let mut text = input.draft.to_string();
            if text.pop().is_some() {
                input.draft = text.into();
            }
        }
    }

    /// Esc: leave the box and send nothing.
    pub fn cancel_panel_input(&mut self) {
        self.plugin_panels.input = None;
    }

    /// Enter: send the draft and leave the box. The panel's next tree shows
    /// what the plugin kept of it.
    pub fn submit_panel_input(&mut self) {
        let Some(input) = self.plugin_panels.input.take() else {
            return;
        };
        self.panel_event(
            &input.panel,
            PanelEvent::Submitted {
                id: input.control,
                text: Arc::from(&*input.draft),
            },
        );
    }

    /// Press on panel `panel`'s slider `control` at window x `at`: the value
    /// moves under the pointer and follows it until the release. Returns
    /// `false` when the slider is not in the tree or has not painted yet.
    pub fn grab_panel_slider(&mut self, panel: &str, control: &Arc<str>, at: f32) -> bool {
        let Some(UiNode::Slider { min, max, step, .. }) =
            control_in(&self.plugin_panels.shown, panel, control)
        else {
            return false;
        };
        let (min, max, step) = (*min, *max, *step);
        let key = PluginPanels::slider_key(panel, control);
        let Some(track) = self.plugin_panels.tracks.get(&key).copied() else {
            return false;
        };
        self.plugin_panels.slide = Some(PanelSlide {
            panel: Arc::from(panel),
            control: Arc::clone(control),
            value: slider_value(track, at, min, max, step),
            min,
            max,
            step,
            track,
        });
        true
    }

    /// The pointer moved to window x `at` with a slider held. Returns whether
    /// the value changed.
    pub fn ride_panel_slider(&mut self, at: f32) -> bool {
        let Some(slide) = self.plugin_panels.slide.as_mut() else {
            return false;
        };
        let value = slider_value(slide.track, at, slide.min, slide.max, slide.step);
        let moved = value.to_bits() != slide.value.to_bits();
        slide.value = value;
        moved
    }

    /// Let go of the held slider: send the value it ended on, unless that is
    /// the value its tree already shows. Returns whether a slider was held.
    pub fn release_panel_slider(&mut self) -> bool {
        let Some(slide) = self.plugin_panels.slide.take() else {
            return false;
        };
        let shown = match control_in(&self.plugin_panels.shown, &slide.panel, &slide.control) {
            Some(UiNode::Slider { value, .. }) => Some(*value),
            _ => None,
        };
        if shown.is_some_and(|value| value.to_bits() != slide.value.to_bits()) {
            self.panel_event(
                &slide.panel,
                PanelEvent::Slid {
                    id: slide.control,
                    value: slide.value,
                },
            );
        }
        true
    }
}
