use std::ffi::c_void;

use aria_plugin_abi::{
    ARIA_LOG_DEBUG, ARIA_LOG_INFO, ARIA_LOG_TRACE, ARIA_LOG_WARN, AriaBytes, AriaBytesResult,
    AriaResult, AriaStr,
};

use super::{call_ctx, read_str};
use crate::{LogLevel, host_wire};

pub(super) extern "C" fn host_log(host_ctx: *mut c_void, level: u32, text: AriaStr) {
    // SAFETY: `host_ctx` and `text` are provided by the guest during an active callback.
    let Ok(ctx) = (unsafe { call_ctx(host_ctx) }) else {
        return;
    };
    // SAFETY: `text` borrows guest memory for the duration of `host_log`.
    let Ok(message) = (unsafe { read_str(text, "log text") }) else {
        return;
    };
    let level = match level {
        ARIA_LOG_TRACE => LogLevel::Trace,
        ARIA_LOG_DEBUG => LogLevel::Debug,
        ARIA_LOG_INFO => LogLevel::Info,
        ARIA_LOG_WARN => LogLevel::Warn,
        _ => LogLevel::Error,
    };
    ctx.scoped_ctx.log_for(&ctx.state.id, level, message);
}

pub(super) extern "C" fn host_storage_get(host_ctx: *mut c_void, key: AriaStr) -> AriaBytesResult {
    // SAFETY: `host_ctx` and `key` are valid for the duration of `host_storage_get`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "storage key")?;
            ctx.scoped_ctx.storage_get(&ctx.state.id, key)
        })
    };
    match result {
        Ok(Some(bytes)) => AriaBytesResult::found(bytes),
        Ok(None) => AriaBytesResult::not_found(),
        Err(err) => AriaBytesResult::err(err),
    }
}

pub(super) extern "C" fn host_storage_set(
    host_ctx: *mut c_void,
    key: AriaStr,
    value: AriaBytes,
) -> AriaResult {
    // SAFETY: `host_ctx`, `key`, and `value` are valid for the duration of `host_storage_set`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "storage key")?;
            ctx.scoped_ctx
                .storage_set(&ctx.state.id, key, value.as_slice())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_storage_delete(host_ctx: *mut c_void, key: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `key` are valid for the duration of `host_storage_delete`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "storage key")?;
            ctx.scoped_ctx
                .storage_delete(&ctx.state.id, key)
                .map(|_| ())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_storage_keys(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_storage_keys`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let keys = ctx.scoped_ctx.storage_keys(&ctx.state.id)?;
            serde_json::to_string(&keys).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_setting_get(host_ctx: *mut c_void, key: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `key` are valid for the duration of `host_setting_get`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "setting key")?;
            host_wire::setting_get_json(ctx.scoped_ctx, key)
        })
    };
    match result {
        Ok(Some(json)) => AriaResult::ok(json),
        Ok(None) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}
