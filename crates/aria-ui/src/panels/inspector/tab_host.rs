//! The inspector's tabs, as data.
//!
//! A tab is a row: an id, a label, the clip kinds it applies to, and the body
//! it draws. The inspector renders the rows the host carries and nothing else,
//! so a tab arrives by adding a row rather than by extending an enum and the
//! three matches that read it. The id is the identity the app stores as the
//! selected tab, which is why it is a string: a tab contributed at runtime has
//! no enum variant to be.

use std::sync::LazyLock;

use gpui::{AnyElement, Entity, IntoElement, SharedString};

use crate::app::AppState;
use crate::theme::Theme;
use crate::types::{UiClip, UiClipKind};

/// A tab body. Every tab takes the same arguments so the row can be data; a
/// body that does not need the frame rate or the entity ignores them.
pub type TabBody = fn(
    &Theme,
    &UiClip,
    i32,
    i32,
    &Entity<AppState>,
    &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement;

/// One tab.
#[derive(Debug)]
pub struct TabEntry {
    /// Stable identity, and what `AppState::inspector_tab` holds.
    pub id: SharedString,
    /// What the tab strip shows.
    pub label: SharedString,
    /// Whether the tab applies to `clip`.
    pub applies: fn(&UiClip) -> bool,
    /// What the tab draws.
    pub body: TabBody,
}

/// The tabs the inspector draws, in strip order.
#[derive(Debug)]
pub struct TabHost {
    tabs: Vec<TabEntry>,
}

impl TabHost {
    /// A host with no tabs.
    pub fn new() -> Self {
        Self { tabs: Vec::new() }
    }

    /// Append a tab. Order is contribution order, which is strip order.
    pub fn add(&mut self, tab: TabEntry) {
        self.tabs.push(tab);
    }

    /// Every tab, in strip order.
    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    /// How many tabs the host carries.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the host carries no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The tab with this id, whether or not it applies to anything.
    pub fn get(&self, id: &str) -> Option<&TabEntry> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// The tabs that apply to `clip`, in strip order.
    pub fn visible<'a, 'b>(
        &'a self,
        clip: &'b UiClip,
    ) -> impl Iterator<Item = &'a TabEntry> + use<'a, 'b> {
        self.tabs.iter().filter(move |tab| (tab.applies)(clip))
    }

    /// The tab to draw for `clip` when `requested` is selected.
    ///
    /// A selected tab that does not apply to the clip falls back to the first
    /// that does, so selecting Text and then an audio clip draws something.
    /// `None` means no tab applies at all, which the caller draws as empty
    /// rather than guessing.
    pub fn resolve(&self, requested: &str, clip: &UiClip) -> Option<&TabEntry> {
        match self.get(requested) {
            Some(tab) if (tab.applies)(clip) => Some(tab),
            _ => self.visible(clip).next(),
        }
    }
}

impl Default for TabHost {
    fn default() -> Self {
        Self::new()
    }
}

/// The shipped tabs.
pub fn host_of() -> &'static TabHost {
    &TAB_HOST
}

static TAB_HOST: LazyLock<TabHost> = LazyLock::new(|| {
    let mut host = TabHost::new();
    for (id, label, applies, body) in SHIPPED {
        host.add(TabEntry {
            id: SharedString::new_static(id),
            label: SharedString::new_static(label),
            applies: *applies,
            body: *body,
        });
    }
    host
});

type ShippedTab = (&'static str, &'static str, fn(&UiClip) -> bool, TabBody);

static SHIPPED: &[ShippedTab] = &[
    ("transform", "Transform", not_audio, transform_body),
    ("look", "Look", not_audio, look_body),
    ("effects", "Effects", not_audio, effects_body),
    ("audio", "Audio", has_sound, audio_body),
    ("speed", "Speed", has_motion, speed_body),
    ("text", "Text", text_only, text_body),
    ("shape", "Shape", shape_only, shape_body),
    ("info", "Info", not_audio, info_body),
];

fn not_audio(clip: &UiClip) -> bool {
    clip.kind != UiClipKind::Audio
}

// Audio controls apply to a clip that has sound on its own lane: an audio
// clip, or an unsplit movie clip with sound. A still, a silent movie, a
// generated clip, or the picture half of a linked pair has none.
fn has_sound(clip: &UiClip) -> bool {
    clip.has_audio
}

// Retiming is a moving-media property: a generated clip or a still image has
// no frames to speed up, slow down, or reverse.
fn has_motion(clip: &UiClip) -> bool {
    clip.has_motion()
}

fn text_only(clip: &UiClip) -> bool {
    clip.kind == UiClipKind::Text
}

// A shape's geometry is its own tab for the same reason a title's type is: the
// properties exist on one kind of clip and on no other.
fn shape_only(clip: &UiClip) -> bool {
    clip.kind == UiClipKind::Shape
}

fn transform_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::transform::transform_tab(theme, clip, state, cx).into_any_element()
}

fn look_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::look::look_tab(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

fn effects_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::effects::effects_tab(theme, clip, state, cx).into_any_element()
}

fn speed_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::speed::speed_block(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

fn audio_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::audio::audio_tab(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

fn text_body(
    theme: &Theme,
    clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::text::text_tab(theme, clip, state, cx).into_any_element()
}

fn shape_body(
    theme: &Theme,
    _clip: &UiClip,
    _fps_num: i32,
    _fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::shape::shape_tab(theme, state, cx).into_any_element()
}

fn info_body(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    super::tabs::info::info_tab(theme, clip, fps_num, fps_den, state, cx).into_any_element()
}

#[cfg(test)]
#[path = "tab_heading_tests.rs"]
mod tab_heading_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The strip is what the host carries, so the host is measured rather than
    /// named: a tab added without a decision about which clips it applies to
    /// shows up here, not in a screenshot.
    #[test]
    fn the_host_carries_the_shipped_tabs_once_each() {
        let host = host_of();
        assert_eq!(host.len(), SHIPPED.len());
        for tab in host.tabs() {
            assert_eq!(
                host.tabs()
                    .iter()
                    .filter(|other| other.id == tab.id)
                    .count(),
                1,
                "{} is registered twice",
                tab.id
            );
            assert!(!tab.label.is_empty(), "{} has no label", tab.id);
        }
        assert!(
            host.get(crate::app::DEFAULT_INSPECTOR_TAB).is_some(),
            "the tab the app opens on, {}, is not registered",
            crate::app::DEFAULT_INSPECTOR_TAB
        );
    }

    #[test]
    fn shipped_tabs_are_allocated_once_for_the_process() {
        assert!(
            std::ptr::eq(host_of(), host_of()),
            "a repaint rebuilt the inspector tab host"
        );
    }

    fn sample_clip(kind: UiClipKind) -> UiClip {
        use aria_models::{
            Generator, MediaSource, Project, Rational, RationalTime, Shape, TimeRange, TrackKind,
        };
        let rate = Rational::FPS_24;
        let mut project = Project::new("Tabs", rate);
        let range = TimeRange::at_rate(0, 48, rate);
        let start = RationalTime::new(0, rate);
        let clip_id = match kind {
            UiClipKind::Media => {
                let media =
                    project.add_media(MediaSource::new("take.mp4", 1920, 1080, rate, 240, true));
                let v1 = project.add_track(TrackKind::Video, "V1");
                project
                    .add_clip(v1, media, range, start)
                    .expect("place video")
            }
            UiClipKind::Audio => {
                let media = project.add_media(MediaSource::new("tone.wav", 0, 0, rate, 240, true));
                let a1 = project.add_track(TrackKind::Audio, "A1");
                project
                    .add_clip(a1, media, range, start)
                    .expect("place audio")
            }
            UiClipKind::Text => {
                let t1 = project.add_track(TrackKind::Text, "T1");
                project
                    .add_generated(t1, Generator::text("Title"), range)
                    .expect("place title")
            }
            UiClipKind::Shape => {
                let s1 = project.add_track(TrackKind::Sticker, "S1");
                project
                    .add_generated(
                        s1,
                        Generator::shape(Shape::Rectangle, [255, 255, 255, 255]),
                        range,
                    )
                    .expect("place shape")
            }
            UiClipKind::Generator => {
                let s1 = project.add_track(TrackKind::Sticker, "S1");
                project
                    .add_generated(
                        s1,
                        Generator::SolidColor {
                            rgba: [255, 255, 255, 255],
                        },
                        range,
                    )
                    .expect("place solid")
            }
        };
        let store = crate::projection::project_to_ui(&project, start, &[]);
        store
            .sequence
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == clip_id.raw())
            .expect("projected clip")
            .clone()
    }

    /// Every clip kind must have somewhere to go. A kind with no applicable tab
    /// draws an empty inspector, which is the defect this asserts against.
    /// `UiClipKind::ALL` is generated from the list that declares the enum, so
    /// a new variant is in it without an edit here.
    #[test]
    fn every_clip_kind_has_a_tab_and_resolution_is_total() {
        let host = host_of();
        for kind in UiClipKind::ALL {
            let clip = sample_clip(kind);
            let visible: Vec<_> = host.visible(&clip).map(|tab| tab.id.clone()).collect();
            assert!(!visible.is_empty(), "{kind:?} has no inspector tab");

            // Whatever is selected, the resolved tab applies to the clip.
            for tab in host.tabs() {
                let resolved = host
                    .resolve(&tab.id, &clip)
                    .unwrap_or_else(|| panic!("{kind:?} resolved {} to nothing", tab.id));
                assert!(
                    (resolved.applies)(&clip),
                    "{kind:?} selected {} and resolved to {}, which does not apply",
                    tab.id,
                    resolved.id
                );
            }

            // An id nothing registered resolves to the first applicable tab
            // rather than to nothing.
            let fallback = host
                .resolve("no-such-tab", &clip)
                .unwrap_or_else(|| panic!("{kind:?} has no fallback tab"));
            assert_eq!(fallback.id, visible[0]);
        }
    }

    /// A selected tab that applies is kept. Without this the strip would snap
    /// back to the first tab on every render.
    #[test]
    fn a_selected_tab_that_applies_is_kept() {
        let host = host_of();
        let media = sample_clip(UiClipKind::Media);
        let resolved = host.resolve("info", &media).expect("info applies to media");
        assert_eq!(resolved.id, "info");
        // Text does not apply to an audio clip, so the strip falls back to the
        // first row that does: Audio, the tab with the clip's volume, pan and
        // fades, rather than Speed.
        let audio_clip = sample_clip(UiClipKind::Audio);
        let audio = host.resolve("text", &audio_clip).expect("audio has a tab");
        assert_eq!(audio.id, "audio");
    }

    /// Regression test for Effect Controls tab applicability across clip media
    /// facts.
    ///
    /// Closes the defect class where `TabEntry::applies` inspected only
    /// `UiClipKind`, offering the `Audio` tab on silent stills and on the
    /// picture half of a linked V+A pair, and offering the `Speed` tab on still
    /// images. Projects a timeline holding a silent still, a movie with sound
    /// split across linked V and A lanes, an unsplit movie with sound on V1, an
    /// audio clip, a title, and a shape, then asserts the exact visible tab id
    /// list per clip and that requesting `audio` or `speed` on a silent still
    /// resolves to `transform`.
    ///
    /// Does not catch: the controls drawn inside each tab body (covered by
    /// `columns_tests`, `units_tests`, and `tab_heading_tests`).
    #[test]
    fn visible_tabs_follow_whether_the_clip_has_sound_and_moving_media() {
        use aria_models::{
            Generator, MediaSource, Project, Rational, RationalTime, Shape, TimeRange, TrackKind,
        };
        let rate = Rational::FPS_24;
        let mut project = Project::new("TabFit", rate);
        let range = TimeRange::at_rate(0, 48, rate);
        let start = RationalTime::new(0, rate);

        let mut still_src = MediaSource::new("still.png", 1920, 1080, rate, 120, false);
        still_src.is_image = true;
        let still_media = project.add_media(still_src);
        let av_media =
            project.add_media(MediaSource::new("movie.mp4", 1920, 1080, rate, 240, true));
        let wav_media = project.add_media(MediaSource::new("tone.wav", 0, 0, rate, 240, true));

        let v_still = project.add_track(TrackKind::Video, "V1");
        let still = project
            .add_clip(v_still, still_media, range, start)
            .expect("place still");

        let v_linked = project.add_track(TrackKind::Video, "V2");
        let a_linked = project.add_track(TrackKind::Audio, "A1");
        let movie_v = project
            .add_clip(v_linked, av_media, range, start)
            .expect("place linked movie picture");
        let movie_a = project
            .add_clip(a_linked, av_media, range, start)
            .expect("place linked movie sound");
        project
            .link_clips(&[movie_v, movie_a])
            .expect("link picture and sound");

        let v_unsplit = project.add_track(TrackKind::Video, "V3");
        let unsplit = project
            .add_clip(v_unsplit, av_media, range, start)
            .expect("place unsplit movie");

        let a_wav = project.add_track(TrackKind::Audio, "A2");
        let audio = project
            .add_clip(a_wav, wav_media, range, start)
            .expect("place audio clip");

        let t1 = project.add_track(TrackKind::Text, "T1");
        let title = project
            .add_generated(t1, Generator::text("Title"), range)
            .expect("place title");

        let s1 = project.add_track(TrackKind::Sticker, "S1");
        let shape = project
            .add_generated(
                s1,
                Generator::shape(Shape::Rectangle, [255, 255, 255, 255]),
                range,
            )
            .expect("place shape");

        let store = crate::projection::project_to_ui(&project, start, &[]);
        let clip_of = |id: aria_models::ClipId| -> &UiClip {
            store
                .sequence
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .find(|c| c.id == id.raw())
                .expect("projected clip")
        };
        let host = host_of();
        let tabs_of = |id: aria_models::ClipId| -> Vec<&str> {
            host.visible(clip_of(id)).map(|t| t.id.as_ref()).collect()
        };

        assert_eq!(
            tabs_of(still),
            vec!["transform", "look", "effects", "info"],
            "silent still must not show Audio or Speed"
        );
        assert_eq!(
            tabs_of(movie_v),
            vec!["transform", "look", "effects", "speed", "info"],
            "picture half of a linked V+A movie shows Speed without Audio"
        );
        assert_eq!(
            tabs_of(movie_a),
            vec!["audio", "speed"],
            "audio half of a linked V+A movie shows Audio and Speed"
        );
        assert_eq!(
            tabs_of(unsplit),
            vec!["transform", "look", "effects", "audio", "speed", "info"],
            "unsplit movie with sound on V3 shows both Audio and Speed"
        );
        assert_eq!(
            tabs_of(audio),
            vec!["audio", "speed"],
            "audio clip shows Audio and Speed"
        );
        assert_eq!(
            tabs_of(title),
            vec!["transform", "look", "effects", "text", "info"],
            "title clip shows Text without Audio or Speed"
        );
        assert_eq!(
            tabs_of(shape),
            vec!["transform", "look", "effects", "shape", "info"],
            "shape clip shows Shape without Audio or Speed"
        );

        let still_ui = clip_of(still);
        assert_eq!(
            host.resolve("audio", still_ui).map(|t| t.id.as_ref()),
            Some("transform"),
            "selected Audio tab on a silent still must fall back to Transform"
        );
        assert_eq!(
            host.resolve("speed", still_ui).map(|t| t.id.as_ref()),
            Some("transform"),
            "selected Speed tab on a silent still must fall back to Transform"
        );
        let movie_v_ui = clip_of(movie_v);
        assert_eq!(
            host.resolve("audio", movie_v_ui).map(|t| t.id.as_ref()),
            Some("transform"),
            "selected Audio tab on the picture half of a linked movie must fall back to Transform"
        );
        assert_eq!(
            host.resolve("speed", movie_v_ui).map(|t| t.id.as_ref()),
            Some("speed"),
            "selected Speed tab on the picture half of a linked movie stays on Speed"
        );
    }

    /// The Effect Controls inspector tabs table in `docs/reference/panels.md`
    /// lists every registered tab, in strip order, with its label and the clip
    /// kinds it applies to. Fails on a tab added, renamed, reordered, or given
    /// other kinds without the table, and on a row for a tab nothing registers.
    /// Not caught: the prose of the Summary column.
    #[test]
    fn the_panels_reference_lists_every_tab_with_its_clip_kinds() {
        use crate::test_support::doc_tables::{
            parse_table_rows, read_doc, section_slice, strip_ticks,
        };
        let doc = read_doc("reference/panels.md");
        let clips: Vec<(UiClipKind, UiClip)> = UiClipKind::ALL
            .iter()
            .copied()
            .map(|k| (k, sample_clip(k)))
            .collect();
        let expected_insp_tabs: Vec<(String, String, String)> = host_of()
            .tabs()
            .iter()
            .map(|tab| {
                let kinds: Vec<String> = clips
                    .iter()
                    .filter(|(_, clip)| (tab.applies)(clip))
                    .map(|(k, _)| format!("{k:?}"))
                    .collect();
                (tab.id.to_string(), tab.label.to_string(), kinds.join(", "))
            })
            .collect();
        let documented_insp_tabs: Vec<(String, String, String)> =
            parse_table_rows(section_slice(&doc, "Effect Controls inspector tabs"))
                .iter()
                .map(|cells| {
                    assert!(cells.len() >= 3, "malformed inspector tab row: {cells:?}");
                    (
                        strip_ticks(cells[0]).to_string(),
                        cells[1].to_string(),
                        strip_ticks(cells[2]).to_string(),
                    )
                })
                .collect();
        assert_eq!(
            documented_insp_tabs, expected_insp_tabs,
            "docs/reference/panels.md Effect Controls inspector tabs drifted from TabHost"
        );
    }
}
