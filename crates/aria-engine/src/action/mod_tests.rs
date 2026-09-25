//! Tests for this module.
//!
//! The cases live in sibling files, one per area, declared at the bottom. This
//! file holds only the shared fixtures; a child sees them through `use super::*`.
//! Declared from `mod.rs` with `#[path]`, so `use super::*` still means that
//! module.
//!
//! A child module is named `*_cases` rather than after the action it exercises:
//! the action's own module is in scope here, and a test module sharing its name
//! would shadow it for every child that reaches through `use super::*`.

use super::*;
use crate::action::edit::{
    add_clip, add_generated, add_track, link_clips, move_clip, remove_media, ripple_delete,
    ripple_insert, set_project_name, set_track_flags, shift_clips, split_clip, trim_clip,
};
use aria_models::{
    Clip, Generator, LinkId, MediaSource, ModelError, Rational, RationalTime, TimeRange, TrackKind,
};

fn setup() -> Project {
    Project::new("test", Rational::FPS_24)
}

fn test_ctx<'a>(
    project: &'a mut Project,
    project_path: &'a mut Option<std::path::PathBuf>,
    history: &'a mut History,
) -> ApplyContext<'a> {
    ApplyContext {
        project,
        project_path,
        history,
    }
}

/// Inert action for history bookkeeping tests.
struct NoopAction;

impl EditAction for NoopAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Ok(Box::new(NoopAction))
    }
}

/// Adds a throwaway track; its inverse removes it. Lets a compound test
/// observe a real, reversible mutation.
struct AddTrackTest;

impl EditAction for AddTrackTest {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = ctx.project.add_track(TrackKind::Video, "tmp");
        Ok(Box::new(RemoveTrackTest { id }))
    }
}

struct RemoveTrackTest {
    id: aria_models::TrackId,
}

impl EditAction for RemoveTrackTest {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        ctx.project
            .timeline_mut()
            .remove_track(self.id)
            .ok_or(aria_models::ModelError::UnknownTrack(self.id))?;
        Ok(Box::new(AddTrackTest))
    }
}

/// Always fails without touching the project.
struct FailAction;

impl EditAction for FailAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Err(aria_models::ModelError::InvalidRange.into())
    }
}

fn step(action: Box<dyn EditAction>) -> HistoryStep {
    HistoryStep {
        label: Cow::Borrowed("Move clip"),
        action,
    }
}

fn ctx_project_clip(project: &Project, id: aria_models::ClipId) -> aria_models::Clip {
    project.clip(id).unwrap().clone()
}

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, Rational::FPS_24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, Rational::FPS_24)
}

#[path = "mod_tests_ripple_delete.rs"]
mod ripple_delete_cases;

#[path = "mod_tests_link_clips.rs"]
mod link_clips_cases;

#[path = "mod_tests_unlink_rejections.rs"]
mod unlink_rejections_cases;

#[path = "mod_tests_routing.rs"]
mod routing_cases;
