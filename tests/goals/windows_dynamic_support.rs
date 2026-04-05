//! Shared helpers for Windows dynamic-library hook integration tests.

use std::ffi::CString;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

/// One typed function pointer for the helper caller symbol.
pub type HookTestCallAddOneFn = unsafe extern "system" fn(i32) -> i32;

/// Returns the absolute path to the helper target DLL compiled by the build script.
pub fn test_target_dll_path() -> &'static str {
    option_env!("BLINDER_HOOK_TEST_TARGET_DLL")
        .expect("Windows dynamic test target DLL path is only available on a Windows host build")
}

/// Returns the absolute path to the helper caller DLL compiled by the build script.
pub fn test_caller_dll_path() -> &'static str {
    option_env!("BLINDER_HOOK_TEST_CALLER_DLL")
        .expect("Windows dynamic test caller DLL path is only available on a Windows host build")
}

/// Opens the helper target DLL and returns the raw module handle.
pub fn open_test_target_dll() -> HMODULE {
    let path = CString::new(test_target_dll_path()).expect("test target dll path contained NUL");
    let handle = unsafe { LoadLibraryA(path.as_ptr() as *const u8) };
    assert_ne!(
        handle,
        std::ptr::null_mut(),
        "failed to LoadLibraryA target DLL"
    );
    handle
}

/// Opens the helper caller DLL and returns the raw module handle.
pub fn open_test_caller_dll() -> HMODULE {
    let path = CString::new(test_caller_dll_path()).expect("test caller dll path contained NUL");
    let handle = unsafe { LoadLibraryA(path.as_ptr() as *const u8) };
    assert_ne!(
        handle,
        std::ptr::null_mut(),
        "failed to LoadLibraryA caller DLL"
    );
    handle
}

/// Resolves the exported helper caller symbol from the given DLL handle.
pub fn load_hook_test_call_add_one(handle: HMODULE) -> HookTestCallAddOneFn {
    let symbol = CString::new("hook_test_call_add_one").expect("symbol contained NUL");
    let address = unsafe { GetProcAddress(handle, symbol.as_ptr() as *const u8) }
        .expect("failed to resolve hook_test_call_add_one");
    unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, HookTestCallAddOneFn>(address)
    }
}
