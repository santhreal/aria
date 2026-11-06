#![allow(unsafe_code)]

//! Linux file and confirmation dialogs: portal detection and managed `zenity`
//! fallback.
//!
//! On a Linux session where `org.freedesktop.portal.Desktop` has no active
//! `org.freedesktop.portal.FileChooser` backend, `rfd` waits for the D-Bus call
//! to time out and then spawns `zenity` without `--title`, without `--modal`,
//! without tying the child to the future or parent process, and swallowing a
//! missing binary as `None`. Moreover, `zenity` 4 (GTK4) itself queries the
//! broken desktop portal on startup unless `GTK_USE_PORTAL=0` is set, stalling a
//! second time before mapping its window behind the maximized editor.
//!
//! This module probes `org.freedesktop.portal.FileChooser`'s `version` property
//! once per session with a bounded timeout, caches the result, and when no
//! portal backend is available spawns `zenity` directly with:
//! - `--title` and `--modal`, plus X11 `WM_TRANSIENT_FOR`, `_NET_WM_STATE_MODAL`,
//!   `_NET_WM_STATE_ABOVE`, and `_NET_ACTIVE_WINDOW` against the active editor
//!   window so the dialog maps on top and focused;
//! - `GTK_USE_PORTAL=0`, `GIO_USE_VFS=local`, and
//!   `DBUS_SESSION_BUS_ADDRESS=unix:path=/dev/null` so GTK4 inside `zenity` maps
//!   immediately without waiting on a broken portal daemon;
//! - `PR_SET_PDEATHSIG(SIGKILL)` on the dedicated waiter thread and a `Drop`
//!   guard on the future so dropping the future or exiting the editor kills and
//!   reaps the child process;
//! - [`DialogError::NoBackend`] when neither backend is available, distinct from
//!   a cancelled dialog.

use std::ffi::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use crate::{DialogError, FileSpec};
use parking_lot::{Condvar, Mutex};

const RTLD_LAZY: c_int = 1;
const PR_SET_PDEATHSIG: c_int = 1;
const SIGKILL: c_ulong = 9;

const PORTAL_UNKNOWN: u8 = 0;
const PORTAL_AVAILABLE: u8 = 1;
const PORTAL_UNAVAILABLE: u8 = 2;

/// Bounded timeout (ms) when checking whether `org.freedesktop.portal.FileChooser`
/// is exported and responsive on the session bus.
const PORTAL_PROBE_TIMEOUT_MS: c_int = 1050;

static PORTAL_STATE: AtomicU8 = AtomicU8::new(PORTAL_UNKNOWN);
static PROBE_LOCK: Mutex<()> = Mutex::new(());

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn prctl(option: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong, arg5: c_ulong) -> c_int;
    fn getpid() -> c_int;
    fn getppid() -> c_int;
    fn _exit(status: c_int) -> !;
}

#[cfg(test)]
pub(crate) fn reset_portal_state_for_test() {
    PORTAL_STATE.store(PORTAL_UNKNOWN, Ordering::SeqCst);
}

/// Ask for one existing file.
pub(crate) async fn open_file(spec: FileSpec) -> Result<Option<PathBuf>, DialogError> {
    if portal_available() {
        return Ok(spec
            .build()
            .pick_file()
            .await
            .map(|handle| handle.path().to_path_buf()));
    }
    let cmd = build_file_command(&spec, FileMode::OpenOne);
    let raw = ZenityFuture::spawn(cmd).await?;
    Ok(raw.and_then(|s| parse_single_path(&s)))
}

/// Ask for any number of existing files.
pub(crate) async fn open_files(spec: FileSpec) -> Result<Vec<PathBuf>, DialogError> {
    if portal_available() {
        return Ok(spec
            .build()
            .pick_files()
            .await
            .unwrap_or_default()
            .iter()
            .map(|handle| handle.path().to_path_buf())
            .collect());
    }
    let cmd = build_file_command(&spec, FileMode::OpenMany);
    let raw = ZenityFuture::spawn(cmd).await?;
    Ok(raw.map(|s| parse_multi_paths(&s)).unwrap_or_default())
}

/// Ask where to write a file.
pub(crate) async fn save_file(spec: FileSpec) -> Result<Option<PathBuf>, DialogError> {
    if portal_available() {
        return Ok(spec
            .build()
            .save_file()
            .await
            .map(|handle| handle.path().to_path_buf()));
    }
    let cmd = build_file_command(&spec, FileMode::Save);
    let raw = ZenityFuture::spawn(cmd).await?;
    Ok(raw.and_then(|s| parse_single_path(&s)))
}

/// Ask for a directory.
pub(crate) async fn open_folder(spec: FileSpec) -> Result<Option<PathBuf>, DialogError> {
    if portal_available() {
        return Ok(spec
            .build()
            .pick_folder()
            .await
            .map(|handle| handle.path().to_path_buf()));
    }
    let cmd = build_file_command(&spec, FileMode::Folder);
    let raw = ZenityFuture::spawn(cmd).await?;
    Ok(raw.and_then(|s| parse_single_path(&s)))
}

/// Ask a question with two answers.
pub(crate) async fn confirm(title: String, question: String) -> Result<bool, DialogError> {
    let cmd = build_confirm_command(&title, &question);
    let raw = ZenityFuture::spawn(cmd).await?;
    Ok(raw.is_some())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileMode {
    OpenOne,
    OpenMany,
    Save,
    Folder,
}

pub(crate) fn build_file_command(spec: &FileSpec, mode: FileMode) -> Command {
    let mut cmd = Command::new("zenity");
    configure_zenity_env(&mut cmd);
    cmd.arg("--no-markup");
    match mode {
        FileMode::OpenOne => {
            cmd.args(["--file-selection", "--modal"]);
        }
        FileMode::OpenMany => {
            cmd.args(["--file-selection", "--multiple", "--modal"]);
        }
        FileMode::Save => {
            cmd.args([
                "--file-selection",
                "--save",
                "--confirm-overwrite",
                "--modal",
            ]);
        }
        FileMode::Folder => {
            cmd.args(["--file-selection", "--directory", "--modal"]);
        }
    }
    if !spec.title.is_empty() {
        cmd.args(["--title", &spec.title]);
    }
    for (name, exts) in &spec.filters {
        if !exts.is_empty() {
            let patterns: Vec<String> = exts.iter().map(|ext| format!("*.{ext}")).collect();
            cmd.args(["--file-filter", &format!("{name} | {}", patterns.join(" "))]);
        }
    }
    if let Some(name) = &spec.file_name {
        cmd.args(["--filename", name]);
    }
    if let Some(dir) = spec.directory.as_ref().filter(|dir| dir.is_dir()) {
        cmd.current_dir(dir);
    }
    cmd
}

pub(crate) fn build_confirm_command(title: &str, question: &str) -> Command {
    let mut cmd = Command::new("zenity");
    configure_zenity_env(&mut cmd);
    cmd.args(["--no-markup", "--question", "--modal"]);
    if !title.is_empty() {
        cmd.args(["--title", title]);
    }
    cmd.args([
        "--text",
        question,
        "--ok-label",
        "Ok",
        "--cancel-label",
        "Cancel",
    ]);
    cmd
}

fn configure_zenity_env(cmd: &mut Command) {
    cmd.env("GTK_USE_PORTAL", "0")
        .env("GIO_USE_VFS", "local")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
}

fn parse_single_path(stdout: &str) -> Option<PathBuf> {
    let trimmed = stdout.trim_end_matches(['\r', '\n']);
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn parse_multi_paths(stdout: &str) -> Vec<PathBuf> {
    let trimmed = stdout.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split('|')
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

struct Shared {
    state: Mutex<State>,
    spawned_cv: Condvar,
}

struct State {
    spawned: bool,
    cancelled: bool,
    child: Option<Child>,
    result: Option<Result<Option<String>, DialogError>>,
    waker: Option<Waker>,
}

struct ZenityFuture {
    shared: Arc<Shared>,
}

impl ZenityFuture {
    fn spawn(mut cmd: Command) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                spawned: false,
                cancelled: false,
                child: None,
                result: None,
                waker: None,
            }),
            spawned_cv: Condvar::new(),
        });

        let parent_xid = active_x11_window();
        let worker_shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            let mut guard = worker_shared.state.lock();
            if guard.cancelled {
                guard.spawned = true;
                worker_shared.spawned_cv.notify_all();
                return;
            }

            // Capture the waiter thread's process id before fork so the child
            // exits immediately if the parent process dies during fork.
            let parent_pid = unsafe { getpid() };
            unsafe {
                cmd.pre_exec(move || {
                    prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0);
                    if getppid() != parent_pid {
                        _exit(1);
                    }
                    Ok(())
                });
            }

            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(_) => {
                    guard.result = Some(Err(DialogError::NoBackend));
                    guard.spawned = true;
                    worker_shared.spawned_cv.notify_all();
                    return;
                }
            };

            let child_pid = child.id();
            let mut stdout = child.stdout.take();
            guard.child = Some(child);
            guard.spawned = true;
            worker_shared.spawned_cv.notify_all();
            drop(guard);

            let done = Arc::new(AtomicBool::new(false));
            if std::env::var_os("DISPLAY").is_some() {
                let raiser_done = Arc::clone(&done);
                std::thread::spawn(move || {
                    raise_zenity_window(child_pid, parent_xid, &raiser_done);
                });
            }

            let mut bytes = Vec::new();
            if let Some(pipe) = &mut stdout {
                let _ = pipe.read_to_end(&mut bytes);
            }
            done.store(true, Ordering::Release);

            let mut guard = worker_shared.state.lock();
            let status = guard.child.take().and_then(|mut c| c.wait().ok());
            if !guard.cancelled {
                let output = match status {
                    Some(exit) if exit.success() => {
                        Some(String::from_utf8_lossy(&bytes).into_owned())
                    }
                    _ => None,
                };
                guard.result = Some(Ok(output));
            }
            let waker = guard.waker.take();
            drop(guard);
            if let Some(waker) = waker {
                waker.wake();
            }
        });

        let mut guard = shared.state.lock();
        while !guard.spawned {
            shared.spawned_cv.wait(&mut guard);
        }
        drop(guard);

        Self { shared }
    }
}

impl Future for ZenityFuture {
    type Output = Result<Option<String>, DialogError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.shared.state.lock();
        if let Some(result) = guard.result.take() {
            return Poll::Ready(result);
        }
        guard.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for ZenityFuture {
    fn drop(&mut self) {
        let mut guard = self.shared.state.lock();
        guard.cancelled = true;
        guard.waker = None;
        if let Some(mut child) = guard.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Whether `org.freedesktop.portal.FileChooser` is available on the session bus.
///
/// Probed at most once per session so a missing or hung portal backend does not
/// stall every subsequent pick.
pub(crate) fn portal_available() -> bool {
    match PORTAL_STATE.load(Ordering::Acquire) {
        PORTAL_AVAILABLE => return true,
        PORTAL_UNAVAILABLE => return false,
        _ => {}
    }
    let _guard = PROBE_LOCK.lock();
    match PORTAL_STATE.load(Ordering::Acquire) {
        PORTAL_AVAILABLE => return true,
        PORTAL_UNAVAILABLE => return false,
        _ => {}
    }
    let ok = probe_portal_file_chooser(PORTAL_PROBE_TIMEOUT_MS);
    let next = if ok {
        PORTAL_AVAILABLE
    } else {
        PORTAL_UNAVAILABLE
    };
    PORTAL_STATE.store(next, Ordering::Release);
    ok
}

#[repr(C)]
struct DBusError {
    name: *const c_char,
    message: *const c_char,
    dummy: c_uint,
    padding1: *mut c_void,
}

#[repr(C)]
struct DBusMessageIter {
    dummy1: *mut c_void,
    dummy2: *mut c_void,
    dummy3: u32,
    dummy4: c_int,
    dummy5: c_int,
    dummy6: c_int,
    dummy7: c_int,
    dummy8: c_int,
    dummy9: c_int,
    dummy10: c_int,
    dummy11: c_int,
    pad1: c_int,
    pad2: *mut c_void,
    pad3: *mut c_void,
}

const DBUS_BUS_SESSION: c_int = 0;
const DBUS_TYPE_STRING: c_int = b's' as c_int;
const DBUS_TYPE_UINT32: c_int = b'u' as c_int;
const DBUS_TYPE_VARIANT: c_int = b'v' as c_int;

fn probe_portal_file_chooser(timeout_ms: c_int) -> bool {
    let Some(bus_addr) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS") else {
        return false;
    };
    if bus_addr.is_empty() || bus_addr == "unix:path=/dev/null" {
        return false;
    }
    let Some(path) = bus_socket_path(&bus_addr.to_string_lossy()) else {
        return false;
    };
    if !Path::new(&path).exists() {
        return false;
    }

    unsafe {
        let mut lib = dlopen(c"libdbus-1.so.3".as_ptr(), RTLD_LAZY);
        if lib.is_null() {
            lib = dlopen(c"libdbus-1.so".as_ptr(), RTLD_LAZY);
        }
        if lib.is_null() {
            return false;
        }

        type ErrorInit = unsafe extern "C" fn(*mut DBusError);
        type ErrorFree = unsafe extern "C" fn(*mut DBusError);
        type BusGetPrivate = unsafe extern "C" fn(c_int, *mut DBusError) -> *mut c_void;
        type ConnSetExit = unsafe extern "C" fn(*mut c_void, u32);
        type ConnClose = unsafe extern "C" fn(*mut c_void);
        type ConnUnref = unsafe extern "C" fn(*mut c_void);
        type MsgNewCall = unsafe extern "C" fn(
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
        ) -> *mut c_void;
        type MsgUnref = unsafe extern "C" fn(*mut c_void);
        type IterInitAppend = unsafe extern "C" fn(*mut c_void, *mut DBusMessageIter);
        type IterAppendBasic =
            unsafe extern "C" fn(*mut DBusMessageIter, c_int, *const c_void) -> u32;
        type SendBlock =
            unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut DBusError) -> *mut c_void;
        type IterInit = unsafe extern "C" fn(*mut c_void, *mut DBusMessageIter) -> u32;
        type IterGetType = unsafe extern "C" fn(*mut DBusMessageIter) -> c_int;
        type IterRecurse = unsafe extern "C" fn(*mut DBusMessageIter, *mut DBusMessageIter);
        type IterGetBasic = unsafe extern "C" fn(*mut DBusMessageIter, *mut c_void);

        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let ptr = dlsym(lib, $name.as_ptr());
                if ptr.is_null() {
                    return false;
                }
                std::mem::transmute::<*mut c_void, $ty>(ptr)
            }};
        }

        let dbus_error_init = sym!(c"dbus_error_init", ErrorInit);
        let dbus_error_free = sym!(c"dbus_error_free", ErrorFree);
        let dbus_bus_get_private = sym!(c"dbus_bus_get_private", BusGetPrivate);
        let dbus_connection_set_exit_on_disconnect =
            sym!(c"dbus_connection_set_exit_on_disconnect", ConnSetExit);
        let dbus_connection_close = sym!(c"dbus_connection_close", ConnClose);
        let dbus_connection_unref = sym!(c"dbus_connection_unref", ConnUnref);
        let dbus_message_new_method_call = sym!(c"dbus_message_new_method_call", MsgNewCall);
        let dbus_message_unref = sym!(c"dbus_message_unref", MsgUnref);
        let dbus_message_iter_init_append = sym!(c"dbus_message_iter_init_append", IterInitAppend);
        let dbus_message_iter_append_basic =
            sym!(c"dbus_message_iter_append_basic", IterAppendBasic);
        let dbus_connection_send_with_reply_and_block =
            sym!(c"dbus_connection_send_with_reply_and_block", SendBlock);
        let dbus_message_iter_init = sym!(c"dbus_message_iter_init", IterInit);
        let dbus_message_iter_get_arg_type = sym!(c"dbus_message_iter_get_arg_type", IterGetType);
        let dbus_message_iter_recurse = sym!(c"dbus_message_iter_recurse", IterRecurse);
        let dbus_message_iter_get_basic = sym!(c"dbus_message_iter_get_basic", IterGetBasic);

        let mut err: DBusError = std::mem::zeroed();
        dbus_error_init(&raw mut err);
        let conn = dbus_bus_get_private(DBUS_BUS_SESSION, &raw mut err);
        if conn.is_null() {
            dbus_error_free(&raw mut err);
            return false;
        }
        dbus_connection_set_exit_on_disconnect(conn, 0);

        let msg = dbus_message_new_method_call(
            c"org.freedesktop.portal.Desktop".as_ptr(),
            c"/org/freedesktop/portal/desktop".as_ptr(),
            c"org.freedesktop.DBus.Properties".as_ptr(),
            c"Get".as_ptr(),
        );
        if msg.is_null() {
            dbus_connection_close(conn);
            dbus_connection_unref(conn);
            dbus_error_free(&raw mut err);
            return false;
        }

        let mut iter: DBusMessageIter = std::mem::zeroed();
        dbus_message_iter_init_append(msg, &raw mut iter);
        let iface: *const c_char = c"org.freedesktop.portal.FileChooser".as_ptr();
        let prop: *const c_char = c"version".as_ptr();
        let ok_iface = dbus_message_iter_append_basic(
            &raw mut iter,
            DBUS_TYPE_STRING,
            (&raw const iface).cast(),
        );
        let ok_prop = dbus_message_iter_append_basic(
            &raw mut iter,
            DBUS_TYPE_STRING,
            (&raw const prop).cast(),
        );
        if ok_iface == 0 || ok_prop == 0 {
            dbus_message_unref(msg);
            dbus_connection_close(conn);
            dbus_connection_unref(conn);
            dbus_error_free(&raw mut err);
            return false;
        }

        let reply = dbus_connection_send_with_reply_and_block(conn, msg, timeout_ms, &raw mut err);
        dbus_message_unref(msg);
        dbus_connection_close(conn);
        dbus_connection_unref(conn);
        if reply.is_null() {
            dbus_error_free(&raw mut err);
            return false;
        }

        let mut version: u32 = 0;
        let mut r_iter: DBusMessageIter = std::mem::zeroed();
        if dbus_message_iter_init(reply, &raw mut r_iter) != 0
            && dbus_message_iter_get_arg_type(&raw mut r_iter) == DBUS_TYPE_VARIANT
        {
            let mut sub: DBusMessageIter = std::mem::zeroed();
            dbus_message_iter_recurse(&raw mut r_iter, &raw mut sub);
            if dbus_message_iter_get_arg_type(&raw mut sub) == DBUS_TYPE_UINT32 {
                dbus_message_iter_get_basic(&raw mut sub, (&raw mut version).cast());
            }
        }
        dbus_message_unref(reply);
        dbus_error_free(&raw mut err);
        version >= 1
    }
}

fn bus_socket_path(addr: &str) -> Option<String> {
    let rest = addr.strip_prefix("unix:")?;
    for part in rest.split(',') {
        if let Some(path) = part.strip_prefix("path=") {
            let trimmed = path.split(';').next().unwrap_or(path);
            return Some(trimmed.to_owned());
        }
    }
    None
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XClientMessageEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
    message_type: c_ulong,
    format: c_int,
    data_l: [c_long; 5],
    pad: [c_long; 12],
}

unsafe extern "C" fn ignore_x_error(_dpy: *mut c_void, _err: *mut c_void) -> c_int {
    0
}

struct X11Api {
    open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    close_display: unsafe extern "C" fn(*mut c_void) -> c_int,
    default_root_window: unsafe extern "C" fn(*mut c_void) -> c_ulong,
    intern_atom: unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_ulong,
    get_window_property: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_ulong,
        c_long,
        c_long,
        c_int,
        c_ulong,
        *mut c_ulong,
        *mut c_int,
        *mut c_ulong,
        *mut c_ulong,
        *mut *mut c_uchar,
    ) -> c_int,
    free: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_transient_for_hint: unsafe extern "C" fn(*mut c_void, c_ulong, c_ulong) -> c_int,
    raise_window: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    send_event: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_int,
        c_long,
        *mut XClientMessageEvent,
    ) -> c_int,
    flush: unsafe extern "C" fn(*mut c_void) -> c_int,
}

impl X11Api {
    unsafe fn load() -> Option<Self> {
        unsafe {
            let mut lib = dlopen(c"libX11.so.6".as_ptr(), RTLD_LAZY);
            if lib.is_null() {
                lib = dlopen(c"libX11.so".as_ptr(), RTLD_LAZY);
            }
            if lib.is_null() {
                return None;
            }
            type SetErr = unsafe extern "C" fn(
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
            ) -> *mut c_void;
            let set_err_ptr = dlsym(lib, c"XSetErrorHandler".as_ptr());
            if !set_err_ptr.is_null() {
                let set_err = std::mem::transmute::<*mut c_void, SetErr>(set_err_ptr);
                set_err(ignore_x_error);
            }

            macro_rules! sym {
                ($name:literal) => {{
                    let ptr = dlsym(lib, $name.as_ptr());
                    if ptr.is_null() {
                        return None;
                    }
                    std::mem::transmute(ptr)
                }};
            }

            Some(Self {
                open_display: sym!(c"XOpenDisplay"),
                close_display: sym!(c"XCloseDisplay"),
                default_root_window: sym!(c"XDefaultRootWindow"),
                intern_atom: sym!(c"XInternAtom"),
                get_window_property: sym!(c"XGetWindowProperty"),
                free: sym!(c"XFree"),
                set_transient_for_hint: sym!(c"XSetTransientForHint"),
                raise_window: sym!(c"XRaiseWindow"),
                send_event: sym!(c"XSendEvent"),
                flush: sym!(c"XFlush"),
            })
        }
    }
}

fn active_x11_window() -> Option<c_ulong> {
    std::env::var_os("DISPLAY")?;
    unsafe {
        let x11 = X11Api::load()?;
        let dpy = (x11.open_display)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let root = (x11.default_root_window)(dpy);
        let atom = (x11.intern_atom)(dpy, c"_NET_ACTIVE_WINDOW".as_ptr(), 0);
        let active = read_cardinal_list(&x11, dpy, root, atom, 1)
            .into_iter()
            .next()
            .filter(|&w| w != 0);
        (x11.close_display)(dpy);
        active
    }
}

fn raise_zenity_window(child_pid: u32, parent_xid: Option<c_ulong>, done: &AtomicBool) {
    unsafe {
        let Some(x11) = X11Api::load() else {
            return;
        };
        let dpy = (x11.open_display)(std::ptr::null());
        if dpy.is_null() {
            return;
        }
        let root = (x11.default_root_window)(dpy);
        let client_list_atom = (x11.intern_atom)(dpy, c"_NET_CLIENT_LIST".as_ptr(), 0);
        let wm_pid_atom = (x11.intern_atom)(dpy, c"_NET_WM_PID".as_ptr(), 0);
        let wm_state_atom = (x11.intern_atom)(dpy, c"_NET_WM_STATE".as_ptr(), 0);
        let modal_atom = (x11.intern_atom)(dpy, c"_NET_WM_STATE_MODAL".as_ptr(), 0);
        let above_atom = (x11.intern_atom)(dpy, c"_NET_WM_STATE_ABOVE".as_ptr(), 0);
        let active_atom = (x11.intern_atom)(dpy, c"_NET_ACTIVE_WINDOW".as_ptr(), 0);

        let deadline = Instant::now() + Duration::from_secs(3);
        while !done.load(Ordering::Acquire) && Instant::now() < deadline {
            let clients = read_cardinal_list(&x11, dpy, root, client_list_atom, 256);
            let mut target = None;
            for win in clients {
                let pid = read_cardinal_list(&x11, dpy, win, wm_pid_atom, 1)
                    .into_iter()
                    .next();
                if pid == Some(c_ulong::from(child_pid)) {
                    target = Some(win);
                    break;
                }
            }
            if let Some(win) = target {
                if let Some(parent) = parent_xid.filter(|&p| p != 0 && p != win) {
                    (x11.set_transient_for_hint)(dpy, win, parent);
                }
                const CLIENT_MESSAGE: c_int = 33;
                const SUBSTRUCTURE_MASK: c_long = (1 << 19) | (1 << 20);
                let mut state_ev = XClientMessageEvent {
                    type_: CLIENT_MESSAGE,
                    serial: 0,
                    send_event: 1,
                    display: dpy,
                    window: win,
                    message_type: wm_state_atom,
                    format: 32,
                    data_l: [1, modal_atom as c_long, above_atom as c_long, 1, 0],
                    pad: [0; 12],
                };
                (x11.send_event)(dpy, root, 0, SUBSTRUCTURE_MASK, &raw mut state_ev);
                (x11.raise_window)(dpy, win);

                let mut active_ev = XClientMessageEvent {
                    type_: CLIENT_MESSAGE,
                    serial: 0,
                    send_event: 1,
                    display: dpy,
                    window: win,
                    message_type: active_atom,
                    format: 32,
                    data_l: [2, 0, parent_xid.unwrap_or(0) as c_long, 0, 0],
                    pad: [0; 12],
                };
                (x11.send_event)(dpy, root, 0, SUBSTRUCTURE_MASK, &raw mut active_ev);
                (x11.flush)(dpy);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        (x11.close_display)(dpy);
    }
}

unsafe fn read_cardinal_list(
    x11: &X11Api,
    dpy: *mut c_void,
    win: c_ulong,
    prop_atom: c_ulong,
    max_longs: c_long,
) -> Vec<c_ulong> {
    unsafe {
        let mut actual_type: c_ulong = 0;
        let mut actual_format: c_int = 0;
        let mut nitems: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut prop: *mut c_uchar = std::ptr::null_mut();
        let rc = (x11.get_window_property)(
            dpy,
            win,
            prop_atom,
            0,
            max_longs,
            0,
            0,
            &raw mut actual_type,
            &raw mut actual_format,
            &raw mut nitems,
            &raw mut bytes_after,
            &raw mut prop,
        );
        if rc != 0 || prop.is_null() {
            return Vec::new();
        }
        let out = if actual_format == 32 && nitems > 0 {
            let slice = std::slice::from_raw_parts(prop.cast::<c_ulong>(), nitems as usize);
            slice.to_vec()
        } else {
            Vec::new()
        };
        (x11.free)(prop.cast());
        out
    }
}
