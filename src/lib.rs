//! Small, typed, straightforward hooks for macOS and Windows.
//!
//! `retarget` is meant to make common native hooking cases feel like normal
//! Rust code instead of a pile of runtime plumbing. The public surface is
//! intentionally root-first and declarative:
//!
//! - `hook::c` for exported functions
//! - `hook::objc::*` for Objective-C methods
//! - `hook::com_impl` for interface-oriented COM hooks
//! - `install_registered_hooks()` to activate everything
//! - `intercept::{Mode, Signal}` when you want lightweight observation
//!
//! The crate still owns the hard parts internally, but the intended user
//! experience is convenient, compact, and easy to read in one file.
//!
//! # Why Use It
//!
//! `retarget` aims for a few specific qualities:
//!
//! - small hook declarations instead of manual registration boilerplate
//! - typed target resolution over raw stringly glue
//! - one install step for the whole crate
//! - optional observation without forcing a heavyweight runtime model
//! - escape hatches when you need lower-level target types directly
//!
//! # Getting Started
//!
//! ```ignore
//! use retarget::{
//!     hook,
//!     install_registered_hooks,
//!     intercept::{Mode, Signal},
//! };
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Notice {
//!     ProcessId,
//! }
//!
//! #[hook::observer(default = Mode::FirstHit)]
//! fn on_interception(signal: Signal<Notice>) {
//!     eprintln!("observed {:?} from {}", signal.value, signal.event.hook_id);
//! }
//!
//! #[hook::observe(Notice::ProcessId)]
//! #[hook::c]
//! unsafe extern "C" fn getpid() -> libc::pid_t {
//!     forward!()
//! }
//!
//! fn main() -> std::io::Result<()> {
//!     install_registered_hooks()?;
//!     Ok(())
//! }
//! ```
//!
//! That is the core shape: declare hooks near the code that owns them, add an
//! observer only when you need one, then install once.
//!
//! # Public Surface
//!
//! Most users only need a few entrypoints:
//!
//! - [`hook`] for declarative hook definitions
//! - [`install_registered_hooks`] to activate generated hooks
//! - [`intercept`] for observation types
//! - [`Function`], [`Module`], and [`Symbol`] if you want direct resolution APIs
//!
//! # Warnings
//!
//! - This crate is still experimental and the API may continue to change.
//! - Install hooks as early as practical, before the code you want to observe
//!   or replace has already run.
//! - Anything under `retarget::__macro_support` and any generated item whose
//!   name starts with `__retarget_` is an implementation detail. Even if it is
//!   visible in autocomplete or expanded code, do not call or depend on it.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("This crate only supports macOS and Windows");

/// Declares one function and exposes it as one concrete function-pointer value.
///
/// This is useful when one API expects a typed function pointer and Rust would
/// otherwise keep a bare function item uncoerced. The generated item keeps the
/// declared name, but that name refers to one function pointer constant instead
/// of the raw function item.
///
/// This macro is not required for normal `retarget` hooks. It exists for the
/// cases where another API needs a named function-pointer value rather than a
/// function item.
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
            unsafe extern $abi fn __retarget_impl($($arg : $arg_ty),*) $(-> $ret)? $body
            __retarget_impl
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
            extern $abi fn __retarget_impl($($arg : $arg_ty),*) $(-> $ret)? $body
            __retarget_impl
        };
    };
}

#[cfg(target_os = "windows")]
mod com;
mod function;
mod imp;
#[cfg(target_os = "macos")]
pub mod objc;

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

#[cfg(feature = "registry")]
pub use __macro_support::{InterceptionEvent, InterceptionHit, InterceptionMode, Signal};
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
pub mod intercept {
    //! Public interception types and mode aliases.
    //!
    //! These are the types used by `#[hook::observer]` and `#[hook::observe]`.
    //!
    //! `retarget` keeps observation deliberately small:
    //!
    //! - one local observer function per module
    //! - one optional `#[hook::observe(...)]` annotation per hook
    //! - one per-hook `FirstHit` gate when needed
    //!
    //! The observation path is event-oriented; it does not retain history or
    //! buffering for you.

    pub use crate::__macro_support::{
        Event, EveryHit, FirstHit, Hit, InterceptionEvent, InterceptionHit, InterceptionMode, Mode,
        Off, Signal,
    };
}

/// Installs every generated hook that was registered in this crate.
///
/// This function walks the generated installer list and applies each hook at
/// most once. Repeated calls are allowed; already-installed hooks are skipped
/// by their generated install guards.
///
/// # Errors
///
/// Returns an error if any required hook cannot be resolved or replaced.
/// Optional hooks are ignored when their target is absent.
///
/// # Warnings
///
/// Hook resolution happens against the current process state. If a target image
/// or runtime object is not available yet, installation may fail or skip that
/// hook depending on whether it was declared optional.
#[cfg(feature = "registry")]
pub fn install_registered_hooks() -> std::io::Result<()> {
    __macro_support::install_registered_hooks()
}

pub mod hook {
    //! User-facing hook declaration macros.
    //!
    //! These are the supported declarative hook macros.
    //!
    //! These macros are the supported authoring surface for generated hooks.
    //! If docs or IDE completion show `retarget::__macro_support` or generated
    //! `__retarget_*` items nearby, treat them as unstable implementation
    //! details rather than public API.

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

    #[cfg(target_os = "macos")]
    pub mod objc {
        //! Objective-C hook declarations.
        //!
        //! These macros declare Objective-C hooks.
        //!
        //! Use `class` for class methods, `instance` for instance methods, and
        //! `methods` to group related hooks in one inherent impl block.

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
    pub use linkme::distributed_slice;

    #[cfg(feature = "registry")]
    mod intercept {
        use std::time::SystemTime;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        /// Observation mode used by `#[hook::observer]` and `#[hook::observe]`.
        ///
        /// `FirstHit` bookkeeping is local to each generated hook function.
        /// Once a hook has emitted its first-hit event in a process, later hits
        /// from that same generated hook stay suppressed for the rest of that
        /// process.
        pub enum InterceptionMode {
            /// Disable observation for this hook.
            Off,
            /// Emit one observation the first time this hook is hit.
            FirstHit,
            /// Emit one observation every time this hook is hit.
            EveryHit,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        /// One observation event emitted directly from a generated hook body.
        ///
        /// This value is created at the function head before the hook body runs.
        /// That means the event can still be delivered even if the hook body
        /// later returns early or panics.
        pub struct InterceptionHit {
            /// Stable hook identifier derived from the enclosing module and item.
            pub hook_id: &'static str,
            /// Mode that decided whether this event should be emitted.
            pub mode: InterceptionMode,
            /// Wall-clock time captured when the hook emitted the event.
            ///
            /// This uses [`std::time::SystemTime`], so it is convenient for
            /// logging and diagnostics but may move forwards or backwards if the
            /// system clock changes.
            pub at: SystemTime,
        }

        pub type InterceptionEvent = InterceptionHit;
        pub type Mode = InterceptionMode;
        pub type Event = InterceptionHit;
        pub type Hit = InterceptionHit;

        #[derive(Debug, Clone, PartialEq, Eq)]
        /// One typed observer payload.
        ///
        /// `#[hook::observe(payload)]` wraps the emitted event and the payload
        /// expression together so observers can receive domain-specific data
        /// without any runtime type registry.
        pub struct Signal<T> {
            /// Metadata about the hook hit.
            pub event: Event,
            /// Typed payload supplied by `#[hook::observe(...)]`.
            pub value: T,
        }

        pub use InterceptionMode::{EveryHit, FirstHit, Off};

        /// Returns the wall-clock timestamp used for generated observation events.
        pub fn interception_time() -> SystemTime {
            SystemTime::now()
        }
    }

    #[cfg(feature = "registry")]
    mod install {
        #[cfg(target_os = "macos")]
        use crate::objc::ObjcMethodError;
        use crate::{FunctionError, FunctionReplaceError};
        use linkme::distributed_slice;
        use std::error::Error;
        use std::io;

        pub trait HookFailure: Error {
            fn is_absent(&self) -> bool;
        }

        impl HookFailure for FunctionError {
            fn is_absent(&self) -> bool {
                self.is_absent()
            }
        }

        impl HookFailure for FunctionReplaceError {
            fn is_absent(&self) -> bool {
                false
            }
        }

        #[cfg(target_os = "macos")]
        impl HookFailure for ObjcMethodError {
            fn is_absent(&self) -> bool {
                self.is_absent()
            }
        }

        #[derive(Debug, Clone, Copy)]
        pub struct HookSpec {
            pub name: &'static str,
            pub optional: bool,
        }

        pub struct HookDef {
            pub install: fn() -> io::Result<()>,
        }

        pub fn install_registered_hooks() -> io::Result<()> {
            for hook in HOOKS {
                (hook.install)()?;
            }
            Ok(())
        }

        pub fn finish_install<E: HookFailure>(
            spec: &HookSpec,
            result: Result<(), E>,
        ) -> io::Result<()> {
            match result {
                Ok(()) => Ok(()),
                Err(error) if spec.optional && error.is_absent() => Ok(()),
                Err(error) => Err(io::Error::other(format!(
                    "required hook {} failed: {}",
                    spec.name, error
                ))),
            }
        }

        #[distributed_slice]
        pub static HOOKS: [HookDef];

        #[cfg(test)]
        mod tests {
            use super::{HookFailure, HookSpec, finish_install};
            use std::fmt;

            #[derive(Debug)]
            struct MissingHook;

            impl fmt::Display for MissingHook {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "missing hook target")
                }
            }

            impl std::error::Error for MissingHook {}

            impl HookFailure for MissingHook {
                fn is_absent(&self) -> bool {
                    true
                }
            }

            #[test]
            fn finish_install_ignores_optional_absence() {
                let spec = HookSpec {
                    name: "missing-optional-test",
                    optional: true,
                };

                finish_install(&spec, Err(MissingHook)).expect("optional miss is allowed");
            }

            #[test]
            fn finish_install_reports_required_hook_name_and_error() {
                let result = finish_install(
                    &HookSpec {
                        name: "missing-required-test",
                        optional: false,
                    },
                    Err(MissingHook),
                )
                .expect_err("required missing hook should report an install error");

                assert_eq!(
                    result.to_string(),
                    "required hook missing-required-test failed: missing hook target"
                );
            }
        }
    }

    #[cfg(feature = "registry")]
    pub use install::{HOOKS, HookDef, HookSpec, finish_install, install_registered_hooks};
    #[cfg(feature = "registry")]
    pub use intercept::{
        Event, EveryHit, FirstHit, Hit, InterceptionEvent, InterceptionHit, InterceptionMode, Mode,
        Off, Signal, interception_time,
    };

    #[cfg(target_os = "windows")]
    pub mod windows {
        pub mod com {
            pub use crate::imp::com::*;
        }
    }
}
