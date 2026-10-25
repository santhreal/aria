//! A clip type that declares its fields: listed once, placed, edited through
//! the inspector as one undo step, shown on the timeline, saved and reopened
//! with the value, keyed, and drawn by its callback.

use aria_compositor::{CpuFrameOut, EffectParams};
use aria_models::ClipParam;
use aria_plugin::{ClipTypeDefinition, ClipTypeRender};
use aria_ui::app::effects_browser::EntryKind;
use aria_ui::app::property_rows::{PropertyGroup, TimelineOutlineRow, TimelinePropertyRow};
use aria_ui::surfaces::effects_rack::{self, Gesture};
use aria_ui::types::UiSlots as _;

use super::{
    AppState, ClipTypeSpec, EffectParamKind, EffectParamSpec, MarkerColor, Offered, PluginContext,
    assert_generator_clip, clip_count, clip_type_row, in_generator_grid, only_clip, state,
};

/// A scalar field `level`, 0 to 1, 0.25 by default.
fn level_field() -> EffectParamSpec {
    EffectParamSpec {
        name: "level".into(),
        label: "Level".into(),
        kind: EffectParamKind::Scalar,
        default: 0.25,
        min: 0.0,
        max: 1.0,
        ..Default::default()
    }
}

/// Fill the frame with opaque grey at the `level` field.
fn grey(fields: &EffectParams<'_>, _time: f32, frame: &mut CpuFrameOut<'_>) -> Result<(), String> {
    let level = fields
        .scalar("level")
        .ok_or_else(|| "no level field".to_owned())?;
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            frame.set_pixel_f32(x, y, [level, level, level, 1.0]);
        }
    }
    Ok(())
}

/// The timeline property row for the selected clip's first field, with the
/// clip's Effects group open.
fn field_row(state: &mut AppState) -> TimelinePropertyRow {
    let clip = only_clip(state).id;
    let track = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.clips.iter().any(|ui| ui.id == clip.raw()))
        .expect("the clip's track is projected")
        .id;
    let field = ClipParam::Effect {
        effect: 0,
        param: 0,
    };
    if !state.visible_params_for_clip(clip).contains(&field) {
        state.toggle_property_group(clip, PropertyGroup::Effects);
    }
    state
        .track_outline_rows(track)
        .into_iter()
        .find_map(|row| match row {
            TimelineOutlineRow::Property(row) if row.param == field => Some(row),
            _ => None,
        })
        .expect("the timeline rows draw the field")
}

/// The centre pixel of the frame at the playhead.
fn centre(state: &AppState) -> [u8; 4] {
    let mut engine = state.engine().lock();
    let frame = engine.get_frame(state.playhead).expect("the frame renders");
    frame.pixel(frame.width / 2, frame.height / 2)
}

fn assert_grey(pixel: [u8; 4], level: u8) {
    assert!(
        pixel[..3].iter().all(|c| c.abs_diff(level) <= 2) && pixel[3] == 255,
        "expected opaque gray {level}, drew {pixel:?}"
    );
}

#[test]
fn a_defined_clip_type_is_placed_edited_saved_reloaded_and_drawn_by_its_callback() {
    let mut state = state("defined-clip-type");
    state.new_project().expect("a project");
    state.playhead.value = 0;
    let id = "test.ui.meter";
    let plugin = PluginContext::new();
    let definition = ClipTypeDefinition::new(
        ClipTypeSpec::new(id, "Meter", "", MarkerColor::Green),
        vec![level_field()],
        ClipTypeRender::cpu(grey),
    );
    let registration = plugin
        .define_clip_type(definition)
        .expect("the type is defined");

    // Listed once, as the type: its own generator is not a second grid row.
    let row = clip_type_row(id).expect("the grid lists the type");
    assert_eq!(row.unavailable, None);
    assert!(
        !in_generator_grid(id),
        "the type's own generator is listed beside the type"
    );
    let panel_rows: Vec<EntryKind> = aria_ui::catalog_text::effect_entries()
        .iter()
        .filter(|entry| entry.id == id)
        .map(|entry| entry.kind)
        .collect();
    assert_eq!(
        panel_rows,
        [EntryKind::ClipType],
        "the Effects panel lists the type's generator beside the type"
    );
    let offers: Vec<bool> = state
        .palette_catalog()
        .iter()
        .filter(|offer| offer.id == id)
        .map(|offer| {
            matches!(
                offer.action,
                Offered::ClipType {
                    placeable: true,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(offers, [true], "the palette does not offer the type once");

    state.add_clip_type(id).expect("places the type");
    assert_generator_clip(&state, id, "Meter", 0);
    let placed = only_clip(&state);
    assert_eq!(placed.clip_type.as_deref(), Some(id));
    state.select_clip(placed.id);
    let shown = state.selected_ui_clip().expect("selected");
    assert_eq!(shown.effects[0].name.as_ref(), "Meter");
    assert_eq!(
        shown.effects[0].scalar(0),
        Some(0.25),
        "the field's default"
    );
    assert_eq!(field_row(&mut state).label.as_ref(), "Meter Level");
    assert_grey(centre(&state), 64);

    // The inspector's stepper, one undo step.
    let step = Gesture::NudgeScalar {
        index: 0,
        slot: 0,
        by: 0.25,
    };
    effects_rack::apply(&mut state, step).expect("the field is edited");
    assert_eq!(state.selected_effect_param(0, 0), Some(0.5));
    assert_eq!(field_row(&mut state).value_label.as_ref(), "0.50");
    assert!(state.undo(), "the edit recorded no undo step");
    assert_eq!(
        state.selected_effect_param(0, 0),
        Some(0.25),
        "one undo did not restore the field"
    );
    assert!(state.redo(), "the edit cannot be redone");
    assert_eq!(state.selected_effect_param(0, 0), Some(0.5));
    assert_grey(centre(&state), 128);

    state.save_project().expect("saves");
    let path = state.project_path.clone().expect("a saved path");
    state.new_project().expect("a new project");
    assert_eq!(clip_count(&state), 0);
    state.open_project(path).expect("reopens");
    let reopened = only_clip(&state);
    assert_eq!(reopened.clip_type.as_deref(), Some(id));
    state.select_clip(reopened.id);
    assert_eq!(
        state.selected_effect_param(0, 0),
        Some(0.5),
        "the saved field value did not come back"
    );
    assert_grey(centre(&state), 128);

    // The field keys like any effect parameter.
    state
        .add_keyframe_at_playhead(ClipParam::Effect {
            effect: 0,
            param: 0,
        })
        .expect("the field keys");
    assert!(
        field_row(&mut state).keyframed,
        "the field took no keyframe"
    );

    drop(registration);
    assert_eq!(clip_type_row(id), None, "the grid kept a removed type");
    assert_eq!(
        aria_models::effect_spec(id),
        None,
        "the generator outlived the type"
    );
}
