//! Final Cut Pro XML: the document the writer produces, what it reports and
//! leaves out, what a reader takes from a file as Final Cut Pro writes one,
//! and that what the writer produces reads back as the cut it came from.
//!
//! Not caught here: compound clips, multicam and auditions are reported and
//! skipped, never read; their contents are not covered.

use aria_models::{MarkerColor, Rational};

use super::{read, write};
use crate::model::{Cut, Event, LaneKind, Read};
use crate::test_support::{R24, at, cut, event, lane, mark, written};

/// A picture with its sound, a shot at double speed dissolving in, a second
/// picture lane read at its own rate, music counted in samples, and a marker
/// with a note.
fn promo(rate: Rational) -> Cut {
    let interview = Event {
        sound: true,
        ..event("Interview", 0, 48, "/media/A001.mov", at(24, rate))
    };
    let broll = Event {
        speed: Rational::new(2, 1),
        dissolve: Some(12),
        ..event("Broll", 48, 24, "/media/B002.mov", at(0, rate))
    };
    let logo = event("Logo", 12, 24, "/media/logo.mov", at(10, Rational::FPS_25));
    let music = Event {
        sound: true,
        ..event(
            "music",
            0,
            72,
            "/media/music.wav",
            at(0, Rational::new(48_000, 1)),
        )
    };
    Cut {
        size: Some((1920, 1080)),
        ..cut(
            "Promo",
            rate,
            vec![
                lane(LaneKind::Video, "V1", vec![interview, broll]),
                lane(LaneKind::Video, "V2", vec![logo]),
                lane(LaneKind::Audio, "A1", vec![music]),
            ],
            vec![mark(24, "Check", "Focus & go", None)],
        )
    }
}

#[test]
fn what_the_writer_produces_reads_back_as_the_cut() {
    for rate in [R24, Rational::FPS_25, Rational::FPS_29_97] {
        let original = promo(rate);
        let (text, skipped) = written(write, &original);
        assert!(skipped.is_empty(), "{rate:?}: {skipped:?}");
        let drop = if rate == Rational::FPS_29_97 {
            "DF"
        } else {
            "NDF"
        };
        assert!(
            text.contains(&format!("tcFormat=\"{drop}\"")),
            "{rate:?}: {text}"
        );

        let read = read(&text, Rational::new(7, 1), "fallback").expect("reads");
        assert_eq!(
            read,
            Read {
                cut: original,
                skipped: Vec::new()
            },
            "{rate:?}"
        );
    }
}

#[test]
fn the_writer_produces_fcpxml() {
    let interview = Event {
        sound: true,
        ..event("Interview", 0, 48, "/media/A001.mov", at(24, R24))
    };
    let broll = Event {
        speed: Rational::new(2, 1),
        dissolve: Some(12),
        ..event("Broll", 48, 24, "/media/B002.mov", at(0, R24))
    };
    let original = Cut {
        size: Some((1920, 1080)),
        ..cut(
            "Promo <cut>",
            R24,
            vec![lane(LaneKind::Video, "V1", vec![interview, broll])],
            vec![mark(24, "Check", "Focus & go", None)],
        )
    };
    let (text, skipped) = written(write, &original);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(
        text,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE fcpxml>

<fcpxml version="1.10">
    <resources>
        <format id="r1" frameDuration="1/24s" width="1920" height="1080"/>
        <asset id="r2" name="A001" start="0s" duration="3s" hasVideo="1" hasAudio="1" format="r1">
            <media-rep kind="original-media" src="file:///media/A001.mov"/>
        </asset>
        <asset id="r3" name="B002" start="0s" duration="2s" hasVideo="1" format="r1">
            <media-rep kind="original-media" src="file:///media/B002.mov"/>
        </asset>
        <effect id="r4" name="Cross Dissolve" uid="FxPlug:4731E73A-8DAC-4113-9A30-AE85B1761265"/>
    </resources>
    <library>
        <event name="Promo &lt;cut&gt;">
            <project name="Promo &lt;cut&gt;">
                <sequence format="r1" duration="3s" tcStart="0s" tcFormat="NDF">
                    <spine>
                        <asset-clip ref="r2" offset="0s" name="Interview" start="1s" duration="2s">
                            <marker start="2s" duration="1/24s" value="Check" note="Focus &amp; go"/>
                        </asset-clip>
                        <transition name="Cross Dissolve" offset="7/4s" duration="1/2s">
                            <filter-video ref="r4" name="Cross Dissolve"/>
                        </transition>
                        <asset-clip ref="r3" offset="2s" name="Broll" start="0s" duration="1s" srcEnable="video">
                            <timeMap>
                                <timept time="0s" value="0s" interp="linear"/>
                                <timept time="1s" value="2s" interp="linear"/>
                            </timeMap>
                        </asset-clip>
                    </spine>
                </sequence>
            </project>
        </event>
    </library>
</fcpxml>
"#
    );
    let read = read(&text, R24, "fallback").expect("reads");
    assert_eq!(
        read,
        Read {
            cut: original,
            skipped: Vec::new()
        }
    );
}

#[test]
fn sound_counted_in_milliseconds_is_written_at_48_khz() {
    let music = Event {
        sound: true,
        ..event(
            "music",
            0,
            36,
            "/media/music.wav",
            at(1500, Rational::new(1000, 1)),
        )
    };
    let original = cut(
        "",
        R24,
        vec![lane(LaneKind::Audio, "A1", vec![music])],
        vec![],
    );
    let (text, skipped) = written(write, &original);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(
        text.contains(
            "<asset id=\"r2\" name=\"music\" start=\"0s\" duration=\"3s\" hasAudio=\"1\" audioRate=\"48000\">"
        ),
        "{text}"
    );
    assert!(text.contains("<event name=\"Timeline\">"), "{text}");

    let read = read(&text, R24, "fallback").expect("reads");
    let music = Event {
        sound: true,
        ..event(
            "music",
            0,
            36,
            "/media/music.wav",
            at(72_000, Rational::new(48_000, 1)),
        )
    };
    assert_eq!(
        read,
        Read {
            cut: cut(
                "Timeline",
                R24,
                vec![lane(LaneKind::Audio, "A1", vec![music])],
                vec![]
            ),
            skipped: Vec::new()
        }
    );
}

#[test]
fn names_survive_escaping() {
    let name = "Say \"hi\" & 'bye' <now>\nline\r\tend\u{1}\u{FFFE}";
    let (text, _) = written(write, &cut(name, R24, vec![], vec![]));
    assert!(
        text.contains(
            "<event name=\"Say &quot;hi&quot; &amp; &apos;bye&apos; &lt;now&gt;&#10;line&#13;&#9;end  \">"
        ),
        "{text}"
    );
    let read = read(&text, R24, "fallback").expect("reads");
    assert_eq!(read.cut.name, "Say \"hi\" & 'bye' <now>\nline\r\tend  ");
}

#[test]
fn the_writer_reports_what_fcpxml_cannot_hold() {
    let c = Event {
        dissolve: Some(6),
        ..event("C", 48, 24, "/m/c.mov", at(0, R24))
    };
    let w = Event {
        speed: Rational::new(0, 1),
        ..event("W", 100, 5, "/m/w.mov", at(0, R24))
    };
    let y = Event {
        dissolve: Some(4),
        ..event("Y", 30, 4, "/m/y.mov", at(0, R24))
    };
    let original = cut(
        "Reports",
        R24,
        vec![
            lane(
                LaneKind::Video,
                "V1",
                vec![
                    event("A", 0, 24, "/m/a.mov", at(0, R24)),
                    event("B", 12, 24, "/m/b.mov", at(0, R24)),
                    c,
                    event("E", 72, 0, "/m/e.mov", at(0, R24)),
                    w,
                ],
            ),
            lane(
                LaneKind::Video,
                "V2",
                vec![
                    event("X", -5, 10, "/m/x.mov", at(0, R24)),
                    y,
                    event("Z", 40, 10, "/m/z.mov", at(0, Rational::new(0, 1))),
                ],
            ),
        ],
        vec![
            mark(-3, "M", "", Some(MarkerColor::Red)),
            mark(10, "N", "", None),
        ],
    );
    let (text, skipped) = written(write, &original);
    assert_eq!(
        skipped,
        [
            "V1 W: its source rate or speed is not a rate, not written",
            "V2 Z: its source rate or speed is not a rate, not written",
            "V1 B: overlaps the clip before it, not written",
            "V2: dissolves not written; FCPXML connected clips hold no transitions",
            "V2 X: before the start, not written",
            "markers: colors not written; FCPXML markers have none",
            "marker M: before the start, not written",
            "V1 C: a dissolve from no clip, not written",
        ]
    );
    assert!(!text.contains("<effect"), "{text}");

    let read = read(&text, R24, "fallback").expect("reads");
    assert_eq!(read.skipped, Vec::<String>::new());
    let placed: Vec<(&str, &str, i64)> = read
        .cut
        .lanes
        .iter()
        .flat_map(|lane| {
            lane.events
                .iter()
                .map(move |event| (lane.name.as_str(), event.name.as_str(), event.start))
        })
        .collect();
    assert_eq!(placed, [("V1", "A", 0), ("V1", "C", 48), ("V2", "Y", 30)]);
    assert_eq!(read.cut.marks, [mark(10, "N", "", None)]);

    let mut out = Vec::new();
    let err = write(&cut("x", Rational::new(0, 1), vec![], vec![]), &mut out).expect_err("no rate");
    assert_eq!(err.to_string(), "the timeline has no frame rate");
}

/// A Final Cut Pro 1.9 export at 25 fps starting at one hour: a clip with a
/// connected music bed, markers and effects, a dissolve into a speed ramp, a
/// title over a gap, a compound clip, a disabled clip, and a `clip` wrapping
/// its media with a second project after it.
const FOREIGN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE fcpxml>
<fcpxml version="1.9">
    <resources>
        <format id="r1" name="FFVideoFormat1080p25" frameDuration="100/2500s" width="1920" height="1080" colorSpace="1-1-1 (Rec. 709)"/>
        <asset id="r2" name="A001" uid="1A2B" start="3600s" duration="20s" hasVideo="1" format="r1" hasAudio="1" audioSources="1" audioChannels="2" audioRate="48000">
            <media-rep kind="proxy-media" src="file:///Volumes/Proxy/A001.mov"/>
            <media-rep kind="original-media" sig="AB12" src="file:///Volumes/Media/A001.mov"/>
        </asset>
        <asset id="r3" name="Music Bed" start="0s" duration="60s" hasAudio="1" audioSources="1" audioChannels="2" audioRate="44.1k">
            <media-rep kind="original-media" src="file:///Volumes/Media/Music%20Bed.wav"/>
        </asset>
        <asset id="r4" name="B002" start="0s" duration="30s" hasVideo="1" format="r1">
            <media-rep kind="original-media" src="file:///Volumes/Media/B002.mov"/>
        </asset>
        <effect id="r5" name="Cross Dissolve" uid="FxPlug:4731E73A-8DAC-4113-9A30-AE85B1761265"/>
        <effect id="r6" name="Gaussian Blur" uid=".../Effects.localized/Blur.localized/Gaussian.localized/Gaussian.moef"/>
        <effect id="r7" name="Basic Title" uid=".../Titles.localized/Bumper:Opener.localized/Basic Title.localized/Basic Title.moti"/>
        <media id="r8" name="Compound"><sequence format="r1" duration="1s"><spine/></sequence></media>
    </resources>
    <library location="file:///Users/editor/Movies/Library.fcpbundle/">
        <event name="Day 1" uid="E1">
            <project name="Edit 1" uid="P1">
                <sequence format="r1" duration="11s" tcStart="3600s" tcFormat="NDF" audioLayout="stereo" audioRate="48k">
                    <spine>
                        <asset-clip ref="r2" offset="3600s" name="Shot 1" start="3610s" duration="4s" tcFormat="NDF">
                            <adjust-transform position="10 0"/>
                            <asset-clip ref="r3" lane="-1" offset="3611s" name="Music Bed" start="0s" duration="3s"/>
                            <marker start="3612s" duration="100/2500s" value="Slate" note="take 2"/>
                            <chapter-marker start="3613s" duration="100/2500s" value="Chapter" posterOffset="0s"/>
                            <filter-video ref="r6" name="Gaussian Blur"/>
                        </asset-clip>
                        <transition name="Cross Dissolve" offset="7207/2s" duration="1s">
                            <filter-video ref="r5" name="Cross Dissolve"/>
                        </transition>
                        <asset-clip ref="r4" offset="3604s" name="B-roll" duration="2s">
                            <timeMap>
                                <timept time="0s" value="0s" interp="smooth2"/>
                                <timept time="1s" value="2s" interp="smooth2"/>
                                <timept time="2s" value="3s" interp="smooth2"/>
                            </timeMap>
                        </asset-clip>
                        <gap name="Gap" offset="3606s" start="3600s" duration="1s">
                            <title ref="r7" lane="1" offset="3600s" name="Lower Third" start="3600s" duration="1s"/>
                        </gap>
                        <ref-clip ref="r8" offset="3607s" name="Compound" duration="1s"/>
                        <asset-clip ref="r2" offset="3608s" name="Hidden" start="3600s" duration="1s" enabled="0"/>
                        <clip name="Wrapped" offset="3609s" duration="2s" tcFormat="NDF">
                            <video ref="r4" offset="0s" start="20s" duration="5s"/>
                            <audio ref="r3" lane="-1" offset="0s" start="0s" duration="2s"/>
                        </clip>
                    </spine>
                </sequence>
            </project>
            <project name="Other"><sequence format="r1"><spine/></sequence></project>
        </event>
    </library>
</fcpxml>
"#;

#[test]
fn a_final_cut_pro_export_reads_with_what_it_leaves_out() {
    let rate = Rational::FPS_25;
    let shot = Event {
        sound: true,
        ..event("Shot 1", 0, 100, "/Volumes/Media/A001.mov", at(250, rate))
    };
    let broll = Event {
        speed: Rational::new(3, 2),
        dissolve: Some(25),
        ..event("B-roll", 100, 50, "/Volumes/Media/B002.mov", at(0, rate))
    };
    let wrapped = event("Wrapped", 225, 50, "/Volumes/Media/B002.mov", at(500, rate));
    let bed = |start, duration| Event {
        sound: true,
        ..event(
            "Music Bed",
            start,
            duration,
            "/Volumes/Media/Music Bed.wav",
            at(0, Rational::new(44_100, 1)),
        )
    };
    let expected = Cut {
        size: Some((1920, 1080)),
        ..cut(
            "Edit 1",
            rate,
            vec![
                lane(LaneKind::Video, "V1", vec![shot, broll, wrapped]),
                lane(LaneKind::Audio, "A1", vec![bed(25, 75), bed(225, 50)]),
            ],
            vec![
                mark(50, "Slate", "take 2", None),
                mark(75, "Chapter", "", None),
            ],
        )
    };
    assert_eq!(
        read(FOREIGN, R24, "fallback").expect("reads"),
        Read {
            cut: expected,
            skipped: [
                "project Other: not read; only the first project is",
                "Shot 1 at 00:00:00:00: adjust-transform, filter-video, not read",
                "B-roll at 00:00:04:00: a speed ramp, read at its average speed",
                "Lower Third at 00:00:06:00: a title, not read",
                "Compound at 00:00:07:00: a ref-clip, not read",
                "Hidden at 00:00:08:00: disabled, not read",
            ]
            .map(str::to_owned)
            .to_vec(),
        }
    );
}

#[test]
fn sound_detached_from_its_picture_folds_back_in() {
    let text = r#"<fcpxml version="1.10">
    <resources>
        <format id="r1" frameDuration="1/24s"/>
        <asset id="r2" name="A001" start="0s" hasVideo="1" hasAudio="1" format="r1" src="file:///m/A001.mov"/>
    </resources>
    <library><event><project name="P"><sequence format="r1"><spine>
        <asset-clip ref="r2" offset="0s" name="Take" start="10s" duration="2s" srcEnable="video">
            <asset-clip ref="r2" lane="-1" offset="10s" name="Take" start="10s" duration="2s" srcEnable="audio"/>
        </asset-clip>
        <asset-clip ref="r2" offset="2s" name="Split" start="20s" duration="2s" srcEnable="video">
            <asset-clip ref="r2" lane="-1" offset="19s" name="Split" start="19s" duration="3s" srcEnable="audio"/>
        </asset-clip>
    </spine></sequence></project></event></library>
</fcpxml>"#;
    let take = Event {
        sound: true,
        ..event("Take", 0, 48, "/m/A001.mov", at(240, R24))
    };
    let split = event("Split", 48, 48, "/m/A001.mov", at(480, R24));
    let early = Event {
        sound: true,
        ..event("Split", 24, 72, "/m/A001.mov", at(456, R24))
    };
    assert_eq!(
        read(text, Rational::new(7, 1), "fallback").expect("reads"),
        Read {
            cut: cut(
                "P",
                R24,
                vec![
                    lane(LaneKind::Video, "V1", vec![take, split]),
                    lane(LaneKind::Audio, "A1", vec![early]),
                ],
                vec![],
            ),
            skipped: Vec::new(),
        }
    );
}

#[test]
fn the_reader_reports_what_it_does_not_read() {
    let text = r#"<fcpxml version="1.10">
    <resources>
        <format id="r1" frameDuration="1/24s"/>
        <asset id="r2" name="A" start="0s" hasVideo="1" hasAudio="1" format="r1" src="file:///m/A.mov"/>
        <asset id="r3" name="S" start="0s" hasAudio="1" audioRate="48000" src="file:///m/S.wav"/>
    </resources>
    <library><event><project name="Reports"><sequence format="r1"><spine>
        <transition offset="0s" duration="1s"/>
        <asset-clip ref="r2" offset="0s" name="One" duration="1s"><marker start="x" value="M"/></asset-clip>
        <asset-clip ref="r9" offset="1s" name="Lost" duration="1s"/>
        <asset-clip ref="r2" offset="2s" name="Frozen" duration="1s"><timeMap><timept time="0s" value="5s"/><timept time="1s" value="5s"/></timeMap></asset-clip>
        <asset-clip ref="r2" offset="3s" name="Fast" duration="1s"><timeMap><timept time="0s" value="0s"/><timept time="1s" value="2000s"/></timeMap></asset-clip>
        <asset-clip ref="r3" offset="4s" name="Mute" duration="1s" srcEnable="video"/>
        <asset-clip ref="r2" offset="4.5x" name="Bad" duration="1s"/>
        <clip offset="5s" name="Nest" duration="1s"><spine><asset-clip ref="r2" offset="0s" duration="1s"/></spine></clip>
        <mc-clip offset="6s" name="Multi" duration="1s"/>
        <asset-clip ref="r2" offset="7s" name="Two" duration="1s"/>
        <transition offset="15/2s" duration="1s"/>
        <gap offset="8s" duration="1s"/>
        <asset-clip ref="r2" offset="9s" name="Three" duration="1s"/>
        <transition offset="37/4s" duration="1s"/>
        <asset-clip ref="r2" offset="10s" name="Four" duration="1s"/>
        <asset-clip ref="r2" offset="11s" name="Five" duration="1s"><asset-clip ref="r2" lane="up" offset="0s" name="Odd" duration="1s"/></asset-clip>
        <transition offset="23/2s" duration="1s"/>
    </spine></sequence></project></event></library>
</fcpxml>"#;
    let read = read(text, Rational::new(7, 1), "fallback").expect("reads");
    assert_eq!(
        read.skipped,
        [
            "transition at 00:00:00:00: a transition from nothing, not read",
            "marker at 00:00:00:00: its start is not a time",
            "Lost at 00:00:01:00: its media reference names no asset",
            "Frozen at 00:00:02:00: a freeze frame, not read",
            "Fast at 00:00:03:00: plays faster than 1000 times, not read",
            "Mute at 00:00:04:00: its media has no picture, not read",
            "Bad at 00:00:00:00: its offset is not a time",
            "Nest at 00:00:05:00: a compound clip, not read",
            "Multi at 00:00:06:00: a mc-clip, not read",
            "transition at 00:00:07:12: a transition into no clip, not read",
            "transition at 00:00:09:06: off its cut, read centered on it",
            "Odd at 00:00:11:00: its lane is not a whole number",
            "transition at 00:00:11:12: a transition to nothing, not read",
        ]
    );
    let placed: Vec<(&str, i64, Option<i64>)> = read
        .cut
        .lanes
        .iter()
        .flat_map(|lane| &lane.events)
        .map(|event| (event.name.as_str(), event.start, event.dissolve))
        .collect();
    assert_eq!(
        placed,
        [
            ("One", 0, None),
            ("Two", 168, None),
            ("Three", 216, None),
            ("Four", 240, Some(24)),
            ("Five", 264, None),
        ]
    );
    assert!(read.cut.marks.is_empty());
}

#[test]
fn a_file_without_formats_or_names_reads_at_the_callers_rate() {
    let text = r#"<fcpxml>
    <resources><asset id="a" src="file:///m/Clip%201.mov"/></resources>
    <library><event><project><sequence><spine>
        <asset-clip ref="a" offset="1s" duration="1s"/>
    </spine></sequence></project></event></library>
</fcpxml>"#;
    let rate = Rational::FPS_30;
    let clip = Event {
        sound: true,
        ..event("Clip 1", 30, 30, "/m/Clip 1.mov", at(0, rate))
    };
    assert_eq!(
        read(text, rate, "fallback").expect("reads"),
        Read {
            cut: cut(
                "fallback",
                rate,
                vec![lane(LaneKind::Video, "V1", vec![clip])],
                vec![]
            ),
            skipped: Vec::new(),
        }
    );
}

#[test]
fn drop_frame_sequences_report_drop_frame_timecode() {
    let file = |tc_format: &str| {
        format!(
            r#"<fcpxml version="1.10"><resources><format id="r1" frameDuration="1001/30000s"/></resources>
<library><event><project name="P"><sequence format="r1"{tc_format}><spine>
<gap offset="0s" duration="1801800/30000s"/>
<gap name="Off" offset="1801800/30000s" duration="1001/30000s" enabled="0"/>
</spine></sequence></project></event></library></fcpxml>"#
        )
    };
    for (tc_format, expected) in [
        ("", "Off at 00:01:00;02: disabled, not read"),
        (" tcFormat=\"DF\"", "Off at 00:01:00;02: disabled, not read"),
        (
            " tcFormat=\"NDF\"",
            "Off at 00:01:00:00: disabled, not read",
        ),
    ] {
        let read = read(&file(tc_format), R24, "fallback").expect("reads");
        assert_eq!(read.cut.rate, Rational::FPS_29_97, "{tc_format}");
        assert_eq!(read.skipped, [expected], "{tc_format}");
    }
}

#[test]
fn clips_nested_past_the_limit_are_reported() {
    let mut clips = String::new();
    for depth in 0..40 {
        let lane = if depth == 0 { "" } else { " lane=\"1\"" };
        clips.push_str(&format!(
            "<asset-clip ref=\"r2\"{lane} offset=\"0s\" name=\"c{depth}\" duration=\"1s\">"
        ));
    }
    clips.push_str(&"</asset-clip>".repeat(40));
    let text = format!(
        r#"<fcpxml version="1.10"><resources><format id="r1" frameDuration="1/24s"/>
<asset id="r2" name="A" hasVideo="1" format="r1" src="file:///m/A.mov"/></resources>
<library><event><project name="P"><sequence format="r1"><spine>{clips}</spine></sequence></project></event></library></fcpxml>"#
    );
    let read = read(&text, R24, "fallback").expect("reads");
    assert_eq!(
        read.skipped,
        ["c33 at 00:00:00:00: nested deeper than 32, not read"]
    );
    let names: Vec<&str> = read
        .cut
        .lanes
        .iter()
        .flat_map(|lane| &lane.events)
        .map(|event| event.name.as_str())
        .collect();
    let expected: Vec<String> = (0..=32).map(|depth| format!("c{depth}")).collect();
    assert_eq!(names, expected);
}

#[test]
fn malformed_files_are_refused() {
    let err = read("plain text", R24, "x").expect_err("not XML");
    assert!(err.starts_with("the file is not XML: "), "{err}");
    for (text, expected) in [
        (
            "<!DOCTYPE fcpxml [<!ENTITY lol \"lol\">]><fcpxml version=\"1.10\"/>",
            "the file declares XML entities, which are not read",
        ),
        ("<fcp/>", "the file holds no fcpxml element"),
        (
            "<fcpxml version=\"2.0\"/>",
            "the file is not FCPXML version 1",
        ),
        ("<fcpxml version=\"1.10\"/>", "the file holds no project"),
        (
            "<fcpxml version=\"1.10\"><library><event><project name=\"P\"/></event></library></fcpxml>",
            "the project holds no sequence",
        ),
    ] {
        assert_eq!(read(text, R24, "x").expect_err(text), expected);
    }
}
