//! Declarative UI panel types and [`Panel`] trait for native plugins.

use std::ops::RangeInclusive;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Host;

/// The frame a declared panel docks in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelLocation {
    /// The right frame.
    Inspector,
    /// The left frame.
    Sidebar,
    /// The bottom frame.
    Timeline,
}

/// Registration metadata for a plugin panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSpec {
    /// Not empty, and unique among panels. By convention
    /// `<plugin id>.<panel>`.
    pub id: String,
    /// The dock tab title and the Window menu row. Not empty.
    pub title: String,
    /// The frame the panel opens in.
    pub location: PanelLocation,
}

impl PanelSpec {
    /// Construct a panel registration spec.
    pub fn new(id: impl Into<String>, title: impl Into<String>, location: PanelLocation) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            location,
        }
    }
}

/// The colour a line of text is drawn in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    /// The body text colour.
    #[default]
    Normal,
    /// The secondary text colour.
    Muted,
    /// The theme's success colour.
    Success,
    /// The theme's warning colour.
    Warning,
    /// The theme's danger colour.
    Danger,
}

fn empty() -> Arc<str> {
    Arc::from("")
}

/// Declarative widget tree returned by [`Panel::view`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiNode {
    /// Children top to bottom.
    Column {
        /// The children, top to bottom.
        children: Vec<UiNode>,
    },
    /// Children left to right, wrapping onto further lines when narrow.
    Row {
        /// The children, left to right.
        children: Vec<UiNode>,
    },
    /// A section title.
    Heading {
        /// The title.
        text: Arc<str>,
    },
    /// A paragraph, wrapped at word boundaries.
    Text {
        /// The paragraph.
        text: Arc<str>,
        /// Its colour.
        #[serde(default)]
        tone: Tone,
    },
    /// A label and its value on one line.
    Field {
        /// The label on the left.
        label: Arc<str>,
        /// The value on the right.
        value: Arc<str>,
    },
    /// A horizontal rule.
    Separator,
    /// A push button. Pressing it sends [`PanelEvent::Pressed`].
    Button {
        /// Control id: unique in the tree, 1 to 128 bytes, no control characters.
        id: Arc<str>,
        /// The button text.
        label: Arc<str>,
        /// Drawn disabled. A press sends no event.
        #[serde(default)]
        disabled: bool,
    },
    /// An on/off switch. Clicking it sends [`PanelEvent::Toggled`] with the opposite of `on`.
    Toggle {
        /// Control id: unique in the tree, 1 to 128 bytes, no control characters.
        id: Arc<str>,
        /// The switch label.
        label: Arc<str>,
        /// The current state.
        on: bool,
    },
    /// A value dragged between `min` and `max`. Releasing the drag sends [`PanelEvent::Slid`].
    Slider {
        /// Control id: unique in the tree, 1 to 128 bytes, no control characters.
        id: Arc<str>,
        /// The slider label.
        label: Arc<str>,
        /// The current value, inside `min..=max`.
        value: f64,
        /// The lowest value: finite and below `max`.
        min: f64,
        /// The highest value: finite.
        max: f64,
        /// The snap step, in `0..=max - min`. 0 clamps without snapping.
        #[serde(default)]
        step: f64,
    },
    /// A one-line text box. Enter sends [`PanelEvent::Submitted`].
    Input {
        /// Control id: unique in the tree, 1 to 128 bytes, no control characters.
        id: Arc<str>,
        /// The current text.
        value: Arc<str>,
        /// The text shown while the box is empty.
        #[serde(default = "empty")]
        placeholder: Arc<str>,
    },
    /// One of a short list of options, drawn as a row of chips. Clicking an option sends [`PanelEvent::Chose`].
    Choice {
        /// Control id: unique in the tree, 1 to 128 bytes, no control characters.
        id: Arc<str>,
        /// The selector label.
        label: Arc<str>,
        /// The chip labels: 1 to 64.
        options: Vec<Arc<str>>,
        /// Index of the selected option, below the option count.
        selected: usize,
    },
    /// A bar filled to `fraction`, in `0.0..=1.0`.
    Progress {
        /// The bar label.
        #[serde(default = "empty")]
        label: Arc<str>,
        /// The filled fraction.
        fraction: f32,
    },
    /// Straight-alpha RGBA8 pixels, `width * height * 4` bytes.
    Image {
        /// Width in pixels, 1 to 2048.
        width: u32,
        /// Height in pixels, 1 to 2048.
        height: u32,
        /// `width * height * 4` bytes of straight-alpha RGBA8.
        rgba: Arc<[u8]>,
    },
}

impl UiNode {
    /// Build a [`Self::Column`].
    #[must_use]
    pub fn column(children: impl IntoIterator<Item = UiNode>) -> Self {
        Self::Column {
            children: children.into_iter().collect(),
        }
    }

    /// Build a [`Self::Row`].
    #[must_use]
    pub fn row(children: impl IntoIterator<Item = UiNode>) -> Self {
        Self::Row {
            children: children.into_iter().collect(),
        }
    }

    /// Build a [`Self::Heading`].
    #[must_use]
    pub fn heading(text: impl Into<Arc<str>>) -> Self {
        Self::Heading { text: text.into() }
    }

    /// Build a [`Self::Text`] with [`Tone::Normal`].
    #[must_use]
    pub fn text(text: impl Into<Arc<str>>) -> Self {
        Self::Text {
            text: text.into(),
            tone: Tone::Normal,
        }
    }

    /// Build a [`Self::Text`] with `tone`.
    #[must_use]
    pub fn toned(text: impl Into<Arc<str>>, tone: Tone) -> Self {
        Self::Text {
            text: text.into(),
            tone,
        }
    }

    /// Build a [`Self::Field`].
    #[must_use]
    pub fn field(label: impl Into<Arc<str>>, value: impl Into<Arc<str>>) -> Self {
        Self::Field {
            label: label.into(),
            value: value.into(),
        }
    }

    /// Build an enabled [`Self::Button`].
    #[must_use]
    pub fn button(id: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self::Button {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }

    /// Build a [`Self::Toggle`].
    #[must_use]
    pub fn toggle(id: impl Into<Arc<str>>, label: impl Into<Arc<str>>, on: bool) -> Self {
        Self::Toggle {
            id: id.into(),
            label: label.into(),
            on,
        }
    }

    /// Build a [`Self::Slider`].
    #[must_use]
    pub fn slider(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        value: f64,
        range: RangeInclusive<f64>,
    ) -> Self {
        Self::Slider {
            id: id.into(),
            label: label.into(),
            value,
            min: *range.start(),
            max: *range.end(),
            step: 0.0,
        }
    }

    /// Build a [`Self::Input`].
    #[must_use]
    pub fn input(
        id: impl Into<Arc<str>>,
        value: impl Into<Arc<str>>,
        placeholder: impl Into<Arc<str>>,
    ) -> Self {
        Self::Input {
            id: id.into(),
            value: value.into(),
            placeholder: placeholder.into(),
        }
    }

    /// Build a [`Self::Choice`].
    #[must_use]
    pub fn choice(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        options: impl IntoIterator<Item = impl Into<Arc<str>>>,
        selected: usize,
    ) -> Self {
        Self::Choice {
            id: id.into(),
            label: label.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
        }
    }

    /// Build a [`Self::Progress`].
    #[must_use]
    pub fn progress(label: impl Into<Arc<str>>, fraction: f32) -> Self {
        Self::Progress {
            label: label.into(),
            fraction,
        }
    }

    /// The id of this node when it is a control.
    pub fn widget_id(&self) -> Option<&Arc<str>> {
        match self {
            Self::Button { id, .. }
            | Self::Toggle { id, .. }
            | Self::Slider { id, .. }
            | Self::Input { id, .. }
            | Self::Choice { id, .. } => Some(id),
            _ => None,
        }
    }

    fn children(&self) -> &[UiNode] {
        match self {
            Self::Column { children } | Self::Row { children } => children,
            _ => &[],
        }
    }

    /// Look up a control in this subtree by `id`.
    pub fn find(&self, id: &str) -> Option<&UiNode> {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            if node.widget_id().is_some_and(|held| &**held == id) {
                return Some(node);
            }
            stack.extend(node.children().iter().rev());
        }
        None
    }
}

/// Interaction event delivered from a panel control to [`Panel::event`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PanelEvent {
    /// A [`UiNode::Button`] was pressed.
    Pressed {
        /// The control's id.
        id: Arc<str>,
    },
    /// A [`UiNode::Toggle`] was switched to `on`.
    Toggled {
        /// The control's id.
        id: Arc<str>,
        /// The new state.
        on: bool,
    },
    /// A [`UiNode::Slider`] was released at `value`.
    Slid {
        /// The control's id.
        id: Arc<str>,
        /// The released value, clamped to the slider's range and snapped to its
        /// step.
        value: f64,
    },
    /// Enter was pressed in a [`UiNode::Input`] holding `text`.
    Submitted {
        /// The control's id.
        id: Arc<str>,
        /// The box's text, at most 16 KiB.
        text: Arc<str>,
    },
    /// Option `index` of a [`UiNode::Choice`] was clicked.
    Chose {
        /// The control's id.
        id: Arc<str>,
        /// Index of the clicked option, below the option count.
        index: usize,
    },
}

impl PanelEvent {
    /// Control id that produced this event.
    pub fn id(&self) -> &Arc<str> {
        match self {
            Self::Pressed { id }
            | Self::Toggled { id, .. }
            | Self::Slid { id, .. }
            | Self::Submitted { id, .. }
            | Self::Chose { id, .. } => id,
        }
    }
}

/// Declarative UI panel implemented by a native plugin and registered with [`Host::register_panel`].
pub trait Panel: Send + Sync + 'static {
    /// Build the declarative widget tree for the panel's current state.
    fn view(&self, host: &Host) -> UiNode;

    /// Handle a user interaction event from one of the panel's controls.
    fn event(&self, _host: &Host, _event: &PanelEvent) -> Result<(), String> {
        Ok(())
    }
}

pub use Panel as PanelView;
