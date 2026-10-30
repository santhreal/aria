//! Cloud stock feed and routing tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `cloud_tests.rs` through `use super::*`.

use super::*;

#[test]
fn a_configured_stock_key_routes_the_search_at_the_provider() {
    // The BYOK rule. Backwards, this sends every query through our servers.
    let mut keyed = Settings::default();
    assert_eq!(
        stock_route(&keyed),
        StockRoute::Backend,
        "an unconfigured install must use the anonymous surface"
    );

    for name in STOCK_PROVIDERS {
        let mut one = keyed.clone();
        one.providers.insert(
            name.to_string(),
            ProviderSettings {
                api_key: Some("secret".into()),
                api_key_env: None,
            },
        );
        assert_eq!(
            stock_route(&one),
            StockRoute::Direct,
            "a {name} key did not route direct"
        );
    }

    // A present-but-blank key is not a key.
    keyed.providers.insert(
        STOCK_PROVIDERS[0].to_string(),
        ProviderSettings {
            api_key: Some("   ".into()),
            api_key_env: None,
        },
    );
    assert_eq!(stock_route(&keyed), StockRoute::Backend);
}
#[test]
fn the_session_reports_the_route_its_own_config_implies() {
    let (state, _) = with_clip();
    assert_eq!(
        state.stock_route(),
        StockRoute::Backend,
        "a fresh session has no provider keys"
    );
}
#[test]
fn every_feed_starts_idle_and_is_labelled() {
    let (state, _) = with_clip();
    let mut labels = Vec::new();
    for feed in CloudFeed::ALL {
        assert_eq!(state.feed(feed), FeedState::Idle);
        assert!(!feed.label().is_empty(), "{feed:?} has no label");
        labels.push(feed.label());
        // Exactly one feed is a search; the rest must name the kind they render.
        assert_eq!(
            feed.is_search(),
            feed.asset_kind().is_none(),
            "{feed:?} is inconsistent about being a search"
        );
    }
    labels.sort_unstable();
    let mut unique = labels.clone();
    unique.dedup();
    assert_eq!(unique.len(), labels.len(), "two feeds share a label");
}
#[test]
fn a_catalog_feed_drops_entries_of_another_kind() {
    // The server may ship a kind this build cannot render. A row that does nothing is
    // worse than one fewer row.
    let response = CatalogResponse {
        entries: vec![
            entry("a", AssetKind::Lut, "https://cdn.example/a.cube"),
            entry("b", AssetKind::Sfx, "https://cdn.example/b.wav"),
            entry("c", AssetKind::Other, "https://cdn.example/c.blob"),
        ],
    };
    let luts = cards_from_catalog(CloudFeed::Luts, &response);
    assert_eq!(luts.len(), 1);
    assert_eq!(luts[0].id, "a");
    assert_eq!(luts[0].extension, "cube");

    let sfx = cards_from_catalog(CloudFeed::Sfx, &response);
    assert_eq!(sfx.len(), 1);
    assert_eq!(sfx[0].extension, "wav");
}
#[test]
fn a_catalog_card_carries_what_the_operator_needs_to_choose() {
    let response = CatalogResponse {
        entries: vec![entry(
            "t",
            AssetKind::Template,
            "https://cdn.example/t.ariat",
        )],
    };
    let cards = cards_from_catalog(CloudFeed::Templates, &response);
    let card = &cards[0];
    assert_eq!(card.name, "t name");
    for part in ["4s", "3 slots", "2.0 MiB", "CC0"] {
        assert!(
            card.detail.contains(part),
            "the detail line lost {part}: {}",
            card.detail
        );
    }
}
#[test]
fn a_stock_item_with_no_downloadable_file_is_dropped() {
    let response = StockSearchResponse {
        items: vec![
            stock_item("has", vec![file("https://cdn.example/v.mp4?token=abc")]),
            stock_item("none", Vec::new()),
        ],
        page: 1,
        has_more: false,
        warnings: Vec::new(),
    };
    let cards = cards_from_stock(&response);
    assert_eq!(cards.len(), 1, "an unimportable item was shown");
    assert_eq!(cards[0].id, "has");
    assert_eq!(
        cards[0].extension, "mp4",
        "a query string was taken for an extension"
    );
    assert!(
        cards[0].attribution.contains("Pexels"),
        "attribution was dropped"
    );
}
#[test]
fn a_failed_feed_does_not_render_as_an_empty_one() {
    // An outage and an empty catalog must be distinguishable, or an outage looks like
    // a server with nothing on it.
    let failed = FeedState::Failed("Stock search: offline".into());
    let empty = FeedState::Ready(Vec::new());
    assert!(failed.cards().is_empty() && empty.cards().is_empty());
    assert_eq!(
        failed.placeholder().as_deref(),
        Some("Stock search: offline")
    );
    assert_eq!(empty.placeholder().as_deref(), Some("Nothing found"));
    // Before the first fetch the control that fetches is the whole answer: a
    // line under it would restate that nothing was fetched.
    assert_eq!(FeedState::Idle.placeholder(), None);
    assert_eq!(
        FeedState::Loading.placeholder().as_deref(),
        Some("Loading…")
    );

    let ready = FeedState::Ready(cards_from_stock(&StockSearchResponse {
        items: vec![stock_item("x", vec![file("https://cdn.example/x.mp4")])],
        page: 1,
        has_more: false,
        warnings: Vec::new(),
    }));
    assert_eq!(
        ready.placeholder(),
        None,
        "a loaded feed showed a placeholder"
    );
    assert_eq!(ready.cards().len(), 1);
}
#[test]
fn an_extension_is_derived_from_the_url_and_never_missing() {
    assert_eq!(extension_of("https://cdn.example/a.mp4"), "mp4");
    assert_eq!(extension_of("https://cdn.example/a.MP4?sig=1&x=2"), "mp4");
    assert_eq!(extension_of("https://cdn.example/a.cube#frag"), "cube");
    // No extension, a directory URL, and a dot that is not one all fall back rather
    // than producing a filename the importer cannot dispatch on.
    assert_eq!(extension_of("https://cdn.example/download"), "bin");
    assert_eq!(extension_of("https://cdn.example/dir/"), "bin");
    assert_eq!(extension_of("https://cdn.example/v1.2/asset"), "bin");
}
#[test]
fn a_cached_asset_path_is_keyed_by_feed_and_id_and_is_filename_safe() {
    let (state, _) = with_clip();
    let cards = cards_from_stock(&StockSearchResponse {
        items: vec![stock_item("pexels/12 34", vec![file("https://cdn/a.mp4")])],
        page: 1,
        has_more: false,
        warnings: Vec::new(),
    });
    let path = state
        .cloud_asset_path(CloudFeed::Stock, &cards[0])
        .expect("a cache path");
    assert!(
        path.starts_with(state.download_cache_root()),
        "a cloud asset escaped the cache root: {}",
        path.display()
    );
    let name = path.file_name().expect("a file name").to_string_lossy();
    assert!(name.ends_with(".mp4"), "the extension was lost: {name}");
    assert!(
        !name.contains('/') && !name.contains(' '),
        "an opaque provider id reached the filesystem: {name}"
    );
}
#[test]
fn an_empty_stock_query_clears_the_feed_instead_of_searching() {
    // Every provider answers an empty query with its idea of "popular", which is not
    // what the query requested.
    let (mut state, _) = with_clip();
    state.cloud_feeds.insert(
        CloudFeed::Stock,
        FeedState::Failed("Stock search: offline".into()),
    );
    state.stock_query = "   ".into();
    // The kind change alone resets the stale feed, and a blank query leaves it reset
    // rather than dispatching.
    state.set_stock_kind(StockKind::Photo);
    assert_eq!(state.feed(CloudFeed::Stock), FeedState::Idle);
    assert!(state.stock_query.trim().is_empty());
}
#[test]
fn the_search_box_edits_the_query_only_while_it_holds_focus() {
    // Focus is what keeps a bare "b" from being the razor tool. The state must say so
    // explicitly; the panel cannot infer it from the visible tab.
    let (mut state, _) = with_clip();
    assert!(
        !state.stock_search_focus,
        "the box must not start focused, or the tool shortcuts are dead on launch"
    );

    state.focus_stock_search(true);
    for ch in "sun set".chars() {
        state.stock_query_push(ch);
    }
    assert_eq!(state.stock_query, "sun set");
    state.stock_query_pop();
    assert_eq!(state.stock_query, "sun se");

    // Backspacing past the start is a no-op, not a panic.
    for _ in 0..32 {
        state.stock_query_pop();
    }
    assert!(state.stock_query.is_empty());

    state.focus_stock_search(false);
    assert!(!state.stock_search_focus);
}
#[test]
fn changing_the_kind_drops_results_that_answered_the_previous_one() {
    // Leaving video clips under a Photo chip is worse than an empty panel: the
    // operator would import the wrong kind.
    let (mut state, _) = with_clip();
    assert_eq!(state.stock_kind, StockKind::Video, "video is the default");

    let ready = FeedState::Ready(cards_from_stock(&StockSearchResponse {
        items: vec![stock_item("x", vec![file("https://cdn.example/x.mp4")])],
        page: 1,
        has_more: false,
        warnings: Vec::new(),
    }));
    state.cloud_feeds.insert(CloudFeed::Stock, ready.clone());

    // Re-selecting the current kind is inert, so a stray click does not clear results.
    state.set_stock_kind(StockKind::Video);
    assert_eq!(state.feed(CloudFeed::Stock), ready);

    for kind in [StockKind::Photo, StockKind::Audio, StockKind::Video] {
        state.cloud_feeds.insert(CloudFeed::Stock, ready.clone());
        state.set_stock_kind(kind);
        assert_eq!(state.stock_kind, kind);
        assert_eq!(
            state.feed(CloudFeed::Stock),
            FeedState::Idle,
            "results for the previous kind survived the switch to {kind:?}"
        );
    }
}
#[test]
fn a_skill_installs_into_the_agent_directory_and_a_malformed_one_does_not() {
    let (mut state, _) = with_clip();

    // A file that is not a skill is refused at the click that caused it. Installed, it
    // would instead become a warning on every later prompt.
    let junk = cached(&state, "skills/pack.md", "{}");
    let err = state
        .adopt_cloud_asset(CloudFeed::Skills, &junk, &card("pack", "md"))
        .expect_err("a file with no frontmatter is not a skill");
    assert!(err.contains("frontmatter"), "unhelpful refusal: {err}");
    assert!(
        state.installed_skills().is_empty(),
        "a refused pack must leave nothing behind"
    );

    let good = cached(
        &state,
        "skills/rough-cut.md",
        "---\nname: Rough cut\ndescription: Assemble a first pass\n---\nStep one.\n",
    );
    state
        .adopt_cloud_asset(CloudFeed::Skills, &good, &card("rough/cut", "md"))
        .expect("a valid skill installs");

    // Installed means the agent's own loader finds it: the id is the directory name, so
    // the catalog's slash must not have escaped the skills directory.
    let installed = state.installed_skills();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "rough-cut");
    assert_eq!(installed[0].name, "Rough cut");
    assert_eq!(installed[0].description, "Assemble a first pass");
    assert!(
        state
            .agent_dir()
            .join("skills/rough-cut/SKILL.md")
            .is_file(),
        "the skill is written where load_agent_dir looks"
    );
}
#[test]
fn a_cloud_lut_needs_a_selection_and_otherwise_applies_to_it() {
    let (mut state, clip) = with_clip();
    let path = cached(&state, "luts/warm.cube", "TITLE \"warm\"\nLUT_3D_SIZE 2\n");

    state.selection.clear();
    let err = state
        .adopt_cloud_asset(CloudFeed::Luts, &path, &card("warm", "cube"))
        .expect_err("a LUT with nothing selected must be refused");
    assert!(err.contains("select a clip"), "unhelpful refusal: {err}");

    state.select_clip(clip);
    state
        .adopt_cloud_asset(CloudFeed::Luts, &path, &card("warm", "cube"))
        .expect("the LUT applies to the selection");
    let applied = state
        .selected_ui_clip()
        .and_then(|clip| clip.lut)
        .expect("the clip carries the LUT");
    assert!(
        applied.path.contains("warm.cube"),
        "a different file was applied: {}",
        applied.path
    );
}
#[test]
fn a_downloaded_asset_is_pinned_before_it_is_used() {
    // Pinning after the fact leaves a window where an eviction can delete a file the
    // project already references.
    let (mut state, clip) = with_clip();
    let before = state.protected_download_count();
    let path = cached(&state, "luts/cool.cube", "TITLE \"cool\"\n");
    state.select_clip(clip);
    state
        .adopt_cloud_asset(CloudFeed::Luts, &path, &card("cool", "cube"))
        .expect("the LUT applies");
    assert_eq!(
        state.protected_download_count(),
        before + 1,
        "the adopted file was not pinned against Clear cache"
    );
}
#[test]
fn a_text_preset_pack_loads_its_presets_and_a_malformed_one_is_refused() {
    let (mut state, _) = with_clip();
    assert!(state.text_presets.is_empty());

    let broken = cached(&state, "presets/broken.json", "not json");
    let err = state
        .adopt_cloud_asset(CloudFeed::TextPresets, &broken, &card("broken", "json"))
        .expect_err("a malformed pack must be refused");
    assert!(err.contains("broken name"), "the pack was not named: {err}");
    assert!(
        state.text_presets.is_empty(),
        "a malformed pack left presets behind"
    );

    // An empty pack is refused too: loading it would silently replace a working pack
    // with nothing.
    let empty = cached(&state, "presets/empty.json", r#"{"presets":[]}"#);
    let err = state
        .adopt_cloud_asset(CloudFeed::TextPresets, &empty, &card("empty", "json"))
        .expect_err("an empty pack must be refused");
    assert!(err.contains("no presets"), "unhelpful refusal: {err}");

    let good = cached(
        &state,
        "presets/good.json",
        r#"{"presets":[
            {"id":"a","name":"Bold Yellow","font_size":120.0,"fill":[255,220,0,255]},
            {"id":"b","name":"Fine Print","font_size":32.0,"fill":[10,10,10,255]}
        ]}"#,
    );
    state
        .adopt_cloud_asset(CloudFeed::TextPresets, &good, &card("good", "json"))
        .expect("the pack loads");
    assert_eq!(state.text_presets.len(), 2);
    assert_eq!(state.text_presets[0].name, "Bold Yellow");
}
#[test]
fn applying_a_text_preset_writes_font_size_and_fill_onto_the_title() {
    let mut state = crate::app::text_tests::with_title();
    let preset = aria_cloud::dto::TextPreset {
        id: "a".into(),
        name: "Bold Yellow".into(),
        category: String::new(),
        font_family: "Inter".into(),
        font_size: 120.0,
        fill: [255, 220, 0, 255],
        animation_in: Some("nonexistent-preset-id".into()),
        animation_out: None,
        animation_combo: None,
        sample_text: String::new(),
    };
    state
        .apply_text_preset(&preset)
        .expect("an unknown animation id must be skipped, not refused");

    let clip = state.selected_clip().expect("a title is selected");
    let engine = state.engine().lock();
    let found = engine.project().clip(clip).expect("the clip");
    let aria_models::ClipSource::Generated(aria_models::Generator::Text { style, content }) =
        &found.content
    else {
        panic!("the selection stopped being a title");
    };
    assert_eq!(style.font, "Inter");
    assert_eq!(style.size.constant(), Some(120.0));
    assert_eq!(style.fill.constant(), Some([255, 220, 0, 255]));
    // The preset is a look, not a rewrite: the words survive it.
    assert_eq!(content, "Hello");
}
