//! RGBA images a plugin registers under an id, read by effect graph passes.
//!
//! A pass declares [`aria_models::PassInput::Texture`] with the id and binds the
//! image as `texture_<id>`. The image is premultiplied once, here, so every
//! texture a pass samples holds premultiplied RGBA like the frames it reads.
//! The compositor uploads an image the first time a frame binds it and keeps
//! the upload while the registration lives.
//!
//! An effect holds the textures it reads from the moment it registers: dropping
//! a [`TextureRegistration`] removes the id, so later effects cannot read it,
//! and effects already registered keep drawing with the image they were given.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use aria_core::RgbaImage;
use rustc_hash::FxHashMap;

/// Largest side of a plugin texture, in pixels.
pub const MAX_TEXTURE_SIDE: u32 = 8192;

/// A registered plugin texture, premultiplied.
pub struct PluginTexture {
    serial: u64,
    id: String,
    width: u32,
    height: u32,
    premultiplied: Box<[u8]>,
    digest: u64,
}

impl std::fmt::Debug for PluginTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginTexture")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}

impl PluginTexture {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// FNV-1a over the size and premultiplied pixels: equal images, equal
    /// digests, whatever their registration order.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// Premultiplied RGBA8, row-major, no padding.
    pub(crate) fn premultiplied(&self) -> &[u8] {
        &self.premultiplied
    }

    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }
}

static TEXTURES: LazyLock<RwLock<FxHashMap<String, Arc<PluginTexture>>>> =
    LazyLock::new(RwLock::default);
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

/// The texture registered under `id`, or `None`.
pub fn plugin_texture(id: &str) -> Option<Arc<PluginTexture>> {
    TEXTURES
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(id)
        .cloned()
}

/// A registered texture. Dropping it removes the id.
#[must_use = "dropping the registration removes the texture"]
#[derive(Debug)]
pub struct TextureRegistration {
    texture: Arc<PluginTexture>,
}

impl TextureRegistration {
    pub fn texture(&self) -> &Arc<PluginTexture> {
        &self.texture
    }
}

impl Drop for TextureRegistration {
    fn drop(&mut self) {
        let mut table = TEXTURES.write().unwrap_or_else(PoisonError::into_inner);
        if table
            .get(self.texture.id())
            .is_some_and(|held| held.serial == self.texture.serial)
        {
            table.remove(self.texture.id());
        }
    }
}

/// Why a texture was not registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("texture `{id}`: {message}")]
pub struct TextureError {
    pub id: String,
    pub message: String,
}

/// Register `image`, straight-alpha RGBA8, under `id`.
///
/// Refused when the id is empty or has characters other than ASCII letters,
/// digits, `_`, `.`, and `-`; when the image has a zero side, a side above
/// [`MAX_TEXTURE_SIDE`], or a pixel buffer that is not `width * height * 4`
/// bytes; and when `id` is already registered.
pub fn register_texture(
    id: impl Into<String>,
    image: &RgbaImage,
) -> Result<TextureRegistration, TextureError> {
    let id = id.into();
    let refuse = |message: String| TextureError {
        id: id.clone(),
        message,
    };
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    {
        return Err(refuse(
            "a texture id is ASCII letters, digits, `_`, `.`, and `-`".into(),
        ));
    }
    let (width, height) = (image.width, image.height);
    if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
        return Err(refuse(format!(
            "{width}x{height}; each side is 1 to {MAX_TEXTURE_SIDE} pixels"
        )));
    }
    let expected = width as usize * height as usize * 4;
    if image.pixels.len() != expected {
        return Err(refuse(format!(
            "{} bytes for {width}x{height}; RGBA8 needs {expected}",
            image.pixels.len()
        )));
    }
    let mut premultiplied = image.pixels.clone().into_boxed_slice();
    for px in premultiplied.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        for channel in &mut px[..3] {
            *channel = ((u16::from(*channel) * a + 127) / 255) as u8;
        }
    }
    let digest = digest(width, height, &premultiplied);
    let mut table = TEXTURES.write().unwrap_or_else(PoisonError::into_inner);
    if table.contains_key(&id) {
        return Err(refuse("already registered".into()));
    }
    let texture = Arc::new(PluginTexture {
        serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
        id: id.clone(),
        width,
        height,
        premultiplied,
        digest,
    });
    table.insert(id, Arc::clone(&texture));
    Ok(TextureRegistration { texture })
}

fn digest(width: u32, height: u32, pixels: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in width
        .to_le_bytes()
        .into_iter()
        .chain(height.to_le_bytes())
        .chain(pixels.iter().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::new(width, height, rgba.repeat((width * height) as usize))
    }

    #[test]
    fn a_texture_is_premultiplied_once_and_removed_with_its_registration() {
        let registration =
            register_texture("test.tex.premul", &image(2, 1, [200, 100, 50, 128])).expect("ok");
        let held = plugin_texture("test.tex.premul").expect("registered");
        assert_eq!(&held.premultiplied()[..4], &[100, 50, 25, 128]);
        drop(registration);
        assert!(plugin_texture("test.tex.premul").is_none());
        // The Arc an effect took keeps the pixels.
        assert_eq!(held.width(), 2);
    }

    #[test]
    fn a_duplicate_id_and_malformed_images_are_refused() {
        let _first = register_texture("test.tex.dup", &image(1, 1, [0; 4])).expect("ok");
        let duplicate = register_texture("test.tex.dup", &image(1, 1, [0; 4])).unwrap_err();
        assert_eq!(duplicate.message, "already registered");
        assert!(register_texture("bad id", &image(1, 1, [0; 4])).is_err());
        assert!(register_texture("test.tex.zero", &image(0, 1, [0; 4])).is_err());
        let short = RgbaImage::new(2, 2, vec![0; 4]);
        let error = register_texture("test.tex.short", &short).unwrap_err();
        assert!(error.message.contains("RGBA8 needs 16"), "{error}");
    }

    #[test]
    fn equal_images_have_equal_digests() {
        let a = register_texture("test.tex.dig.a", &image(2, 2, [9, 8, 7, 255])).expect("ok");
        let b = register_texture("test.tex.dig.b", &image(2, 2, [9, 8, 7, 255])).expect("ok");
        let c = register_texture("test.tex.dig.c", &image(2, 2, [9, 8, 6, 255])).expect("ok");
        assert_eq!(a.texture().digest(), b.texture().digest());
        assert_ne!(a.texture().digest(), c.texture().digest());
    }
}
