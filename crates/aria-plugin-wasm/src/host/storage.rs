use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use aria_plugin::{LogLevel, PluginContext, host_wire};

use super::HostState;
use crate::bindings;

/// Marker type used with `PluginContext::has` to record a `CapabilityDenial` when a plugin
/// attempts to read or write an unpermitted filesystem path via `fs-read` or `fs-write`.
pub(super) struct FsPermission;

/// Most bytes one `fs-read-at` call returns.
const FS_READ_AT_MAX: u32 = 64 * 1024 * 1024;

impl HostState {
    fn sanitize_storage_key(key: &str) -> Result<PathBuf, String> {
        sanitize_relative_path(key).ok_or_else(|| {
            format!("invalid storage key {key:?}: must be a relative path without `..`")
        })
    }

    fn normalize_project_storage_key(key: &str) -> Result<String, String> {
        let rel = Self::sanitize_storage_key(key)?;
        Ok(rel.to_string_lossy().replace('\\', "/"))
    }

    fn require_project_host_for_storage(
        &self,
        op: &str,
    ) -> Result<std::sync::Arc<aria_plugin::ProjectHost>, String> {
        let plugin_id = &self.package.manifest.id;
        let ctx = self.active_ctx.as_ref().ok_or_else(|| {
            format!(
                "{op} failed for plugin `{plugin_id}`: plugin runs with no open project (no active PluginContext)"
            )
        })?;
        ctx.unscoped_project_host().ok_or_else(|| {
            format!(
                "{op} failed for plugin `{plugin_id}`: plugin runs with no open project (no ProjectHost is registered)"
            )
        })
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

        let project_dir = self
            .active_ctx
            .as_ref()
            .and_then(PluginContext::unscoped_project_host)
            .and_then(|host| host_wire::project_dir(&host));
        if host_wire::fs_scope_grants(&self.package, project_dir.as_deref(), path) {
            return Ok(path.to_path_buf());
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
        let norm = Self::normalize_project_storage_key(&key)?;
        let host = self.require_project_host_for_storage("storage_get")?;
        Ok(host.storage_get(&self.package.manifest.id, &norm))
    }

    pub(super) fn host_storage_set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        let norm = Self::normalize_project_storage_key(&key)?;
        let host = self.require_project_host_for_storage("storage_set")?;
        host.storage_set(&self.package.manifest.id, &norm, value);
        Ok(())
    }

    pub(super) fn host_storage_delete(&mut self, key: String) -> Result<(), String> {
        let norm = Self::normalize_project_storage_key(&key)?;
        let host = self.require_project_host_for_storage("storage_delete")?;
        host.storage_delete(&self.package.manifest.id, &norm);
        Ok(())
    }

    pub(super) fn host_storage_keys(&mut self) -> Result<Vec<String>, String> {
        let host = self.require_project_host_for_storage("storage_keys")?;
        Ok(host.storage_keys(&self.package.manifest.id))
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

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
