//! Objective-C resolution and method replacement helpers.

use libc::c_void;
use objc2::runtime::{AnyClass, Imp, Method, Sel};
use std::ffi::CStr;
use std::ptr::NonNull;

/// Resolves one Objective-C class by name.
pub(crate) fn resolve_class(name: &CStr) -> Option<NonNull<c_void>> {
    AnyClass::get(name).map(|class| NonNull::from(class).cast())
}

/// Resolves one Objective-C method from one raw class pointer and selector name.
pub(crate) fn resolve_method(
    class: NonNull<c_void>,
    selector: &CStr,
    is_instance: bool,
) -> Option<NonNull<c_void>> {
    let class = unsafe { &*class.as_ptr().cast::<AnyClass>() };
    let selector = Sel::register(selector);
    let method = if is_instance {
        class.instance_method(selector)
    } else {
        class.class_method(selector)
    }?;
    Some(NonNull::from(method).cast())
}

/// Replaces one Objective-C method implementation and returns the previous implementation.
pub(crate) fn replace_method(
    method: NonNull<c_void>,
    replacement: NonNull<c_void>,
) -> NonNull<c_void> {
    let replacement_ptr = replacement.as_ptr();
    let replacement: Imp =
        unsafe { std::mem::transmute_copy::<*mut c_void, Imp>(&replacement_ptr) };
    let original_imp = unsafe { resolved_method(method).set_implementation(replacement) };
    let original = unsafe { std::mem::transmute_copy::<Imp, *mut c_void>(&original_imp) };
    NonNull::new(original).expect("Objective-C runtime must return the previous implementation")
}

/// Returns one typed Objective-C method reference from one raw method pointer.
fn resolved_method(method: NonNull<c_void>) -> &'static Method {
    unsafe { &*method.as_ptr().cast::<Method>() }
}
