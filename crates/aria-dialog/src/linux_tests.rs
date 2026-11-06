#![allow(unsafe_code)]

//! Regression tests for the Linux portal probe and managed `zenity` fallback.
//!
//! WHY: On Linux sessions where `org.freedesktop.portal.Desktop` has no active
//! `FileChooser` backend, `rfd` waited 25 s on D-Bus, spawned `zenity` without
//! `--title` or `--modal` (where GTK4 `zenity` waited another 25 s on the same
//! dead portal and then mapped behind the maximized editor window), orphaned
//! `zenity` on future drop or `Ctrl+Q` (`_exit(0)`), and returned `None` when no
//! backend was installed so a missing picker looked identical to a user cancel.
//!
//! Closes that class by testing:
//! 1. `zenity` `argv`, working directory, environment overrides, and stdout
//!    parsing across `open_file`, `open_files`, `save_file`, `open_folder`, and
//!    `confirm`.
//! 2. Dropping an in-flight dialog future immediately kills and reaps the
//!    `zenity` child process.
//! 3. Abrupt parent termination via `_exit(0)` kills the `zenity` child via
//!    `PR_SET_PDEATHSIG`.
//! 4. Missing both portal and `zenity` returns [`DialogError::NoBackend`] with
//!    `"No file picker is available. Install xdg-desktop-portal or zenity."`.
//! 5. A portal stub that never answers is probed once per session so the second
//!    pick spawns the fallback within 1 s.
//!
//! Does not catch: real X11 window manager stacking under Openbox, which is
//! covered by the headed Xvfb proof.

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::{Command, Stdio};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use crate::linux::reset_portal_state_for_test;
use crate::{
    DialogError, FileSpec, NO_PICKER_BACKEND_MESSAGE, confirm, open_file, open_files, open_folder,
    save_file,
};
use parking_lot::{Mutex, MutexGuard};

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn fork() -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn _exit(status: c_int) -> !;
}

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

struct EnvRestore {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvRestore {
    fn new(keys: &[&'static str]) -> Self {
        let saved = keys.iter().map(|&k| (k, std::env::var_os(k))).collect();
        Self { saved }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (k, val) in self.saved.drain(..) {
            unsafe {
                match val {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = pin!(fut);
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    loop {
        if let Poll::Ready(val) = fut.as_mut().poll(&mut cx) {
            return val;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn make_temp_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("aria-dialog-{label}"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("create test temp dir");
    base
}

fn write_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write script");
    let mut perms = std::fs::metadata(path).expect("stat script").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("chmod +x script");
}

#[test]
fn fake_zenity_on_path_receives_title_modal_filters_filename_and_modes() {
    let _lock = lock_env();
    let _restore = EnvRestore::new(&[
        "PATH",
        "DBUS_SESSION_BUS_ADDRESS",
        "DISPLAY",
        "ZENITY_LOG",
        "ZENITY_PWD",
        "ZENITY_ENV",
        "ZENITY_OUT",
        "ZENITY_EXIT",
    ]);
    reset_portal_state_for_test();

    let dir = make_temp_dir("argv");
    let start_dir = dir.join("start");
    std::fs::create_dir_all(&start_dir).expect("create start dir");
    let log_file = dir.join("argv.txt");
    let pwd_file = dir.join("pwd.txt");
    let env_file = dir.join("env.txt");
    let z_bin = dir.join("zenity");

    write_executable(
        &z_bin,
        "#!/bin/sh\n\
         : > \"$ZENITY_LOG\"\n\
         for arg in \"$@\"; do\n\
             printf '%s\\n' \"$arg\" >> \"$ZENITY_LOG\"\n\
         done\n\
         pwd > \"$ZENITY_PWD\"\n\
         printf '%s|%s|%s\\n' \"$GTK_USE_PORTAL\" \"$GIO_USE_VFS\" \"$DBUS_SESSION_BUS_ADDRESS\" > \"$ZENITY_ENV\"\n\
         if [ -n \"$ZENITY_OUT\" ]; then\n\
             printf '%s\\n' \"$ZENITY_OUT\"\n\
         fi\n\
         exit \"${ZENITY_EXIT:-0}\"\n",
    );

    let orig_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", format!("{}:{orig_path}", dir.display()));
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null");
        std::env::remove_var("DISPLAY");
        std::env::set_var("ZENITY_LOG", &log_file);
        std::env::set_var("ZENITY_PWD", &pwd_file);
        std::env::set_var("ZENITY_ENV", &env_file);
        std::env::set_var("ZENITY_OUT", "/tmp/clip.mp4");
        std::env::set_var("ZENITY_EXIT", "0");
    }

    let spec = FileSpec::new("Import Media")
        .filter("Video", &["mp4", "mov"])
        .filter("Audio", &["wav"])
        .file_name("clip.mp4")
        .directory(&start_dir);
    let chosen = block_on(open_file(spec)).expect("open_file via fake zenity");
    assert_eq!(chosen, Some(PathBuf::from("/tmp/clip.mp4")));

    let args: Vec<String> = std::fs::read_to_string(&log_file)
        .expect("read argv log")
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        args,
        vec![
            "--no-markup",
            "--file-selection",
            "--modal",
            "--title",
            "Import Media",
            "--file-filter",
            "Video | *.mp4 *.mov",
            "--file-filter",
            "Audio | *.wav",
            "--filename",
            "clip.mp4",
        ]
    );
    let recorded_pwd = std::fs::read_to_string(&pwd_file).expect("read pwd");
    assert_eq!(
        Path::new(recorded_pwd.trim()).canonicalize().ok(),
        start_dir.canonicalize().ok()
    );
    let recorded_env = std::fs::read_to_string(&env_file).expect("read env");
    assert_eq!(recorded_env.trim(), "0|local|unix:path=/dev/null");

    // Multiple files: `--multiple` and `|`-separated output.
    unsafe {
        std::env::set_var("ZENITY_OUT", "/tmp/a.mp4|/tmp/b.mov");
    }
    let multi = block_on(open_files(
        FileSpec::new("Import Media").filter("Video", &["mp4", "mov"]),
    ))
    .expect("open_files via fake zenity");
    assert_eq!(
        multi,
        vec![PathBuf::from("/tmp/a.mp4"), PathBuf::from("/tmp/b.mov")]
    );
    let multi_args: Vec<String> = std::fs::read_to_string(&log_file)
        .expect("read multi argv")
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        multi_args,
        vec![
            "--no-markup",
            "--file-selection",
            "--multiple",
            "--modal",
            "--title",
            "Import Media",
            "--file-filter",
            "Video | *.mp4 *.mov",
        ]
    );

    // Save file: `--save`, `--confirm-overwrite`, `--title`, `--file-filter`, `--filename`.
    unsafe {
        std::env::set_var("ZENITY_OUT", "/tmp/project.aria");
    }
    let saved = block_on(save_file(
        FileSpec::new("Save As")
            .filter("Aria Project", &["aria"])
            .file_name("Untitled.aria"),
    ))
    .expect("save_file via fake zenity");
    assert_eq!(saved, Some(PathBuf::from("/tmp/project.aria")));
    let save_args: Vec<String> = std::fs::read_to_string(&log_file)
        .expect("read save argv")
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        save_args,
        vec![
            "--no-markup",
            "--file-selection",
            "--save",
            "--confirm-overwrite",
            "--modal",
            "--title",
            "Save As",
            "--file-filter",
            "Aria Project | *.aria",
            "--filename",
            "Untitled.aria",
        ]
    );

    // Folder pick: `--directory`.
    unsafe {
        std::env::set_var("ZENITY_OUT", "/tmp/media-folder");
    }
    let folder =
        block_on(open_folder(FileSpec::new("Relink All"))).expect("open_folder via fake zenity");
    assert_eq!(folder, Some(PathBuf::from("/tmp/media-folder")));
    let folder_args: Vec<String> = std::fs::read_to_string(&log_file)
        .expect("read folder argv")
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        folder_args,
        vec![
            "--no-markup",
            "--file-selection",
            "--directory",
            "--modal",
            "--title",
            "Relink All",
        ]
    );

    // Confirm dialog: `--question`, `--modal`, `--title`, `--text`, labels, and exit code.
    let yes = block_on(confirm("Delete Project".into(), "Delete this file?".into()))
        .expect("confirm yes");
    assert!(yes);
    let confirm_args: Vec<String> = std::fs::read_to_string(&log_file)
        .expect("read confirm argv")
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        confirm_args,
        vec![
            "--no-markup",
            "--question",
            "--modal",
            "--title",
            "Delete Project",
            "--text",
            "Delete this file?",
            "--ok-label",
            "Ok",
            "--cancel-label",
            "Cancel",
        ]
    );

    unsafe {
        std::env::set_var("ZENITY_EXIT", "1");
    }
    let no =
        block_on(confirm("Delete Project".into(), "Delete this file?".into())).expect("confirm no");
    assert!(!no);
    let cancelled = block_on(open_file(FileSpec::new("Import Media"))).expect("cancelled open");
    assert_eq!(cancelled, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropping_the_dialog_future_kills_and_reaps_the_child() {
    let _lock = lock_env();
    let _restore = EnvRestore::new(&[
        "PATH",
        "DBUS_SESSION_BUS_ADDRESS",
        "DISPLAY",
        "ZENITY_PID_FILE",
    ]);
    reset_portal_state_for_test();

    let dir = make_temp_dir("drop");
    let pid_file = dir.join("zenity.pid");
    let z_bin = dir.join("zenity");

    write_executable(
        &z_bin,
        "#!/bin/sh\n\
         echo $$ > \"$ZENITY_PID_FILE\"\n\
         exec sleep 60\n",
    );

    let orig_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", format!("{}:{orig_path}", dir.display()));
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null");
        std::env::remove_var("DISPLAY");
        std::env::set_var("ZENITY_PID_FILE", &pid_file);
    }

    let mut fut = Box::pin(open_file(FileSpec::new("Import Media")));
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    assert_eq!(fut.as_mut().poll(&mut cx), Poll::Pending);

    let deadline = Instant::now() + Duration::from_secs(2);
    let pid: i32 = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file)
            && let Ok(parsed) = text.trim().parse::<i32>()
        {
            break parsed;
        }
        assert!(
            Instant::now() < deadline,
            "fake zenity did not record its pid"
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(
        unsafe { kill(pid, 0) },
        0,
        "zenity child {pid} must be alive while the future is pending"
    );

    drop(fut);

    assert_ne!(
        unsafe { kill(pid, 0) },
        0,
        "dropping the dialog future must kill and reap zenity child {pid}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parent_process_exit_kills_the_zenity_child() {
    let _lock = lock_env();
    let _restore = EnvRestore::new(&[
        "PATH",
        "DBUS_SESSION_BUS_ADDRESS",
        "DISPLAY",
        "ZENITY_PID_FILE",
    ]);
    reset_portal_state_for_test();

    let dir = make_temp_dir("pdeath");
    let pid_file = dir.join("zenity.pid");
    let z_bin = dir.join("zenity");

    write_executable(
        &z_bin,
        "#!/bin/sh\n\
         echo $$ > \"$ZENITY_PID_FILE\"\n\
         exec sleep 60\n",
    );

    let orig_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", format!("{}:{orig_path}", dir.display()));
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null");
        std::env::remove_var("DISPLAY");
        std::env::set_var("ZENITY_PID_FILE", &pid_file);
    }

    let child_proc = unsafe { fork() };
    assert!(child_proc >= 0, "fork failed");
    if child_proc == 0 {
        let mut fut = Box::pin(open_file(FileSpec::new("Import Media")));
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let _ = fut.as_mut().poll(&mut cx);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&pid_file)
                && text.trim().parse::<i32>().is_ok()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Exit via _exit(0) without dropping `fut`, matching `aria_decoder::exit::terminate_now`.
        unsafe { _exit(0) };
    }

    let mut status: c_int = 0;
    unsafe {
        waitpid(child_proc, &raw mut status, 0);
    }

    let z_pid: i32 = std::fs::read_to_string(&pid_file)
        .expect("zenity pid file written before parent exited")
        .trim()
        .parse()
        .expect("valid zenity pid");

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline && unsafe { kill(z_pid, 0) } == 0 {
        std::thread::sleep(Duration::from_millis(10));
    }
    let alive = unsafe { kill(z_pid, 0) } == 0;
    if alive {
        unsafe {
            kill(z_pid, 9);
        }
    }
    assert!(
        !alive,
        "zenity child {z_pid} survived parent _exit(0); PR_SET_PDEATHSIG did not fire"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_backend_returns_no_backend_error_distinct_from_cancel() {
    let _lock = lock_env();
    let _restore = EnvRestore::new(&["PATH", "DBUS_SESSION_BUS_ADDRESS", "DISPLAY"]);
    reset_portal_state_for_test();

    let empty_dir = make_temp_dir("nobackend");
    unsafe {
        std::env::set_var("PATH", &empty_dir);
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null");
        std::env::remove_var("DISPLAY");
    }

    let err1 = block_on(open_file(FileSpec::new("Open Project"))).unwrap_err();
    let err2 = block_on(open_files(FileSpec::new("Import Media"))).unwrap_err();
    let err3 = block_on(save_file(FileSpec::new("Save As"))).unwrap_err();
    let err4 = block_on(open_folder(FileSpec::new("Relink All"))).unwrap_err();
    let err5 = block_on(confirm("Delete".into(), "Delete?".into())).unwrap_err();

    for err in [err1, err2, err3, err4, err5] {
        assert_eq!(err, DialogError::NoBackend);
        assert_eq!(err.to_string(), NO_PICKER_BACKEND_MESSAGE);
        assert_eq!(
            err.as_str(),
            "No file picker is available. Install xdg-desktop-portal or zenity."
        );
    }

    let _ = std::fs::remove_dir_all(&empty_dir);
}

#[repr(C)]
struct DBusError {
    name: *const c_char,
    message: *const c_char,
    dummy: c_uint,
    padding1: *mut c_void,
}

#[test]
fn portal_stub_that_never_answers_is_detected_once_and_second_pick_spawns_fallback_within_one_second()
 {
    let _lock = lock_env();
    let _restore = EnvRestore::new(&["PATH", "DBUS_SESSION_BUS_ADDRESS", "DISPLAY"]);
    reset_portal_state_for_test();

    let dir = make_temp_dir("portal-stub");
    let z_bin = dir.join("zenity");
    write_executable(&z_bin, "#!/bin/sh\nprintf '/tmp/fallback.aria\\n'\n");

    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--print-address=1", "--nofork"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn isolated dbus-daemon");

    let mut addr = String::new();
    BufReader::new(daemon.stdout.take().expect("dbus-daemon stdout"))
        .read_line(&mut addr)
        .expect("read dbus-daemon address");
    let addr = addr.trim().to_owned();
    assert!(!addr.is_empty(), "dbus-daemon printed no address");

    let orig_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", format!("{}:{orig_path}", dir.display()));
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &addr);
        std::env::remove_var("DISPLAY");
    }

    // Claim `org.freedesktop.portal.Desktop` on the isolated session bus and never
    // reply to incoming method calls, simulating a hung portal activation.
    let stub_conn = unsafe {
        let lib = dlopen(c"libdbus-1.so.3".as_ptr(), 1);
        assert!(!lib.is_null(), "libdbus-1.so.3 must be present");
        type ErrorInit = unsafe extern "C" fn(*mut DBusError);
        type BusGetPrivate = unsafe extern "C" fn(c_int, *mut DBusError) -> *mut c_void;
        type ConnSetExit = unsafe extern "C" fn(*mut c_void, u32);
        type RequestName =
            unsafe extern "C" fn(*mut c_void, *const c_char, c_uint, *mut DBusError) -> c_int;
        type Flush = unsafe extern "C" fn(*mut c_void);

        let dbus_error_init: ErrorInit =
            std::mem::transmute(dlsym(lib, c"dbus_error_init".as_ptr()));
        let dbus_bus_get_private: BusGetPrivate =
            std::mem::transmute(dlsym(lib, c"dbus_bus_get_private".as_ptr()));
        let dbus_connection_set_exit_on_disconnect: ConnSetExit = std::mem::transmute(dlsym(
            lib,
            c"dbus_connection_set_exit_on_disconnect".as_ptr(),
        ));
        let dbus_bus_request_name: RequestName =
            std::mem::transmute(dlsym(lib, c"dbus_bus_request_name".as_ptr()));
        let dbus_connection_flush: Flush =
            std::mem::transmute(dlsym(lib, c"dbus_connection_flush".as_ptr()));

        let mut err: DBusError = std::mem::zeroed();
        dbus_error_init(&raw mut err);
        let conn = dbus_bus_get_private(0, &raw mut err);
        assert!(!conn.is_null(), "connect stub to isolated dbus-daemon");
        dbus_connection_set_exit_on_disconnect(conn, 0);
        let rc = dbus_bus_request_name(
            conn,
            c"org.freedesktop.portal.Desktop".as_ptr(),
            0,
            &raw mut err,
        );
        assert!(rc > 0, "claim org.freedesktop.portal.Desktop on stub bus");
        dbus_connection_flush(conn);
        conn
    };

    // First pick detects the unresponsive portal via the bounded probe timeout
    // and falls back to `zenity`.
    let first = block_on(save_file(FileSpec::new("Save As"))).expect("first pick falls back");
    assert_eq!(first, Some(PathBuf::from("/tmp/fallback.aria")));

    // Second pick must skip the dead portal and spawn the fallback within 1 s.
    let second_start = Instant::now();
    let second = block_on(save_file(FileSpec::new("Save As"))).expect("second pick falls back");
    let second_elapsed = second_start.elapsed();
    assert_eq!(second, Some(PathBuf::from("/tmp/fallback.aria")));
    assert!(
        second_elapsed < Duration::from_secs(1),
        "second pick took {second_elapsed:?}; broken portal was not cached across the session"
    );

    unsafe {
        let lib = dlopen(c"libdbus-1.so.3".as_ptr(), 1);
        type ConnClose = unsafe extern "C" fn(*mut c_void);
        type ConnUnref = unsafe extern "C" fn(*mut c_void);
        let close: ConnClose = std::mem::transmute(dlsym(lib, c"dbus_connection_close".as_ptr()));
        let unref: ConnUnref = std::mem::transmute(dlsym(lib, c"dbus_connection_unref".as_ptr()));
        close(stub_conn);
        unref(stub_conn);
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
