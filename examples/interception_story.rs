use std::fs::File;
use std::io::ErrorKind;

use retarget::{hook, install_registered_hooks, intercept::Signal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoIntercept {
    FileOpen,
}

#[hook::observer(default = retarget::intercept::EveryHit)]
fn on_interception(signal: Signal<DemoIntercept>) {
    println!(
        "observed {:?} via {} at {:?}",
        signal.value, signal.event.hook_id, signal.event.at
    );
}

#[cfg(target_os = "macos")]
#[hook::observe(DemoIntercept::FileOpen)]
#[hook::c]
unsafe extern "C" fn open(
    _path: *const libc::c_char,
    _flags: libc::c_int,
    _mode: libc::mode_t,
) -> libc::c_int {
    unsafe {
        *libc::__error() = libc::ENOENT;
    }
    -1
}

#[cfg(target_os = "windows")]
#[hook::observe(DemoIntercept::FileOpen)]
#[hook::c(("kernel32.dll", "CreateFileW"))]
unsafe extern "system" fn create_file_w(
    _path: *const u16,
    _access: u32,
    _share: u32,
    _security: *const std::ffi::c_void,
    _creation: u32,
    _flags: u32,
    _template: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    unsafe {
        windows_sys::Win32::Foundation::SetLastError(
            windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND,
        );
    }
    windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
}

fn main() -> std::io::Result<()> {
    install_registered_hooks()?;

    for path in ["Cargo.toml", "Cargo.lock"] {
        let error = File::open(path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    Ok(())
}
