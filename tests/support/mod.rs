//! Shared integration-test helpers that should not compile as standalone test crates.

#![allow(dead_code)]

use std::sync::{Mutex, OnceLock};

/// Small reusable event buffer for one integration test binary.
pub struct TestBuffer<T> {
    entries: OnceLock<Mutex<Vec<T>>>,
}

impl<T> TestBuffer<T> {
    /// Builds one empty buffer.
    pub const fn new() -> Self {
        Self {
            entries: OnceLock::new(),
        }
    }

    /// Pushes one observed value into the shared buffer.
    pub fn push(&self, value: T) {
        self.entries()
            .lock()
            .expect("test buffer should stay available")
            .push(value);
    }

    /// Takes all buffered values, leaving the buffer empty again.
    pub fn take(&self) -> Vec<T> {
        std::mem::take(
            &mut *self
                .entries()
                .lock()
                .expect("test buffer should stay available"),
        )
    }

    fn entries(&self) -> &Mutex<Vec<T>> {
        self.entries.get_or_init(|| Mutex::new(Vec::new()))
    }
}

/// Shared typed entrypoint used by the caller fixture image.
pub type HookTestCallAddOneFn = unsafe extern "C" fn(i32) -> i32;
/// Shared typed entrypoint used by the target fixture image.
pub type HookTestAddOneFn = unsafe extern "C" fn(i32) -> i32;

#[cfg(target_os = "macos")]
type TestImageHandle = *mut std::ffi::c_void;

#[cfg(target_os = "windows")]
type TestImageHandle = windows_sys::Win32::Foundation::HMODULE;

/// Returns the absolute path to the helper target image compiled by `build.rs`.
pub fn test_dylib_path() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        env!("RETARGET_HOOK_TEST_DYLIB")
    }

    #[cfg(target_os = "windows")]
    {
        option_env!("RETARGET_HOOK_TEST_TARGET_DLL").expect(
            "Windows dynamic test target DLL path is only available on a Windows host build",
        )
    }
}

/// Returns the absolute path to the helper caller image compiled by `build.rs`.
pub fn test_caller_dylib_path() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        env!("RETARGET_HOOK_TEST_CALLER_DYLIB")
    }

    #[cfg(target_os = "windows")]
    {
        option_env!("RETARGET_HOOK_TEST_CALLER_DLL").expect(
            "Windows dynamic test caller DLL path is only available on a Windows host build",
        )
    }
}

/// Opens the helper target image and keeps it loaded for the rest of the test process.
pub fn open_test_dylib() -> TestImageHandle {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;

        let path = CString::new(test_dylib_path()).expect("test dylib path contained NUL");
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
        assert!(
            !handle.is_null(),
            "failed to dlopen helper dylib at {}",
            test_dylib_path()
        );
        handle.cast()
    }

    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        use windows_sys::Win32::System::LibraryLoader::LoadLibraryA;

        let path = CString::new(test_dylib_path()).expect("test target dll path contained NUL");
        let handle = unsafe { LoadLibraryA(path.as_ptr() as *const u8) };
        assert_ne!(
            handle,
            std::ptr::null_mut(),
            "failed to LoadLibraryA target DLL"
        );
        handle
    }
}

/// Opens the helper caller image and keeps it loaded for the rest of the test process.
pub fn open_test_caller_dylib() -> TestImageHandle {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;

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

    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        use windows_sys::Win32::System::LibraryLoader::LoadLibraryA;

        let path =
            CString::new(test_caller_dylib_path()).expect("test caller dll path contained NUL");
        let handle = unsafe { LoadLibraryA(path.as_ptr() as *const u8) };
        assert_ne!(
            handle,
            std::ptr::null_mut(),
            "failed to LoadLibraryA caller DLL"
        );
        handle
    }
}

/// Resolves the exported helper caller symbol from the given image handle.
pub fn load_hook_test_call_add_one(handle: TestImageHandle) -> HookTestCallAddOneFn {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::{CString, c_void};

        let symbol = CString::new("hook_test_call_add_one").expect("symbol contained NUL");
        let address = unsafe { libc::dlsym(handle.cast(), symbol.as_ptr()) };
        assert!(
            !address.is_null(),
            "failed to resolve hook_test_call_add_one"
        );
        unsafe { std::mem::transmute::<*mut c_void, HookTestCallAddOneFn>(address) }
    }

    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

        let symbol = CString::new("hook_test_call_add_one").expect("symbol contained NUL");
        let address = unsafe { GetProcAddress(handle, symbol.as_ptr() as *const u8) }
            .expect("failed to resolve hook_test_call_add_one");
        unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, HookTestCallAddOneFn>(
                address,
            )
        }
    }
}

/// Resolves the exported target symbol from the given image handle.
pub fn load_hook_test_add_one(handle: TestImageHandle) -> HookTestAddOneFn {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::{CString, c_void};

        let symbol = CString::new("hook_test_add_one").expect("symbol contained NUL");
        let address = unsafe { libc::dlsym(handle.cast(), symbol.as_ptr()) };
        assert!(!address.is_null(), "failed to resolve hook_test_add_one");
        unsafe { std::mem::transmute::<*mut c_void, HookTestAddOneFn>(address) }
    }

    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

        let symbol = CString::new("hook_test_add_one").expect("symbol contained NUL");
        let address = unsafe { GetProcAddress(handle, symbol.as_ptr() as *const u8) }
            .expect("failed to resolve hook_test_add_one");
        unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, HookTestAddOneFn>(address)
        }
    }
}

/// Opens both helper images and keeps them loaded for the rest of the test process.
pub fn open_test_images() -> (TestImageHandle, TestImageHandle) {
    (open_test_dylib(), open_test_caller_dylib())
}

/// Opens the caller image and returns its typed exported entrypoint.
pub fn open_test_caller() -> HookTestCallAddOneFn {
    let caller = open_test_caller_dylib();
    load_hook_test_call_add_one(caller)
}

/// Opens the target image and returns its typed exported entrypoint.
pub fn open_test_target() -> HookTestAddOneFn {
    let target = open_test_dylib();
    load_hook_test_add_one(target)
}

/// Calls the helper caller image, which in turn calls the helper target export.
pub fn call_test_caller_add_one(value: i32) -> i32 {
    let call_add_one = open_test_caller();
    unsafe { call_add_one(value) }
}

/// Calls the helper target image directly.
pub fn call_test_target_add_one(value: i32) -> i32 {
    let call_add_one = open_test_target();
    unsafe { call_add_one(value) }
}
