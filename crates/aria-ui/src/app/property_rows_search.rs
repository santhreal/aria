//! The property search in the timeline header.
//!
//! Typing filters the property rows of every clip whose rows are open to the
//! properties whose names hold the text, ignoring case, and opens the groups
//! that contain them. A group with no match is left out. Clearing the search
//! brings back the groups each clip had open: the search filters the outline
//! and does not change it.

use super::{ClipRevealMode, PropertyGroup};
use crate::app::AppState;
use crate::app::effects_browser::{QUERY_MAX_LEN, needle};

impl AppState {
    /// Take or release the keyboard for the property search. Taking it closes
    /// the other boxes that type without a click-away: the playhead fields
    /// and the two sidebar searches.
    pub fn focus_property_search(&mut self, focused: bool) {
        self.prop_rows.search_focused = focused;
        if focused {
            self.focus_clock(false);
            self.focus_effects_search(false);
            self.media_search_focus = false;
        }
    }

    pub fn property_search_focused(&self) -> bool {
        self.prop_rows.search_focused
    }

    /// What the property search holds.
    pub fn property_query(&self) -> &str {
        &self.prop_rows.query
    }

    pub fn property_query_push(&mut self, ch: char) {
        if self.prop_rows.query.chars().count() < QUERY_MAX_LEN {
            self.prop_rows.query.push(ch);
        }
    }

    pub fn property_query_pop(&mut self) {
        self.prop_rows.query.pop();
    }

    /// Empty the search, which shows each clip's own outline again.
    pub fn clear_property_query(&mut self) {
        self.prop_rows.query.clear();
    }

    /// The search as folded characters. Empty for a blank search, which every
    /// name holds.
    pub(super) fn property_needle(&self) -> Vec<char> {
        needle(&self.prop_rows.query).unwrap_or_default()
    }

    /// How the rows of the clip `raw` are revealed: every group open while a
    /// search holds text, the clip's own reveal otherwise.
    pub(super) fn reveal_mode(&self, raw: u64, searching: bool) -> ClipRevealMode {
        if searching {
            return ClipRevealMode::Outline {
                open_groups: PropertyGroup::ALL.into_iter().collect(),
            };
        }
        self.with_prop_rows(|s| s.clip_modes.get(&raw).cloned())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "property_rows_search_tests.rs"]
mod tests;
