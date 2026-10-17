//! What an update asks for beyond its grant, and how the prompt words it.
//!
//! The class this closes: an update that widens a package's reach without the
//! host noticing. Every field of [`Permissions`] is widened one at a time
//! below; a field added to the struct without a case here fails
//! `every_field_is_widened_by_a_case`.

use std::path::PathBuf;

use super::*;

fn all() -> Permissions {
    Permissions {
        project: Access::ReadWrite,
        media: Access::Read,
        gpu: true,
        net: vec!["api.example.com".into()],
        fs: vec![
            FsScope::Project,
            FsScope::Pick,
            FsScope::Path("/srv/luts".into()),
        ],
        process: vec!["ffprobe".into()],
        clipboard: true,
    }
}

/// One widening per field, each over an empty grant.
fn widenings() -> Vec<(&'static str, Permissions)> {
    let none = Permissions::default();
    vec![
        (
            "project",
            Permissions {
                project: Access::Read,
                ..none.clone()
            },
        ),
        (
            "media",
            Permissions {
                media: Access::Read,
                ..none.clone()
            },
        ),
        (
            "gpu",
            Permissions {
                gpu: true,
                ..none.clone()
            },
        ),
        (
            "net",
            Permissions {
                net: vec!["api.example.com".into()],
                ..none.clone()
            },
        ),
        (
            "fs",
            Permissions {
                fs: vec![FsScope::Pick],
                ..none.clone()
            },
        ),
        (
            "process",
            Permissions {
                process: vec!["ffprobe".into()],
                ..none.clone()
            },
        ),
        (
            "clipboard",
            Permissions {
                clipboard: true,
                ..none
            },
        ),
    ]
}

#[test]
fn every_field_is_widened_by_a_case() {
    // Destructured without `..`, so a new field does not compile until it has
    // a case in `widenings`.
    let Permissions {
        project: _,
        media: _,
        gpu: _,
        net: _,
        fs: _,
        process: _,
        clipboard: _,
    } = Permissions::default();
    assert_eq!(widenings().len(), 7);
}

#[test]
fn each_widening_is_reported_alone_and_a_matching_grant_covers_it() {
    for (field, asked) in widenings() {
        assert_eq!(
            asked.beyond(&Permissions::default()),
            asked,
            "{field} was not reported over an empty grant"
        );
        assert!(
            asked.beyond(&asked).is_empty(),
            "{field} not covered by itself"
        );
        assert!(
            asked.beyond(&all()).is_empty(),
            "{field} not covered by everything"
        );
        assert_eq!(asked.describe().len(), 1, "{field}: {:?}", asked.describe());
    }
}

#[test]
fn a_narrower_or_equal_request_needs_nothing_more() {
    let granted = all();
    let narrower = Permissions {
        project: Access::Read,
        fs: vec![FsScope::Path("/srv/luts/film".into())],
        ..Permissions::default()
    };
    assert!(narrower.beyond(&granted).is_empty());
    assert!(granted.beyond(&granted).is_empty());
    assert!(Permissions::default().is_empty());
    assert!(!granted.is_empty());
}

#[test]
fn only_the_added_parts_are_reported() {
    let granted = Permissions {
        project: Access::Read,
        net: vec!["api.example.com".into()],
        fs: vec![FsScope::Path("/srv/luts".into())],
        ..Permissions::default()
    };
    let asked = Permissions {
        project: Access::ReadWrite,
        net: vec!["api.example.com".into(), "cdn.example.com".into()],
        fs: vec![
            FsScope::Path("/srv/luts/film".into()),
            FsScope::Path("/srv/other".into()),
        ],
        ..Permissions::default()
    };
    assert_eq!(
        asked.beyond(&granted),
        Permissions {
            project: Access::ReadWrite,
            net: vec!["cdn.example.com".into()],
            fs: vec![FsScope::Path(PathBuf::from("/srv/other"))],
            ..Permissions::default()
        }
    );
}

#[test]
fn the_prompt_lists_every_permission_in_order() {
    assert_eq!(
        all().describe(),
        [
            "Read and edit the open project",
            "Read and decode the project's media",
            "Run work on the GPU",
            "Connect to api.example.com",
            "Read and write files in the project folder",
            "Read and write files you pick",
            "Read and write /srv/luts",
            "Run ffprobe",
            "Read the clipboard",
        ]
    );
    assert!(Permissions::default().describe().is_empty());
}

/// A grant record survives a round trip, and one from a newer build with a
/// field this build does not know grants nothing.
#[test]
fn a_grant_record_round_trips_and_an_unknown_field_is_refused() {
    let json = serde_json::to_string(&all()).expect("serializes");
    let back: Permissions = serde_json::from_str(&json).expect("parses");
    assert_eq!(back, all());
    assert_eq!(
        serde_json::from_str::<Permissions>("{}").expect("parses"),
        Permissions::default()
    );
    assert!(serde_json::from_str::<Permissions>(r#"{"camera":true}"#).is_err());
}
