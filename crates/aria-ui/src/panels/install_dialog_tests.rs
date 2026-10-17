//! The install prompt's text: the package, its tier and publisher, what it
//! replaces, and each permission it asks for.
//!
//! Not covered: which packages reach the prompt and what an answer does to the
//! plugin folder, which `app/plugin_install_tests.rs` covers.

use super::*;

#[test]
fn the_native_warning_states_that_native_code_reaches_beyond_its_permissions() {
    assert!(
        NATIVE_WARNING.contains("beyond"),
        "the warning must say a native package reaches beyond its permissions"
    );
}

#[test]
fn the_prompt_lines_name_the_package_and_each_permission() {
    let mut prompt = InstallPrompt {
        name: "Looks".into(),
        id: "com.example.looks".into(),
        publisher: "Example".into(),
        version: "2.0.0".into(),
        replaces: None,
        tier: "WASM",
        permissions: Vec::new(),
        native: false,
        waiting: 0,
    };
    assert_eq!(
        lines(&prompt),
        vec![
            "Looks 2.0.0",
            "WASM plugin from Example, id com.example.looks",
            "It asks for no permissions.",
        ]
    );
    prompt.permissions = vec!["Run work on the GPU".into(), "Connect to a.example".into()];
    prompt.waiting = 2;
    assert_eq!(
        lines(&prompt)[2..],
        [
            "It asks for:",
            "  Run work on the GPU",
            "  Connect to a.example",
            "2 more waiting after this one.",
        ]
    );
    prompt.replaces = Some("1.0.0".into());
    assert_eq!(
        lines(&prompt)[2..4],
        [
            "Replaces the installed version 1.0.0.",
            "This version also asks for:"
        ]
    );
}
