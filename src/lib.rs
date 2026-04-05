//! Low-level hook installation mechanics shared by injected hook code.
//!
//! This crate intentionally owns only hook-engine concerns such as symbol
//! resolution and platform-specific function or method attachment. Product
//! policy and reporting stay in higher layers.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("This crate only supports macOS and Windows");

/// Declares one function and exposes it as one concrete function-pointer value.
///
/// This is useful when one API expects a typed function pointer and Rust would
/// otherwise keep a bare function item uncoerced. The generated item keeps the
/// declared name, but that name refers to one function pointer constant instead
/// of the raw function item.
///
/// # Examples
///
/// ```ignore
/// use retarget::fn_pointer;
/// use std::ffi::c_int;
///
/// fn_pointer! {
///     unsafe extern "C" fn getpid_hook() -> c_int {
///         1
///     }
/// }
/// ```
#[macro_export]
macro_rules! fn_pointer {
    (
        $(#[$attr:meta])*
        $vis:vis unsafe extern $abi:literal fn $name:ident(
            $($arg:ident : $arg_ty:ty),* $(,)?
        ) $(-> $ret:ty)? $body:block
    ) => {
        $(#[$attr])*
        #[allow(non_upper_case_globals)]
        $vis const $name: unsafe extern $abi fn($($arg_ty),*) $(-> $ret)? = {
            unsafe extern $abi fn __blinder_impl($($arg : $arg_ty),*) $(-> $ret)? $body
            __blinder_impl
        };
    };
    (
        $(#[$attr:meta])*
        $vis:vis extern $abi:literal fn $name:ident(
            $($arg:ident : $arg_ty:ty),* $(,)?
        ) $(-> $ret:ty)? $body:block
    ) => {
        $(#[$attr])*
        #[allow(non_upper_case_globals)]
        $vis const $name: extern $abi fn($($arg_ty),*) $(-> $ret)? = {
            extern $abi fn __blinder_impl($($arg : $arg_ty),*) $(-> $ret)? $body
            __blinder_impl
        };
    };
}

#[cfg(target_os = "windows")]
mod com;
mod function;
mod imp;
#[cfg(target_os = "macos")]
pub mod objc;
#[cfg(feature = "registry")]
mod registry;

/// Shared retained-name helpers used by typed public error values.
mod error {
    use std::ffi::{CStr, NulError};
    use std::fmt;

    /// One invalid C-compatible input captured for diagnostics.
    #[derive(Debug)]
    pub(crate) struct InvalidName {
        /// Original bytes that failed validation.
        bytes: Vec<u8>,
        /// Offset of the first interior NUL byte.
        nul_position: usize,
    }

    impl InvalidName {
        /// Builds one invalid-input diagnostic from one `CString::new` failure.
        pub(crate) fn from_nul_error(source: NulError) -> Self {
            let nul_position = source.nul_position();
            let bytes = source.into_vec();
            Self {
                bytes,
                nul_position,
            }
        }
    }

    /// Returns one retained C-compatible name as UTF-8 text.
    pub(crate) fn expect_utf8(value: &CStr) -> &str {
        value
            .to_str()
            .expect("hook targets only store UTF-8 Rust strings")
    }

    /// Formats one invalid retained name with lossy escaped text.
    pub(crate) fn write_invalid_name(
        f: &mut fmt::Formatter<'_>,
        kind: &str,
        input: &InvalidName,
    ) -> fmt::Result {
        write!(
            f,
            "invalid {kind} name '{}': interior NUL at byte {}",
            String::from_utf8_lossy(&input.bytes).escape_debug(),
            input.nul_position
        )
    }
}

#[cfg(target_os = "windows")]
pub use com::{
    ComError, ComInstance, ComMethod, ComMethodId, IntoComInstance, IntoComMethod, IntoComMethodId,
    into_com_instance, into_com_method, into_com_method_id,
};
pub use function::{
    Function, FunctionError, FunctionPointer, FunctionReplaceError, IntoFunction, IntoModule,
    IntoSymbol, Module, ModuleError, Symbol, SymbolError, into_function, into_module, into_symbol,
};
#[cfg(target_os = "macos")]
pub use objc::{
    IntoObjcClass, IntoObjcMethod, IntoObjcSelector, ObjcClass, ObjcClassError, ObjcMethod,
    ObjcMethodError, ObjcMethodKind, ObjcSelector, ObjcSelectorError, into_objc_class,
    into_objc_method, into_objc_selector,
};
#[cfg(feature = "registry")]
pub use registry::{
    HookFailure, HookSpec, InterceptionEvent, InterceptionMode, InterceptionState,
    finish_named_install, install_registered_hooks, interception_snapshot, probe_hook,
};

/// User-facing hook declaration macros.
pub mod hook {
    /// Cross-platform exported function hooks.
    pub use retarget_macros::hook_c as function;
    /// Compatibility alias for cross-platform exported function hooks.
    pub use retarget_macros::hook_c as c;
    /// Windows COM or interface-style hooks.
    pub use retarget_macros::hook_com as com;
    /// Decorative impl-block grouping for COM hooks.
    pub use retarget_macros::hook_com_impl as com_impl;
    /// Per-hook interception observation override.
    pub use retarget_macros::hook_observe as observe;
    /// Hook interception observer registration.
    pub use retarget_macros::hook_observer as observer;

    /// Objective-C hook declarations.
    #[cfg(target_os = "macos")]
    pub mod objc {
        /// Objective-C class method hooks.
        pub use retarget_macros::hook_objc_class as class;
        /// Objective-C impl-block hook declarations.
        pub use retarget_macros::hook_objc_impl as methods;
        /// Objective-C instance method hooks.
        pub use retarget_macros::hook_objc_instance as instance;
    }
}

/// Internal support surface used by proc-macro expansion.
#[doc(hidden)]
pub mod __macro_support {
    pub use crate::function::{
        Function, IntoFunction, IntoModule, IntoSymbol, Module, Symbol, into_function, into_module,
        into_symbol,
    };
    #[cfg(target_os = "macos")]
    pub use crate::objc::{
        IntoObjcClass, IntoObjcMethod, IntoObjcSelector, ObjcMethod, ObjcMethodError,
        ObjcMethodKind, into_objc_class, into_objc_method, into_objc_selector,
    };
    #[cfg(feature = "registry")]
    pub use crate::registry::{
        HOOKS, HookDef, HookSpec, INTERCEPTION_OBSERVERS, INTERCEPTION_OVERRIDES, InterceptionMode,
        InterceptionObserverDef, InterceptionOverrideDef, finish_install, finish_named_install,
        probe_hook, record_interception, register_interception_hook,
    };
    #[cfg(feature = "registry")]
    pub use linkme::distributed_slice;

    /// Internal Windows install support.
    #[cfg(target_os = "windows")]
    pub mod windows {
        pub mod com {
            pub use crate::imp::com::*;
        }
    }
}
