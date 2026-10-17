//! Declarative UI panel types and [`Panel`] trait for native plugins.

use std::ops::RangeInclusive;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Host;

/// Where a contributed panel docks in the editor shell.
///
/// External plugins may register panels in [`Self::Sidebar`], [`Self::Inspector`],
/// or [`Self::Timeline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelLocation {
    Inspector,
    Sidebar,
    Overlay,
    Timeline,
    Viewer,
    Home,
}

/// Registration metadata for a plugin panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
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
    #[default]
    Normal,
    Muted,
    Success,
    Warning,
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
    Column { children: Vec<UiNode> },
    /// Children left to right, wrapping onto further lines when narrow.
    Row { children: Vec<UiNode> },
    /// A section title.
    Heading { text: Arc<str> },
    /// A paragraph, wrapped at word boundaries.
    Text {
        text: Arc<str>,
        #[serde(default)]
        tone: Tone,
    },
    /// A label and its value on one line.
    Field { label: Arc<str>, value: Arc<str> },
    /// A horizontal rule.
    Separator,
    /// A push button. Pressing it sends [`PanelEvent::Pressed`].
    Button {
        id: Arc<str>,
        label: Arc<str>,
        #[serde(default)]
        disabled: bool,
    },
    /// An on/off switch. Clicking it sends [`PanelEvent::Toggled`] with the opposite of `on`.
    Toggle {
        id: Arc<str>,
        label: Arc<str>,
        on: bool,
    },
    /// A value dragged between `min` and `max`. Releasing the drag sends [`PanelEvent::Slid`].
    Slider {
        id: Arc<str>,
        label: Arc<str>,
        value: f64,
        min: f64,
        max: f64,
        #[serde(default)]
        step: f64,
    },
    /// A one-line text box. Enter sends [`PanelEvent::Submitted`].
    Input {
        id: Arc<str>,
        value: Arc<str>,
        #[serde(default = "empty")]
        placeholder: Arc<str>,
    },
    /// One of a short list of options, drawn as a row of chips. Clicking an option sends [`PanelEvent::Chose`].
    Choice {
        id: Arc<str>,
        label: Arc<str>,
        options: Vec<Arc<str>>,
        selected: usize,
    },
    /// A bar filled to `fraction`, in `0.0..=1.0`.
    Progress {
        #[serde(default = "empty")]
        label: Arc<str>,
        fraction: f32,
    },
    /// Straight-alpha RGBA8 pixels, `width * height * 4` bytes.
    Image {
        width: u32,
        height: u32,
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
    Pressed { id: Arc<str> },
    /// A [`UiNode::Toggle`] was switched to `on`.
    Toggled { id: Arc<str>, on: bool },
    /// A [`UiNode::Slider`] was released at `value`.
    Slid { id: Arc<str>, value: f64 },
    /// Enter was pressed in a [`UiNode::Input`] holding `text`.
    Submitted { id: Arc<str>, text: Arc<str> },
    /// Option `index` of a [`UiNode::Choice`] was clicked.
    Chose { id: Arc<str>, index: usize },
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
