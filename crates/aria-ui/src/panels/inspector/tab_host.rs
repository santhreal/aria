//! The inspector's tabs, as data.
//!
//! A tab is a row: an id, a label, the clip kinds it applies to, and the body
//! it draws. The inspector renders the rows the host carries and nothing else,
//! so a tab arrives by adding a row rather than by extending an enum and the
//! three matches that read it. The id is the identity the app stores as the
//! selected tab, which is why it is a string: a tab contributed at runtime has
//! no enum variant to be.

use std::sync::LazyLock;

use gpui::{AnyElement, Entity, IntoElement, SharedString};

use crate::app::AppState;
use crate::theme::Theme;
use crate::types::{UiClip, UiClipKind};

/// A tab body. Every tab takes the same arguments so the row can be data; a
/// body that does not need the frame rate or the entity ignores them.
pub type TabBody = fn(
    &Theme,
    &UiClip,
    i32,
    i32,
    &Entity<AppState>,
    &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement;

/// One tab.
pub struct TabEntry {
    /// Stable identity, and what `AppState::inspector_tab` holds.
    pub id: SharedString,
    /// What the tab strip shows.
    pub label: SharedString,
    /// Whether the tab applies to a clip of this kind.
    pub applies: fn(UiClipKind) -> bool,
    /// What the tab draws.
    pub body: TabBody,
}

/// The tabs the inspector draws, in strip order.
pub struct TabHost {
    tabs: Vec<TabEntry>,
}

impl TabHost {
    /// A host with no tabs.
    pub fn new() -> Self {
        Self { tabs: Vec::new() }
    }

    /// Append a tab. Order is contribution order, which is strip order.
    pub fn add(&mut self, tab: TabEntry) {
        self.tabs.push(tab);
    }

    /// Every tab, in strip order.
    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    /// How many tabs the host carries.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the host carries no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The tab with this id, whether or not it applies to anything.
    pub fn get(&self, id: &str) -> Option<&TabEntry> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// The tabs that apply to a clip of this kind, in strip order.
    pub fn visible(&self, kind: UiClipKind) -> impl Iterator<Item = &TabEntry> {
        self.tabs.iter().filter(move |tab| (tab.applies)(kind))
    }

    /// The tab to draw for a clip of `kind` when `requested` is selected.
    ///
    /// A selected tab that does not apply to the clip falls back to the first
    /// that does, so selecting Text and then an audio clip draws something.
    /// `None` means no tab applies at all, which the caller draws as empty
    /// rather than guessing.
    pub fn resolve(&self, requested: &str, kind: UiClipKind) -> Option<&TabEntry> {
        match self.get(requested) {
            Some(tab) if (tab.applies)(kind) => Some(tab),
            _ => self.visible(kind).next(),
        }
    }
}

impl Default for TabHost {
    fn default() -> Self {
        Self::new()
    }
}

/// The shipped tabs.
pub fn host_of() -> &'static TabHost {
    &TAB_HOST
}

static TAB_HOST: LazyLock<TabHost> = LazyLock::new(|| {
    let mut host = TabHost::new();
    for (id, label, applies, body) in SHIPPED {
        host.add(TabEntry {
            id: SharedString::new_static(id),
            label: SharedString::new_static(label),
            applies: *applies,
            body: *body,
        });
    }
    host
});

type ShippedTab = (&'static str, &'static str, fn(UiClipKind) -> bool, TabBody);

static SHIPPED: &[ShippedTab] = &[
    ("transform", "Transform", not_audio, transform_body),
    ("look", "Look", not_audio, look_body),
    ("effects", "Effects", not_audio, effects_body),
    ("speed", "Speed", media_or_audio, speed_body),
    ("audio", "Audio", media_or_audio, audio_body),
    ("text", "Text", text_only, text_body),
    ("shape", "Shape", shape_only, shape_body),
    ("info", "Info", not_audio, info_body),
];

fn not_audio(kind: UiClipKind) -> bool {
    kind != UiClipKind::Audio
}

// Retiming is a media property, so Speed follows the same rule as Audio: a
// generated clip has nothing to retime.
fn media_or_audio(kind: UiClipKind) -> bool {
    matches!(kind, UiClipKind::Audio | UiClipKind::Media)
}

fn text_only(kind: UiClipKind) -> bool {
    kind == UiClipKind::Text
}

// A shape's geometry is its own tab for the same reason a title's type is: the
// properties exist on one kind of clip and on no other.
fn shape_only(kind: UiClipKind) -> bool {
    kind == UiClipKind::Shape
}

fn transform_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::transform::transform_tab(theme, clip, state, cx).into_any_element()
}

fn look_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::look::look_tab(theme, clip, state, cx).into_any_element()
}

fn effects_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::effects::effects_tab(theme, clip, state, cx).into_any_element()
}

fn speed_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::speed::speed_block(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

fn audio_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::audio::audio_tab(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

fn text_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::text::text_tab(theme, clip, state, cx).into_any_element()
}

fn shape_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::shape::shape_tab(theme, clip, state, cx).into_any_element()
}

fn info_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::info::info_tab(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strip is what the host carries, so the host is measured rather than
    /// named: a tab added without a decision about which clips it applies to
    /// shows up here, not in a screenshot.
    #[test]
    fn the_host_carries_the_shipped_tabs_once_each() {
        let host = host_of();
        assert_eq!(host.len(), SHIPPED.len());
        for tab in host.tabs() {
            assert_eq!(
                host.tabs()
                    .iter()
                    .filter(|other| other.id == tab.id)
                    .count(),
                1,
                "{} is registered twice",
                tab.id
            );
            assert!(!tab.label.is_empty(), "{} has no label", tab.id);
        }
        assert!(
            host.get(crate::app::DEFAULT_INSPECTOR_TAB).is_some(),
            "the tab the app opens on, {}, is not registered",
            crate::app::DEFAULT_INSPECTOR_TAB
        );
    }

    #[test]
    fn shipped_tabs_are_allocated_once_for_the_process() {
        assert!(
            std::ptr::eq(host_of(), host_of()),
            "a repaint rebuilt the inspector tab host"
        );
    }

    /// Every clip kind must have somewhere to go. A kind with no applicable tab
    /// draws an empty inspector, which is the defect this asserts against, and a
    /// new `UiClipKind` variant fails `UiClipKind::ALL`'s exhaustive census
    /// before it reaches here.
    #[test]
    fn every_clip_kind_has_a_tab_and_resolution_is_total() {
        let host = host_of();
        for kind in UiClipKind::ALL {
            let visible: Vec<_> = host.visible(kind).map(|tab| tab.id.clone()).collect();
            assert!(!visible.is_empty(), "{kind:?} has no inspector tab");

            // Whatever is selected, the resolved tab applies to the clip.
            for tab in host.tabs() {
                let resolved = host
                    .resolve(&tab.id, kind)
                    .unwrap_or_else(|| panic!("{kind:?} resolved {} to nothing", tab.id));
                assert!(
                    (resolved.applies)(kind),
                    "{kind:?} selected {} and resolved to {}, which does not apply",
                    tab.id,
                    resolved.id
                );
            }

            // An id nothing registered resolves to the first applicable tab
            // rather than to nothing.
            let fallback = host
                .resolve("no-such-tab", kind)
                .unwrap_or_else(|| panic!("{kind:?} has no fallback tab"));
            assert_eq!(fallback.id, visible[0]);
        }
    }

    /// A selected tab that applies is kept. Without this the strip would snap
    /// back to the first tab on every render.
    #[test]
    fn a_selected_tab_that_applies_is_kept() {
        let host = host_of();
        let resolved = host
            .resolve("info", UiClipKind::Media)
            .expect("info applies to media");
        assert_eq!(resolved.id, "info");
        // Text does not apply to an audio clip, so the strip falls back to the
        // first row that does, which is Speed's position in the table.
        let audio = host
            .resolve("text", UiClipKind::Audio)
            .expect("audio has a tab");
        assert_eq!(audio.id, "speed");
    }
}
