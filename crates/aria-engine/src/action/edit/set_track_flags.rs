use aria_models::{ModelError, TrackId};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Which of a lane's flags an edit writes. `None` leaves a flag as it is.
///
/// A patch rather than seven positional `Option<bool>` arguments: every
/// `SetTrack*` command writes exactly one of these, and the call site should
/// name which one.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlagPatch {
    pub enabled: Option<bool>,
    pub muted: Option<bool>,
    pub solo: Option<bool>,
    pub locked: Option<bool>,
    pub duck_source: Option<bool>,
    pub targeted: Option<bool>,
    pub sync_lock: Option<bool>,
}

impl FlagPatch {
    pub fn enabled(enabled: bool) -> Self {
        Self {
            enabled: Some(enabled),
            ..Self::default()
        }
    }

    pub fn muted(muted: bool) -> Self {
        Self {
            muted: Some(muted),
            ..Self::default()
        }
    }

    pub fn solo(solo: bool) -> Self {
        Self {
            solo: Some(solo),
            ..Self::default()
        }
    }

    pub fn locked(locked: bool) -> Self {
        Self {
            locked: Some(locked),
            ..Self::default()
        }
    }

    pub fn duck_source(duck_source: bool) -> Self {
        Self {
            duck_source: Some(duck_source),
            ..Self::default()
        }
    }

    pub fn targeted(targeted: bool) -> Self {
        Self {
            targeted: Some(targeted),
            ..Self::default()
        }
    }

    pub fn sync_lock(sync_lock: bool) -> Self {
        Self {
            sync_lock: Some(sync_lock),
            ..Self::default()
        }
    }
}

/// The undoable unit for a track's on/off flags (enabled / muted / solo /
/// locked / duck source / targeted / sync lock).
///
/// Each `SetTrack*` command changes exactly one flag; the action snapshots all
/// of the *previous* values so its inverse is a plain swap that oscillates
/// like trim's `RestoreClip`.
pub struct SetTrackFlagsAction {
    pub track: TrackId,
    pub enabled: bool,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub duck_source: bool,
    pub targeted: bool,
    pub sync_lock: bool,
}

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    patch: FlagPatch,
) -> Result<Box<dyn EditAction>, EngineError> {
    let t = ctx
        .project
        .timeline_mut()
        .track_mut(track)
        .ok_or(ModelError::UnknownTrack(track))?;
    // Snapshot the pre-edit flags into the inverse (a full restore).
    let inverse = Box::new(SetTrackFlagsAction {
        track,
        enabled: t.enabled,
        muted: t.muted,
        solo: t.solo,
        locked: t.locked,
        duck_source: t.duck_source,
        targeted: t.targeted,
        sync_lock: t.sync_lock,
    });
    if let Some(v) = patch.enabled {
        t.enabled = v;
    }
    if let Some(v) = patch.muted {
        t.muted = v;
    }
    if let Some(v) = patch.solo {
        t.solo = v;
    }
    if let Some(v) = patch.locked {
        t.locked = v;
    }
    if let Some(v) = patch.duck_source {
        t.duck_source = v;
    }
    if let Some(v) = patch.targeted {
        t.targeted = v;
    }
    if let Some(v) = patch.sync_lock {
        t.sync_lock = v;
    }
    Ok(inverse)
}

impl EditAction for SetTrackFlagsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        execute(
            ctx,
            self.track,
            FlagPatch {
                enabled: Some(self.enabled),
                muted: Some(self.muted),
                solo: Some(self.solo),
                locked: Some(self.locked),
                duck_source: Some(self.duck_source),
                targeted: Some(self.targeted),
                sync_lock: Some(self.sync_lock),
            },
        )
    }
}
