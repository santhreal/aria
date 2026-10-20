use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use aria_plugin::package::FsScope;
use aria_plugin::{LogLevel, host_wire};

use super::HostState;
use crate::bindings;

/// Marker type used with `PluginContext::has` to record a `CapabilityDenial` when a plugin
/// attempts to read or write an unpermitted filesystem path via `fs-read` or `fs-write`.
struct FsPermission;

/// Most bytes one `fs-read-at` call returns.
const FS_READ_AT_MAX: u32 = 64 * 1024 * 1024;

impl HostState {
    fn storage_root(&self) -> PathBuf {
        self.package.data_dir(&self.data_root)
    }

    fn resolve_storage_key(&self, key: &str) -> Result<PathBuf, String> {
        let rel = sanitize_relative_path(key).ok_or_else(|| {
            format!("invalid storage key {key:?}: must be a relative path without `..`")
        })?;
        Ok(self.storage_root().join(rel))
    }

    fn check_fs_path(&self, raw_path: &str, write: bool) -> Result<PathBuf, String> {
        let path = Path::new(raw_path);
        let active = if write {
            &self.active_encode_path
        } else {
            &self.active_read_path
        };
        if let Some(active) = active
            && path == active
        {
            return Ok(active.clone());
        }

        for scope in &self.package.manifest.permissions.fs {
            if let FsScope::Path(prefix) = scope
                && path.is_absolute()
                && !contains_parent_dir(path)
                && path.starts_with(prefix)
            {
                return Ok(path.to_path_buf());
            }
        }

        if let Ok(ctx) = self.require_ctx() {
            let _ = ctx.has::<FsPermission>();
        }
        let mode = if write { "write" } else { "read" };
        Err(format!(
            "permission denied: {}: fs {mode} for {raw_path:?} is not granted by permissions.fs",
            self.package.manifest.id
        ))
    }

    pub(super) fn host_log(&mut self, level: bindings::LogLevel, text: String) {
        let plugin = self.package.manifest.id.as_str();
        if let Some(ctx) = &self.active_ctx {
            let level = match level {
                bindings::LogLevel::Trace => LogLevel::Trace,
                bindings::LogLevel::Debug => LogLevel::Debug,
                bindings::LogLevel::Info => LogLevel::Info,
                bindings::LogLevel::Warn => LogLevel::Warn,
                bindings::LogLevel::Error => LogLevel::Error,
            };
            ctx.log_for(plugin, level, &text);
        } else {
            match level {
                bindings::LogLevel::Trace => tracing::trace!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Debug => tracing::debug!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Info => tracing::info!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Warn => tracing::warn!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Error => tracing::error!(plugin = %plugin, "{text}"),
            }
        }
    }

    pub(super) fn host_storage_get(&mut self, key: String) -> Result<Option<Vec<u8>>, String> {
        let path = self.resolve_storage_key(&key)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(format!("storage_get {key:?}: {err}")),
        }
    }

    pub(super) fn host_storage_set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        let path = self.resolve_storage_key(&key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("storage_set {key:?}: {err}"))?;
        }
        fs::write(&path, &value).map_err(|err| format!("storage_set {key:?}: {err}"))
    }

    pub(super) fn host_storage_delete(&mut self, key: String) -> Result<(), String> {
        let path = self.resolve_storage_key(&key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("storage_delete {key:?}: {err}")),
        }
    }

    pub(super) fn host_storage_keys(&mut self) -> Result<Vec<String>, String> {
        let root = self.storage_root();
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut keys = Vec::new();
        collect_storage_keys(&root, &root, &mut keys)?;
        keys.sort();
        Ok(keys)
    }

    pub(super) fn host_setting_get(&mut self, key: String) -> Result<Option<String>, String> {
        let ctx = self.require_ctx()?;
        host_wire::setting_get_json(&ctx, &key)
    }

    pub(super) fn host_fs_read(&mut self, path: String) -> Result<Vec<u8>, String> {
        let allowed = self.check_fs_path(&path, false)?;
        fs::read(&allowed).map_err(|err| format!("fs_read {path:?}: {err}"))
    }

    pub(super) fn host_fs_read_at(
        &mut self,
        path: String,
        offset: u64,
        len: u32,
    ) -> Result<Vec<u8>, String> {
        if len > FS_READ_AT_MAX {
            return Err(format!(
                "fs_read_at {path:?}: {len} bytes is over the {FS_READ_AT_MAX}-byte limit per call"
            ));
        }
        let allowed = self.check_fs_path(&path, false)?;
        let mut file =
            fs::File::open(&allowed).map_err(|err| format!("fs_read_at {path:?}: {err}"))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|err| format!("fs_read_at {path:?}: {err}"))?;
        let mut bytes = Vec::with_capacity(len as usize);
        file.take(u64::from(len))
            .read_to_end(&mut bytes)
            .map_err(|err| format!("fs_read_at {path:?}: {err}"))?;
        Ok(bytes)
    }

    pub(super) fn host_fs_len(&mut self, path: String) -> Result<u64, String> {
        let allowed = self.check_fs_path(&path, false)?;
        fs::metadata(&allowed)
            .map(|meta| meta.len())
            .map_err(|err| format!("fs_len {path:?}: {err}"))
    }

    pub(super) fn host_fs_write(&mut self, path: String, bytes: Vec<u8>) -> Result<(), String> {
        if let Some(active) = &mut self.active_export
            && path == active.path
        {
            active.bytes = bytes;
            return Ok(());
        }
        let allowed = self.check_fs_path(&path, true)?;
        if let Some(parent) = allowed.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("fs_write {path:?}: {err}"))?;
        }
        fs::write(&allowed, &bytes).map_err(|err| format!("fs_write {path:?}: {err}"))
    }
}

pub(crate) fn sanitize_relative_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    let path = Path::new(raw);
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn contains_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|comp| matches!(comp, Component::ParentDir))
}

fn collect_storage_keys(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.is_dir() {
            collect_storage_keys(root, &path, out)?;
        } else if path.is_file() {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let key = rel.to_string_lossy().replace('\\', "/");
            if !key.is_empty() {
                out.push(key);
            }
        }
    }
    Ok(())
}
