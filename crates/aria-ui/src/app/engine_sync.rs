//! `AppState`'s poll of the engine, and the projection memo that decides
//! whether a poll rebuilds the store.
//!
//! Inherent methods keep their existing paths. [`ProjectionKey`] is defined
//! here because `sync_from_engine` is.

use super::*;

impl AppState {
    /// The projection, for a write.
    ///
    /// The panels share the store for the length of a frame, which is what makes
    /// reading it free; taking it back for a write copies it only while one of
    /// those frames is still holding the old one. Every writer goes through here,
    /// so there is no path that mutates a projection a panel is drawing.
    pub(crate) fn store_mut(&mut self) -> &mut UiEditorStore {
        std::sync::Arc::make_mut(&mut self.store)
    }

    /// Apply a selection-only change to the existing projection.
    ///
    /// Selection changes no model data. Re-walking every track and clip would
    /// reproduce the same store except for its selected ids and flags.
    pub(crate) fn refresh_projected_selection(&mut self) {
        let held = self.projected.take();
        let (selected, key) = {
            let mut engine = self.engine.lock();
            let rate = engine.project().timeline().frame_rate;
            let at = RationalTime::new(self.playhead.value, rate);
            let selected = selected_detail::SelectedDetail::of(&mut engine, &self.selection, at);
            let mut key = ProjectionKey::of_with_basis(
                &engine,
                self.playhead,
                &self.selection,
                held.as_ref().and_then(|held| held.basis.as_ref()),
            );
            // The store keeps the names it was built with, so the key keeps
            // the catalog revisions they were read at.
            if let Some(held) = &held {
                key.catalogs = held.catalogs;
            }
            (selected, key)
        };
        let mut basis = held.and_then(|held| held.basis);
        let store = std::sync::Arc::make_mut(&mut self.store);
        store.selection.clear();
        store
            .selection
            .extend(self.selection.iter().map(|id| id.raw()));
        crate::projection::reselect(store, &self.selection, basis.as_mut());
        self.selected = selected;
        self.projected = Some(ProjectionKey { basis, ..key });
    }

    /// Take a fresh projection as the one the panels draw.
    ///
    /// The two things the engine knows nothing about — the zoom level and the
    /// scroll offset — are written on the way in, so no reader ever sees a
    /// projection carrying a stale view, and the reconciliation that follows a
    /// new projection cannot be forgotten at one of the call sites.
    ///
    /// The selected clip's detail arrives in the same snapshot and is adopted
    /// here, so a caller cannot take one and forget the other: an inspector
    /// value that disagreed with the store beside it would be worse than the
    /// lock it replaced.
    fn adopt(&mut self, snapshot: Snapshot) {
        // Counted, because the whole point of the memo and of the playhead-only
        // path is that this is rare: a test can hold the number flat across a
        // scrub instead of trusting a comment.
        self.projections += 1;
        let mut store = snapshot.store;
        store.zoom = self.zoom;
        store.scroll_x = self.scroll_x;
        self.store = std::sync::Arc::new(store);
        if let Some(key) = &mut self.projected {
            key.basis = Some(snapshot.basis);
        }
        self.refresh_clock_text();
        self.selected = snapshot.selected;
        self.facts = snapshot.facts;
        // The open property rows draw their clips' models, which this
        // projection may have moved.
        if !self.expanded_clips.is_empty() || !self.expanded_models.is_empty() {
            self.refresh_expanded_models();
        }
        self.reconcile_bins();
        self.announce_offline_media();
        self.reconcile_source_view();
        // Reset the lazy magnet index so the next drag frame builds it once
        // from the newly adopted sequence.
        self.snap_index = std::sync::OnceLock::new();
        // A report on screen follows the edit that just landed. Closed, it
        // costs nothing: the counts are walks of the whole project and nobody
        // is reading them.
        if self.dialog == crate::app::DialogKind::ProjectReport {
            self.measure_project_report();
        }
    }

    /// Poll the engine and re-project only when something the projection reads
    /// has moved.
    ///
    /// Every gesture frame calls this — a trim drag calls it twice per frame —
    /// and a projection walks every track, clip and keyframe. So the inputs are
    /// read under the lock first and compared against the key the current store
    /// was built from; an unchanged poll returns before allocating anything.
    ///
    /// A poll where only the playhead moved — every frame of playback and every
    /// frame of a scrub — re-samples what is read at the playhead and keeps the
    /// store it already has.
    ///
    /// A package waiting for an effect, transition or generator the project
    /// uses loads first, with the engine lock released, and the store is then
    /// built at the same revision so it reads what the package registered.
    pub(crate) fn sync_from_engine(&mut self) {
        self.sync(true);
    }

    /// Poll as [`Self::sync_from_engine`] does, unless another thread holds
    /// the engine. The preview worker holds it for the length of each frame it
    /// renders, so a playback vsync that waited for it would wait out a
    /// render.
    pub(crate) fn sync_from_engine_if_free(&mut self) {
        self.sync(false);
    }

    /// The poll of [`Self::sync_from_engine`]; `wait` waits for an engine
    /// another thread holds, and without it a held engine is left unpolled.
    fn sync(&mut self, wait: bool) {
        let mut poll = self.poll_engine(true, wait);
        if let Some(Poll::Load(wanted)) = &poll {
            for event in wanted {
                self.load_plugins_for(event);
            }
            // A package that did not load is asked for again by the next
            // poll, not by this one.
            self.projected = None;
            poll = self.poll_engine(false, wait);
        }
        match poll {
            None | Some(Poll::Held | Poll::Load(_)) => {}
            Some(Poll::Moved(at, selected)) => {
                self.playhead = at;
                self.store_mut().playhead_ticks = at.value;
                self.refresh_clock_text();
                self.selected = selected;
                if let Some(key) = &mut self.projected {
                    key.playhead = at;
                }
            }
            Some(Poll::Rebuilt(snapshot)) => {
                // `apply_command` advances this by span before projection, and
                // `roll_back` reverts it by span. Any other engine mutation
                // reaches this fallback and drops every cell, so no direct
                // engine path can retain stale pixels.
                self.filmstrips.sync_revision(snapshot.revision);
                let edited = snapshot.revision != self.revision;
                self.revision = snapshot.revision;
                self.playhead = snapshot.playhead;
                self.adopt(snapshot);
                if edited {
                    self.picture_changed();
                }
            }
        }
    }

    /// What [`Self::sync_from_engine`] has to do, decided under the engine
    /// lock. `None` when the store already covers the engine. With `load`,
    /// the events of waiting packages the project uses come first. Without
    /// `wait`, an engine another thread holds is [`Poll::Held`].
    fn poll_engine(&mut self, load: bool, wait: bool) -> Option<Poll> {
        let mut engine = if wait {
            self.engine.lock()
        } else {
            match self.engine.try_lock() {
                Some(engine) => engine,
                None => return Some(Poll::Held),
            }
        };
        if load {
            let wanted = self
                .ctx
                .project_activations(engine.project(), engine.revision());
            if !wanted.is_empty() {
                return Some(Poll::Load(wanted));
            }
        }
        // Decided as plain answers first: the arms below write the key back,
        // which they could not do while the match still held it.
        let (covered, playhead_only) = match self.projected.as_ref() {
            Some(key) if key.covers(&engine, self.playhead, &self.selection) => (true, false),
            Some(key) => (false, key.covers_except_playhead(&engine, &self.selection)),
            None => (false, false),
        };
        if covered {
            return None;
        }
        Some(if playhead_only {
            // The projection reads the playhead as one field (pinned by
            // `a_projection_reads_the_playhead_only_as_the_playhead`), so the
            // store is already right and the walk would produce the store it
            // already has. What is genuinely sampled at the playhead is the
            // selected clip's detail, so that is all this re-takes: a scrub
            // frame costs one resolve instead of a walk of every track, clip
            // and keyframe plus a magnet index rebuild.
            let rate = engine.project().timeline().frame_rate;
            let at = RationalTime::new(self.playhead.value, rate);
            let selected = match &self.selected {
                // Nothing but the playhead moved, so the clip, its peers and
                // their property rows are the values already held: only what
                // is sampled at this instant is taken again.
                Some(detail) if !self.selection.is_empty() => {
                    Some(detail.resampled(&mut engine, at))
                }
                _ => selected_detail::SelectedDetail::of(&mut engine, &self.selection, at),
            };
            Poll::Moved(at, selected)
        } else {
            let previous = self.projected.take().and_then(|key| key.basis);
            let snapshot = Snapshot::from_engine(
                &mut engine,
                self.playhead,
                &self.selection,
                previous.as_ref().map(|basis| crate::projection::Previous {
                    store: &self.store,
                    basis,
                }),
            );
            self.projected = Some(ProjectionKey::of_with_basis(
                &engine,
                self.playhead,
                &self.selection,
                Some(&snapshot.basis),
            ));
            Poll::Rebuilt(snapshot)
        })
    }

    /// Re-project unconditionally.
    ///
    /// The caller changed something the projection reads and knows it, so there
    /// is nothing to compare; the key is refreshed on the way through so the
    /// next poll can still short-circuit.
    pub(crate) fn rebuild_store(&mut self) {
        let snapshot = {
            let mut engine = self.engine.lock();
            let previous = self.projected.take().and_then(|key| key.basis);
            let snapshot = Snapshot::from_engine(
                &mut engine,
                self.playhead,
                &self.selection,
                previous.as_ref().map(|basis| crate::projection::Previous {
                    store: &self.store,
                    basis,
                }),
            );
            self.projected = Some(ProjectionKey::of_with_basis(
                &engine,
                self.playhead,
                &self.selection,
                Some(&snapshot.basis),
            ));
            snapshot
        };
        self.adopt(snapshot);
    }
}

/// Everything a projection is built from.
///
/// `project_to_ui_with` is a pure function of the project plus the playhead,
/// the selection, the dirty flag, the history list, and the effect, transition
/// and clip type catalogs it reads names and parameter layouts from, and the
/// proxy pass
/// that follows it reads only the proxy table. So two equal keys describe the same store,
/// byte for byte, and the second projection is work with no output.
///
/// The key is built beside the projection, from the same values in the same
/// order, because a projection input missing from this list would make the memo
/// lie: the store would be stale and no reader could tell.
pub(crate) struct ProjectionKey {
    revision: u64,
    proxy_revision: u64,
    playhead: RationalTime,
    selection: Vec<ClipId>,
    dirty: bool,
    /// The history list's generation: a committed group or a merged pair
    /// changes the list without moving `revision`.
    history: u64,
    catalogs: CatalogRevisions,
    /// The lanes the store was built from, for the next projection to take
    /// what did not move. `None` until the store it describes is adopted.
    basis: Option<crate::projection::LaneBasis>,
}

impl ProjectionKey {
    fn of_with_basis(
        engine: &Engine,
        playhead: RationalTime,
        selection: &[ClipId],
        basis: Option<&crate::projection::LaneBasis>,
    ) -> Self {
        Self {
            revision: engine.revision(),
            proxy_revision: engine.proxy_revision(),
            playhead,
            selection: selection.to_vec(),
            dirty: engine.is_dirty(),
            history: engine.history_generation(),
            catalogs: CatalogRevisions::of_with_basis(engine.project(), basis),
            basis: None,
        }
    }

    /// Whether a projection built from these inputs would equal the one this
    /// key describes. Compares in place: the unchanged path allocates nothing.
    fn covers(&self, engine: &Engine, playhead: RationalTime, selection: &[ClipId]) -> bool {
        self.playhead == playhead && self.covers_except_playhead(engine, selection)
    }

    /// Whether the playhead is the only projection input that moved.
    ///
    /// Separate from [`Self::covers`] because a moved playhead is the common
    /// case — every frame of playback and every frame of a scrub — and it does
    /// not invalidate the store: the projection carries the tick and nothing
    /// derived from it.
    fn covers_except_playhead(&self, engine: &Engine, selection: &[ClipId]) -> bool {
        self.revision == engine.revision()
            && self.proxy_revision == engine.proxy_revision()
            && self.selection == selection
            && self.dirty == engine.is_dirty()
            && self.history == engine.history_generation()
            && self.catalogs.current()
    }
}

/// The effect, transition and clip type catalog revisions a projection read,
/// each `None` when the project holds nothing from that catalog.
///
/// A plugin that registers or drops an entry changes a clip's names, values
/// and clip type without moving the project, so a catalog the project uses is
/// a projection input. One it does not use cannot stale the store, and leaving
/// it out keeps a registration elsewhere from rebuilding a projection that
/// reads nothing of it. The walk runs once per projection, not per poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CatalogRevisions {
    effects: Option<u64>,
    transitions: Option<u64>,
    clip_types: Option<u64>,
}

impl CatalogRevisions {
    fn of_with_basis(
        project: &aria_models::Project,
        basis: Option<&crate::projection::LaneBasis>,
    ) -> Self {
        let (mut effects, transitions, clip_types) = match basis {
            Some(basis) => basis.catalog_usage(),
            None => {
                let (mut effects, mut transitions, mut clip_types) = (false, false, false);
                for track in project.timeline().tracks_ordered() {
                    transitions |= !track.transitions().is_empty();
                    for clip in track.clips() {
                        effects |= !clip.effects.is_empty();
                        clip_types |= clip.clip_type.is_some();
                    }
                }
                (effects, transitions, clip_types)
            }
        };
        // A clip type is drawn from its generator's effect row.
        effects |= clip_types;
        Self {
            effects: effects.then(|| aria_models::effect_catalog().revision()),
            transitions: transitions.then(|| aria_models::transition_catalog().revision()),
            clip_types: clip_types.then(|| aria_models::clip_type_catalog().revision()),
        }
    }

    /// Whether every catalog these revisions hold is still at that revision.
    fn current(&self) -> bool {
        let at = |held: Option<u64>, now: fn() -> u64| held.is_none_or(|held| held == now());
        at(self.effects, || aria_models::effect_catalog().revision())
            && at(self.transitions, || {
                aria_models::transition_catalog().revision()
            })
            && at(self.clip_types, || {
                aria_models::clip_type_catalog().revision()
            })
    }
}

/// What a poll found, decided under the lock and applied after it is dropped.
enum Poll {
    /// Another thread held the engine, and nothing was polled.
    Held,
    /// Waiting packages the project uses, to load before the store is built.
    Load(Vec<aria_plugin::Activation>),
    /// Only the playhead moved: the tick, and the detail sampled at it.
    Moved(RationalTime, Option<selected_detail::SelectedDetail>),
    /// Something the projection reads changed, so it was rebuilt.
    Rebuilt(Snapshot),
}

#[cfg(test)]
#[path = "projection_memo_tests.rs"]
mod projection_memo_tests;
