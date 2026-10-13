//! What the command palette offers, and which row is highlighted.
//!
//! One owner, because the palette used to hold its own short table while the app
//! bar held a longer one: a row added to a menu was reachable with the mouse and
//! unreachable by name. The offer is derived here instead, from the registries
//! that already exist:
//!
//! 1. every command in [`CommandHost`](crate::commands::host::CommandHost),
//! 2. every item the [`MenuHost`](crate::chrome::menu_host::MenuHost) would draw
//!    in any menu, whose label the commands did not already offer,
//! 3. every effect in the model's catalog, which is data read at run time and so
//!    cannot be a compiled row at all. Without it the whole effect catalog was
//!    pointer-only: the rack's Add gesture and the sidebar catalog are clicks.
//!
//! Nothing is compiled into a fourth list, so a new menu row is offered by name
//! without a second edit, a plugin's menu item is offered too, and an effect
//! added to the data file is offered the moment the catalog loads it.
//!
//! A row that needs the entity context is handed back to the caller rather than
//! run here: a menu item that starts an async task, and a command that starts a
//! job (detect, transcribe, duck, measure). The shell has the context,
//! `AppState` does not, and keeping it that way is what lets a headless state
//! still run every other row.

use gpui::SharedString;

use super::AppState;
use crate::commands::host::{CommandBody, CommandRun, WindowedRun, host_of};
use crate::types::AppMenu;

/// How an offered row is invoked.
#[derive(Clone)]
pub enum Offered {
    /// A registered command. Runs against the state alone.
    Command(CommandRun),
    /// A registered command that starts work. Needs the entity context, so the
    /// caller runs it.
    Windowed(WindowedRun),
    /// A menu row, named by the menu it lives in and its id. Needs the entity
    /// context, so the caller runs it.
    Menu(AppMenu, &'static str),
    /// An effect in the catalog, by its id: the row adds it to the selection.
    /// Its own kind rather than a `Command`, because the row carries the id and
    /// a `fn` pointer cannot.
    Effect(SharedString),
    /// A generator in the effect catalog, by its id: the row places it as a
    /// clip at the playhead.
    Generator(SharedString),
}

/// What the caller must still run, because it needs the entity context.
#[derive(Clone, Copy, Debug)]
pub enum Pending {
    Menu(AppMenu, &'static str),
    Windowed(WindowedRun),
}

/// Two pendings are the same row, which is what a caller asserting the palette
/// handed back the row it picked needs.
///
/// Written out rather than derived: the derive compares the `Windowed` function
/// pointers with `==`, and two `fn` pointers to the same function are not
/// guaranteed to compare equal — the addresses depend on codegen. `fn_addr_eq`
/// is the comparison that says what is meant here, and it says it once.
impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Pending::Menu(menu, id), Pending::Menu(other_menu, other_id)) => {
                menu == other_menu && id == other_id
            }
            (Pending::Windowed(run), Pending::Windowed(other_run)) => {
                std::ptr::fn_addr_eq(*run, *other_run)
            }
            _ => false,
        }
    }
}

/// One row the palette can offer.
#[derive(Clone)]
pub struct PaletteOffer {
    /// Element id, and the identity the offer de-duplicates on.
    pub id: SharedString,
    /// What the palette shows and filters on.
    pub label: SharedString,
    /// Where the row came from: `None` for a command, the menu for a menu row.
    pub source: Option<AppMenu>,
    pub action: Offered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaletteKey {
    project_report: bool,
    custom_workspace: bool,
    command_revision: Option<u64>,
    menu_revision: Option<u64>,
    /// The effect catalog's revision: a plugin can add or remove an effect.
    effects_revision: u64,
}

#[derive(Default)]
pub(crate) struct PaletteCache {
    key: Option<PaletteKey>,
    catalog: std::sync::Arc<[PaletteOffer]>,
    query: Option<String>,
    hits: std::sync::Arc<[PaletteOffer]>,
}

impl AppState {
    /// Every row on offer, before filtering: commands in registration order, then
    /// the menu rows the commands did not already name.
    ///
    /// The registries are state-dependent, so this is what the palette could
    /// offer *now*. A menu that hides an item because no project is open does not
    /// offer it by name either.
    pub fn palette_catalog(&self) -> std::sync::Arc<[PaletteOffer]> {
        let effects = aria_models::effect_catalog();
        let commands = host_of(&self.ctx);
        let menus = crate::chrome::menu_host::host_of(&self.ctx);
        // The shipped contributor's two state dependencies are represented in
        // `PaletteKey`. A plugin contributor may inspect any `AppState` field and
        // has no declared revision for those reads, so keep that path uncached.
        let cacheable = menus.as_ref().is_none_or(|host| host.len() <= 1);
        let key = PaletteKey {
            project_report: self.project_report_available(),
            custom_workspace: self.ui_settings.workspace.is_some(),
            command_revision: commands.as_ref().map(|host| host.revision()),
            menu_revision: menus.as_ref().map(|host| host.revision()),
            effects_revision: effects.revision(),
        };
        {
            let cache = self.palette_cache.borrow();
            if cacheable && cache.key == Some(key) {
                return std::sync::Arc::clone(&cache.catalog);
            }
        }

        let mut offers: Vec<PaletteOffer> = Vec::new();
        if let Some(host) = commands {
            for entry in host.all() {
                offers.push(PaletteOffer {
                    id: SharedString::new_static(entry.id),
                    label: SharedString::new_static(entry.label),
                    source: None,
                    action: match entry.body {
                        CommandBody::Plain(run) => Offered::Command(run),
                        CommandBody::Windowed(run) => Offered::Windowed(run),
                    },
                });
            }
        }
        if let Some(menus) = menus {
            for menu in AppMenu::ALL {
                for item in menus.items(menu, self).iter().copied() {
                    let known = offers
                        .iter()
                        .any(|offer| offer.label == item.label || offer.id == item.id);
                    if known {
                        continue;
                    }
                    offers.push(PaletteOffer {
                        id: SharedString::new_static(item.id),
                        label: SharedString::new_static(item.label),
                        source: Some(menu),
                        action: Offered::Menu(menu, item.id),
                    });
                }
            }
        }
        for spec in &effects {
            let (id, label) = (spec.id.as_str(), spec.label.as_str());
            if offers
                .iter()
                .any(|offer| offer.label == label || offer.id == id)
            {
                continue;
            }
            let id = SharedString::from(id);
            offers.push(PaletteOffer {
                id: id.clone(),
                label: SharedString::from(label),
                source: None,
                action: if spec.generator {
                    Offered::Generator(id)
                } else {
                    Offered::Effect(id)
                },
            });
        }

        let catalog = std::sync::Arc::from(offers);
        let mut cache = self.palette_cache.borrow_mut();
        cache.key = cacheable.then_some(key);
        cache.catalog = std::sync::Arc::clone(&catalog);
        cache.query = None;
        cache.hits = std::sync::Arc::from([]);
        catalog
    }

    /// The rows the current query matches, best first.
    pub fn palette_hits(&self) -> std::sync::Arc<[PaletteOffer]> {
        let catalog = self.palette_catalog();
        {
            let cache = self.palette_cache.borrow();
            if cache.query.as_deref() == Some(self.palette_query.as_str()) {
                return std::sync::Arc::clone(&cache.hits);
            }
        }
        let labels: Vec<&str> = catalog.iter().map(|offer| offer.label.as_str()).collect();
        let hits: std::sync::Arc<[PaletteOffer]> =
            crate::popover::filter_indices(&self.palette_query, &labels)
                .into_iter()
                .filter_map(|ix| catalog.get(ix).cloned())
                .collect();
        let mut cache = self.palette_cache.borrow_mut();
        cache.query = Some(self.palette_query.clone());
        cache.hits = std::sync::Arc::clone(&hits);
        hits
    }

    /// Which hit is highlighted, clamped to what the query actually matches.
    ///
    /// Clamped on read rather than on every edit, because the hit count changes
    /// with the query and with the state behind it: a stored index that is only
    /// corrected when the query changes goes stale when a project opens.
    pub fn palette_highlight(&self) -> Option<usize> {
        let count = self.palette_hits().len();
        if count == 0 {
            return None;
        }
        Some(self.palette_index.min(count - 1))
    }

    /// Move the highlight. Wraps at both ends, so Down on the last row returns to
    /// the first.
    pub fn palette_move(&mut self, delta: isize) {
        let count = self.palette_hits().len();
        let index = crate::popover::menu_step(self.palette_highlight(), count, delta).unwrap_or(0);
        self.palette_index = index;
        // Scrolls the least distance that draws the row whole, on the next
        // frame, when the row's bounds are known.
        self.palette_scroll.scroll_to_item(index);
    }

    /// Type into the query. The highlight returns to the first hit, because the
    /// row under it is not the row it was.
    pub fn palette_push(&mut self, ch: char) {
        self.palette_query.push(ch);
        self.palette_restart();
    }

    pub fn palette_pop(&mut self) {
        self.palette_query.pop();
        self.palette_restart();
    }

    /// Highlights the first hit and returns the list to its top.
    ///
    /// The offset is set outright rather than scrolled to, since scrolling to
    /// the first row stops with the row at the region's edge and the padding
    /// above it out of view. The scroll to the first row replaces a scroll
    /// requested before this frame, which would otherwise land on whatever
    /// row now has that index.
    pub(crate) fn palette_restart(&mut self) {
        self.palette_index = 0;
        self.palette_scroll.set_offset(gpui::Point::default());
        self.palette_scroll.scroll_to_item(0);
    }

    /// Run the row at `index`.
    ///
    /// Returns the work the caller must run with the entity context, or `None`
    /// when the row ran here or there was no such row.
    pub fn run_palette_offer(&mut self, index: usize) -> Option<Pending> {
        let offer = self.palette_hits().get(index).cloned()?;
        match offer.action {
            Offered::Command(run) => {
                run(self);
                None
            }
            Offered::Windowed(run) => Some(Pending::Windowed(run)),
            Offered::Menu(menu, id) => Some(Pending::Menu(menu, id)),
            // The refusal lives in the command, not here: a row that finds no
            // selection says so through the same reader every other command
            // reads the selection with.
            Offered::Effect(id) => {
                if let Err(err) = self.add_effect_to_selected(id.as_str()) {
                    self.report_error(&err);
                }
                None
            }
            Offered::Generator(id) => {
                if let Err(err) = self.add_effect_generator(id.as_str()) {
                    self.report_error(&err);
                }
                None
            }
        }
    }

    /// Run the highlighted row, or the best hit when nothing is highlighted.
    pub fn run_palette_highlight(&mut self) -> Option<Pending> {
        let index = self.palette_highlight()?;
        self.run_palette_offer(index)
    }
}

#[cfg(test)]
#[path = "palette_effects_tests.rs"]
mod effect_row_tests;
