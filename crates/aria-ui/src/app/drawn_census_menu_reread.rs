//! How a context menu's facts are read again from the state a press left.

use super::*;

/// The facts a table's rows read, gathered again from the state a press left,
/// as a menu opened on the same target would gather them, or `None` when the
/// target is gone.
pub(super) trait Reread: Clone {
    fn reread(&self, state: &AppState) -> Option<Self>;
}

impl Reread for ClipMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state, self.clip))
    }
}

impl Reread for TrackMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.track)
    }
}

impl Reread for PoolMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state, self.media))
    }
}

impl Reread for EffectMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state, self.clip, self.index))
    }
}

impl Reread for BadgeMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.clip)
    }
}

impl Reread for KeyframeMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.clip.raw(), self.param, self.tick)
    }
}

impl Reread for MaskPathKeyMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.clip.raw(), self.mask, self.tick)
    }
}

impl Reread for PreviewMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state))
    }
}

/// A recents menu reads only the path it was opened on.
impl Reread for PathBuf {
    fn reread(&self, _: &AppState) -> Option<Self> {
        Some(self.clone())
    }
}
