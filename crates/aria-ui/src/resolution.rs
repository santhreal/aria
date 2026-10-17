//! Viewer resolution: the size the Viewer renders frames at during playback
//! and while paused, and the Auto level that follows dropped frames.
//!
//! A reduced frame is rendered small from the start: the scene is scaled
//! before compositing (`Engine::get_frame_fit`), and the Viewer scales the
//! picture up to the pane.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A fraction of the sequence's canvas size.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    #[default]
    Full,
    Half,
    Quarter,
    Eighth,
}

impl Resolution {
    /// Every level, largest first.
    pub const ALL: [Self; 4] = [Self::Full, Self::Half, Self::Quarter, Self::Eighth];

    /// What each side of the canvas is divided by.
    pub const fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
            Self::Eighth => 8,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Half => "1/2",
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
        }
    }

    /// One level smaller, or 1/8 at 1/8.
    pub const fn lower(self) -> Self {
        match self {
            Self::Full => Self::Half,
            Self::Half => Self::Quarter,
            Self::Quarter | Self::Eighth => Self::Eighth,
        }
    }

    /// One level larger, or Full at Full.
    pub const fn higher(self) -> Self {
        match self {
            Self::Full | Self::Half => Self::Full,
            Self::Quarter => Self::Half,
            Self::Eighth => Self::Quarter,
        }
    }

    /// The largest size a frame of `canvas` renders at: each side divided and
    /// rounded up, at least 1 px. `None` at Full, which renders the canvas.
    pub fn fit(self, canvas: (u32, u32)) -> Option<(u32, u32)> {
        let divisor = self.divisor();
        (divisor > 1).then(|| {
            (
                canvas.0.div_ceil(divisor).max(1),
                canvas.1.div_ceil(divisor).max(1),
            )
        })
    }
}

/// The playback resolution setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackResolution {
    Full,
    Half,
    Quarter,
    Eighth,
    /// Full until frames drop; see [`AutoResolution`].
    #[default]
    Auto,
}

impl PlaybackResolution {
    /// Every choice, in the order the Viewer lists them.
    pub const ALL: [Self; 5] = [
        Self::Full,
        Self::Half,
        Self::Quarter,
        Self::Eighth,
        Self::Auto,
    ];

    /// The level a fixed choice renders at; `None` for Auto.
    pub const fn fixed(self) -> Option<Resolution> {
        match self {
            Self::Full => Some(Resolution::Full),
            Self::Half => Some(Resolution::Half),
            Self::Quarter => Some(Resolution::Quarter),
            Self::Eighth => Some(Resolution::Eighth),
            Self::Auto => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self.fixed() {
            Some(level) => level.label(),
            None => "Auto",
        }
    }
}

/// The paused resolution setting: a paused or scrubbed frame renders at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PausedResolution {
    #[default]
    Full,
    Half,
}

impl PausedResolution {
    /// Every choice, in the order the Viewer lists them.
    pub const ALL: [Self; 2] = [Self::Full, Self::Half];

    pub const fn resolution(self) -> Resolution {
        match self {
            Self::Full => Resolution::Full,
            Self::Half => Resolution::Half,
        }
    }

    pub const fn label(self) -> &'static str {
        self.resolution().label()
    }
}

/// Dropped frames within [`AUTO_DROP_WINDOW`] that step Auto down one level.
pub const AUTO_DROPS: usize = 3;

/// The span [`AUTO_DROPS`] drops must fall in.
pub const AUTO_DROP_WINDOW: Duration = Duration::from_secs(1);

/// Playback this long with no drop and no level change steps Auto up one
/// level.
pub const AUTO_CLEAN: Duration = Duration::from_secs(5);

/// The level Auto renders playback at. Starts at Full when the session
/// opens and keeps its level between runs; time spent paused does not count
/// toward a step up.
#[derive(Debug, Clone, Default)]
pub struct AutoResolution {
    level: Resolution,
    /// When each drop of the last [`AUTO_DROP_WINDOW`] happened, oldest first.
    drops: VecDeque<Instant>,
    /// The start of the current run, the last drop, or the last level change,
    /// whichever came last.
    quiet_since: Option<Instant>,
}

impl AutoResolution {
    pub fn level(&self) -> Resolution {
        self.level
    }

    /// A playback run started at `now`.
    pub fn start(&mut self, now: Instant) {
        self.drops.clear();
        self.quiet_since = Some(now);
    }

    /// `count` frames dropped at `now`. True when the level stepped down.
    pub fn dropped(&mut self, count: u32, now: Instant) -> bool {
        if count == 0 {
            return false;
        }
        self.quiet_since = Some(now);
        let new = usize::try_from(count).unwrap_or(AUTO_DROPS).min(AUTO_DROPS);
        self.drops.extend(std::iter::repeat_n(now, new));
        while self
            .drops
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > AUTO_DROP_WINDOW)
        {
            self.drops.pop_front();
        }
        if self.drops.len() < AUTO_DROPS || self.level == Resolution::Eighth {
            return false;
        }
        self.level = self.level.lower();
        self.drops.clear();
        true
    }

    /// A frame presented at `now` with no drop. True when the level stepped
    /// up.
    pub fn presented(&mut self, now: Instant) -> bool {
        let Some(since) = self.quiet_since else {
            self.quiet_since = Some(now);
            return false;
        };
        if self.level == Resolution::Full || now.saturating_duration_since(since) < AUTO_CLEAN {
            return false;
        }
        self.level = self.level.higher();
        self.quiet_since = Some(now);
        true
    }
}

#[cfg(test)]
#[path = "resolution_tests.rs"]
mod tests;
