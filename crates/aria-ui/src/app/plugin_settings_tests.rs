//! Plugin settings in the Settings dialog: declared, drawn, checked, saved to
//! the file, and read back with their defaults.
//!
//! The class closed here is a declared field the dialog shows and the file does
//! not keep, keeps in a form its kind refuses, or keeps after the plugin left.
//! Every kind goes through the one write path, `set_plugin_setting`, and is read
//! back from the dialog's reader, the plugin's reader (`SettingsService`), and
//! the file on disk. `samples` matches `SettingKind` without a wildcard, so a
//! kind added later does not compile here until it has cases.
//!
//! Not covered: drawing the chips and rows, which needs a window.

use std::path::{Path, PathBuf};

use aria_plugin::{
    PluginContext, Registration, SettingKind, SettingSpec, SettingsSection, SettingsService,
};

use super::SettingsField;
use crate::app::AppState;

/// A state whose settings plugin writes `settings.toml` in its own directory.
fn state() -> (AppState, PathBuf) {
    let dir = crate::scratch_pool::scratch("plugin-settings");
    let path = dir.join("settings.toml");
    let ctx = PluginContext::new();
    ctx.mount(&crate::plugins::SettingsPlugin { path: path.clone() })
        .expect("the settings plugin mounts");
    (AppState::new(ctx, dir).expect("state"), path)
}

fn spec(key: &str, label: &str, kind: SettingKind, default: Option<&str>) -> SettingSpec {
    SettingSpec {
        key: key.into(),
        label: label.into(),
        placeholder: String::new(),
        kind,
        default: default.map(str::to_owned),
    }
}

fn declare(state: &AppState, fields: Vec<SettingSpec>) -> Registration {
    state
        .ctx
        .register_settings(SettingsSection {
            plugin: "example.notes".into(),
            title: "Notes".into(),
            fields,
        })
        .expect("the section registers")
}

/// What the plugin reads for `key`.
fn served(state: &AppState, key: &str) -> Option<serde_json::Value> {
    state.ctx.get::<dyn SettingsService>().get(key)
}

/// What the file on disk holds for `key`.
fn on_disk(path: &Path, key: &str) -> Option<String> {
    aria_settings::load(path)
        .expect("the file loads")
        .plugin_value(key)
        .map(str::to_owned)
}

/// Input the kind stores as the second text, and input it refuses with the
/// second text.
type Samples = (
    Vec<(&'static str, &'static str)>,
    Vec<(&'static str, &'static str)>,
);

fn samples(kind: &SettingKind) -> Samples {
    match kind {
        SettingKind::Text | SettingKind::Secret => {
            (vec![("  ~/notes  ", "~/notes"), ("a b", "a b")], vec![])
        }
        SettingKind::Toggle => (
            vec![("On", "true"), ("off", "false"), ("YES", "true")],
            vec![("maybe", "maybe is not on or off")],
        ),
        SettingKind::Number { .. } => (
            vec![("0.25", "0.25"), ("1", "1"), ("0", "0")],
            vec![
                ("1.5", "1.5 is above 1"),
                ("-1", "-1 is below 0"),
                ("half", "half is not a number"),
                ("NaN", "NaN is not a number"),
            ],
        ),
        SettingKind::Choice(_) => (
            vec![("WARM", "warm"), ("cool", "cool")],
            vec![("hot", "hot is not one of warm, cool")],
        ),
    }
}

fn every_kind() -> Vec<SettingSpec> {
    vec![
        spec("example.notes.dir", "Folder", SettingKind::Text, None),
        spec("example.notes.token", "Token", SettingKind::Secret, None),
        spec("example.notes.sync", "Sync", SettingKind::Toggle, None),
        spec(
            "example.notes.mix",
            "Mix",
            SettingKind::Number {
                min: 0.0,
                max: 1.0,
                step: 0.05,
            },
            None,
        ),
        spec(
            "example.notes.tone",
            "Tone",
            SettingKind::Choice(vec!["warm".into(), "cool".into()]),
            None,
        ),
    ]
}

#[test]
fn every_kind_saves_what_it_accepts_and_refuses_the_rest() {
    let (mut state, path) = state();
    let fields = every_kind();
    let _section = declare(&state, fields.clone());

    for field in &fields {
        let (accepted, refused) = samples(&field.kind);
        for (input, stored) in accepted {
            let label = state
                .set_plugin_setting(&field.key, input)
                .unwrap_or_else(|err| panic!("{}: {input:?} was refused: {err}", field.key));
            assert_eq!(label, field.label.as_str());
            assert_eq!(state.plugin_setting(&field.key), stored, "{input:?}");
            assert_eq!(
                served(&state, &field.key),
                Some(serde_json::Value::String(stored.to_owned())),
                "the plugin reads another value than the dialog"
            );
            assert_eq!(on_disk(&path, &field.key).as_deref(), Some(stored));
        }
        let kept = state.plugin_setting(&field.key);
        for (input, reason) in refused {
            assert_eq!(
                state.set_plugin_setting(&field.key, input),
                Err(format!("{}: {reason}", field.label)),
            );
            assert_eq!(state.plugin_setting(&field.key), kept, "a refusal wrote");
        }
        state
            .set_plugin_setting(&field.key, "  ")
            .expect("an empty value clears");
        assert_eq!(served(&state, &field.key), None);
        assert_eq!(on_disk(&path, &field.key), None);
    }
}

/// An unset key reads as the declared default, for the dialog and for the
/// plugin, and clearing a value brings the default back.
#[test]
fn an_unset_field_reads_as_its_default() {
    let (mut state, path) = state();
    let key = "example.notes.mix";
    let _section = declare(
        &state,
        vec![spec(
            key,
            "Mix",
            SettingKind::Number {
                min: 0.0,
                max: 1.0,
                step: 0.05,
            },
            Some("0.5"),
        )],
    );
    assert_eq!(state.plugin_setting(key), "0.5");
    assert_eq!(
        served(&state, key),
        Some(serde_json::Value::String("0.5".into()))
    );
    assert_eq!(
        on_disk(&path, key),
        None,
        "a default is not written to the file"
    );

    state.set_plugin_setting(key, "0.75").expect("in range");
    assert_eq!(
        served(&state, key),
        Some(serde_json::Value::String("0.75".into()))
    );
    state.set_plugin_setting(key, "").expect("cleared");
    assert_eq!(state.plugin_setting(key), "0.5");
}

/// The typed row: declared, focused, typed, committed, read back. A draft its
/// kind refuses keeps the focus and the draft so it can be corrected.
#[test]
fn a_typed_plugin_field_round_trips_through_the_dialog() {
    let (mut state, path) = state();
    let mut folder = spec(
        "example.notes.dir",
        "Notes directory",
        SettingKind::Text,
        None,
    );
    folder.placeholder = "the project directory".into();
    let mix = spec(
        "example.notes.mix",
        "Mix",
        SettingKind::Number {
            min: 0.0,
            max: 1.0,
            step: 0.05,
        },
        None,
    );
    let _section = declare(&state, vec![folder, mix]);

    let titles: Vec<String> = state
        .plugin_settings_sections()
        .iter()
        .map(|section| section.title.clone())
        .collect();
    assert_eq!(titles, ["Notes"], "a declared section is not offered");

    let field = SettingsField::Plugin(crate::interned::intern("example.notes.dir"));
    let look = state.setting_look(field).expect("the field is declared");
    assert_eq!(look.label, "Notes directory");
    assert_eq!(look.placeholder, "the project directory");
    assert_eq!(look.kind, SettingKind::Text);
    let number = SettingsField::Plugin(crate::interned::intern("example.notes.mix"));
    assert_eq!(
        state.setting_look(number).map(|look| look.placeholder),
        Some("0 to 1".into()),
        "a number with no placeholder states its range"
    );

    state.focus_setting(Some(field));
    for ch in "/srv/notes".chars() {
        state.setting_push(ch);
    }
    state.commit_setting().expect("a plugin value is savable");
    assert_eq!(state.settings_form.focused, None);
    assert_eq!(state.setting_draft(field).0, "/srv/notes");
    assert_eq!(
        state.status_message.as_deref(),
        Some("Notes directory saved")
    );
    assert_eq!(
        on_disk(&path, "example.notes.dir").as_deref(),
        Some("/srv/notes")
    );

    state.focus_setting(Some(number));
    state.setting_push('7');
    assert_eq!(state.commit_setting(), Err("Mix: 7 is above 1".to_owned()));
    assert_eq!(state.settings_form.focused, Some(number));
    assert_eq!(state.settings_form.draft, "7");
}

/// A field leaves with the plugin that declared it: the dialog stops drawing
/// it, a write is refused, and the stored value no longer reads as a default.
#[test]
fn a_field_leaves_with_its_registration() {
    let (mut state, _) = state();
    let key = "example.notes.sync";
    let section = declare(
        &state,
        vec![spec(key, "Sync", SettingKind::Toggle, Some("true"))],
    );
    assert_eq!(state.plugin_setting(key), "true");
    drop(section);

    assert!(state.plugin_settings_sections().is_empty());
    let field = SettingsField::Plugin(crate::interned::intern(key));
    assert_eq!(state.setting_look(field), None);
    assert_eq!(
        state.set_plugin_setting(key, "false"),
        Err(format!("{key}: no mounted plugin declares this setting"))
    );
    assert_eq!(served(&state, key), None, "a default outlived its field");
}
