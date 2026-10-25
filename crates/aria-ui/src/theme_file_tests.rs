//! Theme files: the token tables, colour values, parse errors, the directory
//! scan, and what applying a file changes.

use gpui::Hsla;

use super::*;
use crate::theme::{Appearance, Theme, oklch};

/// `(field, value)` for each top-level field of a theme's `Debug` output, the
/// value with its nested lines joined.
fn fields(theme: &Theme) -> Vec<(String, String)> {
    let text = format!("{theme:#?}");
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines().skip(1).filter(|line| *line != "}") {
        let top = line
            .strip_prefix("    ")
            .filter(|rest| !rest.starts_with(' '))
            .and_then(|rest| rest.split_once(": "));
        match top {
            Some((name, value)) => fields.push((name.to_owned(), value.to_owned())),
            None => {
                if let Some((_, value)) = fields.last_mut() {
                    value.push_str(line.trim());
                }
            }
        }
    }
    fields
}

fn names<T>(table: &[(&str, T)]) -> Vec<String> {
    table.iter().map(|(name, _)| (*name).to_owned()).collect()
}

fn hex(value: u32) -> Hsla {
    Hsla::from(gpui::rgb(value))
}

/// Every colour and font field of [`Theme`] is a token a file can set, once.
///
/// The fields are read from the theme's `Debug` output, so a field added to
/// `Theme` without a row in `COLOR_TOKENS` or `FONT_TOKENS` turns this red
/// instead of being a colour no theme file can reach. A field of another type
/// fails too, until it is listed here as not a token.
#[test]
fn every_theme_field_is_a_theme_file_token() {
    let mut colors = Vec::new();
    let mut fonts = Vec::new();
    let mut other = Vec::new();
    for (name, value) in fields(&Theme::dark()) {
        if value.starts_with("Hsla {") {
            colors.push(name);
        } else if value.starts_with('"') {
            fonts.push(name);
        } else {
            other.push(name);
        }
    }
    assert_eq!(other, ["appearance"], "fields that are not tokens");
    for (kind, mut fields, mut tokens) in [
        ("color", colors, names(COLOR_TOKENS)),
        ("font", fonts, names(FONT_TOKENS)),
    ] {
        let listed = tokens.len();
        fields.sort();
        tokens.sort();
        tokens.dedup();
        assert_eq!(tokens.len(), listed, "a {kind} token is listed twice");
        assert_eq!(
            tokens, fields,
            "{kind} tokens against Theme's {kind} fields"
        );
    }
}

/// Each token, set alone in a file, changes its own field and no other, in
/// the appearance it is set for.
#[test]
fn each_token_sets_only_its_own_field() {
    let cases = COLOR_TOKENS
        .iter()
        .map(|(token, _)| (*token, "#123456"))
        .chain(FONT_TOKENS.iter().map(|(token, _)| (*token, "Test Family")));
    for appearance in [Appearance::Dark, Appearance::Light] {
        let base = match appearance {
            Appearance::Dark => Theme::dark(),
            Appearance::Light => Theme::light(),
        };
        let before = fields(&base);
        let section = match appearance {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        };
        for (token, value) in cases.clone() {
            let text = format!("[{section}]\n{token} = \"{value}\"\n");
            let file =
                ThemeFile::parse("one", &text).unwrap_or_else(|err| panic!("{token}: {err}"));
            let after = fields(&base.clone().with_file(&file));
            let changed: Vec<&str> = before
                .iter()
                .zip(&after)
                .filter(|(was, now)| was != now)
                .map(|((name, _), _)| name.as_str())
                .collect();
            assert_eq!(changed, [token], "[{section}] {token}");
        }
    }
}

#[test]
fn a_file_changes_only_the_appearances_it_covers() {
    let file = ThemeFile::parse(
        "dusk",
        "name = \"Dusk\"\n[dark]\naccent = \"#336699\"\nfont_mono = \"Iosevka\"\n",
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(file.name, "Dusk");
    assert_eq!(file.stem, "dusk");
    assert!(file.covers(Appearance::Dark));
    assert!(!file.covers(Appearance::Light));
    let dark = Theme::dark().with_file(&file);
    assert_eq!(dark.accent, hex(0x336699));
    assert_eq!(dark.font_mono.as_ref(), "Iosevka");
    assert_eq!(dark.bg, Theme::dark().bg);
    assert_eq!(
        fields(&Theme::light().with_file(&file)),
        fields(&Theme::light())
    );
}

#[test]
fn colour_values_parse_to_the_colour_they_name() {
    let cases: [(&str, Hsla); 8] = [
        ("#fff", hex(0xffffff)),
        ("#336699", hex(0x336699)),
        ("  #336699  ", hex(0x336699)),
        ("#33669980", Hsla::from(gpui::rgba(0x3366_9980))),
        ("#3698", Hsla::from(gpui::rgba(0x3366_9988))),
        ("#ABCDEF", hex(0xabcdef)),
        ("oklch(0.673 0.182 276.935)", oklch(0.673, 0.182, 276.935)),
        ("oklch( 0.5 0 0 / 0.25 )", {
            let mut color = oklch(0.5, 0.0, 0.0);
            color.a = 0.25;
            color
        }),
    ];
    for (text, want) in cases {
        assert_eq!(parse_color(text), Ok(want), "{text}");
    }
    // The bounds themselves are in range.
    for text in ["oklch(0 0 0)", "oklch(1 0.4 360 / 1)", "oklch(1 0 -20 / 0)"] {
        assert!(parse_color(text).is_ok(), "{text}");
    }
}

#[test]
fn malformed_colour_values_are_errors() {
    for text in [
        "",
        "fff",
        "#",
        "#ff",
        "#fffff",
        "#gggggg",
        "#+12345",
        "#-1-2-3",
        "#ffffff00ff",
        "rgb(1, 2, 3)",
        "red",
        "oklch(0.5 0.1)",
        "oklch(0.5 0.1 20 40)",
        "oklch(0.5 0.1 20",
        "oklch(1.5 0.1 20)",
        "oklch(-0.1 0.1 20)",
        "oklch(0.5 -0.1 20)",
        "oklch(0.5 0.1 20 / 2)",
        "oklch(0.5 0.1 nan)",
        "oklch(0.5 0.1 inf)",
        "oklch(0.5 0.1 x)",
        "oklch(0.5 0.1 20 /)",
    ] {
        assert!(parse_color(text).is_err(), "{text:?} parsed");
    }
}

#[test]
fn a_file_with_an_unknown_or_malformed_entry_is_rejected() {
    let cases = [
        ("", "sets no tokens"),
        ("name = \"Only a name\"\n", "sets no tokens"),
        ("[dark]\n", "sets no tokens"),
        (
            "[dark]\naccent = \"#336699\"\n[extra]\n",
            "unknown key `extra`",
        ),
        ("color = \"#fff\"\n", "unknown key `color`"),
        ("[dark]\naccnet = \"#336699\"\n", "no token `accnet`"),
        ("[light]\naccent = 3\n", "[light] accent is not a string"),
        ("[dark]\naccent = \"blue\"\n", "[dark] accent:"),
        ("[dark]\nfont_sans = \"  \"\n", "names no font family"),
        ("dark = \"#fff\"\n", "`dark` is not a table"),
        (
            "name = 3\n[dark]\naccent = \"#fff\"\n",
            "`name` is not a string",
        ),
        ("[dark\naccent = \"#fff\"\n", "not valid TOML"),
    ];
    for (text, reason) in cases {
        match ThemeFile::parse("bad", text) {
            Ok(_) => panic!("{text:?} parsed"),
            Err(err) => assert!(err.contains(reason), "{text:?}: {err}"),
        }
    }
}

#[test]
fn a_blank_name_falls_back_to_the_stem() {
    let file = ThemeFile::parse("midnight", "name = \"  \"\n[light]\nbg = \"#000\"\n")
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(file.name, "midnight");
}

fn write_theme(dir: &std::path::Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap_or_else(|err| panic!("{file}: {err}"));
}

/// The scan lists readable themes by name, lists each unreadable `.toml`
/// with its reason, skips other files, and treats a missing directory as
/// empty.
#[test]
fn scan_lists_themes_and_failures_and_skips_other_files() {
    let data = crate::scratch_pool::scratch("theme-scan");
    let empty = scan(&data, &[]);
    assert!(empty.themes.is_empty() && empty.failed.is_empty());

    let dir = data.join(THEMES_DIR);
    std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
    write_theme(
        &dir,
        "b.toml",
        "name = \"Alpha\"\n[dark]\naccent = \"#fff\"\n",
    );
    write_theme(
        &dir,
        "a.toml",
        "name = \"Zulu\"\n[light]\naccent = \"#000\"\n",
    );
    write_theme(&dir, "c.toml", "[dark]\naccent = \"#111\"\n");
    write_theme(&dir, "bad.toml", "[dark]\nnot_a_token = \"#fff\"\n");
    write_theme(&dir, "notes.txt", "not a theme");
    let padding = "# padding\n".repeat(7_000);
    write_theme(
        &dir,
        "big.toml",
        &format!("{padding}[dark]\naccent = \"#fff\"\n"),
    );
    std::fs::create_dir_all(dir.join("folder.toml")).unwrap_or_else(|err| panic!("{err}"));

    let found = scan(&data, &[]);
    let listed: Vec<(&str, &str)> = found
        .themes
        .iter()
        .map(|theme| (theme.stem.as_str(), theme.name.as_str()))
        .collect();
    assert_eq!(listed, [("b", "Alpha"), ("c", "c"), ("a", "Zulu")]);
    let failed: Vec<&str> = found.failed.iter().map(|(file, _)| file.as_str()).collect();
    assert_eq!(failed, ["bad.toml", "big.toml", "folder.toml"]);
    assert!(found.failed[0].1.contains("no token `not_a_token`"));
    assert!(found.failed[1].1.contains("larger than 64 KiB"));
    assert!(found.find("a").is_some_and(|theme| theme.name == "Zulu"));
    assert!(found.find("notes").is_none());
    assert!(found.find("bad").is_none());
}

/// A plugin theme lists under `<plugin id>/<theme id>` beside the data-dir
/// files, so a file and a plugin theme sharing a name stay apart; its name
/// defaults to the theme id, its label carries the plugin, and a theme that
/// does not read is listed as failed with the plugin named.
#[test]
fn scan_lists_plugin_themes_beside_files_under_their_own_keys() {
    let data = crate::scratch_pool::scratch("theme-plugins");
    let dir = data.join(THEMES_DIR);
    std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
    write_theme(&dir, "dusk.toml", "[dark]\naccent = \"#111111\"\n");
    let plugin = |id: &str, text: &str| aria_plugin::PluginTheme {
        plugin_id: "com.example.themes".into(),
        plugin: "Theme Pack".into(),
        id: id.into(),
        text: text.into(),
    };
    let plugins = [
        plugin("dusk", "[dark]\naccent = \"#336699\"\n"),
        plugin("named", "name = \"Harbor\"\n[light]\naccent = \"#000\"\n"),
        plugin("broken", "[dark]\nnot_a_token = \"#fff\"\n"),
    ];

    let found = scan(&data, &plugins);
    let listed: Vec<(&str, String)> = found
        .themes
        .iter()
        .map(|theme| (theme.stem.as_str(), theme.label()))
        .collect();
    assert_eq!(
        listed,
        [
            ("com.example.themes/dusk", "dusk from Theme Pack".to_owned()),
            ("dusk", "dusk".to_owned()),
            (
                "com.example.themes/named",
                "Harbor from Theme Pack".to_owned()
            ),
        ]
    );
    let file = found
        .find("dusk")
        .unwrap_or_else(|| panic!("the data-dir dusk"));
    let from_plugin = found
        .find("com.example.themes/dusk")
        .unwrap_or_else(|| panic!("the plugin dusk"));
    assert_eq!(Theme::dark().with_file(file).accent, hex(0x111111));
    assert_eq!(Theme::dark().with_file(from_plugin).accent, hex(0x336699));
    assert_eq!(found.failed.len(), 1);
    assert_eq!(
        found.failed[0].0,
        "com.example.themes/broken from Theme Pack"
    );
    assert!(found.failed[0].1.contains("no token `not_a_token`"));
}

/// The installed theme carries the chosen file's values, and installing
/// without a file restores the shipped ones.
#[test]
fn install_applies_the_file_over_the_shipped_theme() {
    let file = ThemeFile::parse("dusk", "[dark]\naccent = \"#336699\"\n")
        .unwrap_or_else(|err| panic!("{err}"));
    let cx = gpui::TestAppContext::single();
    let (with_file, shipped) = cx.update(|app| {
        Theme::install(
            Appearance::Dark,
            aria_settings::ThemeChoice::Blue,
            Some(&file),
            app,
        );
        let with_file = Theme::of(app).accent;
        Theme::install(
            Appearance::Dark,
            aria_settings::ThemeChoice::Blue,
            None,
            app,
        );
        (with_file, Theme::of(app).accent)
    });
    assert_eq!(with_file, hex(0x336699));
    assert_eq!(shipped, Theme::dark().accent);
}
