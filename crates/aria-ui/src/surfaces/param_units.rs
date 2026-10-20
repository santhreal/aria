//! Properties a block reads in units the catalog does not carry.
//!
//! A property's range comes from the model, and for almost every row that is
//! also the display scale: a mask feather in the unit interval, a
//! shadow offset in pixels. A gain is the exception. It is stored as the linear
//! multiplier the mixer applies and ridden in decibels, so one step is a decibel
//! at unity and a decibel at a whisper, while a fixed step in the multiplier is
//! inaudible at the top of the fader and a cliff near silence.
//!
//! That is a property of the property, not of the panel that happens to draw it,
//! so it is declared here beside the other surface registries rather than inside
//! an inspector block. The row generator consults this list; a property with no
//! entry is drawn in the units the catalog declares. The write is still a command
//! the model validates: the units are the row's, the bound is the model's.

use aria_engine::EngineError;
use aria_models::ClipParam;

use crate::app::AppState;

/// One property's scale.
pub struct Units {
    pub param: ClipParam,
    /// What the row prints, from the stored value.
    pub read: fn(f32) -> String,
    /// The stored value in the units `read` prints, which is what a typed
    /// value replaces.
    pub shown: fn(f32) -> f32,
    /// Digits after the point when the value is typed.
    pub decimals: u8,
    /// What one unit of drag moves, in the units `read` prints.
    pub unit: f32,
    /// Move the property by `delta` of those units.
    pub write: fn(&mut AppState, f32) -> Result<(), EngineError>,
}

/// The gain of the selected clips, read in decibels.
pub const AUDIO: &[Units] = &[Units {
    param: ClipParam::Volume,
    read: level_label,
    shown: level_db,
    decimals: 1,
    unit: 0.5,
    write: |state, delta| state.nudge_shared_volume_db(delta),
}];

/// Every group holding a property with a scale of its own.
///
/// A group absent from here is drawn entirely from the catalog, which is almost
/// all of them.
pub const OVERRIDES: &[(&str, &[Units])] = &[("audio", AUDIO)];

/// The overrides one group carries.
pub fn of_group(group: &str) -> &'static [Units] {
    OVERRIDES
        .iter()
        .find(|(name, _)| *name == group)
        .map_or(&[][..], |(_, units)| *units)
}

/// The units one property is read in, when they are not the catalog's.
pub fn of_param(group: &str, param: ClipParam) -> Option<&'static Units> {
    of_group(group).iter().find(|units| units.param == param)
}

/// A gain in decibels. Silence reads one step under the floor, so a step up
/// from it lands on the floor and anything typed under the floor mutes.
fn level_db(volume: f32) -> f32 {
    aria_analysis::db_from_linear(volume).unwrap_or(crate::app::clip_audio::SILENT_DB)
}

/// What a gain reads as beside the fader.
///
/// Silence has no decibel value, which is what the bottom of the fader means: a
/// readout printing `-inf` would be a number the field could not take back.
fn level_label(volume: f32) -> String {
    match aria_analysis::db_from_linear(volume) {
        Some(db) => format!("{db:+.1} dB · {:.0}%", volume * 100.0),
        None => "Silent".to_string(),
    }
}
