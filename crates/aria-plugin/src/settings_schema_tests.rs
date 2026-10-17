//! Contracts for declared settings.
//!
//! Closes the class "a field is declared that Settings cannot draw or that
//! writes over another's key": every refusal reason goes through
//! `register_settings`, the one entry point a plugin of any tier declares
//! through, and every kind's value check is driven with accepted and refused
//! text. Drawing the rows is the app's, and its tests own it.

use std::sync::Arc;

use super::*;
use crate::plugin::{Plugin, PluginManifest};

fn field(key: &str, kind: SettingKind) -> SettingSpec {
    SettingSpec {
        key: key.to_owned(),
        label: "Field".to_owned(),
        placeholder: String::new(),
        kind,
        default: None,
    }
}

fn section(plugin: &str, fields: Vec<SettingSpec>) -> SettingsSection {
    SettingsSection {
        plugin: plugin.to_owned(),
        title: "Notes".to_owned(),
        fields,
    }
}

/// A host context with the schema a settings host mounts. The registration
/// keeps the schema registered.
fn host() -> (PluginContext, Arc<SettingsSchema>, Registration) {
    let ctx = PluginContext::new();
    let schema = Arc::new(SettingsSchema::default());
    let held = ctx.register_arc(Arc::clone(&schema));
    (ctx, schema, held)
}

fn refusal(result: Result<Registration, PluginError>) -> String {
    match result {
        Err(PluginError::SettingsRefused(reason)) => reason,
        other => panic!("expected a settings refusal, got {other:?}"),
    }
}

#[test]
fn every_kind_stores_what_it_accepts_and_refuses_the_rest() {
    let number = SettingKind::Number {
        min: -1.0,
        max: 2.5,
        step: 0.5,
    };
    let choice = SettingKind::Choice(vec!["Fast".to_owned(), "Best".to_owned()]);
    let accepted: [(&SettingKind, &str, &str); 12] = [
        (&SettingKind::Text, "  a b ", "a b"),
        (&SettingKind::Secret, "sk-1", "sk-1"),
        (&SettingKind::Toggle, "On", "true"),
        (&SettingKind::Toggle, "yes", "true"),
        (&SettingKind::Toggle, "FALSE", "false"),
        (&SettingKind::Toggle, "off", "false"),
        (&number, "2.5", "2.5"),
        (&number, "-1", "-1"),
        (&number, " 0.25 ", "0.25"),
        (&choice, "best", "Best"),
        (&choice, "Fast", "Fast"),
        (&choice, "", ""),
    ];
    for (kind, text, stored) in accepted {
        assert_eq!(kind.check(text).as_deref(), Ok(stored), "{kind:?} {text:?}");
    }
    let refused: [(&SettingKind, &str, &str); 6] = [
        (&SettingKind::Toggle, "maybe", "maybe is not on or off"),
        (&number, "2.6", "2.6 is above 2.5"),
        (&number, "-1.01", "-1.01 is below -1"),
        (&number, "NaN", "NaN is not a number"),
        (&number, "inf", "inf is not a number"),
        (&choice, "Good", "Good is not one of Fast, Best"),
    ];
    for (kind, text, reason) in refused {
        assert_eq!(
            kind.check(text),
            Err(reason.to_owned()),
            "{kind:?} {text:?}"
        );
    }
}

#[test]
fn a_field_settings_cannot_draw_is_refused_with_the_reason() {
    let number = |min: f64, max: f64, step: f64| SettingKind::Number { min, max, step };
    let cases = [
        (field("", SettingKind::Text), "a setting needs a key"),
        (
            SettingSpec {
                label: " ".to_owned(),
                ..field("host.notes.a", SettingKind::Text)
            },
            "host.notes.a: the setting needs a label",
        ),
        (
            field("host.notes.a", number(2.0, 1.0, 1.0)),
            "host.notes.a: the range 2 to 1 holds nothing",
        ),
        (
            field("host.notes.a", number(f64::NAN, 1.0, 1.0)),
            "host.notes.a: the range NaN to 1 holds nothing",
        ),
        (
            field("host.notes.a", number(0.0, 1.0, 0.0)),
            "host.notes.a: the step 0 is not above zero",
        ),
        (
            field("host.notes.a", SettingKind::Choice(Vec::new())),
            "host.notes.a: the choice has no options",
        ),
        (
            field(
                "host.notes.a",
                SettingKind::Choice(vec!["A".to_owned(), " ".to_owned()]),
            ),
            "host.notes.a: an option is empty",
        ),
        (
            field(
                "host.notes.a",
                SettingKind::Choice(vec!["A".to_owned(), "a".to_owned()]),
            ),
            "host.notes.a: the option a is listed twice",
        ),
        (
            SettingSpec {
                default: Some("7".to_owned()),
                ..field("host.notes.a", number(0.0, 1.0, 0.5))
            },
            "host.notes.a: the default is refused: 7 is above 1",
        ),
    ];
    for (spec, reason) in cases {
        let (ctx, schema, _host) = host();
        assert_eq!(
            refusal(ctx.register_settings(section("host.notes", vec![spec]))),
            reason
        );
        assert!(
            schema.sections().is_empty(),
            "{reason}: a refused section was kept"
        );
    }
}

#[test]
fn a_key_is_claimed_once_across_every_section() {
    let (ctx, schema, _host) = host();
    let a = field("host.a.key", SettingKind::Text);
    assert_eq!(
        refusal(ctx.register_settings(section("host.a", vec![a.clone(), a.clone()]))),
        "host.a.key: the setting is declared twice"
    );
    let _held = ctx
        .register_settings(section("host.a", vec![a.clone()]))
        .expect("first section");
    assert_eq!(
        refusal(ctx.register_settings(section("host.b", vec![a]))),
        "host.a.key is declared by host.a"
    );
    assert_eq!(
        refusal(ctx.register_settings(section(
            "host.a",
            vec![field("host.a.other", SettingKind::Toggle)]
        ))),
        "host.a already has a settings section"
    );
    assert_eq!(schema.sections().len(), 1);
    let (plugin, spec) = schema.field("host.a.key").expect("declared");
    assert_eq!(plugin, "host.a");
    assert_eq!(spec.kind, SettingKind::Text);
    assert!(schema.field("host.a.other").is_none());
}

/// A plugin that declares one section while it mounts.
struct Declares(PluginManifest, SettingsSection);

impl Plugin for Declares {
    fn manifest(&self) -> &PluginManifest {
        &self.0
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_settings(self.1.clone()).map(drop)
    }
}

/// None: the schema is the host's, reached without the plugin declaring it.
const USES: &[&str] = &[];

fn declares(id: &'static str, fields: Vec<SettingSpec>) -> Declares {
    Declares(
        PluginManifest::new(id, "Notes", "1.0.0", &[], USES),
        // The section names another plugin: a plugin's view declares its own.
        section("com.other.plugin", fields),
    )
}

#[test]
fn a_plugin_declares_its_own_keys_and_they_leave_with_it() {
    let (ctx, schema, _host) = host();
    ctx.mount(&declares(
        "com.example.notes",
        vec![
            SettingSpec {
                default: Some("sk-default".to_owned()),
                ..field("com.example.notes.token", SettingKind::Secret)
            },
            field("com.example.notes.fast", SettingKind::Toggle),
        ],
    ))
    .expect("mounts");
    let sections = schema.sections();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].plugin, "com.example.notes");
    assert!(sections[0].fields[0].secret());
    assert!(!sections[0].fields[1].secret());
    let listed: Vec<String> = ctx
        .mounted()
        .iter()
        .flat_map(|info| info.contributions.iter().map(ToString::to_string))
        .collect();
    assert_eq!(listed, ["settings Notes"]);
    ctx.unmount("com.example.notes").expect("unmounts");
    assert!(
        schema.sections().is_empty(),
        "the section outlived its plugin"
    );
}

#[test]
fn a_plugin_key_outside_its_id_fails_the_mount() {
    for key in [
        "com.other.plugin.token",
        "com.example.notes",
        "com.example.notes.",
    ] {
        let (ctx, schema, _host) = host();
        let err = ctx
            .mount(&declares(
                "com.example.notes",
                vec![field(key, SettingKind::Text)],
            ))
            .expect_err("a key outside the plugin id");
        assert!(
            err.to_string().contains(&format!(
                "{key}: a setting of com.example.notes sits under com.example.notes."
            )),
            "{key}: {err}"
        );
        assert!(schema.sections().is_empty());
    }
}

#[test]
fn with_no_settings_host_nothing_is_held() {
    let ctx = PluginContext::new();
    let held = ctx
        .register_settings(section(
            "host.a",
            vec![field("host.a.key", SettingKind::Text)],
        ))
        .expect("no host is not an error");
    assert!(held.contribution().is_none());
}
