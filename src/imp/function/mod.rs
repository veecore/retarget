//! Private exported-function resolution and replacement mechanics.

#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

use std::ffi::{CStr, CString, c_void};
use std::io;
use std::ptr::NonNull;

/// One raw resolved symbol address paired with its owning module details.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedSymbol {
    /// Resolved symbol address.
    pub(crate) symbol: NonNull<c_void>,
    /// Resolved owning module handle.
    pub(crate) module: NonNull<c_void>,
    /// Retained owning module name.
    pub(crate) module_name: CString,
    /// Retained symbol name.
    pub(crate) symbol_name: CString,
}

/// Builds one synthetic symbol name for one raw function address.
pub(crate) fn synthetic_symbol_name(address: NonNull<c_void>) -> CString {
    CString::new(format!("0x{:x}", address.as_ptr() as usize))
        .expect("hexadecimal pointer text must not contain interior NUL")
}

/// Resolves one already loaded module by name.
pub(crate) fn loaded_module_by_name(name: &CStr) -> Option<NonNull<c_void>> {
    #[cfg(unix)]
    {
        unix::loaded_module_by_name(name)
    }

    #[cfg(target_os = "windows")]
    {
        windows::loaded_module_by_name(name)
    }
}

/// Loads one module by name.
pub(crate) fn prime_module_by_name(name: &CStr) -> io::Result<NonNull<c_void>> {
    #[cfg(unix)]
    {
        unix::prime_module_by_name(name)
    }

    #[cfg(target_os = "windows")]
    {
        windows::prime_module_by_name(name)
    }
}

/// Recovers one retained module name from one already resolved module handle.
pub(crate) fn module_name_from_handle(module: NonNull<c_void>) -> io::Result<CString> {
    #[cfg(unix)]
    {
        unix::module_name_from_handle_impl(module)
    }

    #[cfg(target_os = "windows")]
    {
        windows::module_name_from_handle_impl(module)
    }
}

/// Resolves one exported symbol within one already resolved module handle.
pub(crate) fn resolve_symbol_in_module(
    symbol: &CStr,
    module: NonNull<c_void>,
) -> Option<NonNull<c_void>> {
    #[cfg(unix)]
    {
        unix::resolve_symbol_in_module_handle(module, symbol)
    }

    #[cfg(target_os = "windows")]
    {
        windows::resolve_symbol_in_module_handle(module, symbol)
    }
}

/// Resolves one exported symbol from the process-global namespace together with its owner.
pub(crate) fn resolve_symbol_global(symbol: &CStr) -> Option<ResolvedSymbol> {
    #[cfg(unix)]
    let (module, module_name, address) = unix::resolve_symbol_global_name(symbol)?;

    #[cfg(target_os = "windows")]
    let (module, module_name, address) = windows::resolve_symbol_global_name(symbol)?;

    Some(ResolvedSymbol {
        symbol: address,
        module,
        module_name,
        symbol_name: symbol.to_owned(),
    })
}

/// Recovers one resolved function address together with its owning module and symbol.
pub(crate) fn resolve_function_address(address: NonNull<c_void>) -> io::Result<ResolvedSymbol> {
    #[cfg(unix)]
    let (module, module_name, symbol_name) = unix::module_from_address_impl(address)?;

    #[cfg(target_os = "windows")]
    let (module, module_name, symbol_name) = windows::module_from_address_impl(address)?;

    Ok(ResolvedSymbol {
        symbol: address,
        module,
        module_name,
        symbol_name,
    })
}

/// Replaces one raw function entrypoint with one replacement implementation.
pub(crate) fn replace_function(
    target: NonNull<c_void>,
    replacement: NonNull<c_void>,
) -> io::Result<NonNull<c_void>> {
    #[cfg(unix)]
    {
        unix::replace_function(target, replacement)
    }

    #[cfg(target_os = "windows")]
    {
        windows::replace_function(target, replacement)
    }
}
