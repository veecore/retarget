//! Windows exported-function resolution and replacement mechanics.

use super::synthetic_symbol_name;
use std::ffi::{CStr, CString, c_char, c_void};
use std::io;
use std::ptr::NonNull;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
};

/// Matches the Windows `BOOL` ABI used by the imported Detours callbacks.
type BoolAbi = i32;

#[link(name = "detours_support", kind = "static")]
unsafe extern "system" {
    /// Attaches one process-wide function replacement in one Detours transaction owned by the
    /// calling thread.
    fn HookDetourAttach(pointer: *mut *mut c_void, detour: *mut c_void) -> i32;
    /// Returns one containing module handle for one code address.
    fn DetourGetContainingModule(address: *mut c_void) -> HMODULE;
    /// Returns the next loaded module after one prior module.
    fn DetourEnumerateModules(last: HMODULE) -> HMODULE;
    /// Enumerates one module's exported symbols.
    fn DetourEnumerateExports(
        module: HMODULE,
        context: *mut c_void,
        callback: unsafe extern "system" fn(
            *mut c_void,
            u32,
            *const c_char,
            *mut c_void,
        ) -> BoolAbi,
    ) -> BoolAbi;
}

/// One export-name search running against one specific module.
#[derive(Debug)]
struct ExportNameSearch {
    /// Target exported address to match.
    address: NonNull<c_void>,
    /// First matching exported symbol name.
    name: Option<CString>,
}

/// Resolves one already loaded Windows module by name.
pub(super) fn loaded_module_by_name(name: &CStr) -> Option<NonNull<c_void>> {
    let wide_name = encode_wide(name);
    unsafe { NonNull::new(GetModuleHandleW(wide_name.as_ptr()).cast()) }
}

/// Loads one Windows module by name.
pub(super) fn prime_module_by_name(name: &CStr) -> io::Result<NonNull<c_void>> {
    let wide_name = encode_wide(name);
    unsafe {
        let handle = LoadLibraryW(wide_name.as_ptr());
        NonNull::new(handle.cast()).ok_or_else(std::io::Error::last_os_error)
    }
}

/// Recovers one retained module name from one Windows module handle.
pub(super) fn module_name_from_handle_impl(module: NonNull<c_void>) -> io::Result<CString> {
    let mut buffer = vec![0u16; 260];

    loop {
        let length = unsafe {
            GetModuleFileNameW(
                module.as_ptr().cast(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
            )
        } as usize;

        if length == 0 {
            return Err(std::io::Error::last_os_error());
        }

        if length < buffer.len() - 1 {
            let name = String::from_utf16_lossy(&buffer[..length]);
            let file_name = std::path::Path::new(&name)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&name);
            return CString::new(file_name).map_err(io::Error::from);
        }

        buffer.resize(buffer.len() * 2, 0);
    }
}

/// Recovers one retained module handle, retained module name, and one symbol name from one
/// Windows function address.
pub(super) fn module_from_address_impl(
    address: NonNull<c_void>,
) -> io::Result<(NonNull<c_void>, CString, CString)> {
    let module = NonNull::new(unsafe { DetourGetContainingModule(address.as_ptr()) }.cast())
        .ok_or_else(|| {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(0) {
                io::Error::other("function address did not resolve to a Windows module")
            } else {
                error
            }
        })?;
    let module_name = module_name_from_handle_impl(module)?;
    let symbol_name =
        symbol_name_from_address(module, address).unwrap_or_else(|| synthetic_symbol_name(address));
    Ok((module, module_name, symbol_name))
}

/// Resolves one symbol within one specific Windows module.
pub(super) fn resolve_symbol_in_module_handle(
    module: NonNull<c_void>,
    symbol: &CStr,
) -> Option<NonNull<c_void>> {
    unsafe {
        let address = GetProcAddress(module.as_ptr().cast(), symbol.as_ptr() as *const u8)?;
        NonNull::new(address as *const () as *mut c_void)
    }
}

/// Resolves one symbol from the Windows process-global namespace and returns its owner.
pub(super) fn resolve_symbol_global_name(
    symbol: &CStr,
) -> Option<(NonNull<c_void>, CString, NonNull<c_void>)> {
    find_loaded_module(|module| {
        let address = resolve_symbol_in_module_handle(module, symbol)?;
        let name = module_name_from_handle_impl(module).ok()?;
        Some((module, name, address))
    })
}

/// Finds the first loaded Windows module for which one callback returns one result.
fn find_loaded_module<T>(mut callback: impl FnMut(NonNull<c_void>) -> Option<T>) -> Option<T> {
    let mut last = std::ptr::null_mut();

    loop {
        let module: NonNull<c_void> = NonNull::new(unsafe { DetourEnumerateModules(last) }.cast())?;
        last = module.as_ptr().cast();
        if let Some(value) = callback(module) {
            return Some(value);
        }
    }
}

/// Resolves one exported symbol name for one exact Windows code address.
fn symbol_name_from_address(module: NonNull<c_void>, address: NonNull<c_void>) -> Option<CString> {
    let mut search = ExportNameSearch {
        address,
        name: None,
    };
    let ok = unsafe {
        DetourEnumerateExports(
            module.as_ptr().cast(),
            (&mut search as *mut ExportNameSearch).cast(),
            record_export_name,
        )
    };
    (ok != 0).then_some(search.name).flatten()
}

/// Records one exported symbol name when it matches the requested Windows code address.
unsafe extern "system" fn record_export_name(
    context: *mut c_void,
    _ordinal: u32,
    symbol_name: *const c_char,
    symbol_address: *mut c_void,
) -> BoolAbi {
    let Some(search) = (unsafe { context.cast::<ExportNameSearch>().as_mut() }) else {
        return 1;
    };
    if symbol_name.is_null() || symbol_address != search.address.as_ptr() {
        return 1;
    }

    search.name = Some(unsafe { CStr::from_ptr(symbol_name) }.to_owned());
    0
}

/// Replaces one raw function entrypoint with one replacement implementation.
pub(super) fn replace_function(
    target: NonNull<c_void>,
    replacement: NonNull<c_void>,
) -> io::Result<NonNull<c_void>> {
    let mut original = target.as_ptr();
    let attach = unsafe { HookDetourAttach(&mut original, replacement.as_ptr()) };
    if attach != 0 {
        return Err(io::Error::other(format!(
            "failed to attach the replacement implementation: status {attach}",
        )));
    }

    NonNull::new(original)
        .ok_or_else(|| io::Error::other("replacement transaction returned a null original pointer"))
}

/// Encodes one C-compatible module name as one UTF-16 Windows path string.
fn encode_wide(value: &CStr) -> Vec<u16> {
    value
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}
