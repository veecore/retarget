//! Objective-C resolution and method replacement helpers.

use libc::c_void;
use objc2::ffi;
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
    let owner = if is_instance {
        class
    } else {
        class.metaclass()
    };
    let resolved = if is_instance {
        class.instance_method(selector)
    } else {
        class.class_method(selector)
    }?;

    if has_direct_method(owner, selector) {
        return Some(NonNull::from(resolved).cast());
    }

    unsafe {
        localize_inherited_method(owner, selector, resolved);
    }

    let localized = if is_instance {
        class.instance_method(selector)
    } else {
        class.class_method(selector)
    }?;
    Some(NonNull::from(localized).cast())
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

/// Returns whether the given class object defines the selector directly.
fn has_direct_method(class: &AnyClass, selector: Sel) -> bool {
    class
        .instance_methods()
        .iter()
        .any(|method| method.name() == selector)
}

/// Copies one inherited method onto the concrete class object so later replacement
/// stays scoped to that class instead of mutating the shared ancestor slot.
unsafe fn localize_inherited_method(class: &AnyClass, selector: Sel, inherited: &Method) {
    let types = unsafe { ffi::method_getTypeEncoding(inherited) };
    if types.is_null() {
        return;
    }

    let class = (class as *const AnyClass).cast_mut();
    let _ = unsafe { ffi::class_addMethod(class, selector, inherited.implementation(), types) };
}
