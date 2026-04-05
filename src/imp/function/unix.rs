//! Unix exported-function resolution and replacement mechanics.

use super::synthetic_symbol_name;
use libc::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};
use std::io;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

#[link(name = "dobby", kind = "static")]
unsafe extern "C" {
    /// Installs one inline hook and optionally returns one trampoline to the original.
    fn DobbyHook(
        address: *mut c_void,
        fake_func: *mut c_void,
        out_origin_func: *mut *mut c_void,
    ) -> c_int;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// Returns the number of loaded Mach-O images visible to dyld.
    fn _dyld_image_count() -> u32;
    /// Returns one loaded Mach-O image name by index.
    fn _dyld_get_image_name(image_index: u32) -> *const c_char;
}

/// Resolves one already loaded Unix image by name.
pub(super) fn loaded_module_by_name(name: &CStr) -> Option<NonNull<c_void>> {
    unsafe { NonNull::new(libc::dlopen(name.as_ptr(), loaded_unix_module_flags())) }
}

/// Loads one Unix image by name when auto-priming is enabled.
pub(super) fn prime_module_by_name(name: &CStr) -> io::Result<NonNull<c_void>> {
    let handle = unsafe { libc::dlopen(name.as_ptr(), load_unix_module_flags()) };
    NonNull::new(handle).ok_or_else(dlerror_io)
}

/// Recovers one module name from one Unix image handle when supported.
pub(super) fn module_name_from_handle_impl(module: NonNull<c_void>) -> io::Result<CString> {
    #[cfg(target_os = "macos")]
    {
        loaded_macos_module_name_by_handle(module).ok_or_else(|| {
            io::Error::other(
                "failed to recover one Unix module path from the provided module handle",
            )
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(io::Error::other(
            "module-name recovery from one Unix loader handle is unsupported on this target",
        ))
    }
}

/// Recovers one resolved module handle, retained module name, and nearest symbol from one Unix
/// function address.
pub(super) fn module_from_address_impl(
    address: NonNull<c_void>,
) -> io::Result<(NonNull<c_void>, CString, CString)> {
    let mut info = MaybeUninit::<libc::Dl_info>::zeroed();
    let ok = unsafe { libc::dladdr(address.as_ptr().cast(), info.as_mut_ptr()) };
    if ok == 0 {
        return Err(io::Error::other(
            "failed to recover module information from function address",
        ));
    }

    let info = unsafe { info.assume_init() };
    let path = NonNull::new(info.dli_fname.cast_mut())
        .ok_or_else(|| io::Error::other("resolved function address did not yield a module name"))?;
    let name = CString::from(unsafe { CStr::from_ptr(path.as_ptr() as *const c_char) });
    let handle = open_loaded_unix_module(&name).ok_or_else(|| {
        io::Error::other("resolved function address did not yield a loader handle")
    })?;
    let symbol_name = NonNull::new(info.dli_sname.cast_mut())
        .map(|name| CString::from(unsafe { CStr::from_ptr(name.as_ptr() as *const c_char) }))
        .unwrap_or_else(|| synthetic_symbol_name(address));
    Ok((handle, name, symbol_name))
}

/// Resolves one symbol within one specific Unix image.
pub(super) fn resolve_symbol_in_module_handle(
    module: NonNull<c_void>,
    symbol: &CStr,
) -> Option<NonNull<c_void>> {
    unsafe { NonNull::new(libc::dlsym(module.as_ptr(), symbol.as_ptr())) }
}

/// Resolves one symbol from the Unix process-global namespace and returns its owner.
pub(super) fn resolve_symbol_global_name(
    symbol: &CStr,
) -> Option<(NonNull<c_void>, CString, NonNull<c_void>)> {
    let address = unsafe { NonNull::new(libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr())) }?;

    let (module, name, _) = module_from_address_impl(address).ok()?;
    Some((module, name, address))
}

/// Replaces one exported function with one replacement implementation.
pub(super) fn replace_function(
    target: NonNull<c_void>,
    replacement: NonNull<c_void>,
) -> io::Result<NonNull<c_void>> {
    let mut original = std::ptr::null_mut();
    let status = unsafe { DobbyHook(target.as_ptr(), replacement.as_ptr(), &mut original) };
    if status != 0 {
        return Err(io::Error::other(format!(
            "failed to install the inline hook: status {status}",
        )));
    }

    NonNull::new(original).ok_or_else(|| {
        io::Error::other("inline hook installation returned a null original pointer")
    })
}

/// Recovers one loaded macOS image path by one loader handle.
#[cfg(target_os = "macos")]
fn loaded_macos_module_name_by_handle(handle: NonNull<c_void>) -> Option<CString> {
    let image_count = unsafe { _dyld_image_count() };

    for index in 0..image_count {
        let image_name = unsafe { _dyld_get_image_name(index) };
        if image_name.is_null() {
            continue;
        }

        let image_name = unsafe { CStr::from_ptr(image_name) };
        let Some(candidate) = open_loaded_unix_module(image_name) else {
            continue;
        };
        let matches = candidate == handle;
        let _ = unsafe { libc::dlclose(candidate.as_ptr()) };

        if matches {
            return Some(image_name.to_owned());
        }
    }

    None
}

/// Opens one already loaded Unix image and returns its loader handle.
fn open_loaded_unix_module(name: &CStr) -> Option<NonNull<c_void>> {
    unsafe { NonNull::new(libc::dlopen(name.as_ptr(), loaded_unix_module_flags())) }
}

/// Returns the flags used to open one already loaded Unix image.
const fn loaded_unix_module_flags() -> c_int {
    libc::RTLD_NOLOAD | load_unix_module_flags()
}

/// Returns the flags used to load one Unix image for symbol lookup.
const fn load_unix_module_flags() -> c_int {
    #[cfg(target_os = "macos")]
    {
        libc::RTLD_NOW | libc::RTLD_FIRST
    }
    #[cfg(not(target_os = "macos"))]
    {
        libc::RTLD_NOW
    }
}

/// Builds one `io::Error` from the active `dlerror` message.
fn dlerror_io() -> io::Error {
    io::Error::other(dlerror_detail().unwrap_or_else(|| "unknown dlopen error".to_string()))
}

/// Returns the current `dlerror` detail as owned text.
fn dlerror_detail() -> Option<String> {
    unsafe {
        let error = libc::dlerror();
        (!error.is_null()).then(|| {
            CStr::from_ptr(error as *const c_char)
                .to_string_lossy()
                .to_string()
        })
    }
}
