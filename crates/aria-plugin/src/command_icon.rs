//! Command icons: an SVG a command is drawn with in the palette and in menus.
//!
//! An icon is parsed once, when the command is registered, and kept as the SVG
//! the parser writes back: images and text are dropped, so drawing an icon
//! reads no file and no font. A host draws the icon as a mask in its own text
//! colour, so only the icon's shape is kept.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// The largest SVG accepted, in bytes.
pub const ICON_MAX_BYTES: usize = 64 * 1024;
/// The most XML nodes an icon may have.
pub const ICON_MAX_NODES: u32 = 4096;

/// A command's icon, parsed and rewritten.
#[derive(Clone, PartialEq, Eq)]
pub struct CommandIcon {
    svg: Arc<str>,
    key: u64,
}

impl CommandIcon {
    /// Parse `bytes` as an SVG document.
    ///
    /// Refused when it is empty, larger than [`ICON_MAX_BYTES`], not UTF-8,
    /// declares a DTD, has more than [`ICON_MAX_NODES`] nodes, or is not SVG.
    /// The error states the reason.
    pub fn from_svg(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("the icon is empty".into());
        }
        if bytes.len() > ICON_MAX_BYTES {
            return Err(format!(
                "the icon is {} bytes; the limit is {ICON_MAX_BYTES}",
                bytes.len()
            ));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|err| format!("the icon is not UTF-8 text: {err}"))?;
        let xml = usvg::roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: ICON_MAX_NODES,
            ..usvg::roxmltree::ParsingOptions::default()
        };
        let document = usvg::roxmltree::Document::parse_with_options(text, xml)
            .map_err(|err| format!("the icon is not an SVG document: {err}"))?;
        let options = usvg::Options {
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_xmltree(&document, &options)
            .map_err(|err| format!("the icon is not an SVG document: {err}"))?;
        if !tree.root().has_children() {
            return Err("the icon draws nothing".into());
        }
        let svg: Arc<str> = tree.to_string(&usvg::WriteOptions::default()).into();
        let mut hasher = std::hash::DefaultHasher::new();
        svg.hash(&mut hasher);
        Ok(Self {
            key: hasher.finish(),
            svg,
        })
    }

    /// The SVG as rewritten by the parser.
    pub fn svg(&self) -> &str {
        &self.svg
    }

    /// A hash of [`svg`](Self::svg): equal icons have equal keys, so a host
    /// may cache what it draws by key.
    pub fn key(&self) -> u64 {
        self.key
    }
}

impl fmt::Debug for CommandIcon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandIcon")
            .field("key", &format_args!("{:016x}", self.key))
            .field("bytes", &self.svg.len())
            .finish()
    }
}

#[cfg(test)]
#[path = "command_icon_tests.rs"]
mod tests;
