//! Embedded Lucide icons, the brand mark, and the gpui [`AssetSource`] that
//! serves them.
//!
//! Art lives in repo-root `assets/icon/`. gpui tints SVGs with the element's
//! text color, so one glyph serves both appearances; the brand mark is raster
//! art with its own colours and is drawn through [`brand_mark`].

use std::borrow::Cow;

use gpui::{
    AssetSource, ImageSource, Img, Resource, Result, SharedString, Styled as _, Svg, img, px, svg,
};

macro_rules! icon_assets {
    ($(($const_name:ident, $file:literal, $path:literal)),+ $(,)?) => {
        $(pub const $const_name: &str = $path;)+

        /// Serves the embedded icons to gpui's SVG renderer.
        pub struct Assets;

        impl AssetSource for Assets {
            fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
                Ok(match path {
                    $($path => Some(Cow::Borrowed(
                        include_bytes!(concat!("../../../assets/icon/", $file)).as_slice(),
                    )),)+
                    _ => None,
                })
            }

            fn list(&self, path: &str) -> Result<Vec<SharedString>> {
                let all = [$($path),+];
                Ok(all
                    .iter()
                    .filter(|p| p.starts_with(path))
                    .map(|p| SharedString::from(*p))
                    .collect())
            }
        }
    };
}

icon_assets![
    // The brand mark, and the only raster row: the same art the packagers turn
    // into the app icon, so the mark in the window and the icon in the dock
    // cannot drift apart.
    (BRAND_MARK, "aria-icon.png", "icon/aria-icon.png"),
    (PLAY, "play.svg", "icon/play.svg"),
    (PAUSE, "pause.svg", "icon/pause.svg"),
    (PLUS, "plus.svg", "icon/plus.svg"),
    (FOLDER_OPEN, "folder-open.svg", "icon/folder-open.svg"),
    (CLAPPERBOARD, "clapperboard.svg", "icon/clapperboard.svg"),
    (IMAGE, "library/image.svg", "icon/image.svg"),
    (SPARKLES, "shell/sparkles.svg", "icon/sparkles.svg"),
    (EYE, "timeline/eye.svg", "icon/eye.svg"),
    (EYE_OFF, "timeline/eye-off.svg", "icon/eye-off.svg"),
    (LOCK, "timeline/lock.svg", "icon/lock.svg"),
    (LOCK_OPEN, "timeline/lock-open.svg", "icon/lock-open.svg"),
    (VOLUME, "timeline/volume-2.svg", "icon/volume.svg"),
    (VOLUME_X, "timeline/volume-x.svg", "icon/volume-x.svg"),
    (HEADPHONES, "timeline/headphones.svg", "icon/headphones.svg"),
    (CROSSHAIR, "timeline/crosshair.svg", "icon/crosshair.svg"),
    (LINK, "timeline/link.svg", "icon/link.svg"),
    (UNLINK, "timeline/unlink.svg", "icon/unlink.svg"),
    (
        AUDIO_LINES,
        "timeline/audio-lines.svg",
        "icon/audio-lines.svg"
    ),
    (
        ROTATE_CCW,
        "inspector/rotate-ccw.svg",
        "icon/rotate-ccw.svg"
    ),
    (UPLOAD, "shell/upload.svg", "icon/upload.svg"),
    (SETTINGS, "shell/settings.svg", "icon/settings.svg"),
    (TRASH, "inspector/trash-2.svg", "icon/trash.svg"),
    (FIT, "fullscreen.svg", "icon/fit.svg"),
    (SCISSORS, "timeline/scissors.svg", "icon/scissors.svg"),
    (ZOOM_IN, "timeline/zoom-in.svg", "icon/zoom-in.svg"),
    (ZOOM_OUT, "timeline/zoom-out.svg", "icon/zoom-out.svg"),
    (MAGNET, "timeline/magnet.svg", "icon/magnet.svg"),
    (MINUS, "shell/minus.svg", "icon/minus.svg"),
    (SQUARE, "shell/square.svg", "icon/square.svg"),
    (CLOSE, "shell/x.svg", "icon/close.svg"),
    // The five trim tools. Each is a Lucide glyph chosen for the gesture: the
    // take moving inside a fixed clip, a clip between two neighbours, the shared
    // cut being dragged, playback speed, and a gap closing.
    (SLIP, "timeline/move-horizontal.svg", "icon/slip.svg"),
    (SLIDE, "timeline/columns-3.svg", "icon/slide.svg"),
    (ROLL, "timeline/grip-vertical.svg", "icon/roll.svg"),
    (RATE, "timeline/gauge.svg", "icon/rate.svg"),
    (RIPPLE, "timeline/fold-horizontal.svg", "icon/ripple.svg"),
    // Transport. The strip previously drew the two zoom magnifiers for
    // "go to start/end" and for frame stepping, which read as zoom controls
    // in the one place an operator looks for playback.
    (SKIP_BACK, "skip-back.svg", "icon/skip-back.svg"),
    (SKIP_FORWARD, "skip-forward.svg", "icon/skip-forward.svg"),
    (CHEVRON_LEFT, "chevron-left.svg", "icon/chevron-left.svg"),
    (CHEVRON_RIGHT, "chevron-right.svg", "icon/chevron-right.svg"),
    // One glyph for every disclosure in the app: a row that opens turns this
    // one, rather than swapping to a second asset mid-transition.
    (CHEVRON_DOWN, "ui/chevron-down.svg", "icon/chevron-down.svg"),
];

/// An icon element for an embedded asset path. Size and colour are set by
/// the caller (`.size(..)`, `.text_color(..)`).
pub fn icon(path: &'static str) -> Svg {
    svg().path(path).flex_none().size(px(14.))
}

/// The brand mark at a given height. Raster art, so the caller states the size
/// it wants drawn rather than tinting it: this is the one asset the theme does
/// not colour.
///
/// The source is built rather than parsed from the path: `img(&str)` runs the
/// string through a URL parser and allocates a `String` to decide it is not a
/// URL, and Home and the splash both draw this every animated frame.
pub fn brand_mark(size_px: f32) -> Img {
    img(ImageSource::Resource(Resource::Embedded(
        SharedString::new_static(BRAND_MARK),
    )))
    .flex_none()
    .size(px(size_px))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registered path loads, and what loads is the format its extension
    /// promises. Driven off the registry rather than a list of names, so a row
    /// added with a typo'd file, or a raster asset registered where a glyph is
    /// expected, fails here rather than at the first frame that draws it.
    #[test]
    fn every_registered_asset_loads_as_the_format_its_name_promises() {
        let assets = Assets;
        let listed = assets.list("icon/").unwrap();
        assert!(!listed.is_empty());
        for path in listed {
            let bytes = assets
                .load(&path)
                .unwrap()
                .unwrap_or_else(|| panic!("missing asset {path}"));
            if path.ends_with(".png") {
                assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{path} is not a png");
                continue;
            }
            assert!(path.ends_with(".svg"), "{path} has no known format");
            let text = std::str::from_utf8(&bytes).expect("icon svg is utf-8");
            assert!(text.contains("<svg"), "{path} is not an svg");
            assert!(text.contains("viewBox"), "{path} lacks a viewBox");
        }
    }

    #[test]
    fn unknown_paths_are_none() {
        assert!(Assets.load("icon/nope.svg").unwrap().is_none());
    }

    #[test]
    fn list_filters_by_prefix() {
        assert!(Assets.list("fonts/").unwrap().is_empty());
        assert!(
            Assets
                .list("icon/play")
                .unwrap()
                .iter()
                .all(|p| p.as_ref() == PLAY)
        );
    }
}
