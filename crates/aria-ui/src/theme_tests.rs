//! Tests for [`super`].
//!
//! Moved out of `theme.rs` to stay under the file ceiling. The
//! module is unchanged; only its location moved.
use super::*;

fn srgb_u8(c: [f32; 3]) -> [u8; 3] {
    [
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
    ]
}

fn chroma_of(color: Hsla) -> f32 {
    // Reconstruct chroma from Hsla saturation as a coarse bound; track
    // tints are built via oklch(0.65, 0.03, hue) so the input chroma is
    // the contract.
    color.s
}

#[test]
fn oklch_accents_match_reference() {
    assert_eq!(
        srgb_u8(oklch_to_srgb(0.673, 0.182, 276.935)),
        [124, 134, 255]
    );
    assert_eq!(
        srgb_u8(oklch_to_srgb(0.704, 0.191, 22.216)),
        [255, 100, 103]
    );
    assert_eq!(srgb_u8(oklch_to_srgb(0.828, 0.189, 84.429)), [255, 185, 0]);
}

#[test]
fn text_contrast_is_paired_across_appearances() {
    let (d, l) = (Theme::dark(), Theme::light());
    for (name, dark_fg, light_fg) in [
        ("text", d.text, l.text),
        ("text_muted", d.text_muted, l.text_muted),
        ("text_faint", d.text_faint, l.text_faint),
    ] {
        let dr = contrast_ratio(dark_fg, d.bg);
        let lr = contrast_ratio(light_fg, l.bg);
        assert!(
            (dr - lr).abs() < 1.0,
            "{name}: dark {dr:.2}:1 vs light {lr:.2}:1 — not a matched pair"
        );
    }
}

#[test]
fn wcag_contrast_conformance_for_text_on_surfaces() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for (surf_name, surface) in [
            ("bg", theme.bg),
            ("surface", theme.surface),
            ("surface_raised", theme.surface_raised),
            ("surface_card", theme.surface_card),
            ("surface_dialog", theme.surface_dialog),
        ] {
            let ratio = contrast_ratio(theme.text, surface);
            assert!(
                ratio >= 4.5,
                "{name} theme text on {surf_name} contrast {ratio:.2}:1 fails WCAG AA (>= 4.5:1)"
            );

            let muted_ratio = contrast_ratio(theme.text_muted, surface);
            assert!(
                muted_ratio >= 3.0,
                "{name} theme text_muted on {surf_name} contrast {muted_ratio:.2}:1 fails WCAG AA large (>= 3.0:1)"
            );
        }

        let solid_ratio = contrast_ratio(theme.on_solid, theme.solid);
        assert!(
            solid_ratio >= 4.5,
            "{name} theme on_solid on solid contrast {solid_ratio:.2}:1 fails WCAG AA (>= 4.5:1)"
        );
    }
}

#[test]
fn current_appearance_tracks_set() {
    let _guard = lock_appearance();
    set_current_appearance(Appearance::Light);
    assert_eq!(current_appearance(), Appearance::Light);
    let generation = theme_generation();
    set_current_appearance(Appearance::Dark);
    assert_eq!(current_appearance(), Appearance::Dark);
    assert!(theme_generation() > generation);
}

#[test]
fn ink_hairline_wash_visible_in_both_appearances() {
    let _guard = lock_appearance();
    for appearance in [Appearance::Dark, Appearance::Light] {
        set_current_appearance(appearance);
        assert!(ink(0.5).a > 0.0);
        assert!(hairline(0.2).a > 0.0);
        assert!(wash(0.1).a > 0.0);
    }
    set_current_appearance(Appearance::Dark);
}

#[test]
fn light_is_not_a_simple_inversion() {
    let d = Theme::dark();
    let l = Theme::light();
    assert!(d.bg.l < d.surface.l);
    assert!(l.surface.l < l.bg.l);
}

#[test]
fn busy_is_pink_not_green() {
    let d = Theme::dark();
    assert!((d.busy.h - oklch(0.718, 0.202, 349.761).h).abs() < 0.02);
}

#[test]
fn solid_is_neutral_primary() {
    let d = Theme::dark();
    assert!(d.solid.s < 0.05);
    assert!(d.on_solid.s < 0.05);
}

#[test]
fn glass_selected_shadows_are_inset_1px_rings() {
    let shadows = glass_selected_shadows();
    assert_eq!(shadows.len(), 1);
    assert!(shadows[0].inset);
    assert_eq!(shadows[0].spread_radius, gpui::px(1.0));
    assert_eq!(shadows[0].blur_radius, gpui::px(0.0));
}

#[test]
fn track_tints_are_near_neutral() {
    let d = Theme::dark();
    for tint in [
        d.track_video,
        d.track_audio,
        d.track_text,
        d.track_generator,
    ] {
        assert!(
            chroma_of(tint) <= 0.15,
            "track tint saturation {} too high",
            tint.s
        );
    }
}

#[test]
fn idle_screen_uses_at_most_three_meaning_colors() {
    // WHY: PLAN 8.2 / 8.8 screenshot test. An idle editor may show
    // playhead (accent/blue), snap guide (warning/amber), and busy
    // (pink) at most. Track tints stay near-neutral (chroma 0.03) and
    // do not count. A fourth saturated token on screen is decorative.
    let dark = Theme::dark();
    let light = Theme::light();
    for theme in [&dark, &light] {
        let meaning = [
            theme.accent,
            theme.danger,
            theme.warning,
            theme.success,
            theme.busy,
        ];
        let saturated: Vec<_> = meaning.iter().filter(|c| c.s > 0.08).collect();
        assert!(
            saturated.len() <= 5,
            "accent vocabulary itself is five named roles"
        );
        let idle = [theme.playhead, theme.snap_guide, theme.busy];
        let idle_hues: Vec<u16> = idle
            .iter()
            .filter(|c| c.s > 0.08)
            .map(|c| (c.h * 360.0).round() as u16)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert!(
            idle_hues.len() <= 3,
            "idle screen has {} non-neutral hues: {idle_hues:?}",
            idle_hues.len()
        );
    }
}

#[test]
fn contrast_ratio_hits_known_anchors() {
    let white = grey(0xff);
    let black = grey(0x00);
    assert!((contrast_ratio(white, black) - 21.0).abs() < 0.01);
    assert!((contrast_ratio(white, white) - 1.0).abs() < 0.01);
}

#[test]
fn flatten_composites_translucent_over_opaque() {
    let fg = hsla(0.0, 0.0, 1.0, 0.5);
    let bg = grey(0);
    let out = flatten(fg, bg);
    assert!((out.a - 1.0).abs() < 1e-5);
    assert!(out.l > 0.4 && out.l < 0.6);
}

/// Every colour token is painted by some control.
///
/// The class this closes: a token nothing draws is a design decision no operator
/// can see, and twelve of them had accumulated behind a struct that compiles
/// either way. The token list is read from `theme.rs` at run time and the call
/// sites from the rest of the crate, so a token added without a use starts red,
/// and a use deleted later turns red again — neither can be fixed by editing a
/// list in this file.
///
/// Not covered: whether a token is used *well*. That is what the contrast and
/// pairing tests above are for.
#[test]
fn every_theme_token_is_painted_by_a_control() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let theme_rs = std::fs::read_to_string(src.join("theme.rs")).expect("theme.rs");
    let body = {
        let start = theme_rs
            .find("pub struct Theme {")
            .expect("the Theme struct");
        let end = theme_rs[start..].find("\n}").expect("its closing brace");
        &theme_rs[start..start + end]
    };
    let tokens: Vec<&str> = body
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .filter_map(|line| line.strip_suffix(": Hsla,"))
        .collect();
    assert!(
        tokens.len() > 30,
        "the parse found {} tokens, so the struct's shape changed",
        tokens.len()
    );

    // Every file except the theme itself: a token used only by its own module is
    // a token no control paints.
    let mut sources = String::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !name.ends_with(".rs") || name.starts_with("theme") {
                continue;
            }
            sources.push_str(&std::fs::read_to_string(&path).expect("a source file"));
        }
    }

    let orphans: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|token| !sources.contains(&format!("theme.{token}")))
        .collect();
    assert!(
        orphans.is_empty(),
        "these tokens are declared and never painted: {orphans:?}"
    );
}

/// Every text size in the crate comes from the type scale on [`Theme`].
///
/// The class this closes: a literal `text_size(px(13.))` beside the scale is
/// a size look.md never states, and 98 files had grown their own. The scan
/// reads every source file at run time, so a literal added to any file,
/// present or future, turns this red. A size computed at run time (a plugin
/// overlay's own size) is not a digit literal and is left alone.
///
/// Not covered: a literal hidden behind a local `const` or a variable. The
/// constants on `Theme` are the only sizes this test can see being used.
#[test]
fn text_sizes_come_from_the_type_scale() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut literals = Vec::new();
    let mut scanned = 0;
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") || path.ends_with(file!()) {
                continue;
            }
            scanned += 1;
            let text = std::fs::read_to_string(&path).expect("a source file");
            for (index, line) in text.lines().enumerate() {
                let Some(at) = line.find("text_size(px(") else {
                    continue;
                };
                let after = &line[at + "text_size(px(".len()..];
                if after.starts_with(|c: char| c.is_ascii_digit()) {
                    literals.push(format!("{}:{}", path.display(), index + 1));
                }
            }
        }
    }
    assert!(scanned > 100, "the scan read {scanned} files, so src moved");
    assert!(
        literals.is_empty(),
        "these text sizes are literals; use a Theme::TEXT_* constant: {literals:#?}"
    );
}

/// The theme is built when it is installed and shared after that.
///
/// What this defends: `Theme::of` used to hand back a borrow that every caller
/// copied, so a frame paid for one copy of every token per panel that asked.
/// Two reads now point at one allocation. If someone makes `of` build or clone
/// a `Theme` again, the pointers differ and this goes red.
#[test]
fn the_installed_theme_is_read_not_rebuilt() {
    let cx = gpui::TestAppContext::single();
    let (first, second) = cx.update(|app| {
        Theme::install(
            Appearance::Dark,
            aria_settings::ThemeChoice::Blue,
            None,
            app,
        );
        let first = Theme::of(app);
        let second = Theme::of(app);
        (first, second)
    });
    assert!(
        std::ptr::eq(&*first as *const Theme, &*second as *const Theme),
        "two reads of the installed theme must share one allocation"
    );
    // And the handle still reads as a theme: field, method, and `&Theme`.
    assert_eq!(first.appearance, Appearance::Dark);
    assert_eq!(first.wash(0.0), Theme::dark().wash(0.0));
    fn takes_a_theme(theme: &Theme) -> Appearance {
        theme.appearance
    }
    assert_eq!(takes_a_theme(&first), Appearance::Dark);
}

/// Installing a different appearance replaces what every reader sees, so a
/// shared handle cannot pin the window to a stale palette.
#[test]
fn installing_a_theme_replaces_the_one_readers_get() {
    let cx = gpui::TestAppContext::single();
    let (dark, light) = cx.update(|app| {
        Theme::install(
            Appearance::Dark,
            aria_settings::ThemeChoice::Blue,
            None,
            app,
        );
        let dark = Theme::of(app);
        Theme::install(
            Appearance::Light,
            aria_settings::ThemeChoice::Blue,
            None,
            app,
        );
        let light = Theme::of(app);
        (dark, light)
    });
    assert_eq!(dark.appearance, Appearance::Dark);
    assert_eq!(light.appearance, Appearance::Light);
    assert!(
        !std::ptr::eq(&*dark as *const Theme, &*light as *const Theme),
        "a new install is a new theme, not a mutation of the one already handed out"
    );
}

/// Clip names and badges are drawn in [`Theme::label_ink`] on a label fill.
///
/// The class this closes: a label hue added or changed so that the name on
/// every clip carrying it drops below WCAG AA. Every label in
/// [`aria_models::LabelColor::ALL`], selected and not, is checked in both
/// appearances, so a new label starts covered. Not caught: whether a hue is
/// distinguishable from its neighbours.
#[test]
fn every_label_fill_carries_readable_ink() {
    for theme in [Theme::dark(), Theme::light()] {
        for label in aria_models::LabelColor::ALL {
            for selected in [false, true] {
                let fill = theme.label(label, selected);
                let ink = theme.label_ink(label, selected);
                let ratio = contrast_ratio(ink, fill);
                assert!(
                    ratio >= 4.5,
                    "{label:?} selected={selected}: ink {ratio:.2}:1 fails WCAG AA"
                );
            }
            assert_ne!(
                theme.label(label, false),
                theme.label(label, true),
                "{label:?} draws selected as unselected"
            );
        }
    }
}

/// The dark surfaces and text match the values `docs/spec/look.md` states.
#[test]
fn dark_surfaces_hold_the_values_the_look_chapter_states() {
    let d = Theme::dark();
    for (token, color, hex) in [
        ("bg", d.bg, 0x141414),
        ("surface", d.surface, 0x1c1c1c),
        ("surface_header", d.surface_header, 0x232323),
        ("surface_raised", d.surface_raised, 0x2a2a2a),
        ("surface_overlay", d.surface_overlay, 0x2e2e2e),
        ("border", d.border, 0x0e0e0e),
        ("text", d.text, 0xd6d6d6),
        ("text_muted", d.text_muted, 0x9a9a9a),
        ("text_faint", d.text_faint, 0x6b6b6b),
        ("value", d.value, 0x5aa8ff),
    ] {
        let rgba = gpui::Rgba::from(color);
        let got = [rgba.r, rgba.g, rgba.b].map(|c| (c * 255.0).round() as u32);
        let want = [hex >> 16 & 0xff, hex >> 8 & 0xff, hex & 0xff];
        assert_eq!(got, want, "{token}");
    }
    assert_eq!(d.hairline, hsla(0.0, 0.0, 1.0, 0.06));
}
