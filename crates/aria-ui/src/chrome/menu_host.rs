//! Where a menu item comes from.
//!
//! The app bar used to call the shipped menu tables directly, so a plugin could
//! mount a whole capability and still have no way to offer it from a menu. The
//! bar reads this registry instead: the shipped tables are one contributor, and a
//! plugin adds another.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use aria_plugin::{PluginContext, Service};
use parking_lot::RwLock;

use super::menu_items::{MenuItem, MenuItems};
use crate::app::AppState;
use crate::types::AppMenu;

/// What a contributor offers for one menu, given the state the bar is drawing.
///
/// State-dependent by design: an entry that cannot run is drawn disabled, and
/// only the state knows which those are.
pub type MenuContribute = fn(AppMenu, &AppState) -> MenuItems;

/// Every source of menu items, in registration order.
#[derive(Default)]
pub struct MenuHost {
    contributors: RwLock<Vec<MenuContribute>>,
    revision: AtomicU64,
}

impl Service for MenuHost {}

impl MenuHost {
    /// Add a contributor. Its items are appended after those already registered,
    /// so the shipped entries stay at the top of each menu.
    pub fn add(&self, contribute: MenuContribute) {
        self.contributors.write().push(contribute);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    /// Every item one menu offers, in contributor order. A duplicate id from a
    /// later contributor is dropped: the bar hands ids to GPUI as element ids, and
    /// a plugin must not be able to shadow a shipped entry by reusing its id.
    pub fn items(&self, menu: AppMenu, state: &AppState) -> MenuItems {
        let contributors = self.contributors.read();
        let Some((first, rest)) = contributors.split_first() else {
            return Cow::Borrowed(&[]);
        };
        let first = first(menu, state);
        if rest.is_empty() {
            return first;
        }
        let mut items = first.into_owned();
        for contribute in rest {
            for item in contribute(menu, state).iter().copied() {
                if items.iter().any(|held| held.id == item.id) {
                    continue;
                }
                items.push(item);
            }
        }
        Cow::Owned(items)
    }

    /// How many contributors are registered. For the tests.
    pub fn len(&self) -> usize {
        self.contributors.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Monotonic registration revision for caches derived from this registry.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
}

/// The registry the bar reads, or nothing when no host mounted it.
pub fn host_of(ctx: &PluginContext) -> Option<std::sync::Arc<MenuHost>> {
    ctx.try_get::<MenuHost>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AppMenu;
    use aria_plugin::PluginContext;

    fn state() -> AppState {
        let ctx = PluginContext::new();
        let dir = crate::scratch_pool::scratch("menu-host");
        let _ = crate::plugins::mount_all(&ctx, &dir);
        AppState::new(ctx, dir).expect("state")
    }

    fn contributed(_menu: AppMenu, _state: &AppState) -> MenuItems {
        Cow::Owned(vec![MenuItem {
            id: "example-menu-note",
            label: "Note",
            run: |_, _, _| {},
        }])
    }

    fn shadowing(_menu: AppMenu, _state: &AppState) -> MenuItems {
        Cow::Owned(vec![MenuItem {
            id: "file-new",
            label: "Stolen",
            run: |_, _, _| {},
        }])
    }

    #[test]
    fn the_shipped_contributor_returns_its_static_table() {
        let state = state();
        let host = MenuHost::default();
        host.add(super::super::menu_items::items_for);

        assert!(
            matches!(host.items(AppMenu::File, &state), Cow::Borrowed(_)),
            "the built-in menu was rebuilt into an owned vector"
        );
    }

    /// The point of the registry: an item nobody edited the bar for still shows.
    #[test]
    fn a_contributed_item_is_offered_after_the_shipped_ones() {
        let state = state();
        let host = MenuHost::default();
        host.add(super::super::menu_items::items_for);
        host.add(contributed);
        let items = host.items(AppMenu::File, &state);
        assert!(items.len() > 1);
        assert_eq!(
            items.last().map(|item| item.id),
            Some("example-menu-note"),
            "a contribution must land after the shipped entries"
        );
    }

    /// Ids are GPUI element ids, so a reused one would draw twice and click the
    /// wrong entry. The first registration keeps the id.
    #[test]
    fn a_contributor_cannot_shadow_a_shipped_id() {
        let state = state();
        let host = MenuHost::default();
        host.add(super::super::menu_items::items_for);
        host.add(shadowing);
        let items = host.items(AppMenu::File, &state);
        let shipped = super::super::menu_items::items_for(AppMenu::File, &state);
        assert_eq!(items.len(), shipped.len());
        assert!(
            items.iter().all(|item| item.label != "Stolen"),
            "a duplicate id was allowed to replace a shipped entry"
        );
    }
}
