#![no_main]

use aria_models::{AnimationSpec, FilterSpec, SpeedPresetSpec, TextEffectSpec};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

#[derive(Deserialize)]
struct Catalog<T> {
    entry: Vec<T>,
}

fn parse<T>(text: &str, id: fn(&T) -> &str, label: fn(&T) -> &str)
where
    T: for<'de> Deserialize<'de>,
{
    if let Ok(catalog) = toml::from_str::<Catalog<T>>(text) {
        let _ = aria_models::catalog::unique_ids(&catalog.entry, id, label);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&catalog, body)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(body) else {
        return;
    };

    match catalog % 4 {
        0 => parse::<FilterSpec>(text, |entry| &entry.id, |entry| &entry.label),
        1 => parse::<AnimationSpec>(text, |entry| &entry.id, |entry| &entry.label),
        2 => parse::<SpeedPresetSpec>(text, |entry| &entry.id, |entry| &entry.label),
        _ => parse::<TextEffectSpec>(text, |entry| &entry.id, |entry| &entry.label),
    }
});
