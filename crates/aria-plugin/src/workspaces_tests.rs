//! Plugin workspaces: what `register_workspace` accepts and refuses, how long a
//! registration lasts, and how a package's `[[workspaces]]` reach it.
//!
//! The class these close: a workspace the host cannot apply (a size that is not
//! a size, a name that breaks a menu row), a workspace that outlives its
//! plugin, and a declared table that is read with a typo in it. Applying a
//! workspace to the panes is covered in aria-ui.

use super::*;
use crate::package::{DataPlugin, MANIFEST_FILE, Package};
use crate::plugin::{Plugin, PluginManifest};

fn grading() -> WorkspaceSpec {
    WorkspaceSpec {
        id: "grading".into(),
        name: "Grading".into(),
        panes: WorkspacePanes {
            sidebar_collapsed: Some(true),
            inspector_width: Some(640.0),
            timeline_height: Some(180.0),
            scopes: Some(true),
            focus: Some(WorkspaceFocus::Preview),
            ..WorkspacePanes::default()
        },
    }
}

struct WorkspacePlugin {
    manifest: PluginManifest,
}

impl Plugin for WorkspacePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_workspace(grading())?;
        Ok(())
    }
}

fn plugin(id: &'static str) -> WorkspacePlugin {
    WorkspacePlugin {
        manifest: PluginManifest::new(id, "Color Room", "1.0.0", &[], &[]),
    }
}

fn refusal(result: Result<Registration, PluginError>) -> String {
    match result {
        Err(PluginError::WorkspaceRefused(reason)) => reason,
        Err(other) => panic!("refused as {other:?}, not a workspace refusal"),
        Ok(_) => panic!("registered"),
    }
}

#[test]
fn a_mounted_workspace_lists_under_its_key_until_the_plugin_unmounts() {
    let ctx = PluginContext::new();
    assert_eq!(ctx.workspaces_revision(), 0);
    ctx.mount(&plugin("com.example.color")).expect("mounts");
    let mounted = ctx.workspaces_revision();
    assert!(mounted > 0);
    let listed = ctx.workspaces();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].key(), "com.example.color/grading");
    assert_eq!(&*listed[0].plugin_name, "Color Room");
    assert_eq!(*listed[0].spec, grading());
    let plugins = ctx.mounted();
    let info = plugins
        .iter()
        .find(|info| info.manifest.id == "com.example.color")
        .expect("listed");
    assert!(
        info.contributions
            .iter()
            .any(|c| c.kind == ContributionKind::Workspace && c.id == "grading"),
        "the Plugins list does not show the workspace"
    );

    ctx.unmount("com.example.color").expect("unmounts");
    assert!(ctx.workspaces().is_empty());
    assert!(ctx.workspaces_revision() > mounted);
}

#[test]
fn an_id_is_unique_within_its_plugin_only() {
    let ctx = PluginContext::new();
    let first = plugin("com.example.one");
    ctx.mount(&first).expect("mounts");
    let again = ctx.view(&first.manifest).register_workspace(grading());
    assert!(refusal(again).contains("already registered a workspace named `grading`"));
    ctx.mount(&plugin("com.example.two"))
        .expect("a second plugin's grading mounts");
    let keys: Vec<String> = ctx.workspaces().iter().map(PluginWorkspace::key).collect();
    assert_eq!(keys, ["com.example.one/grading", "com.example.two/grading"]);
}

/// Every field `spec_fault` checks, one bad value each, then the boundary
/// values that must still register.
#[test]
fn a_workspace_the_host_cannot_apply_is_refused() {
    let ctx = PluginContext::new();
    let too_long_id = "a".repeat(WORKSPACE_ID_MAX_BYTES + 1);
    let too_long_name = "n".repeat(WORKSPACE_NAME_MAX_BYTES + 1);
    let with = |edit: &dyn Fn(&mut WorkspaceSpec)| {
        let mut spec = grading();
        edit(&mut spec);
        spec
    };
    let bad = [
        with(&|s| s.id = String::new()),
        with(&|s| s.id = ".hidden".into()),
        with(&|s| s.id = "a/b".into()),
        with(&|s| s.id = "a b".into()),
        with(&|s| s.id = too_long_id.clone()),
        with(&|s| s.name = "  ".into()),
        with(&|s| s.name = "Two\nlines".into()),
        with(&|s| s.name = too_long_name.clone()),
        with(&|s| s.panes.sidebar_width = Some(f32::NAN)),
        with(&|s| s.panes.inspector_width = Some(f32::INFINITY)),
        with(&|s| s.panes.timeline_height = Some(0.0)),
        with(&|s| s.panes.timeline_height = Some(-5.0)),
        with(&|s| s.panes.sidebar_width = Some(WORKSPACE_MAX_SIZE + 1.0)),
        with(&|s| s.panes.tree = Some("   ".into())),
    ];
    for spec in bad {
        let shown = format!("{spec:?}");
        let reason = refusal(ctx.register_workspace(spec));
        assert!(!reason.is_empty(), "{shown}");
    }
    assert!(
        ctx.workspaces().is_empty(),
        "a refused workspace was listed"
    );

    let longest_id = "a".repeat(WORKSPACE_ID_MAX_BYTES);
    let longest_name = "n".repeat(WORKSPACE_NAME_MAX_BYTES);
    let good = [
        with(&|s| s.id = longest_id.clone()),
        with(&|s| {
            s.id = "edge".into();
            s.name = longest_name.clone();
        }),
        with(&|s| {
            s.id = "max".into();
            s.panes.sidebar_width = Some(WORKSPACE_MAX_SIZE);
        }),
        with(&|s| {
            s.id = "unset".into();
            s.panes = WorkspacePanes::default();
        }),
        with(&|s| {
            s.id = "with-tree".into();
            s.panes.tree = Some("{\"version\":1}".into());
        }),
    ];
    let mut held = Vec::new();
    for spec in good {
        let shown = format!("{spec:?}");
        held.push(
            ctx.register_workspace(spec)
                .unwrap_or_else(|err| panic!("{shown}: {err}")),
        );
    }
    assert_eq!(ctx.workspaces().len(), 5);
    drop(held);
    assert!(
        ctx.workspaces().is_empty(),
        "a dropped registration stayed listed"
    );
}

fn package_with(scratch: &std::path::Path, tables: &str) -> std::path::PathBuf {
    let dir = scratch.join("com.example.rooms");
    std::fs::create_dir_all(&dir).expect("package dir");
    std::fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "[plugin]\nid = \"com.example.rooms\"\nname = \"Rooms\"\nversion = \"0.1.0\"\napi = 1\n{tables}"
        ),
    )
    .expect("manifest");
    dir
}

#[test]
fn a_package_registers_its_declared_workspaces() {
    let scratch = tempfile::tempdir().expect("scratch");
    let dir = package_with(
        scratch.path(),
        "[[workspaces]]\nid = \"grading\"\nname = \"Grading\"\nsidebar_collapsed = true\ninspector_width = 640\ntimeline_height = 180\nscopes = true\nfocus = \"preview\"\n\n[[workspaces]]\nid = \"logging\"\nname = \"Logging\"\n",
    );
    let package = Package::open(&dir).expect("opens");
    let ctx = PluginContext::new();
    ctx.mount(&DataPlugin::new(package)).expect("mounts");
    let listed = ctx.workspaces();
    let keys: Vec<String> = listed.iter().map(PluginWorkspace::key).collect();
    assert_eq!(
        keys,
        ["com.example.rooms/grading", "com.example.rooms/logging"]
    );
    assert_eq!(listed[0].spec.panes, grading().panes);
    assert_eq!(listed[1].spec.panes, WorkspacePanes::default());
    ctx.unmount("com.example.rooms").expect("unmounts");
    assert!(ctx.workspaces().is_empty());
}

#[test]
fn a_declared_workspace_with_a_fault_refuses_the_package() {
    let scratch = tempfile::tempdir().expect("scratch");
    for (tables, says) in [
        (
            "[[workspaces]]\nid = \"a\"\nname = \"A\"\nfocus = \"sidebar\"\n",
            "sidebar",
        ),
        (
            "[[workspaces]]\nid = \"a\"\nname = \"A\"\nscope = true\n",
            "scope",
        ),
        (
            "[[workspaces]]\nid = \"a\"\nname = \"A\"\ntimeline_height = 0\n",
            "timeline_height",
        ),
        ("[[workspaces]]\nid = \"a\"\nname = \"\"\n", "name"),
        (
            "[[workspaces]]\nid = \"a\"\nname = \"A\"\n[[workspaces]]\nid = \"a\"\nname = \"B\"\n",
            "twice",
        ),
        (
            "[[workspaces]]\nid = \"a\"\nname = \"A\"\ntree = \"  \"\n",
            "tree",
        ),
    ] {
        let dir = package_with(scratch.path(), tables);
        let err = Package::open(&dir).expect_err(tables);
        assert!(err.to_string().contains(says), "{tables}: {err}");
    }
}
