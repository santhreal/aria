//! Contracts for AI generation.
//!
//! Nothing here reaches fal.ai, and nothing here submits with a key: a test that queued
//! a real request would charge an account and depend on a network. What is asserted is
//! everything that decides whether a request is made at all and what it says, because
//! those were the failure modes:
//!
//! - No key means no request, and the refusal names the provider and where the key goes.
//!   A queued job that dies on the first HTTP round trip reports nothing useful.
//! - No prompt means no request. An empty prompt against a paid provider is a charge for
//!   nothing.
//! - Two identical prompts are two generations. Sharing an idempotency key makes the
//!   provider hand back the first result, so the second attempt silently does nothing.
//! - The prompt box only takes keys while it holds focus, so the bare-letter tool
//!   shortcuts survive the tab being open.
//!
//! The kind table is enumerated at run time, so adding a kind fails here until it has a
//! label and an extension the importer accepts.

use aria_settings::{ProviderSettings, Settings};

use super::AppState;
use super::generate::{GENERATE_PROVIDER, GenerateKind};
use super::premiere_tests::with_clip;

/// Give one provider a literal key in the state's own config file.
fn with_provider_key(state: &AppState, name: &str, key: &str) {
    let path = state.config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("config dir");
    }
    let mut settings = aria_settings::load(&path).unwrap_or_default();
    settings.providers.insert(
        name.to_string(),
        ProviderSettings {
            api_key: Some(key.to_string()),
            api_key_env: None,
        },
    );
    aria_settings::save(&path, &settings).expect("write config");
}

fn without_providers(state: &AppState) {
    let path = state.config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("config dir");
    }
    aria_settings::save(&path, &Settings::default()).expect("write config");
}

#[test]
fn every_kind_has_a_label_and_an_importable_extension() {
    for (kind, label, ext) in GenerateKind::ALL {
        assert!(!label.is_empty(), "{kind:?} has no label");
        assert_eq!(kind.label(), label);
        assert_eq!(kind.fallback_extension(), ext);
        // A kind whose fallback is not a container the pool accepts would save a file
        // that imports as nothing.
        assert!(
            crate::app::media::import::MEDIA_IMPORT_EXTENSIONS.contains(&ext),
            "{kind:?} saves .{ext}, which the importer does not accept"
        );
    }
    // Labels must be distinct, or two chips select the same thing.
    let mut labels: Vec<&str> = GenerateKind::ALL
        .iter()
        .map(|(_, label, _)| *label)
        .collect();
    labels.sort_unstable();
    let count = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), count, "two kinds share a label");
}

#[test]
fn generation_is_configured_only_by_a_resolvable_key_for_this_provider() {
    let (state, _clip) = with_clip();
    without_providers(&state);
    assert!(
        !state.generation_configured(),
        "a config with no providers is not configured"
    );

    // A key for another provider does not unlock generation: the routing question is
    // per provider, not "some key exists".
    with_provider_key(&state, "pexels", "pexels-key");
    assert!(!state.generation_configured());

    with_provider_key(&state, GENERATE_PROVIDER, "fal-key");
    assert!(state.generation_configured());

    // A variable that is not set is not a key.
    let path = state.config_path();
    let mut settings = aria_settings::load(&path).unwrap_or_default();
    settings.providers.insert(
        GENERATE_PROVIDER.to_string(),
        ProviderSettings {
            api_key: None,
            api_key_env: Some("ARIA_TEST_FAL_KEY_THAT_IS_NOT_SET".into()),
        },
    );
    aria_settings::save(&path, &settings).expect("write config");
    assert!(
        !state.generation_configured(),
        "an unset environment variable is not a key"
    );
}

#[test]
fn the_prompt_box_only_takes_keys_while_focused() {
    let (mut state, _clip) = with_clip();

    state.generate_prompt_push('a');
    assert_eq!(state.generate.prompt, "", "unfocused typing is not text");

    state.focus_generate_prompt(true);
    for ch in "cat".chars() {
        state.generate_prompt_push(ch);
    }
    assert_eq!(state.generate.prompt, "cat");

    state.generate_prompt_pop();
    assert_eq!(state.generate.prompt, "ca");

    // Backspace past the start is not an error and not a panic.
    for _ in 0..4 {
        state.generate_prompt_pop();
    }
    assert_eq!(state.generate.prompt, "");

    state.focus_generate_prompt(false);
    state.generate_prompt_push('x');
    assert_eq!(state.generate.prompt, "");
}

#[test]
fn changing_kind_keeps_the_prompt() {
    let (mut state, _clip) = with_clip();
    state.focus_generate_prompt(true);
    for ch in "a dog".chars() {
        state.generate_prompt_push(ch);
    }
    state.set_generate_kind(GenerateKind::Video);
    assert_eq!(state.generate.kind, GenerateKind::Video);
    assert_eq!(
        state.generate.prompt, "a dog",
        "the same idea is generatable as an image or a video"
    );
}

#[test]
fn a_blank_prompt_and_a_missing_key_are_both_refused() {
    let (mut state, _clip) = with_clip();
    without_providers(&state);

    // Blank first: with neither a prompt nor a key, the prompt is the nearer fix.
    state.focus_generate_prompt(true);
    state.generate_prompt_push(' ');
    let err = state
        .generation_precheck()
        .expect_err("a blank prompt is refused");
    assert!(err.contains("Type what to generate"), "{err}");

    for ch in "a cat".chars() {
        state.generate_prompt_push(ch);
    }
    let err = state
        .generation_precheck()
        .expect_err("no key means no request");
    assert!(err.contains(GENERATE_PROVIDER), "{err}");
    assert!(err.contains("Settings"), "{err}");

    // Neither refusal costs anything: no job row, no consumed idempotency key.
    assert_eq!(state.job_view.len(), 0);
    assert_eq!(state.generation_seq, 0);

    // With a key the same prompt passes, and the key is what the request carries.
    with_provider_key(&state, GENERATE_PROVIDER, "fal-key");
    let (prompt, key) = state
        .generation_precheck()
        .expect("configured and prompted");
    assert_eq!(prompt, "a cat", "the prompt is trimmed, not reformatted");
    assert_eq!(key, "fal-key");
}

#[test]
fn each_submit_takes_its_own_idempotency_key() {
    let (mut state, _clip) = with_clip();
    // The counter is what keeps two identical prompts from collapsing into one
    // provider-side result, so it advances per attempt and never repeats.
    let keys: Vec<u64> = (0..3).map(|_| state.next_generation_key()).collect();
    assert_eq!(keys, vec![1, 2, 3]);
}
