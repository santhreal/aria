//! Scopes a plugin adds to the Aria scope strip: the spec it registers with
//! [`Host::register_scope`](crate::Host::register_scope) and the frame and
//! image [`Plugin::scope_measure`](crate::Plugin::scope_measure) reads and
//! draws.

use serde::{Deserialize, Serialize};

/// What a scope is called and how large it draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeSpec {
    /// ASCII letters, digits, `.`, `-` and `_`, unique among every plugin's
    /// scopes.
    pub id: String,
    /// The caption above the image.
    pub name: String,
    /// Width of the image the scope draws, 1 to 512.
    pub width: u32,
    /// Height of the image the scope draws, 1 to 512.
    pub height: u32,
}

/// The frame a scope reads: `width * height` RGBA8 pixels, sRGB, straight
/// alpha, top row first.
#[derive(Debug, Clone, Copy)]
pub struct ScopeFrame<'a> {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub pixels: &'a [u8],
}

impl ScopeFrame<'_> {
    /// The pixel at column `x` and row `y`, or `None` outside the frame.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels
            .get(at..at + 4)
            .and_then(|px| <[u8; 4]>::try_from(px).ok())
    }
}

/// The image a scope draws: `width * height` RGBA8 pixels, straight alpha,
/// top row first, transparent black on entry.
#[derive(Debug)]
pub struct ScopeImage<'a> {
    /// Image width in pixels: the [`ScopeSpec::width`] the scope registered.
    pub width: u32,
    /// Image height in pixels: the [`ScopeSpec::height`] the scope registered.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub pixels: &'a mut [u8],
}

impl ScopeImage<'_> {
    /// Set the pixel at column `x` and row `y`. A position outside the image
    /// is ignored.
    pub fn put(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        if let Some(px) = self.pixels.get_mut(at..at + 4) {
            px.copy_from_slice(&rgba);
        }
    }
}
