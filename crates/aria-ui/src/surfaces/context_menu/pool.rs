//! The rows a pool row's menu draws.
//!
//! Split out of the parent by size.

use aria_models::MediaId;

use crate::app::AppState;

use super::{MenuItem, MenuRow};

/// The pool-row facts a menu row reads.
#[derive(Clone, Copy)]
pub struct PoolMenu {
    pub media: u64,
    /// A clip still references the item, so the model would refuse to drop it.
    pub in_use: bool,
    /// The item decodes through a proxy, so there is something to return.
    pub has_proxy: bool,
}

impl PoolMenu {
    /// Read the pool facts a menu would gather when it opens over `media`.
    pub fn of(state: &AppState, media: u64) -> Self {
        Self {
            media,
            in_use: state.media_is_in_use(MediaId::from_raw(media)),
            has_proxy: state.media_has_proxy(MediaId::from_raw(media)),
        }
    }
}

/// A media-pool menu, in the order it draws.
pub const POOL_ITEMS: &[MenuItem<PoolMenu>] = &[
    // The removal is refused while a clip holds the item, so the row states that
    // and acts on it: it selects the clips holding the item rather than leaving
    // them to be hunted for.
    MenuItem::Row(MenuRow {
        id: "ctx-pool-in-use",
        label: |_| "Used by the timeline: select those clips",
        when: |pool| pool.in_use,
        run: |state, pool| {
            state.select_media_clips(MediaId::from_raw(pool.media));
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-pool-remove",
        label: |_| "Remove from project",
        when: |pool| !pool.in_use,
        run: |state, pool| state.remove_media(MediaId::from_raw(pool.media)),
    }),
    // Which way the substitution can move, named on the row: the substitution is
    // invisible on the pool item itself, so a row that read the same in both
    // states would say nothing about which is true.
    MenuItem::Row(MenuRow {
        id: "ctx-pool-proxy-clear",
        label: |_| "Decode from original file",
        when: |pool| pool.has_proxy,
        run: |state, pool| {
            state.clear_media_proxy(MediaId::from_raw(pool.media));
            Ok(())
        },
    }),
    // One row for both halves of the same request: a proxy already on disk is
    // registered here and now, and one that is not is rendered as a job. Which of
    // the two it is, is not a display question.
    MenuItem::Row(MenuRow {
        id: "ctx-pool-proxy-use",
        label: |_| "Decode through a proxy",
        when: |pool| !pool.has_proxy,
        run: |state, pool| {
            if let Err(err) = state.use_media_proxy(MediaId::from_raw(pool.media)) {
                state.status_message = Some(err);
            }
            Ok(())
        },
    }),
    MenuItem::Labels,
];
