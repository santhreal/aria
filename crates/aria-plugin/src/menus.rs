//! Menus: where a command is offered besides the palette.
//!
//! A menu entry is data bound to a command id. It holds no code: choosing the
//! row runs the command through [`PluginContext::run_command`](crate::PluginContext::run_command).
//! The app draws the rows; this crate links no UI.

use std::fmt;

/// A menu a plugin can add rows to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuLocation {
    File,
    Edit,
    Sequence,
    Markers,
    Effect,
    Animation,
    View,
    Window,
    Help,
    /// The menu a clip opens on the timeline.
    Clip,
    /// The menu a track header opens.
    Track,
    /// The menu an item in the media pool opens.
    Media,
    /// The menu the viewer opens.
    Viewer,
    /// The menu a transition opens on the timeline.
    Transition,
    /// The menu an effect's header opens in the Inspector's Effects tab.
    EffectHeader,
    /// The menu the fx badge on a timeline clip bar opens.
    ClipBadge,
}

impl MenuLocation {
    /// Every location: the menu bar in bar order, then the context menus.
    pub const ALL: [Self; 16] = [
        Self::File,
        Self::Edit,
        Self::Sequence,
        Self::Markers,
        Self::Effect,
        Self::Animation,
        Self::View,
        Self::Window,
        Self::Help,
        Self::Clip,
        Self::Track,
        Self::Media,
        Self::Viewer,
        Self::Transition,
        Self::EffectHeader,
        Self::ClipBadge,
    ];

    /// The location's name, as the menu bar and `plugin.toml` spell it.
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::Sequence => "Sequence",
            Self::Markers => "Markers",
            Self::Effect => "Effect",
            Self::Animation => "Animation",
            Self::View => "View",
            Self::Window => "Window",
            Self::Help => "Help",
            Self::Clip => "Clip",
            Self::Track => "Track",
            Self::Media => "Media",
            Self::Viewer => "Viewer",
            Self::Transition => "Transition",
            Self::EffectHeader => "Effect Header",
            Self::ClipBadge => "Clip Badge",
        }
    }

    /// Whether the location is a menu in the menu bar rather than a context
    /// menu.
    pub fn in_menu_bar(self) -> bool {
        matches!(
            self,
            Self::File
                | Self::Edit
                | Self::Sequence
                | Self::Markers
                | Self::Effect
                | Self::Animation
                | Self::View
                | Self::Window
                | Self::Help
        )
    }

    /// The location `text` names, in any case.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|location| location.label().eq_ignore_ascii_case(text.trim()))
    }
}

/// Where a row goes: a location, and optionally the group it is listed under.
///
/// Written `Location` or `Location/Group`, such as `Sequence/Color Tools`. Menus
/// are one level deep, so a row in a group reads `Color Tools: Reset LUT Stack`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MenuPath {
    pub location: MenuLocation,
    pub group: Option<String>,
}

impl MenuPath {
    /// Read a path, refusing one that names no location or nests deeper than a
    /// group.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parts = text.split('/').map(str::trim);
        let head = parts.next().unwrap_or_default();
        let location = MenuLocation::parse(head).ok_or_else(|| {
            let names: Vec<&str> = MenuLocation::ALL.iter().map(|at| at.label()).collect();
            format!(
                "menu {text:?} names no menu; it starts with one of {}",
                names.join(", ")
            )
        })?;
        let group = match (parts.next(), parts.next()) {
            (None, _) => None,
            (Some(""), _) => return Err(format!("menu {text:?} has an empty group")),
            (Some(group), None) => Some(group.to_owned()),
            (Some(_), Some(_)) => {
                return Err(format!(
                    "menu {text:?} nests deeper than one group, such as `Sequence/Color Tools`"
                ));
            }
        };
        Ok(Self { location, group })
    }

    /// The text a row with this path and `title` draws.
    pub fn row_label(&self, title: &str) -> String {
        match &self.group {
            Some(group) => format!("{group}: {title}"),
            None => title.to_owned(),
        }
    }
}

impl fmt::Display for MenuPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.group {
            Some(group) => write!(f, "{}/{group}", self.location.label()),
            None => f.write_str(self.location.label()),
        }
    }
}

/// A command offered as a menu row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MenuCommand {
    /// The command the row runs.
    pub command: String,
    pub path: MenuPath,
}

#[cfg(test)]
#[path = "menus_tests.rs"]
mod tests;
