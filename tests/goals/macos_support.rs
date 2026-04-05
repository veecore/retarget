//! Shared helpers for macOS hook integration tests.

#![allow(dead_code)]

use std::ffi::{CString, c_void};

/// One typed function pointer for the helper caller symbol.
pub type HookTestCallAddOneFn = unsafe extern "C" fn(i32) -> i32;

/// Returns the absolute path to the helper dylib compiled by the build script.
pub fn test_dylib_path() -> &'static str {
    env!("BLINDER_HOOK_TEST_DYLIB")
}

/// Returns the absolute path to the helper caller dylib compiled by the build script.
pub fn test_caller_dylib_path() -> &'static str {
    env!("BLINDER_HOOK_TEST_CALLER_DYLIB")
}

/// Opens the helper dylib and returns the raw handle.
///
/// The library stays loaded for the rest of the test process.
pub fn open_test_dylib() -> *mut c_void {
    let path = CString::new(test_dylib_path()).expect("test dylib path contained NUL");
    let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    assert!(
        !handle.is_null(),
        "failed to dlopen helper dylib at {}",
        test_dylib_path()
    );
    handle.cast()
}

/// Opens the helper caller dylib and returns the raw handle.
///
/// The library stays loaded for the rest of the test process.
pub fn open_test_caller_dylib() -> *mut c_void {
    let path =
        CString::new(test_caller_dylib_path()).expect("test caller dylib path contained NUL");
    let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    assert!(
        !handle.is_null(),
        "failed to dlopen helper caller dylib at {}",
        test_caller_dylib_path()
    );
    handle.cast()
}

/// Resolves the exported helper caller symbol from the given dylib handle.
pub fn load_hook_test_call_add_one(handle: *mut c_void) -> HookTestCallAddOneFn {
    let symbol = CString::new("hook_test_call_add_one").expect("symbol contained NUL");
    let address = unsafe { libc::dlsym(handle.cast(), symbol.as_ptr()) };
    assert!(
        !address.is_null(),
        "failed to resolve hook_test_call_add_one"
    );
    unsafe { std::mem::transmute::<*mut c_void, HookTestCallAddOneFn>(address) }
}
