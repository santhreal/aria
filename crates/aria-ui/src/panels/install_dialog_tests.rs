//! The install prompt's text: its title, the package, its tier and publisher,
//! what it replaces, and each permission it asks for.
//!
//! Not covered: which packages reach the prompt and what an answer does to the
//! plugin folder, which `app/plugin_install_tests.rs` covers.

use super::PromptLine::{Permissions, Text};
use super::*;
use crate::commands::casing_tests::check_title_case;

/// A WASM package with no permissions that replaces nothing.
fn looks() -> InstallPrompt {
    InstallPrompt {
        name: "Looks".into(),
        id: "com.example.looks".into(),
        publisher: "Example".into(),
        version: "2.0.0".into(),
        replaces: None,
        tier: "WASM",
        permissions: Vec::new(),
        native: false,
        waiting: 0,
    }
}

#[test]
fn the_native_warning_states_that_native_code_reaches_beyond_its_permissions() {
    assert!(
        NATIVE_WARNING.contains("beyond"),
        "the warning must say a native package reaches beyond its permissions"
    );
}

#[test]
fn the_prompt_lines_name_the_package_and_each_permission() {
    let mut prompt = looks();
    assert_eq!(
        lines(&prompt),
        [
            Text("Looks 2.0.0".into()),
            Text("WASM plugin from Example, id com.example.looks".into()),
            Text("It asks for no permissions.".into()),
        ]
    );
    prompt.permissions = vec!["Run work on the GPU".into(), "Connect to a.example".into()];
    prompt.waiting = 2;
    assert_eq!(
        lines(&prompt)[2..],
        [
            Text("It asks for:".into()),
            Permissions(&prompt.permissions),
            Text("2 more waiting after this one.".into()),
        ]
    );
    prompt.replaces = Some("1.0.0".into());
    assert_eq!(
        lines(&prompt)[2..4],
        [
            Text("Replaces the installed version 1.0.0.".into()),
            Text("This version also asks for:".into()),
        ]
    );
}

/// The title reads Update Plugin only over a package that replaces an
/// installed version, and each title is in Title Case. The drawn title of the
/// dialog with no prompt is read by `chrome::dialog_tests`; the update title
/// is drawn only over a staged archive, so it is checked here.
#[test]
fn the_prompt_title_states_install_or_update_in_title_case() {
    let mut prompt = looks();
    assert_eq!(title(None), "Install Plugin");
    assert_eq!(title(Some(&prompt)), "Install Plugin");
    prompt.replaces = Some("1.0.0".into());
    assert_eq!(title(Some(&prompt)), "Update Plugin");
    for drawn in [title(None), title(Some(&prompt))] {
        check_title_case(drawn).unwrap_or_else(|reason| panic!("{reason}"));
    }
}
