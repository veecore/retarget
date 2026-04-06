//! Private COM and vtable hook helpers.

#[cfg(target_os = "windows")]
use crate::imp::ptr_to_fn;
#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use std::ptr::NonNull;
#[cfg(target_os = "windows")]
use windows_core::Interface;

/// Returns the raw vtable slot address for one interface pointer and slot index.
///
/// # Safety
///
/// `interface_ptr` must point to a live COM interface whose vtable is valid for
/// reads at `slot_index`.
#[cfg(target_os = "windows")]
pub unsafe fn vtable_slot(
    interface_ptr: NonNull<c_void>,
    slot_index: usize,
) -> Option<NonNull<c_void>> {
    let vtable = unsafe { *(interface_ptr.as_ptr() as *mut *mut *mut c_void) };
    if vtable.is_null() {
        return None;
    }

    NonNull::new(unsafe { *vtable.add(slot_index) })
}

/// Returns one typed method pointer from one COM interface vtable slot.
///
/// # Safety
///
/// `interface_ptr` must point to a live COM interface whose vtable is valid for
/// reads at `slot_index`, and `T` must exactly match the ABI and signature of
/// that slot.
#[cfg(target_os = "windows")]
pub unsafe fn vtable_method<T: Copy>(
    interface_ptr: NonNull<c_void>,
    slot_index: usize,
) -> Option<T> {
    let slot = unsafe { vtable_slot(interface_ptr, slot_index)? };
    Some(unsafe { ptr_to_fn(slot) })
}

/// Returns one typed method pointer by projecting one Windows interface vtable field.
///
/// # Safety
///
/// `interface_ptr` must point to a live COM interface of type `TInterface`, and
/// `project` must read only a valid method field from that interface's vtable.
#[cfg(target_os = "windows")]
pub unsafe fn interface_method<TInterface, TMethod>(
    interface_ptr: NonNull<c_void>,
    project: impl FnOnce(&TInterface::Vtable) -> TMethod,
) -> Option<TMethod>
where
    TInterface: Interface,
    TMethod: Copy,
{
    let interface_ptr = interface_ptr.as_ptr();
    let interface = unsafe { <TInterface as Interface>::from_raw_borrowed(&interface_ptr) }?;
    Some(project(Interface::vtable(interface)))
}

/// Reads one interface pointer written to one out-parameter.
///
/// # Safety
///
/// `out` must be either null or point to readable storage containing one COM
/// interface pointer written by foreign code.
#[cfg(target_os = "windows")]
pub unsafe fn out_ptr_value(out: *mut *mut c_void) -> Option<NonNull<c_void>> {
    let value = unsafe { out.as_ref().copied()? };
    NonNull::new(value)
}
