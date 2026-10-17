//! Content-addressed disk cache of rendered full-canvas timeline frames.
//!
//! Each entry in [`RenderFiles`] is keyed by [`FrameKey`], a SHA-256 digest of:
//! - [`FORMAT_VERSION`] and `aria-render`'s package version,
//! - the caller's plugin environment string (`id@version` sorted),
//! - the unscaled [`Scene`] description and registered pass shader digests,
//! - the identity (`path`, byte length, and modification timestamp in
//!   nanoseconds) of every file the scene references (`Media`, `Still`,
//!   `Lottie`, and `.cube` `lut`, including nested `Transition` and `Matte`
//!   layers).
//!
//! Scenes that invoke a stateful effect (`EffectSpec::stateful == true`) return
//! `None` from [`RenderFiles::key`] and are never stored.
//!
//! Frames are encoded losslessly as QOI (`<dir>/<first 2 hex>/<64 hex>.qoi`)
//! via a sibling temporary file and atomic rename so readers never observe a
//! partial write.

use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use aria_core::RgbaImage;
use aria_models::Project;
use sha2::{Digest as _, Sha256};

use crate::scene::{LayerSource, Scene, SceneLayer};

/// On-disk render-file format and key version. Bumping this invalidates every
/// previously computed [`FrameKey`].
pub const FORMAT_VERSION: u32 = 1;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Content-addressed 256-bit key for one rendered timeline frame.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameKey([u8; 32]);

impl FrameKey {
    /// Wrap raw SHA-256 digest bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw 32-byte SHA-256 digest.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase 64-character hexadecimal representation.
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(64);
        for byte in &self.0 {
            let _ = write!(&mut out, "{byte:02x}");
        }
        out
    }

    fn shard_prefix(&self) -> [u8; 2] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let b = self.0[0];
        [HEX[(b >> 4) as usize], HEX[(b & 0x0f) as usize]]
    }
}

impl fmt::Debug for FrameKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FrameKey(")?;
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        f.write_str(")")
    }
}

impl fmt::Display for FrameKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Content-addressed store of QOI-encoded full-canvas render files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFiles {
    dir: PathBuf,
    environment: String,
}

impl RenderFiles {
    /// Create a [`RenderFiles`] store rooted at `dir` with the given plugin
    /// `environment` string.
    pub fn new(dir: impl Into<PathBuf>, environment: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            environment: environment.into(),
        }
    }

    /// Root directory where `<2-hex>/<64-hex>.qoi` shards are stored.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Environment string (`id@version` sorted) folded into every [`FrameKey`].
    pub fn environment(&self) -> &str {
        &self.environment
    }

    /// On-disk QOI path for `key`: `<dir>/<first two hex>/<64 hex>.qoi`.
    pub fn path(&self, key: &FrameKey) -> PathBuf {
        let shard_bytes = key.shard_prefix();
        let shard = std::str::from_utf8(&shard_bytes).expect("ascii hex");
        let hex = key.to_hex();
        self.dir.join(shard).join(format!("{hex}.qoi"))
    }

    /// Compute the [`FrameKey`] for an unscaled `scene` resolved from `project`.
    ///
    /// Returns `None` when the canvas is degenerate (`width == 0 || height == 0`)
    /// or when any pass in the scene (layer effects, canvas passes, or nested
    /// transition/matte children) is marked `stateful`.
    pub fn key(&self, project: &Project, scene: &Scene) -> Option<FrameKey> {
        self.key_with_format_version(FORMAT_VERSION, project, scene)
    }

    /// Compute a [`FrameKey`] using an explicit `format_version`.
    pub fn key_with_format_version(
        &self,
        format_version: u32,
        project: &Project,
        scene: &Scene,
    ) -> Option<FrameKey> {
        if scene.width == 0 || scene.height == 0 {
            return None;
        }
        if scene.draw_order().any(layer_has_stateful_pass) {
            return None;
        }

        let mut hasher = Sha256::new();
        hasher.update(b"aria-render-file\0");
        hasher.update(format_version.to_le_bytes());
        update_str(&mut hasher, env!("CARGO_PKG_VERSION"));
        update_str(&mut hasher, &self.environment);

        let mut writer = HashWriter(&mut hasher);
        let _ = write!(
            &mut writer,
            "Scene{{w:{},h:{},bg:{:?},tick:{:?},layers:[",
            scene.width, scene.height, scene.background, scene.tick
        );
        for layer in &scene.layers {
            hash_layer_debug(&mut writer, project, layer);
        }
        let _ = writer.write_str("]}");
        // A referenced clip changes what an effect reads, so its layer is
        // part of the key; a scene with none hashes as it always did.
        if !scene.references.is_empty() {
            let _ = writer.write_str("references:[");
            for layer in &scene.references {
                hash_layer_debug(&mut writer, project, layer);
            }
            let _ = writer.write_str("]");
        }

        for layer in scene.draw_order() {
            hash_layer_files(&mut hasher, project, layer);
        }

        let digest: [u8; 32] = hasher.finalize().into();
        Some(FrameKey(digest))
    }

    /// Whether a rendered frame for `key` exists on disk.
    pub fn contains(&self, key: &FrameKey) -> bool {
        self.path(key).is_file()
    }

    /// Load and decode the QOI frame for `key`, verifying that its dimensions
    /// match `expected`. A missing file, decode error, channel mismatch, or
    /// dimension mismatch returns `None`.
    pub fn load(&self, key: &FrameKey, expected: (u32, u32)) -> Option<RgbaImage> {
        let mut bytes = Vec::new();
        let mut image = RgbaImage::new(0, 0, Vec::new());
        self.load_into(key, expected, &mut bytes, &mut image)
            .then_some(image)
    }

    /// Decode `key` into caller-owned reusable buffers (`bytes_buf` for the raw
    /// QOI file, `image_buf` for the RGBA8 pixels). Returns `true` on a valid
    /// hit whose decoded size matches `expected`.
    pub fn load_into(
        &self,
        key: &FrameKey,
        expected: (u32, u32),
        bytes_buf: &mut Vec<u8>,
        image_buf: &mut RgbaImage,
    ) -> bool {
        if expected.0 == 0 || expected.1 == 0 {
            return false;
        }
        let path = self.path(key);
        let Ok(mut file) = fs::File::open(path) else {
            return false;
        };
        bytes_buf.clear();
        if file.read_to_end(bytes_buf).is_err() {
            return false;
        }
        let Ok(mut decoder) = qoi::Decoder::new(bytes_buf.as_slice()) else {
            return false;
        };
        let header = *decoder.header();
        if (header.width, header.height) != expected || header.channels != qoi::Channels::Rgba {
            return false;
        }
        let required = decoder.required_buf_len();
        let expected_bytes = (expected.0 as usize)
            .checked_mul(expected.1 as usize)
            .and_then(|px| px.checked_mul(4));
        if expected_bytes != Some(required) {
            return false;
        }
        image_buf.width = expected.0;
        image_buf.height = expected.1;
        image_buf.pixels.resize(required, 0);
        decoder
            .decode_to_buf(&mut image_buf.pixels[..required])
            .is_ok()
    }

    /// Encode `image` as QOI and atomically publish it at `<dir>/<2-hex>/<64-hex>.qoi`.
    pub fn store(&self, key: &FrameKey, image: &RgbaImage) -> io::Result<()> {
        if image.width == 0 || image.height == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot store a zero-sized render file",
            ));
        }
        let expected_len = (image.width as usize)
            .checked_mul(image.height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "render file dimensions overflow",
                )
            })?;
        if image.pixels.len() != expected_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "render file pixel buffer length does not match dimensions",
            ));
        }

        let encoded = qoi::encode_to_vec(&image.pixels, image.width, image.height)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

        let final_path = self.path(key);
        let shard_dir = final_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "render file path has no parent",
            )
        })?;
        fs::create_dir_all(shard_dir)?;

        let seq = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let temp_name = format!(".tmp-{}-{pid}-{seq}.qoi", key);
        let temp_path = shard_dir.join(temp_name);

        let write_result = (|| -> io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            file.write_all(&encoded)?;
            file.flush()?;
            drop(file);
            fs::rename(&temp_path, &final_path)?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    /// Remove the stored QOI file for `key`, returning `Ok(true)` when a file
    /// was removed and `Ok(false)` when none existed.
    pub fn remove(&self, key: &FrameKey) -> io::Result<bool> {
        match fs::remove_file(self.path(key)) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }
}

struct HashWriter<'a>(&'a mut Sha256);

impl fmt::Write for HashWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.update(s.as_bytes());
        Ok(())
    }
}

fn update_str(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn layer_has_stateful_pass(layer: &SceneLayer) -> bool {
    if layer.effects.iter().any(|pass| pass.is_stateful()) {
        return true;
    }
    match &layer.source {
        LayerSource::Transition {
            outgoing, incoming, ..
        } => layer_has_stateful_pass(outgoing) || layer_has_stateful_pass(incoming),
        LayerSource::Matte { layers, .. } => {
            layer_has_stateful_pass(&layers[0]) || layer_has_stateful_pass(&layers[1])
        }
        LayerSource::Nest { scene, .. } => scene.draw_order().any(layer_has_stateful_pass),
        _ => false,
    }
}

fn hash_layer_debug(writer: &mut HashWriter<'_>, project: &Project, layer: &SceneLayer) {
    let _ = write!(
        writer,
        "Layer{{clip:{:?},center:{:?},anchor:{:?},size:{:?},rot:{:?},op:{:?},uv:{:?},fx:{:?},masks:{:?},ck:{:?},grade:{:?},lut:{:?},blend:{:?},styles:{:?},",
        layer.clip,
        layer.center,
        layer.anchor_point,
        layer.size,
        layer.rotation,
        layer.opacity,
        layer.uv,
        layer.effects,
        layer.masks,
        layer.chroma_key,
        layer.color_grade,
        layer.lut,
        layer.blend_mode,
        layer.styles,
    );
    if let Some(clip_id) = layer.clip {
        if let Some(clip) = project.clip(clip_id) {
            let _ = write!(writer, "mb:{:?},", clip.motion_blur);
            if clip.motion_blur.enabled {
                let _ = write!(writer, "mb_xf:{:?},", clip.transform);
            }
        }
    } else {
        let _ = write!(writer, "blur:{:?},", layer.blur_passes);
    }
    match &layer.source {
        LayerSource::Transition {
            outgoing,
            incoming,
            transition_id,
            progress,
        } => {
            let _ = write!(
                writer,
                "src:Transition{{id:{transition_id:?},p:{progress:?},out:",
            );
            hash_layer_debug(writer, project, outgoing);
            let _ = writer.write_str(",in:");
            hash_layer_debug(writer, project, incoming);
            let _ = writer.write_str("}}");
        }
        LayerSource::Matte {
            layers,
            read,
            invert,
            softness,
        } => {
            let _ = write!(
                writer,
                "src:Matte{{read:{read:?},inv:{invert:?},soft:{softness:?},l0:"
            );
            hash_layer_debug(writer, project, &layers[0]);
            let _ = writer.write_str(",l1:");
            hash_layer_debug(writer, project, &layers[1]);
            let _ = writer.write_str("}}");
        }
        LayerSource::Nest { scene, frame } => {
            let _ = write!(
                writer,
                "src:Nest{{frame:{frame:?},w:{},h:{},tick:{:?},refs:",
                scene.width, scene.height, scene.tick,
            );
            for inner in &scene.references {
                hash_layer_debug(writer, project, inner);
            }
            let _ = writer.write_str(",layers:");
            for inner in &scene.layers {
                hash_layer_debug(writer, project, inner);
            }
            let _ = writer.write_str("}}");
        }
        other => {
            let _ = write!(writer, "src:{other:?}}}");
        }
    }
}

fn hash_layer_files(hasher: &mut Sha256, project: &Project, layer: &SceneLayer) {
    match &layer.source {
        LayerSource::Media { media, .. } | LayerSource::Still { media } => {
            hasher.update(b"media\0");
            hasher.update(media.raw().to_le_bytes());
            if let Some(item) = project.media(*media) {
                hash_file_identity(hasher, &item.path);
            } else {
                hasher.update([0u8]);
            }
        }
        LayerSource::Lottie { path, .. } => {
            hasher.update(b"lottie\0");
            hash_file_identity(hasher, Path::new(path));
        }
        LayerSource::Transition {
            outgoing, incoming, ..
        } => {
            hasher.update(b"transition\0");
            hash_layer_files(hasher, project, outgoing);
            hash_layer_files(hasher, project, incoming);
        }
        LayerSource::Matte { layers, .. } => {
            hasher.update(b"matte\0");
            hash_layer_files(hasher, project, &layers[0]);
            hash_layer_files(hasher, project, &layers[1]);
        }
        LayerSource::Nest { scene, .. } => {
            hasher.update(b"nest\0");
            for inner in scene.draw_order() {
                hash_layer_files(hasher, project, inner);
            }
        }
        LayerSource::Sticker { .. }
        | LayerSource::Text { .. }
        | LayerSource::Solid(_)
        | LayerSource::CanvasPass
        | LayerSource::Shape { .. }
        | LayerSource::PathShape { .. } => {}
    }

    if let Some(lut) = &layer.lut {
        hasher.update(b"lut\0");
        hash_file_identity(hasher, Path::new(&lut.path));
    }
}

fn hash_file_identity(hasher: &mut Sha256, path: &Path) {
    let raw = path.as_os_str().as_encoded_bytes();
    hasher.update((raw.len() as u64).to_le_bytes());
    hasher.update(raw);
    match fs::metadata(path) {
        Ok(meta) => {
            hasher.update([1u8]);
            hasher.update(meta.len().to_le_bytes());
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0u128, |d| d.as_nanos());
            hasher.update(mtime_ns.to_le_bytes());
        }
        Err(_) => {
            hasher.update([0u8]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_compositor::{EffectDefinition, PassInstance, pass_id, register_effect};
    use aria_core::{Rational, RationalTime};
    use aria_models::{
        ClipId, EffectParamKind, EffectParamSpec, EffectSpec, Generator, MediaSource, Param,
        TimeRange, TrackKind,
    };

    const SHADER_A: &str = "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return textureSample(input_tex, aria_sampler, in.uv);\n}\n";
    const SHADER_B: &str = "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return vec4<f32>(in.uv.x, 0.0, 0.0, 1.0);\n}\n";

    fn sample_project_with_solid() -> (Project, ClipId) {
        let mut project = Project::new("Render Files Test", Rational::FPS_30);
        let v1 = project.add_track(TrackKind::Sticker, "S1");
        let clip_id = project
            .add_generated(
                v1,
                Generator::SolidColor {
                    rgba: [30, 140, 220, 255],
                },
                TimeRange::at_rate(0, 4, Rational::FPS_30),
            )
            .expect("adds solid clip");
        (project, clip_id)
    }

    #[test]
    fn key_is_deterministic_across_render_files_instances_and_tracks_inputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rf_a = RenderFiles::new(temp.path().join("a"), "fx.one@1.0.0");
        let rf_b = RenderFiles::new(temp.path().join("b"), "fx.one@1.0.0");
        let rf_env = RenderFiles::new(temp.path().join("a"), "fx.one@1.1.0");

        let (mut project, clip_id) = sample_project_with_solid();
        let t0 = RationalTime::new(0, Rational::FPS_30);
        let scene = crate::resolve(&project, t0).expect("resolves");

        let k1 = rf_a.key(&project, &scene).expect("key a");
        let k2 = rf_b.key(&project, &scene).expect("key b");
        assert_eq!(
            k1, k2,
            "identical project/scene/env must yield identical FrameKey"
        );

        let k_env = rf_env.key(&project, &scene).expect("key env");
        assert_ne!(
            k1, k_env,
            "changing environment string must change FrameKey"
        );

        // Changing a clip parameter changes the resolved scene and the key.
        project.clip_mut(clip_id).expect("clip").transform.opacity = Param::Constant(0.5);
        let scene_opacity = crate::resolve(&project, t0).expect("resolves");
        let k_opacity = rf_a.key(&project, &scene_opacity).expect("key opacity");
        assert_ne!(k1, k_opacity, "changing clip opacity must change FrameKey");
    }

    #[test]
    fn key_changes_when_referenced_media_file_length_or_mtime_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let media_path = temp.path().join("source.bin");
        fs::write(&media_path, b"frame-bytes-v1").expect("writes media");

        let mut project = Project::new("Media Key Test", Rational::FPS_30);
        let media_id = project.add_media(MediaSource::new(
            media_path.clone(),
            64,
            64,
            Rational::FPS_30,
            30,
            false,
        ));
        let v1 = project.add_track(TrackKind::Video, "V1");
        project
            .add_clip(
                v1,
                media_id,
                TimeRange::at_rate(0, 10, Rational::FPS_30),
                RationalTime::new(0, Rational::FPS_30),
            )
            .expect("adds media clip");

        let rf = RenderFiles::new(temp.path().join("cache"), "");
        let scene =
            crate::resolve(&project, RationalTime::new(0, Rational::FPS_30)).expect("scene");
        let key_initial = rf.key(&project, &scene).expect("initial key");

        // Change file byte length.
        fs::write(&media_path, b"frame-bytes-v2-longer").expect("writes longer media");
        let key_longer = rf.key(&project, &scene).expect("longer key");
        assert_ne!(
            key_initial, key_longer,
            "media byte-length change must change FrameKey"
        );

        // Keep byte length identical, change mtime.
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&media_path)
            .expect("opens media");
        let bumped =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        file.set_times(fs::FileTimes::new().set_modified(bumped))
            .expect("sets mtime");
        drop(file);
        let key_mtime = rf.key(&project, &scene).expect("mtime key");
        assert_ne!(
            key_longer, key_mtime,
            "media mtime change must change FrameKey"
        );
    }

    #[test]
    fn key_tracks_registered_pass_wgsl_and_rejects_stateful_passes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rf = RenderFiles::new(temp.path(), "");
        let (project, _) = sample_project_with_solid();
        let t0 = RationalTime::new(0, Rational::FPS_30);

        let reg_a = register_effect(EffectDefinition {
            spec: EffectSpec {
                id: "test.rf.shader".into(),
                category: String::new(),
                label: "Shader".into(),
                params: vec![EffectParamSpec {
                    name: "amount".into(),
                    label: "Amount".into(),
                    kind: EffectParamKind::Scalar,
                    default: 0.5,
                    min: 0.0,
                    max: 1.0,
                    default_color: [0; 4],
                    default_vec2: [0.0; 2],
                    ..Default::default()
                }],
                generator: false,
                stateful: false,
                ..Default::default()
            },
            passes: vec![SHADER_A.into()],
        })
        .expect("registers shader A");

        let mut scene_a = crate::resolve(&project, t0).expect("scene");
        scene_a.layers[0].effects.push(PassInstance::new(
            pass_id("test.rf.shader").expect("pass"),
            vec![0.5],
        ));
        let key_a1 = rf.key(&project, &scene_a).expect("key a1");
        drop(reg_a);

        // Re-registering the exact same WGSL produces a new serial but the same key.
        let reg_a_again = register_effect(EffectDefinition {
            spec: EffectSpec {
                id: "test.rf.shader".into(),
                category: String::new(),
                label: "Shader".into(),
                params: vec![EffectParamSpec {
                    name: "amount".into(),
                    label: "Amount".into(),
                    kind: EffectParamKind::Scalar,
                    default: 0.5,
                    min: 0.0,
                    max: 1.0,
                    default_color: [0; 4],
                    default_vec2: [0.0; 2],
                    ..Default::default()
                }],
                generator: false,
                stateful: false,
                ..Default::default()
            },
            passes: vec![SHADER_A.into()],
        })
        .expect("re-registers shader A");
        let mut scene_a2 = crate::resolve(&project, t0).expect("scene");
        scene_a2.layers[0].effects.push(PassInstance::new(
            pass_id("test.rf.shader").expect("pass"),
            vec![0.5],
        ));
        let key_a2 = rf.key(&project, &scene_a2).expect("key a2");
        assert_eq!(
            key_a1, key_a2,
            "same WGSL across registrations must yield same FrameKey"
        );
        drop(reg_a_again);

        // Registering different WGSL under the same id changes the key.
        let reg_b = register_effect(EffectDefinition {
            spec: EffectSpec {
                id: "test.rf.shader".into(),
                category: String::new(),
                label: "Shader".into(),
                params: vec![EffectParamSpec {
                    name: "amount".into(),
                    label: "Amount".into(),
                    kind: EffectParamKind::Scalar,
                    default: 0.5,
                    min: 0.0,
                    max: 1.0,
                    default_color: [0; 4],
                    default_vec2: [0.0; 2],
                    ..Default::default()
                }],
                generator: false,
                stateful: false,
                ..Default::default()
            },
            passes: vec![SHADER_B.into()],
        })
        .expect("registers shader B");
        let mut scene_b = crate::resolve(&project, t0).expect("scene");
        scene_b.layers[0].effects.push(PassInstance::new(
            pass_id("test.rf.shader").expect("pass"),
            vec![0.5],
        ));
        let key_b = rf.key(&project, &scene_b).expect("key b");
        assert_ne!(key_a1, key_b, "changed WGSL source must change FrameKey");
        drop(reg_b);

        // A stateful pass makes RenderFiles::key return None.
        let _reg_stateful = register_effect(EffectDefinition {
            spec: EffectSpec {
                id: "test.rf.stateful".into(),
                category: String::new(),
                label: "Stateful".into(),
                params: Vec::new(),
                generator: false,
                stateful: true,
                ..Default::default()
            },
            passes: vec![SHADER_A.into()],
        })
        .expect("registers stateful shader");
        let mut scene_stateful = crate::resolve(&project, t0).expect("scene");
        scene_stateful.layers[0].effects.push(PassInstance::new(
            pass_id("test.rf.stateful").expect("stateful pass"),
            Vec::new(),
        ));
        assert_eq!(
            rf.key(&project, &scene_stateful),
            None,
            "stateful effect must never produce a FrameKey"
        );

        // A nest whose scene holds that effect is as stateful as the scene.
        let mut scene_nested = crate::resolve(&project, t0).expect("scene");
        scene_nested.layers[0].source = LayerSource::Nest {
            scene: Box::new(scene_stateful),
            frame: [0.0, 0.0, 1.0, 1.0],
        };
        assert_eq!(
            rf.key(&project, &scene_nested),
            None,
            "a nest holding a stateful effect must never produce a FrameKey"
        );
    }

    #[test]
    fn store_and_load_round_trip_exact_pixels_and_reject_size_or_format_version_mismatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rf = RenderFiles::new(temp.path(), "env@1");
        let (project, _) = sample_project_with_solid();
        let scene =
            crate::resolve(&project, RationalTime::new(0, Rational::FPS_30)).expect("scene");
        let key = rf.key(&project, &scene).expect("key");

        let image = RgbaImage::new(
            4,
            2,
            vec![
                10, 20, 30, 255, 40, 50, 60, 128, 70, 80, 90, 0, 100, 110, 120, 255, // row 0
                200, 210, 220, 255, 1, 2, 3, 4, 99, 88, 77, 66, 254, 253, 252, 255, // row 1
            ],
        );

        assert!(!rf.contains(&key));
        rf.store(&key, &image).expect("stores QOI");
        assert!(rf.contains(&key));

        let hex = key.to_hex();
        let expected_path = temp.path().join(&hex[..2]).join(format!("{hex}.qoi"));
        assert!(
            expected_path.is_file(),
            "shard file must exist at {expected_path:?}"
        );

        let loaded = rf.load(&key, (4, 2)).expect("loads exact size");
        assert_eq!(loaded, image);

        // Size mismatch is a miss.
        assert_eq!(rf.load(&key, (4, 3)), None);
        assert_eq!(rf.load(&key, (2, 2)), None);

        // A file stored under a stale FORMAT_VERSION key is not served for the current key.
        let stale_dir = tempfile::tempdir().expect("stale tempdir");
        let stale_rf = RenderFiles::new(stale_dir.path(), "env@1");
        let stale_key = stale_rf
            .key_with_format_version(FORMAT_VERSION.wrapping_sub(1), &project, &scene)
            .expect("stale key");
        let current_key = stale_rf.key(&project, &scene).expect("current key");
        assert_ne!(stale_key, current_key);
        stale_rf.store(&stale_key, &image).expect("stores stale");
        assert!(!stale_rf.contains(&current_key));
        assert_eq!(stale_rf.load(&current_key, (4, 2)), None);

        // Removing deletes the file.
        assert!(rf.remove(&key).expect("removes existing"));
        assert!(!rf.contains(&key));
        assert!(!rf.remove(&key).expect("second remove returns false"));
    }
}
