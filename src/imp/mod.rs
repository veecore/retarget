//! Private hook-engine implementation details.

use std::ffi::c_void;
use std::ptr::NonNull;

pub(crate) mod com;
pub(crate) mod function;
#[cfg(target_os = "macos")]
pub(crate) mod objc;

/// Converts one raw pointer to a typed function pointer.
///
/// The caller must ensure the pointer has the correct ABI and signature for
/// `T`.
pub(crate) unsafe fn ptr_to_fn<T: Copy>(ptr: NonNull<c_void>) -> T {
    let raw = ptr.as_ptr().cast_const();
    unsafe { std::mem::transmute_copy::<*const c_void, T>(&raw) }
}

/// Converts one typed function pointer to a non-null raw pointer.
pub(crate) fn fn_to_ptr<T: Copy>(value: T) -> NonNull<c_void> {
    let raw = unsafe { std::mem::transmute_copy::<T, *mut c_void>(&value) };
    NonNull::new(raw).expect("typed function pointers must not be null")
}
