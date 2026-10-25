//! Text the editor writes is spelled in US English: `Color`, `center`,
//! `Favorites`, `Minimize`, `normalize`.
//!
//! WHY: the Color panel, the Color workspace and most labels were spelled in
//! US English while tooltips, parameter labels, adjustment notes and status
//! lines wrote `colour`, `centre`, `Favourites`, `Minimise` and `normalise`, so
//! one screen spelled one word two ways. This walks every `.rs` file under
//! `crates/`, `apps/` and `examples/` on each run and fails on a string
//! literal holding a spelling [`british_words`] reports, in test code as well
//! as production code so no assertion pins the old wording. Comments are not
//! read. `keys::docs_markdown_tests` holds the documentation prose to the same
//! matcher, and [`the_agents_tool_schema_is_spelled_in_us_english`] the tool
//! schema the agent reads.
//!
//! Not caught: a British spelling the lists below do not hold, a word built
//! from parts at run time, and text a plugin or a project supplies.
//! `cancelled` and `cancelling` are not reported: US English writes them with
//! one `l` or two, and the plugin wire protocol writes `"cancelled"`.

use std::path::Path;

use super::collect_rs;
use super::sources::string_literals;

/// A word that starts with one of these is British, and starts with the
/// second spelling in US English: `colours` is `colors`.
const PREFIXES: [(&str, &str); 12] = [
    ("artefact", "artifact"),
    ("behaviour", "behavior"),
    ("catalogue", "catalog"),
    ("centre", "center"),
    ("colour", "color"),
    ("defence", "defense"),
    ("favourite", "favorite"),
    ("fibre", "fiber"),
    ("honour", "honor"),
    ("licence", "license"),
    ("metre", "meter"),
    ("neighbour", "neighbor"),
];

/// Roots whose British forms end in `-ise`, `-ised`, `-ising`, `-isation` and
/// the rest of [`ISE_SUFFIXES`], where US English writes `-ize`. A whole word
/// is matched, so `organism`, `optimism` and `specialist` are not.
const ISE_ROOTS: [&str; 46] = [
    "apolog",
    "author",
    "capital",
    "categor",
    "central",
    "character",
    "critic",
    "custom",
    "digit",
    "emphas",
    "equal",
    "familiar",
    "final",
    "global",
    "harmon",
    "initial",
    "item",
    "legal",
    "linear",
    "local",
    "material",
    "maxim",
    "memor",
    "minim",
    "mobil",
    "neutral",
    "normal",
    "optim",
    "organ",
    "parameter",
    "personal",
    "polar",
    "priorit",
    "quant",
    "random",
    "raster",
    "rational",
    "real",
    "recogn",
    "sanit",
    "serial",
    "special",
    "stabil",
    "summar",
    "synchron",
    "visual",
];

/// The endings an [`ISE_ROOTS`] root takes in British spelling.
const ISE_SUFFIXES: [&str; 9] = [
    "ise", "ised", "ises", "ising", "iser", "isers", "isation", "isations", "isable",
];

/// Whole British words, and their US spelling.
const WORDS: [(&str, &str); 23] = [
    ("analyse", "analyze"),
    ("analysed", "analyzed"),
    ("analyser", "analyzer"),
    ("analysing", "analyzing"),
    ("catalogued", "cataloged"),
    ("cataloguing", "cataloging"),
    ("centred", "centered"),
    ("centring", "centering"),
    ("grey", "gray"),
    ("greyed", "grayed"),
    ("greying", "graying"),
    ("greyish", "grayish"),
    ("greys", "grays"),
    ("greyscale", "grayscale"),
    ("labelled", "labeled"),
    ("labelling", "labeling"),
    ("modelled", "modeled"),
    ("modelling", "modeling"),
    ("programme", "program"),
    ("programmes", "programs"),
    ("travelled", "traveled"),
    ("travelling", "traveling"),
    ("tyre", "tire"),
];

/// Each word of `text` spelled in British English, as written, with its US
/// spelling in lower case. A word is a run of letters. A run that also holds a
/// digit or `_` names an identifier, such as `TARGET_COLOURS`, and is skipped,
/// as is a format placeholder such as `{MAX_GAIN:+.0}`. An escape such as `\n`
/// ends a word.
pub(crate) fn british_words(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut chars = text.chars().peekable();
    let mut word = String::new();
    loop {
        let next = chars.next();
        match next {
            Some(c) if c.is_alphanumeric() || c == '_' => {
                word.push(c);
                continue;
            }
            Some('\\') => {
                chars.next();
            }
            Some('{') if chars.next_if_eq(&'{').is_none() => {
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
            }
            _ => {}
        }
        let token = std::mem::take(&mut word);
        if !token.is_empty()
            && token.chars().all(char::is_alphabetic)
            && let Some(us) = us_spelling(&token.to_lowercase())
        {
            found.push((token, us));
        }
        if next.is_none() {
            return found;
        }
    }
}

/// The US spelling of `word`, written in lower case, when it is British.
fn us_spelling(word: &str) -> Option<String> {
    if let Some(&(_, us)) = WORDS.iter().find(|(british, _)| *british == word) {
        return Some(us.to_owned());
    }
    for root in ISE_ROOTS {
        if let Some(ending) = word.strip_prefix(root)
            && ISE_SUFFIXES.contains(&ending)
        {
            return Some(format!("{root}iz{}", &ending[2..]));
        }
    }
    PREFIXES
        .iter()
        .find_map(|(british, us)| word.strip_prefix(british).map(|rest| format!("{us}{rest}")))
}

/// Literals that are ids rather than text, spelled as they were published,
/// and why each stays. A row no literal matches fails the test, so an
/// exemption cannot outlive its id.
const IDS: [(&str, &str); 4] = [
    (
        "win-workspace-colour",
        "the Color workspace's command id, which keymaps and plugins name",
    ),
    (
        "pal-media-normalise",
        "the loudness command's id, which keymaps and plugins name",
    ),
    (
        "media-normalise",
        "the loudness control's id, which docs/reference/controls.md lists",
    ),
    (
        "favouriteEffects",
        "the Effects favorites' key, which saved settings files hold",
    ),
];

/// British words a file writes as the subject of its literals: the file, the
/// word in lower case, and why. A row no literal matches fails the test.
const EXEMPT: [(&str, &str, &str); 5] = [
    (
        "crates/aria-models/src/effects/params.rs",
        "colour",
        "a manifest that writes the parameter kind `colour` reads as `color`",
    ),
    (
        "crates/aria-models/src/effects_typed_tests_specs.rs",
        "colour",
        "the test of that parameter kind alias",
    ),
    (
        "crates/aria-plugin/src/clip_types_tests.rs",
        "colour",
        "the clip type manifest key, which published manifests write",
    ),
    (
        "crates/aria-plugin/tests/manifest_reference/packages.rs",
        "colour",
        "the clip type manifest key, which published manifests write",
    ),
    (
        "crates/aria-ui/src/app/effects_browser_tests.rs",
        "favouriteeffects",
        "the settings key `favouriteEffects`, which saved settings files hold",
    ),
];

/// This file writes the spellings it rejects.
const SELF: &str = "crates/aria-ui/src/test_support/us_spelling.rs";

#[test]
fn every_string_literal_is_spelled_in_us_english() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    for root in ["crates", "apps", "examples"] {
        collect_rs(&workspace.join(root), &mut files);
    }
    files.sort();
    assert!(
        files.len() > 500,
        "found {} source files under {workspace:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    let mut id_hit = [false; IDS.len()];
    let mut exempt_hit = [false; EXEMPT.len()];
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == SELF {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (line, literal) in string_literals(&text) {
            if let Some(row) = IDS.iter().position(|(id, _)| literal == *id) {
                id_hit[row] = true;
                continue;
            }
            for (word, us) in british_words(&literal) {
                let lower = word.to_lowercase();
                match EXEMPT
                    .iter()
                    .position(|(name, exempt, _)| rel == *name && lower == *exempt)
                {
                    Some(row) => exempt_hit[row] = true,
                    None => offenders.push(format!("{rel}:{line}: `{word}`, US `{us}`")),
                }
            }
        }
    }
    let stale: Vec<String> = IDS
        .iter()
        .zip(id_hit)
        .filter(|(_, hit)| !hit)
        .map(|((id, _), _)| (*id).to_owned())
        .chain(
            EXEMPT
                .iter()
                .zip(exempt_hit)
                .filter(|(_, hit)| !hit)
                .map(|((name, word, _), _)| format!("{name}: {word}")),
        )
        .collect();
    assert!(
        stale.is_empty(),
        "exempt ids and words no literal holds any more; delete their rows: {stale:?}"
    );
    assert!(
        offenders.is_empty(),
        "British spellings in string literals; write the US spelling:\n  {}",
        offenders.join("\n  ")
    );
}

/// WHY: the tool schema the agent reads takes its descriptions from the
/// catalog's literals, which the test above reads, and from the doc comments
/// of the wire types, which it does not, so the schema wrote `Centre offset`
/// beside a catalog that wrote `center`. This reads every description in
/// `crates/aria-ai/tests/snapshots/tools.json`, which the `tool_schema` test
/// of `aria-ai` holds equal to the live schema. Not caught: a British spelling
/// the matcher does not hold.
#[test]
fn the_agents_tool_schema_is_spelled_in_us_english() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../aria-ai/tests/snapshots/tools.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let schema: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
    let mut descriptions = Vec::new();
    collect_descriptions(&schema, &mut descriptions);
    assert!(
        descriptions.len() >= 100,
        "read {} descriptions in {path:?}, which cannot be the tool schema",
        descriptions.len()
    );
    let offenders: Vec<String> = descriptions
        .iter()
        .flat_map(|text| {
            british_words(text)
                .into_iter()
                .map(move |(word, us)| format!("`{word}`, US `{us}`, in {text:?}"))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "British spellings in the agent's tool schema; write the US spelling in the \
         wire type's doc comment or the catalog literal, then re-bless the snapshot:\n  {}",
        offenders.join("\n  ")
    );
}

/// Every `description` string under `value`.
fn collect_descriptions<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                match value {
                    serde_json::Value::String(text) if key == "description" => out.push(text),
                    _ => collect_descriptions(value, out),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_descriptions(item, out);
            }
        }
        _ => {}
    }
}

/// The matcher at its boundaries: each list, case, an escape, a format
/// placeholder, an identifier, a word inside an id, and words that share a
/// British stem without its ending.
#[test]
fn the_matcher_reports_british_words_and_passes_their_lookalikes() {
    let cases: [(&str, &[(&str, &str)]); 13] = [
        ("Reset this colour wheel", &[("colour", "color")]),
        ("Minimise the window", &[("Minimise", "minimize")]),
        (
            "Normalised to -14 dBFS; normalisation",
            &[
                ("Normalised", "normalized"),
                ("normalisation", "normalization"),
            ],
        ),
        (
            "Centred, centres",
            &[("Centred", "centered"), ("centres", "centers")],
        ),
        ("Lifts the blacks toward grey", &[("grey", "gray")]),
        (
            "Favourites, the programme",
            &[("Favourites", "favorites"), ("programme", "program")],
        ),
        ("mask-1-colour", &[("colour", "color")]),
        ("a line\\ncolour", &[("colour", "color")]),
        ("the drag keyed {centre:?}", &[]),
        ("{{colour}}", &[("colour", "color")]),
        ("`TARGET_COLOURS`, test.colour_range, colour2", &[]),
        (
            "an organism, optimism, a specialist, a programmer, analyses",
            &[],
        ),
        ("Color, center, favorite, normalize, gray, cancelled", &[]),
    ];
    for (text, expected) in cases {
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|&(word, us)| (word.to_owned(), us.to_owned()))
            .collect();
        assert_eq!(british_words(text), expected, "{text:?}");
    }
}
