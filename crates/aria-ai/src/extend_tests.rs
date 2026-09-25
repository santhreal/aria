//! Tests for the agent-extension loader.
//!
//! Beside [`super`] rather than inline in it: the file was over the length a
//! reviewer will read in one pass, and a test module is the part that can move
//! without a single call site changing.

use super::*;

const SKILL: &str = "---\nname: Podcast cleanup\ndescription: Clean up a talk recording.\n---\n\nStep 1: denoise.\n";

#[test]
fn parse_skill_reads_frontmatter_and_body() {
    let skill = parse_skill("podcast-cleanup", SKILL).unwrap();
    assert_eq!(skill.id, "podcast-cleanup");
    assert_eq!(skill.name, "Podcast cleanup");
    assert_eq!(skill.description, "Clean up a talk recording.");
    assert_eq!(skill.body, "Step 1: denoise.");
}

#[test]
fn parse_skill_rejects_missing_pieces() {
    assert!(parse_skill("x", "no frontmatter").is_err());
    assert!(parse_skill("x", "---\nname: A\n---\n\nbody").is_err());
    assert!(parse_skill("x", "---\nname: A\ndescription: B\n---\n\n").is_err());
}

#[test]
fn compose_rules_caps_and_flags() {
    let (text, truncated) = compose_rules(&[
        ("user".into(), "always 9:16".into()),
        ("p".into(), "".into()),
    ]);
    assert_eq!(text, "[user]\nalways 9:16");
    assert!(!truncated);

    let big = "x".repeat(MAX_RULES_BYTES * 2);
    let (text, truncated) = compose_rules(&[("user".into(), big)]);
    assert!(truncated);
    assert!(text.len() <= MAX_RULES_BYTES + 64);
    assert!(text.ends_with("[…rules truncated at the size cap]"));
}

#[test]
fn slash_commands_expand_with_arguments() {
    let commands = vec![
        SlashCommand {
            name: "vertical".into(),
            body: "Convert the timeline to 9:16.".into(),
        },
        SlashCommand {
            name: "title".into(),
            body: "Add a title that says $ARGUMENTS at the playhead.".into(),
        },
    ];
    assert_eq!(
        expand_slash_command("/vertical", &commands).as_deref(),
        Some("Convert the timeline to 9:16.")
    );
    assert_eq!(
        expand_slash_command("/title Hello World", &commands).as_deref(),
        Some("Add a title that says Hello World at the playhead.")
    );
    // Extra args with no placeholder append.
    assert_eq!(
        expand_slash_command("/vertical but keep audio", &commands).as_deref(),
        Some("Convert the timeline to 9:16.\n\nbut keep audio")
    );
    assert_eq!(expand_slash_command("/unknown", &commands), None);
    assert_eq!(expand_slash_command("plain prompt", &commands), None);
}

#[test]
fn bundled_skills_parse_and_have_unique_ids() {
    let skills = bundled_skills();
    assert_eq!(skills.len(), 3);
    for (i, skill) in skills.iter().enumerate() {
        assert!(!skill.body.is_empty(), "{} has an empty body", skill.id);
        assert!(
            !skills[..i].iter().any(|s| s.id == skill.id),
            "duplicate id {}",
            skill.id
        );
    }
}

#[test]
fn merge_prefers_user_skills_on_collision() {
    let user = vec![Skill {
        id: "podcast-cleanup".into(),
        name: "My cleanup".into(),
        description: "Custom".into(),
        body: "Do it my way.".into(),
    }];
    let merged = merge_skills(user);
    let podcast = merged.iter().find(|s| s.id == "podcast-cleanup").unwrap();
    assert_eq!(podcast.name, "My cleanup");
    assert_eq!(merged.len(), 3);
}

#[test]
fn load_agent_dir_reads_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("rules")).unwrap();
    std::fs::create_dir_all(root.join("skills/my-skill")).unwrap();
    std::fs::create_dir_all(root.join("commands")).unwrap();
    std::fs::write(root.join("rules/style.md"), "prefer crossfades").unwrap();
    std::fs::write(root.join("skills/my-skill/SKILL.md"), SKILL).unwrap();
    std::fs::write(root.join("skills/my-skill/notes.txt"), "ignored").unwrap();
    std::fs::write(root.join("commands/vertical.md"), "Go 9:16.").unwrap();

    let loaded = load_agent_dir(root);
    assert_eq!(
        loaded.rules,
        vec![("style".into(), "prefer crossfades".into())]
    );
    assert_eq!(loaded.skills.len(), 1);
    assert_eq!(loaded.skills[0].id, "my-skill");
    assert_eq!(
        loaded.commands,
        vec![SlashCommand {
            name: "vertical".into(),
            body: "Go 9:16.".into()
        }]
    );
    assert!(loaded.warnings.is_empty());

    // Missing dir degrades to empty.
    assert_eq!(load_agent_dir(&root.join("nope")), AgentDir::default());
}

#[test]
fn malformed_skill_becomes_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("skills/broken")).unwrap();
    std::fs::write(root.join("skills/broken/SKILL.md"), "no frontmatter").unwrap();
    let loaded = load_agent_dir(root);
    assert!(loaded.skills.is_empty());
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].contains("broken"));
}
