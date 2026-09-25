//! The pool as the sidebar draws it: which items are in view, in what order,
//! and how many the filter left out.
//!
//! # Why
//!
//! The list used to be a `Vec<UiMediaItem>` of full clones — name, path, both
//! codec strings, per item — and the count under the box called the same
//! function again, so drawing the sidebar cost two pool-sized clone passes per
//! frame and a click asked for a third. The clones were not laziness: the panel
//! reads the projection through `state.read(cx)`, and it needs `cx` mutably
//! again to build its listeners, so nothing borrowed out of that read may
//! outlive the line it was read on.
//!
//! The projection is already behind an `Arc`, so a view can hold the store it
//! describes and index into it. One refcount bump and a `Vec<usize>` replace the
//! clones, the borrow problem goes away because the view owns its store, and the
//! count comes off the same walk that produced the order rather than a second
//! filter beside it.

use std::sync::Arc;

use crate::types::{MediaSort, SidebarTab, UiClipKind, UiEditorStore, UiMediaItem};

use super::AppState;

/// The pool rows one tab draws, and the size of the set they were filtered out
/// of.
pub struct PoolRows {
    /// The projection these rows index into. Held so the rows outlive the read
    /// that produced them.
    store: Arc<UiEditorStore>,
    /// Indices into `store.media`, in draw order.
    order: Vec<usize>,
    /// How many items are in scope before the query narrows them.
    total: usize,
}

impl PoolRows {
    /// The rows, in the order the list draws them.
    pub fn iter(&self) -> impl Iterator<Item = &UiMediaItem> {
        self.order.iter().map(|index| &self.store.media[*index])
    }

    /// The item drawn at `index`, or `None` past the end.
    pub fn get(&self, index: usize) -> Option<&UiMediaItem> {
        self.order.get(index).map(|at| &self.store.media[*at])
    }

    /// The ids, in draw order. A span of a selection runs along this.
    pub fn ids(&self) -> Vec<u64> {
        self.iter().map(|item| item.id).collect()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// How many items the tab holds before the query narrows them: the `40` of
    /// "3 of 40".
    pub fn total(&self) -> usize {
        self.total
    }
}

impl AppState {
    /// The pool rows `tab` draws, and what they were filtered out of.
    ///
    /// One walk answers both: an item in scope counts toward the total, and one
    /// that also matches the query and the open folder joins the order. The two
    /// scopes differ — a query searches the open folder and everything under it,
    /// while an unfiltered list shows only what is directly in it — so the walk
    /// tests both predicates rather than reusing one answer for the other.
    pub fn pool_rows(&self, tab: SidebarTab) -> PoolRows {
        let audio_only = tab == SidebarTab::Audio;
        let query = self.media_query.trim().to_lowercase();
        let deep = !query.is_empty();
        let mut order = Vec::new();
        let mut total = 0usize;
        for (index, item) in self.store.media.iter().enumerate() {
            if audio_only && item.kind != UiClipKind::Audio {
                continue;
            }
            if self.in_open_bin(item.bin, true) {
                total += 1;
            }
            if !self.in_open_bin(item.bin, deep) {
                continue;
            }
            if !query.is_empty() && !super::media::media_matches(item, &query) {
                continue;
            }
            order.push(index);
        }
        let media = &self.store.media;
        match self.media_sort {
            MediaSort::Name => order.sort_by(|a, b| {
                media[*a]
                    .name
                    .to_lowercase()
                    .cmp(&media[*b].name.to_lowercase())
            }),
            MediaSort::Duration => {
                order.sort_by(|a, b| media[*b].duration_ticks.cmp(&media[*a].duration_ticks))
            }
            MediaSort::Resolution => order.sort_by(|a, b| {
                (u64::from(media[*b].width) * u64::from(media[*b].height))
                    .cmp(&(u64::from(media[*a].width) * u64::from(media[*a].height)))
            }),
            // Newest first, and an undated entry last: a project saved before
            // import times existed has nothing to place it by, and guessing
            // would put it at one end as if it were a fact.
            MediaSort::DateAdded => {
                order.sort_by(
                    |a, b| match (media[*a].imported_at, media[*b].imported_at) {
                        (Some(a), Some(b)) => b.cmp(&a),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    },
                )
            }
        }
        PoolRows {
            store: Arc::clone(&self.store),
            order,
            total,
        }
    }
}
