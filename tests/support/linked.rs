//! Helpers for tests that intentionally link the fixture target into the binary.

#[cfg(target_os = "macos")]
#[link(name = "hook_test_target")]
unsafe extern "C" {
    #[link_name = "hook_test_add_one"]
    fn linked_hook_test_add_one_ffi(value: i32) -> i32;
}

#[cfg(target_os = "windows")]
#[link(kind = "static", name = "hook_test_target_static")]
unsafe extern "C" {
    #[link_name = "hook_test_add_one"]
    fn linked_hook_test_add_one_ffi(value: i32) -> i32;
}

/// Calls the statically or already-linked fixture export directly.
pub fn call_linked_hook_test_add_one(value: i32) -> i32 {
    unsafe { linked_hook_test_add_one_ffi(value) }
}
