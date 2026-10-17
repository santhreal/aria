//! The Effects panel: every video effect, audio effect, transition, generator,
//! clip type and animation preset the editor can apply, as one searchable tree.
//!
//! The catalog rows are [`crate::catalog_text::effect_entries`], shared per
//! catalog revision, so a frame that draws the panel copies no catalog text. Presets and load failures are read from the
//! state on each frame the panel is shown.
//!
//! The tree is a flat list of rows with a depth, built by [`tree`]. A folder
//! opens on click and stays open for the session. A search opens every folder
//! that holds a match and hides every row that does not match.

use std::collections::BTreeSet;

use aria_engine::EngineError;
use aria_plugin::ContributionKind;
use gpui::SharedString;

use super::AppState;
use crate::types::{CatalogDragKind, SidebarTab};

/// The longest search the panel's box takes, in characters.
pub const QUERY_MAX_LEN: usize = 64;

/// A top-level folder of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// The entries starred in this panel, saved in the UI settings.
    Favourites,
    VideoEffects,
    AudioEffects,
    /// Picture blends: the transition catalog.
    Transitions,
    /// Crossfades for a cut between sounds.
    AudioTransitions,
    /// Generators and plugin clip types: entries that place a clip.
    Generators,
    /// Saved presets and plugin presets.
    Presets,
    /// Plugins that did not mount and preset files that did not read.
    Failed,
}

impl Group {
    /// Every group, in the order the tree lists them.
    pub const ALL: [Self; 8] = [
        Self::Favourites,
        Self::VideoEffects,
        Self::AudioEffects,
        Self::Transitions,
        Self::AudioTransitions,
        Self::Generators,
        Self::Presets,
        Self::Failed,
    ];

    /// The caption over a tree that lists only this group.
    pub fn only_label(self) -> &'static str {
        match self {
            Self::Favourites => "Favourites only",
            Self::VideoEffects => "Video effects only",
            Self::AudioEffects => "Audio effects only",
            Self::Transitions => "Video transitions only",
            Self::AudioTransitions => "Audio transitions only",
            Self::Generators => "Generators only",
            Self::Presets => "Animation presets only",
            Self::Failed => "Failures only",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Favourites => "Favourites",
            Self::VideoEffects => "Video Effects",
            Self::AudioEffects => "Audio Effects",
            Self::Transitions => "Video Transitions",
            Self::AudioTransitions => "Audio Transitions",
            Self::Generators => "Generators",
            Self::Presets => "Animation Presets",
            Self::Failed => "Failed to Load",
        }
    }
}

/// What applying an entry does, and where it is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    /// Added to the selected clip's effect chain.
    VideoEffect,
    /// Added to the insert chain of every selected clip with sound.
    AudioEffect,
    /// Set at the selected clip's right cut, between pictures.
    Transition,
    /// Set at the selected clip's right cut, between sounds.
    AudioTransition,
    /// Placed at the playhead as a clip of its own.
    Generator,
    /// A plugin clip type, placed at the playhead.
    ClipType,
    /// Applied across the selected clips, anchored at the playhead.
    Preset,
}

impl EntryKind {
    /// Every kind. A new kind fails the panel's tests until it is listed,
    /// applied and dropped.
    pub const ALL: [Self; 7] = [
        Self::VideoEffect,
        Self::AudioEffect,
        Self::Transition,
        Self::AudioTransition,
        Self::Generator,
        Self::ClipType,
        Self::Preset,
    ];

    /// The top-level folder the kind is listed under.
    pub fn group(self) -> Group {
        match self {
            Self::VideoEffect => Group::VideoEffects,
            Self::AudioEffect => Group::AudioEffects,
            Self::Transition => Group::Transitions,
            Self::AudioTransition => Group::AudioTransitions,
            Self::Generator | Self::ClipType => Group::Generators,
            Self::Preset => Group::Presets,
        }
    }

    /// The prefix of the key a favourite of this kind is saved under.
    pub fn tag(self) -> &'static str {
        match self {
            Self::VideoEffect => "video",
            Self::AudioEffect => "audio",
            Self::Transition => "transition",
            Self::AudioTransition => "audio-transition",
            Self::Generator => "generator",
            Self::ClipType => "clip-type",
            Self::Preset => "preset",
        }
    }

    /// The payload a drag of an entry of this kind carries.
    pub fn drag(self) -> CatalogDragKind {
        match self {
            Self::VideoEffect => CatalogDragKind::Effect,
            Self::AudioEffect => CatalogDragKind::AudioEffect,
            Self::Transition | Self::AudioTransition => CatalogDragKind::Transition,
            Self::Generator => CatalogDragKind::Generator,
            Self::ClipType => CatalogDragKind::ClipType,
            Self::Preset => CatalogDragKind::Preset,
        }
    }

    /// The registration a plugin makes to add an entry of this kind, or `None`
    /// for a closed set no plugin extends.
    fn contribution(self) -> Option<ContributionKind> {
        match self {
            Self::VideoEffect | Self::Generator => Some(ContributionKind::Effect),
            Self::AudioEffect => Some(ContributionKind::AudioEffect),
            Self::Transition => Some(ContributionKind::Transition),
            Self::AudioTransition => None,
            Self::ClipType => Some(ContributionKind::ClipType),
            Self::Preset => Some(ContributionKind::Preset),
        }
    }
}

/// One entry the panel lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    /// The catalog id, or the preset's name.
    pub id: SharedString,
    pub label: SharedString,
    /// The folder the entry is in under its group; empty for a group without
    /// folders.
    pub category: SharedString,
}

impl Entry {
    pub(crate) fn new(kind: EntryKind, id: &str, label: &str, category: &str) -> Self {
        Self {
            kind,
            id: SharedString::from(id),
            label: SharedString::from(label),
            category: SharedString::from(category),
        }
    }

    /// The key the entry is saved under as a favourite: its kind's tag and its
    /// id, as `video:gaussian_blur`.
    pub fn favourite_key(&self) -> String {
        favourite_key(self.kind, &self.id)
    }

    /// Whether `key`, as saved, names this entry.
    fn is_saved_as(&self, key: &str) -> bool {
        key.split_once(':') == Some((self.kind.tag(), self.id.as_str()))
    }

    /// Whether the label, the id or the folder holds `needle`, ignoring case.
    fn matches(&self, needle: &[char]) -> bool {
        contains_folded(&self.label, needle)
            || contains_folded(&self.id, needle)
            || contains_folded(&self.category, needle)
    }
}

/// The key an entry of `kind` named `id` is saved under as a favourite.
pub fn favourite_key(kind: EntryKind, id: &str) -> String {
    format!("{}:{id}", kind.tag())
}

/// A plugin that did not mount, or a preset file that did not read, with the
/// reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    /// The plugin's name, or the preset file's name.
    pub source: SharedString,
    pub reason: SharedString,
}

/// A folder of the tree: a group, or a category inside one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FolderKey {
    pub group: Group,
    /// Empty for the group's own folder.
    pub category: SharedString,
}

impl FolderKey {
    pub fn group(group: Group) -> Self {
        Self {
            group,
            category: SharedString::default(),
        }
    }
}

/// One row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<'a> {
    Folder {
        key: FolderKey,
        label: SharedString,
        depth: u8,
        open: bool,
        /// The entries inside, counting only matches during a search.
        count: usize,
    },
    Entry {
        entry: &'a Entry,
        /// Where the row sits: under Favourites, or under its own group.
        group: Group,
        depth: u8,
        favourite: bool,
    },
    Failed {
        failed: &'a Failed,
    },
}

/// The panel's own state: the search, the open folders, and the picked row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectsBrowser {
    pub query: String,
    pub focused: bool,
    pub open: BTreeSet<FolderKey>,
    /// The row a single click picked, drawn highlighted.
    pub picked: Option<(EntryKind, SharedString)>,
    /// The one group the tree lists, or `None` for every group. The Audio
    /// workspace sets Audio Effects.
    pub only: Option<Group>,
}

impl Default for EffectsBrowser {
    /// Favourites and Failed to Load start open: the first holds what the
    /// operator starred to keep within reach, and the second is listed so a
    /// failure is read rather than found.
    fn default() -> Self {
        Self {
            query: String::new(),
            focused: false,
            open: BTreeSet::from([
                FolderKey::group(Group::Favourites),
                FolderKey::group(Group::Failed),
            ]),
            picked: None,
            only: None,
        }
    }
}

/// `c` lowercased, as one character. Search compares one character at a time,
/// and the few characters that lowercase to two compare by their first.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The search as folded characters, or `None` for a blank search.
pub fn needle(query: &str) -> Option<Vec<char>> {
    let query = query.trim();
    (!query.is_empty()).then(|| query.chars().map(fold).collect())
}

/// Whether `hay` holds `needle`, ignoring case, without allocating.
pub(crate) fn contains_folded(hay: &str, needle: &[char]) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.char_indices().any(|(start, _)| {
        let mut rest = hay[start..].chars().map(fold);
        needle.iter().all(|&want| rest.next() == Some(want))
    })
}

/// The Other folder of `group`, which sorts after every named folder.
fn other_folder(group: Group) -> &'static str {
    match group {
        Group::VideoEffects => aria_models::OTHER_VIDEO_CATEGORY,
        Group::AudioEffects => aria_models::OTHER_AUDIO_CATEGORY,
        _ => "",
    }
}

/// Sort `rows` by group, then folder with the Other folder last, then label.
pub fn sort_for_tree(rows: &mut [Entry]) {
    rows.sort_by(|a, b| {
        let (ga, gb) = (a.kind.group(), b.kind.group());
        let other = |row: &Entry, group: Group| row.category.as_str() == other_folder(group);
        (ga, other(a, ga), &a.category, &a.label, &a.id).cmp(&(
            gb,
            other(b, gb),
            &b.category,
            &b.label,
            &b.id,
        ))
    });
}

/// The rows the tree draws.
///
/// `entries` are slices each sorted by [`sort_for_tree`] and holding disjoint
/// groups. `favourites` are saved keys. A blank search lists every group and
/// the contents of each open folder; a search lists only matching entries and
/// failures, with every folder that holds one open. `only` lists one group's
/// entries, under Favourites as well as in the group; failures stay listed.
pub fn tree<'a>(
    entries: &[&'a [Entry]],
    failed: &'a [Failed],
    favourites: &[String],
    query: &str,
    open: &BTreeSet<FolderKey>,
    only: Option<Group>,
) -> Vec<Node<'a>> {
    let needle = needle(query);
    let searching = needle.is_some();
    let shown = |entry: &Entry| {
        only.is_none_or(|group| entry.kind.group() == group)
            && needle.as_deref().is_none_or(|n| entry.matches(n))
    };
    let favourite = |entry: &Entry| favourites.iter().any(|key| entry.is_saved_as(key));
    let is_open = |key: &FolderKey| searching || open.contains(key);
    let mut rows = Vec::new();

    // Favourites, in label order across groups.
    let mut starred: Vec<&'a Entry> = entries
        .iter()
        .flat_map(|slice| slice.iter())
        .filter(|entry| favourite(entry) && shown(entry))
        .collect();
    if !starred.is_empty() {
        starred.sort_by(|a, b| (&a.label, a.kind).cmp(&(&b.label, b.kind)));
        let key = FolderKey::group(Group::Favourites);
        let open = is_open(&key);
        rows.push(Node::Folder {
            label: SharedString::new_static(Group::Favourites.label()),
            depth: 0,
            open,
            count: starred.len(),
            key,
        });
        if open {
            rows.extend(starred.into_iter().map(|entry| Node::Entry {
                entry,
                group: Group::Favourites,
                depth: 1,
                favourite: true,
            }));
        }
    }

    for group in Group::ALL {
        if matches!(group, Group::Favourites | Group::Failed)
            || only.is_some_and(|only| only != group)
        {
            continue;
        }
        let members: Vec<&'a Entry> = entries
            .iter()
            .flat_map(|slice| slice.iter())
            .filter(|entry| entry.kind.group() == group && shown(entry))
            .collect();
        if searching && members.is_empty() {
            continue;
        }
        let key = FolderKey::group(group);
        let open = is_open(&key);
        rows.push(Node::Folder {
            label: SharedString::new_static(group.label()),
            depth: 0,
            open,
            count: members.len(),
            key,
        });
        if !open {
            continue;
        }
        let mut at = 0;
        while at < members.len() {
            let category = &members[at].category;
            let end = at
                + members[at..]
                    .iter()
                    .take_while(|entry| &entry.category == category)
                    .count();
            let depth = if category.is_empty() {
                1
            } else {
                let key = FolderKey {
                    group,
                    category: category.clone(),
                };
                let open = is_open(&key);
                rows.push(Node::Folder {
                    label: category.clone(),
                    depth: 1,
                    open,
                    count: end - at,
                    key,
                });
                if !open {
                    at = end;
                    continue;
                }
                2
            };
            rows.extend(members[at..end].iter().map(|&entry| Node::Entry {
                entry,
                group,
                depth,
                favourite: favourite(entry),
            }));
            at = end;
        }
    }

    let failures: Vec<&'a Failed> = failed
        .iter()
        .filter(|failed| {
            needle.as_deref().is_none_or(|n| {
                contains_folded(&failed.source, n) || contains_folded(&failed.reason, n)
            })
        })
        .collect();
    if !failures.is_empty() {
        let key = FolderKey::group(Group::Failed);
        let open = is_open(&key);
        rows.push(Node::Folder {
            label: SharedString::new_static(Group::Failed.label()),
            depth: 0,
            open,
            count: failures.len(),
            key,
        });
        if open {
            rows.extend(failures.into_iter().map(|failed| Node::Failed { failed }));
        }
    }
    rows
}

impl AppState {
    /// The presets the panel lists: saved presets, then plugin presets whose
    /// name no saved preset already takes, since applying a name picks the
    /// saved one. Sorted for the tree.
    pub fn effects_preset_entries(&self) -> Vec<Entry> {
        let plugin = self.plugin_presets();
        let mut rows: Vec<Entry> = Vec::with_capacity(self.presets.len() + plugin.len());
        rows.extend(
            self.presets
                .iter()
                .map(|preset| Entry::new(EntryKind::Preset, &preset.name, &preset.name, "")),
        );
        for entry in &plugin {
            let name = entry.preset.name.as_str();
            if !self
                .presets
                .iter()
                .any(|saved| saved.name.trim().eq_ignore_ascii_case(name.trim()))
            {
                rows.push(Entry::new(EntryKind::Preset, name, name, ""));
            }
        }
        sort_for_tree(&mut rows);
        rows
    }

    /// Every plugin that did not mount and every preset file that did not
    /// read, with the reason.
    pub fn effects_failures(&self) -> Vec<Failed> {
        let broken = self.broken_presets();
        let failures = self.ctx.failures();
        let mut rows = Vec::with_capacity(failures.len() + broken.len());
        rows.extend(failures.into_iter().map(|failure| Failed {
            source: if failure.name.is_empty() {
                failure.id.into()
            } else {
                failure.name.into()
            },
            reason: failure.reason.into(),
        }));
        rows.extend(broken.iter().map(|preset| {
            Failed {
                source: preset
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| preset.path.display().to_string())
                    .into(),
                reason: preset.reason.clone().into(),
            }
        }));
        rows
    }

    /// Where an entry comes from, for its tooltip: the plugin that registered
    /// it, a saved preset, or the editor itself.
    pub fn effects_entry_origin(&self, kind: EntryKind, id: &str) -> SharedString {
        if kind == EntryKind::Preset {
            if self
                .presets
                .iter()
                .any(|preset| preset.name.trim().eq_ignore_ascii_case(id.trim()))
            {
                return SharedString::new_static("Saved preset");
            }
            if let Some(entry) = self
                .plugin_presets()
                .into_iter()
                .find(|entry| entry.preset.name.trim().eq_ignore_ascii_case(id.trim()))
            {
                return format!("Plugin: {}", entry.plugin).into();
            }
        }
        let Some(contribution) = kind.contribution() else {
            return SharedString::new_static("Built in");
        };
        self.ctx
            .mounted()
            .into_iter()
            .find(|info| {
                info.contributions
                    .iter()
                    .any(|registered| registered.kind == contribution && registered.id == id)
            })
            .map(|info| SharedString::from(format!("Plugin: {}", info.manifest.name)))
            .unwrap_or_else(|| SharedString::new_static("Built in"))
    }

    /// Apply the entry the way its menu row or sidebar tile does: an effect to
    /// the selection, a transition at the selected clip's right cut, a
    /// generator or clip type at the playhead, a preset across the selection.
    pub fn apply_effects_entry(&mut self, kind: EntryKind, id: &str) -> Result<(), EngineError> {
        match kind {
            EntryKind::VideoEffect => self.add_effect_to_selected(id),
            EntryKind::AudioEffect => self.add_audio_effect_to_selection(id),
            EntryKind::Transition | EntryKind::AudioTransition => self.set_selected_transition(id),
            EntryKind::Generator => self.add_effect_generator(id),
            EntryKind::ClipType => self.add_clip_type(id),
            EntryKind::Preset => self.apply_preset(id),
        }
    }

    /// The favourites the panel saved, as keys.
    pub fn effect_favourites(&self) -> &[String] {
        &self.ui_settings.favourite_effects
    }

    /// Star the entry, or unstar it when it is starred. The caller persists
    /// the settings.
    pub fn toggle_effect_favourite(&mut self, kind: EntryKind, id: &str) {
        let key = favourite_key(kind, id);
        let favourites = &mut self.ui_settings.favourite_effects;
        match favourites.iter().position(|saved| *saved == key) {
            Some(at) => {
                favourites.remove(at);
            }
            None => favourites.push(key),
        }
    }

    /// Open a closed folder, or close an open one.
    pub fn toggle_effects_folder(&mut self, key: FolderKey) {
        let open = &mut self.effects_browser.open;
        if !open.remove(&key) {
            open.insert(key);
        }
    }

    /// Highlight one entry.
    pub fn pick_effects_entry(&mut self, kind: EntryKind, id: SharedString) {
        self.effects_browser.picked = Some((kind, id));
    }

    pub fn focus_effects_search(&mut self, focused: bool) {
        self.effects_browser.focused = focused;
        if focused {
            self.media_search_focus = false;
        }
    }

    /// Whether the search box holds the keyboard: focused, and drawn. The box
    /// is drawn by Effects & Presets and by the Media panel's Effects tab, each
    /// while it is the front tab of its frame; a box neither draws takes no
    /// keys.
    pub fn effects_search_focused(&self) -> bool {
        self.effects_browser.focused
            && (self.dock_tree.is_active(crate::dock::PANEL_EFFECTS)
                || (self.sidebar_tab == SidebarTab::Effects
                    && self.dock_tree.is_active(crate::dock::PANEL_PROJECT)))
    }

    /// Shift+5: Effects & Presets forward, opened in its default frame when
    /// closed, with its search box holding the keyboard so typing filters the
    /// tree. Esc returns the keys to the editor.
    pub fn open_effects_panel(&mut self) {
        self.focus_dock_panel(crate::dock::PANEL_EFFECTS);
        self.focus_effects_search(true);
    }

    pub fn effects_query_push(&mut self, ch: char) {
        if self.effects_browser.query.chars().count() < QUERY_MAX_LEN {
            self.effects_browser.query.push(ch);
        }
    }

    pub fn effects_query_pop(&mut self) {
        self.effects_browser.query.pop();
    }

    /// List one group's entries, or every group's with `None`.
    pub fn set_effects_filter(&mut self, only: Option<Group>) {
        self.effects_browser.only = only;
    }

    /// Empty the search, which shows the whole tree again.
    pub fn clear_effects_query(&mut self) {
        self.effects_browser.query.clear();
    }
}

#[cfg(test)]
#[path = "effects_browser_tests.rs"]
mod tests;
