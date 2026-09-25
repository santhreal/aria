//! `inverse_edits` tests covering generators. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `inverse_edits` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn undo_redo_set_generator_oscillates_content() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Text, "T1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text("before"),
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::SetGenerator {
            clip: clip_id,
            generator: Generator::text("after"),
        }))
        .expect("set generator");

    let content =
        |engine: &aria_engine::Engine| match &engine.project().clip(clip_id).unwrap().content {
            aria_models::ClipSource::Generated(Generator::Text { content, .. }) => content.clone(),
            other => panic!("expected text generator, got {other:?}"),
        };
    assert_eq!(content(&engine), "after");

    assert!(engine.undo());
    assert_eq!(content(&engine), "before");

    assert!(engine.redo());
    assert_eq!(content(&engine), "after");
}

#[test]
fn undo_redo_set_generator_oscillates_style() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Text, "T1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text("hi"),
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    let styled = aria_models::TextStyle {
        bold: true,
        size: 120.0.into(),
        fill: [10, 20, 30, 255].into(),
        stroke: Some(aria_models::TextStroke {
            rgba: [0, 0, 0, 255].into(),
            width: 8.0.into(),
        }),
        ..Default::default()
    };
    engine
        .apply(Command::Edit(EditCommand::SetGenerator {
            clip: clip_id,
            generator: Generator::Text {
                content: "hi".into(),
                style: styled.clone(),
            },
        }))
        .expect("set generator");

    let style =
        |engine: &aria_engine::Engine| match &engine.project().clip(clip_id).unwrap().content {
            aria_models::ClipSource::Generated(Generator::Text { style, .. }) => style.clone(),
            other => panic!("expected text generator, got {other:?}"),
        };

    assert_eq!(style(&engine), styled);

    assert!(engine.undo());
    assert_eq!(style(&engine), aria_models::TextStyle::default());

    assert!(engine.redo());
    assert_eq!(style(&engine), styled);
}

#[test]
fn set_generator_rejects_media_clip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    // A media-backed clip has no generator to replace.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetGenerator {
                clip: clip_id,
                generator: Generator::text("nope"),
            }))
            .is_err()
    );
}
