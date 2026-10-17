//! Command icons keep their shape and nothing that reaches past the bytes.
//!
//! Closes the class of an icon that makes the host read a file, an image, or a
//! font when it draws, and of an icon whose parse cost is not bounded by its
//! size. Does not cover how a host rasterizes the rewritten SVG.

use super::*;

const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/></svg>"#;
const TRIANGLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 2L14 14H2z"/></svg>"#;

#[test]
fn a_shape_is_kept_and_keyed_by_its_content() {
    let square = CommandIcon::from_svg(SQUARE.as_bytes()).expect("a square parses");
    assert!(square.svg().contains("<path"), "{}", square.svg());
    let again = CommandIcon::from_svg(SQUARE.as_bytes()).expect("parses again");
    assert_eq!(square, again);
    assert_eq!(square.key(), again.key());
    let triangle = CommandIcon::from_svg(TRIANGLE.as_bytes()).expect("a triangle parses");
    assert_ne!(square.key(), triangle.key());
    let reparsed = CommandIcon::from_svg(square.svg().as_bytes()).expect("the rewrite parses");
    assert_eq!(reparsed.svg(), square.svg(), "the rewrite is stable");
}

#[test]
fn images_and_text_are_dropped_from_the_rewrite() {
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 16 16">
        <image href="/etc/hostname" width="16" height="16"/>
        <image xlink:href="data:image/png;base64,iVBORw0KGgo=" width="16" height="16"/>
        <text x="0" y="12">A</text>
        <path d="M2 2h12v12H2z"/>
    </svg>"#;
    let icon = CommandIcon::from_svg(source.as_bytes()).expect("the path is kept");
    for gone in ["image", "hostname", "base64", "text"] {
        assert!(
            !icon.svg().contains(gone),
            "{gone} survived: {}",
            icon.svg()
        );
    }
    assert!(icon.svg().contains("<path"), "{}", icon.svg());
}

#[test]
fn an_icon_of_only_images_or_text_draws_nothing() {
    for body in [
        r#"<image href="/etc/hostname" width="16" height="16"/>"#,
        r#"<text x="0" y="12">A</text>"#,
        "",
    ] {
        let source =
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">{body}</svg>"#);
        assert_eq!(
            CommandIcon::from_svg(source.as_bytes()),
            Err("the icon draws nothing".to_owned()),
            "{body}"
        );
    }
}

#[test]
fn hostile_or_malformed_bytes_are_refused() {
    assert_eq!(
        CommandIcon::from_svg(b""),
        Err("the icon is empty".to_owned())
    );

    let oversized = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg">{}</svg>"#,
        "<g/>".repeat(ICON_MAX_BYTES / 4)
    );
    assert_eq!(
        CommandIcon::from_svg(oversized.as_bytes()),
        Err(format!(
            "the icon is {} bytes; the limit is {ICON_MAX_BYTES}",
            oversized.len()
        ))
    );

    let gzip = [0x1f_u8, 0x8b, 0x08, 0x00, 0xff, 0xfe];
    let err = CommandIcon::from_svg(&gzip).expect_err("compressed bytes");
    assert!(err.starts_with("the icon is not UTF-8 text: "), "{err}");

    let laughs = r#"<?xml version="1.0"?>
<!DOCTYPE svg [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">]>
<svg xmlns="http://www.w3.org/2000/svg"><text>&b;</text><path d="M0 0h1v1z"/></svg>"#;
    let err = CommandIcon::from_svg(laughs.as_bytes()).expect_err("a DTD");
    assert!(
        err.starts_with("the icon is not an SVG document: "),
        "{err}"
    );

    let crowded = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg">{}<path d="M0 0h1v1z"/></svg>"#,
        "<g/>".repeat(ICON_MAX_NODES as usize)
    );
    assert!(crowded.len() <= ICON_MAX_BYTES);
    let err = CommandIcon::from_svg(crowded.as_bytes()).expect_err("too many nodes");
    assert!(
        err.starts_with("the icon is not an SVG document: "),
        "{err}"
    );

    // A document with no SVG root parses to an empty drawing.
    assert!(CommandIcon::from_svg(b"<html/>").is_err());
    for not_svg in ["plain text", "<svg"] {
        let err = CommandIcon::from_svg(not_svg.as_bytes()).expect_err(not_svg);
        assert!(
            err.starts_with("the icon is not an SVG document: "),
            "{not_svg}: {err}"
        );
    }
}
