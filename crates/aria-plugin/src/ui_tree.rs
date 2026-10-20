//! The tree a plugin panel declares, and the events a host sends back.
//!
//! A plugin links no UI toolkit. It returns a [`UiNode`] tree of text and
//! controls; the host draws each node with its own widgets and reports what the
//! user did as a [`PanelEvent`]. Every string is an `Arc<str>`, so a host that
//! draws the tree every frame copies pointers, not text.
//!
//! The tree is data, so it serializes: a native plugin passes it as JSON and a
//! WASM component maps it from its WIT variant.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// How deep a tree may nest.
pub const MAX_TREE_DEPTH: usize = 24;
/// How many nodes a tree may hold.
pub const MAX_TREE_NODES: usize = 4096;
/// The longest single string in a tree, in bytes.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
/// All the text in a tree, in bytes.
pub const MAX_TREE_TEXT_BYTES: usize = 1024 * 1024;
/// The longest widget id, in bytes.
pub const MAX_WIDGET_ID_BYTES: usize = 128;
/// How many options a choice may offer.
pub const MAX_CHOICE_OPTIONS: usize = 64;
/// The widest or tallest image, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 2048;
/// All the pixel bytes in a tree.
pub const MAX_TREE_IMAGE_BYTES: usize = 64 * 1024 * 1024;

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

/// One node of a panel tree.
///
/// A control carries an `id`, unique within its tree, that comes back in the
/// [`PanelEvent`] it produces. A control shows the value in the tree: the host
/// holds no state of its own beyond a text draft and a drag in progress, so the
/// next tree the plugin returns is what the panel shows.
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
    /// An on/off switch. Clicking it sends [`PanelEvent::Toggled`] with the
    /// opposite of `on`.
    Toggle {
        id: Arc<str>,
        label: Arc<str>,
        on: bool,
    },
    /// A value dragged between `min` and `max`. Releasing the drag sends
    /// [`PanelEvent::Slid`]. A `step` above zero snaps the value to multiples
    /// of it counted from `min`.
    Slider {
        id: Arc<str>,
        label: Arc<str>,
        value: f64,
        min: f64,
        max: f64,
        #[serde(default)]
        step: f64,
    },
    /// A one-line text box. Enter sends [`PanelEvent::Submitted`]; Escape
    /// leaves the box and sends nothing.
    Input {
        id: Arc<str>,
        value: Arc<str>,
        #[serde(default = "empty")]
        placeholder: Arc<str>,
    },
    /// One of a short list of options, drawn as a row of chips. Clicking an
    /// option sends [`PanelEvent::Chose`].
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
    /// Straight-alpha RGBA8 pixels, `width * height * 4` bytes, drawn at their
    /// own size or narrower to fit the panel.
    Image {
        width: u32,
        height: u32,
        rgba: Arc<[u8]>,
    },
}

fn empty() -> Arc<str> {
    Arc::from("")
}

impl UiNode {
    #[must_use]
    pub fn column(children: impl IntoIterator<Item = UiNode>) -> Self {
        Self::Column {
            children: children.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn row(children: impl IntoIterator<Item = UiNode>) -> Self {
        Self::Row {
            children: children.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn heading(text: impl Into<Arc<str>>) -> Self {
        Self::Heading { text: text.into() }
    }

    #[must_use]
    pub fn text(text: impl Into<Arc<str>>) -> Self {
        Self::Text {
            text: text.into(),
            tone: Tone::Normal,
        }
    }

    #[must_use]
    pub fn toned(text: impl Into<Arc<str>>, tone: Tone) -> Self {
        Self::Text {
            text: text.into(),
            tone,
        }
    }

    #[must_use]
    pub fn field(label: impl Into<Arc<str>>, value: impl Into<Arc<str>>) -> Self {
        Self::Field {
            label: label.into(),
            value: value.into(),
        }
    }

    #[must_use]
    pub fn button(id: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self::Button {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }

    #[must_use]
    pub fn toggle(id: impl Into<Arc<str>>, label: impl Into<Arc<str>>, on: bool) -> Self {
        Self::Toggle {
            id: id.into(),
            label: label.into(),
            on,
        }
    }

    #[must_use]
    pub fn slider(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        value: f64,
        range: std::ops::RangeInclusive<f64>,
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

    /// The control with this id, searched depth first.
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

    /// Check the tree against the limits above and its own consistency: ids
    /// unique and non-empty, numbers finite, a slider's value inside its range,
    /// a choice's selection inside its options, an image's bytes matching its
    /// size.
    ///
    /// Iterative, so a tree nested past [`MAX_TREE_DEPTH`] is refused rather
    /// than walked on the host's stack.
    pub fn check(&self) -> Result<(), TreeError> {
        let mut budget = Budget::default();
        let mut ids: Vec<&str> = Vec::new();
        let mut stack: Vec<(&UiNode, usize)> = vec![(self, 1)];
        while let Some((node, depth)) = stack.pop() {
            if depth > MAX_TREE_DEPTH {
                return Err(TreeError::TooDeep);
            }
            budget.nodes += 1;
            if budget.nodes > MAX_TREE_NODES {
                return Err(TreeError::TooManyNodes);
            }
            if let Some(id) = node.widget_id() {
                check_id(id)?;
                if ids.contains(&&**id) {
                    return Err(TreeError::DuplicateId(id.to_string()));
                }
                ids.push(id);
            }
            check_node(node, &mut budget)?;
            stack.extend(node.children().iter().rev().map(|child| (child, depth + 1)));
        }
        Ok(())
    }
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    text: usize,
    image: usize,
}

impl Budget {
    fn text(&mut self, text: &str) -> Result<(), TreeError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(TreeError::TextTooLong(text.len()));
        }
        self.text += text.len();
        if self.text > MAX_TREE_TEXT_BYTES {
            return Err(TreeError::TooMuchText);
        }
        Ok(())
    }
}

fn check_id(id: &str) -> Result<(), TreeError> {
    if id.trim().is_empty() {
        return Err(TreeError::EmptyId);
    }
    if id.len() > MAX_WIDGET_ID_BYTES || id.chars().any(char::is_control) {
        return Err(TreeError::BadId(id.chars().take(32).collect()));
    }
    Ok(())
}

fn check_node(node: &UiNode, budget: &mut Budget) -> Result<(), TreeError> {
    match node {
        UiNode::Column { .. } | UiNode::Row { .. } | UiNode::Separator => Ok(()),
        UiNode::Heading { text } | UiNode::Text { text, .. } => budget.text(text),
        UiNode::Field { label, value } => {
            budget.text(label)?;
            budget.text(value)
        }
        UiNode::Button { label, .. } | UiNode::Toggle { label, .. } => budget.text(label),
        UiNode::Slider {
            id,
            label,
            value,
            min,
            max,
            step,
        } => {
            budget.text(label)?;
            let finite = [*value, *min, *max, *step].iter().all(|n| n.is_finite());
            if !finite || min >= max || *step < 0.0 || *step > max - min {
                return Err(TreeError::BadRange(id.to_string()));
            }
            if value < min || value > max {
                return Err(TreeError::OutOfRange(id.to_string()));
            }
            Ok(())
        }
        UiNode::Input {
            value, placeholder, ..
        } => {
            budget.text(value)?;
            budget.text(placeholder)
        }
        UiNode::Choice {
            id,
            label,
            options,
            selected,
        } => {
            budget.text(label)?;
            if options.is_empty() || options.len() > MAX_CHOICE_OPTIONS {
                return Err(TreeError::BadOptions(id.to_string()));
            }
            for option in options {
                budget.text(option)?;
            }
            if *selected >= options.len() {
                return Err(TreeError::OutOfRange(id.to_string()));
            }
            Ok(())
        }
        UiNode::Progress { label, fraction } => {
            budget.text(label)?;
            if !(0.0..=1.0).contains(fraction) {
                return Err(TreeError::BadFraction);
            }
            Ok(())
        }
        UiNode::Image {
            width,
            height,
            rgba,
        } => {
            let sides =
                (1..=MAX_IMAGE_SIDE).contains(width) && (1..=MAX_IMAGE_SIDE).contains(height);
            let expected = (*width as usize) * (*height as usize) * 4;
            if !sides || rgba.len() != expected {
                return Err(TreeError::BadImage {
                    width: *width,
                    height: *height,
                    bytes: rgba.len(),
                });
            }
            budget.image += rgba.len();
            if budget.image > MAX_TREE_IMAGE_BYTES {
                return Err(TreeError::TooMuchImage);
            }
            Ok(())
        }
    }
}

/// Why a host refused a tree.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    #[error("the tree nests deeper than {MAX_TREE_DEPTH} levels")]
    TooDeep,
    #[error("the tree holds more than {MAX_TREE_NODES} nodes")]
    TooManyNodes,
    #[error("a string of {0} bytes is longer than {MAX_TEXT_BYTES}")]
    TextTooLong(usize),
    #[error("the tree holds more than {MAX_TREE_TEXT_BYTES} bytes of text")]
    TooMuchText,
    #[error("a control has an empty id")]
    EmptyId,
    #[error(
        "control id {0:?} is longer than {MAX_WIDGET_ID_BYTES} bytes or holds a control character"
    )]
    BadId(String),
    #[error("two controls share the id {0:?}")]
    DuplicateId(String),
    #[error("slider {0:?} needs finite numbers, min below max, and a step from 0 to max - min")]
    BadRange(String),
    #[error("{0:?} holds a value outside its range")]
    OutOfRange(String),
    #[error("choice {0:?} needs 1 to {MAX_CHOICE_OPTIONS} options")]
    BadOptions(String),
    #[error("a progress fraction is outside 0.0..=1.0")]
    BadFraction,
    #[error(
        "an image of {width}x{height} needs sides of 1 to {MAX_IMAGE_SIDE} and width * height * 4 bytes, not {bytes}"
    )]
    BadImage {
        width: u32,
        height: u32,
        bytes: usize,
    },
    #[error("the tree holds more than {MAX_TREE_IMAGE_BYTES} bytes of pixels")]
    TooMuchImage,
}

/// An input event on one control of a panel.
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
    /// The control this event came from.
    pub fn id(&self) -> &Arc<str> {
        match self {
            Self::Pressed { id }
            | Self::Toggled { id, .. }
            | Self::Slid { id, .. }
            | Self::Submitted { id, .. }
            | Self::Chose { id, .. } => id,
        }
    }

    /// This event as the control in `tree` allows it.
    ///
    /// The control must exist and be of the matching kind; a disabled button
    /// takes no press. A slider value is clamped into the range and snapped to
    /// the step, and submitted text is held to [`MAX_TEXT_BYTES`], so a plugin
    /// receives only values its own tree could produce.
    pub fn conform(self, tree: &UiNode) -> Result<Self, EventMismatch> {
        let Some(node) = tree.find(self.id()) else {
            return Err(EventMismatch::NoControl(self.id().to_string()));
        };
        match (self, node) {
            (event @ Self::Pressed { .. }, UiNode::Button { disabled, .. }) => {
                if *disabled {
                    Err(EventMismatch::Disabled(event.id().to_string()))
                } else {
                    Ok(event)
                }
            }
            (event @ Self::Toggled { .. }, UiNode::Toggle { .. }) => Ok(event),
            (Self::Slid { id, value }, UiNode::Slider { min, max, step, .. }) => {
                if !value.is_finite() {
                    return Err(EventMismatch::Wrong(id.to_string()));
                }
                Ok(Self::Slid {
                    id,
                    value: snap(value, *min, *max, *step),
                })
            }
            (Self::Submitted { id, text }, UiNode::Input { .. }) => {
                if text.len() > MAX_TEXT_BYTES || text.chars().any(char::is_control) {
                    return Err(EventMismatch::Wrong(id.to_string()));
                }
                Ok(Self::Submitted { id, text })
            }
            (Self::Chose { id, index }, UiNode::Choice { options, .. }) => {
                if index >= options.len() {
                    return Err(EventMismatch::Wrong(id.to_string()));
                }
                Ok(Self::Chose { id, index })
            }
            (event, _) => Err(EventMismatch::Wrong(event.id().to_string())),
        }
    }
}

/// `value` clamped into `min..=max` and, for a positive `step`, rounded to the
/// nearest multiple of `step` from `min` that is still inside the range.
pub fn snap(value: f64, min: f64, max: f64, step: f64) -> f64 {
    let clamped = value.clamp(min, max);
    if step <= 0.0 {
        return clamped;
    }
    let steps = ((clamped - min) / step).round();
    (min + steps * step).clamp(min, max)
}

/// Why an event does not fit the tree the panel currently shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventMismatch {
    #[error("the panel has no control {0:?}")]
    NoControl(String),
    #[error("control {0:?} is disabled")]
    Disabled(String),
    #[error("control {0:?} does not produce this event or value")]
    Wrong(String),
}

#[cfg(test)]
#[path = "ui_tree_tests.rs"]
mod tests;
