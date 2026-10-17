//! Source patching: which lanes a Source tab's picture and sound land on.
//!
//! A three- or four-point edit places a Source tab's range on the sequence. The
//! sequence holds one patch per source channel: the picture goes to a video
//! lane, the sound to an audio lane, and either channel can be left out of the
//! edit. Resolving `Auto` to a lane reads the lane order and the target flags,
//! which the caller has, so this module stores the choice and nothing else.

use serde::{Deserialize, Serialize};

use crate::{MediaKind, MediaSource, TrackId, TrackKind};

/// One channel of a source: its picture or its sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceChannel {
    Video,
    Audio,
}

impl SourceChannel {
    /// Both channels, picture first.
    pub const ALL: [Self; 2] = [Self::Video, Self::Audio];

    /// The lane kind this channel lands on.
    pub const fn lane_kind(self) -> TrackKind {
        match self {
            Self::Video => TrackKind::Video,
            Self::Audio => TrackKind::Audio,
        }
    }

    /// The channel a lane of `kind` receives, if any. Only video and audio
    /// lanes take a source; titles, stickers, effects and buses do not.
    pub const fn of_lane(kind: TrackKind) -> Option<Self> {
        match kind {
            TrackKind::Video => Some(Self::Video),
            TrackKind::Audio => Some(Self::Audio),
            TrackKind::Text
            | TrackKind::Sticker
            | TrackKind::Effect
            | TrackKind::Filter
            | TrackKind::Adjustment
            | TrackKind::Submix => None,
        }
    }
}

impl MediaSource {
    /// Whether this source has `channel`. Every source but a sound file has a
    /// picture; a source has sound when its file carries a sound stream and it
    /// is not a still.
    pub fn has_channel(&self, channel: SourceChannel) -> bool {
        match channel {
            SourceChannel::Video => self.kind() != MediaKind::Audio,
            SourceChannel::Audio => self.has_audio && self.kind() != MediaKind::Image,
        }
    }
}

/// Where one source channel lands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Patch {
    /// The topmost targeted lane of the channel's kind, else its first lane.
    /// A patch to a lane that no longer exists resolves the same way.
    #[default]
    Auto,
    /// This lane.
    Track(TrackId),
    /// The channel is left out of the edit.
    Off,
}

/// The sequence's patch for each source channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePatching {
    #[serde(default, skip_serializing_if = "is_auto")]
    pub video: Patch,
    #[serde(default, skip_serializing_if = "is_auto")]
    pub audio: Patch,
}

fn is_auto(patch: &Patch) -> bool {
    *patch == Patch::Auto
}

impl SourcePatching {
    /// Both channels on `Auto`, which is what a sequence starts with and what a
    /// save omits.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub const fn get(&self, channel: SourceChannel) -> Patch {
        match channel {
            SourceChannel::Video => self.video,
            SourceChannel::Audio => self.audio,
        }
    }

    pub fn set(&mut self, channel: SourceChannel, patch: Patch) {
        match channel {
            SourceChannel::Video => self.video = patch,
            SourceChannel::Audio => self.audio = patch,
        }
    }
}

/// Where one source edit puts the source's sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundPlacement {
    /// A clip of its own on this audio lane, linked to the picture.
    Lane(TrackId),
    /// Inside the picture clip, which plays it. A sequence with no audio lane
    /// keeps a source's sound this way.
    InPicture,
    /// Left out of the edit; a picture with sound is placed silent.
    Off,
}

impl SoundPlacement {
    /// The audio lane the sound lands on, if it lands on one.
    pub const fn lane(self) -> Option<TrackId> {
        match self {
            Self::Lane(lane) => Some(lane),
            Self::InPicture | Self::Off => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lane_kind_maps_to_the_channel_that_lands_on_it() {
        for kind in crate::track_catalog::track_kind_specs()
            .iter()
            .map(|spec| spec.kind)
        {
            match SourceChannel::of_lane(kind) {
                Some(channel) => assert_eq!(channel.lane_kind(), kind),
                None => assert!(
                    SourceChannel::ALL
                        .iter()
                        .all(|channel| channel.lane_kind() != kind),
                    "{kind:?} receives a channel but maps to none"
                ),
            }
        }
    }

    #[test]
    fn a_patch_round_trips_and_auto_is_omitted() {
        let mut patching = SourcePatching::default();
        assert_eq!(
            serde_json::to_value(patching).unwrap(),
            serde_json::json!({})
        );

        patching.set(SourceChannel::Video, Patch::Track(TrackId::from_raw(7)));
        patching.set(SourceChannel::Audio, Patch::Off);
        let wire = serde_json::to_value(patching).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"video": {"track": 7}, "audio": "off"})
        );
        let back: SourcePatching = serde_json::from_value(wire).unwrap();
        assert_eq!(back, patching);
        assert_eq!(back.get(SourceChannel::Audio), Patch::Off);

        for (sound, wire) in [
            (
                SoundPlacement::Lane(TrackId::from_raw(4)),
                serde_json::json!({"lane": 4}),
            ),
            (SoundPlacement::InPicture, serde_json::json!("in_picture")),
            (SoundPlacement::Off, serde_json::json!("off")),
        ] {
            assert_eq!(serde_json::to_value(sound).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<SoundPlacement>(wire).unwrap(),
                sound
            );
        }
    }

    /// The stale copy: a sequence written before patching existed has no
    /// `source_patching` key, and must load on `Auto` for both channels.
    #[test]
    fn a_sequence_saved_before_patching_loads_on_auto() {
        let rate = crate::Rational { num: 30, den: 1 };
        let mut timeline = crate::Timeline::new(rate);
        let saved = serde_json::to_value(&timeline).unwrap();
        assert!(
            saved.get("source_patching").is_none(),
            "a sequence on Auto writes the same file it did before patching"
        );
        let loaded: crate::Timeline = serde_json::from_value(saved).unwrap();
        assert!(loaded.source_patching().is_default());

        let sound = timeline.add_track(crate::Track::new(TrackKind::Audio, "A1"));
        timeline
            .set_source_patch(SourceChannel::Audio, Patch::Track(sound))
            .expect("an audio lane takes the sound");
        let loaded: crate::Timeline =
            serde_json::from_value(serde_json::to_value(&timeline).unwrap()).unwrap();
        assert_eq!(
            loaded.source_patching().get(SourceChannel::Audio),
            Patch::Track(sound)
        );
    }

    #[test]
    fn a_lane_patch_must_name_a_lane_of_the_channels_kind() {
        let mut timeline = crate::Timeline::new(crate::Rational { num: 30, den: 1 });
        let sound = timeline.add_track(crate::Track::new(TrackKind::Audio, "A1"));
        let title = timeline.add_track(crate::Track::new(TrackKind::Text, "T1"));
        for (channel, lane) in [
            (SourceChannel::Video, sound),
            (SourceChannel::Video, title),
            (SourceChannel::Audio, title),
        ] {
            assert!(
                timeline
                    .set_source_patch(channel, Patch::Track(lane))
                    .is_err(),
                "{channel:?} patched to {lane:?}"
            );
        }
        assert!(
            timeline
                .set_source_patch(SourceChannel::Audio, Patch::Track(TrackId::from_raw(9999)))
                .is_err(),
            "a patch to a lane the sequence does not have"
        );
        assert!(
            timeline.source_patching().is_default(),
            "a refusal writes nothing"
        );
    }
}
