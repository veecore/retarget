//! Hook registry and install-time helpers.

pub mod intercept;

use self::intercept::prepare_interception_runtime;
#[cfg(target_os = "macos")]
use crate::objc::ObjcMethodError;
use crate::{FunctionError, FunctionReplaceError, Symbol, function::Module};
use linkme::distributed_slice;
use std::error::Error;
use std::io;

/// One hook-install failure that may represent target absence.
pub trait HookFailure: Error {
    /// Returns whether this error only indicates absence.
    fn is_absent(&self) -> bool;
}

impl HookFailure for FunctionError {
    /// Returns whether this function error only indicates absence.
    fn is_absent(&self) -> bool {
        self.is_absent()
    }
}

impl HookFailure for FunctionReplaceError {
    /// Function replacement failures never mean the target is absent.
    fn is_absent(&self) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
impl HookFailure for ObjcMethodError {
    /// Returns whether this Objective-C method error only indicates absence.
    fn is_absent(&self) -> bool {
        self.is_absent()
    }
}

/// One registered system-API hook specification.
#[derive(Debug, Clone)]
pub struct HookSpec {
    /// Stable hook display name.
    pub name: &'static str,
    /// Exported symbol or selector text used for installation.
    pub symbol: Symbol,
    /// Module that should contain the hook target when relevant.
    pub module: Option<Module>,
    /// Whether missing support should be tolerated.
    pub optional: bool,
}

/// One proc-macro-registered hook install function.
pub struct HookDef {
    /// Installs one registered hook.
    pub install: fn() -> io::Result<()>,
}

/// Installs every hook registered in the distributed slice.
pub fn install_registered_hooks() -> io::Result<()> {
    prepare_interception_runtime()?;
    for hook in HOOKS {
        (hook.install)()?;
    }
    Ok(())
}

/// Resolves the target symbol before hook installation begins.
pub fn probe_hook(spec: &HookSpec) -> Result<(), FunctionError> {
    match spec.module.as_ref() {
        Some(module) => spec.symbol.resolve_in(module).map(|_| ()),
        None => spec.symbol.resolve().map(|_| ()),
    }
}

/// Returns an install-time error for one required hook failure.
pub fn finish_install<E: HookFailure>(spec: &HookSpec, result: Result<(), E>) -> io::Result<()> {
    finish_named_install(spec.name, spec.optional, result)
}

/// Returns an install-time error for one named hook failure.
pub fn finish_named_install<E: HookFailure>(
    name: &'static str,
    optional: bool,
    result: Result<(), E>,
) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if optional && error.is_absent() => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "required hook {name} failed: {error}"
        ))),
    }
}

/// Distributed slice of every hook install function registered in one target crate.
#[distributed_slice]
pub static HOOKS: [HookDef];

#[cfg(test)]
mod tests {
    use super::{HookSpec, finish_named_install, probe_hook};
    use crate::function::{into_module, into_symbol};

    #[cfg(target_os = "windows")]
    fn test_module_name() -> &'static str {
        "kernel32.dll"
    }

    #[cfg(target_os = "macos")]
    fn test_module_name() -> &'static str {
        "/usr/lib/libSystem.B.dylib"
    }

    #[cfg(target_os = "windows")]
    fn test_symbol_name() -> &'static str {
        "GetCurrentProcessId"
    }

    #[cfg(target_os = "macos")]
    fn test_symbol_name() -> &'static str {
        "getpid"
    }

    #[test]
    fn probe_hook_resolves_global_exports() {
        let spec = HookSpec {
            name: "global-test",
            symbol: into_symbol(test_symbol_name()).expect("valid symbol"),
            module: None,
            optional: false,
        };

        probe_hook(&spec).expect("global export should resolve");
    }

    #[test]
    fn probe_hook_resolves_scoped_exports() {
        let spec = HookSpec {
            name: "scoped-test",
            symbol: into_symbol(test_symbol_name()).expect("valid symbol"),
            module: Some(into_module(test_module_name()).expect("valid module")),
            optional: false,
        };

        probe_hook(&spec).expect("scoped export should resolve");
    }

    #[test]
    fn probe_hook_reports_missing_exports() {
        let spec = HookSpec {
            name: "missing-test",
            symbol: into_symbol("DefinitelyMissingExport").expect("valid symbol"),
            module: Some(into_module(test_module_name()).expect("valid module")),
            optional: false,
        };

        let error = probe_hook(&spec).expect_err("missing export should fail");
        assert_eq!(
            error.to_string(),
            format!(
                "function '{}' was not found in module '{}'",
                "DefinitelyMissingExport",
                test_module_name()
            )
        );
    }

    #[test]
    fn finish_named_install_ignores_optional_absence() {
        let spec = HookSpec {
            name: "missing-optional-test",
            symbol: into_symbol("DefinitelyMissingExport").expect("valid symbol"),
            module: Some(into_module(test_module_name()).expect("valid module")),
            optional: true,
        };

        let result = probe_hook(&spec);
        finish_named_install(spec.name, spec.optional, result).expect("optional miss is allowed");
    }

    #[test]
    fn finish_named_install_reports_required_hook_name_and_error() {
        let result = finish_named_install(
            "missing-required-test",
            false,
            probe_hook(&HookSpec {
                name: "missing-required-test",
                symbol: into_symbol("DefinitelyMissingExport").expect("valid symbol"),
                module: Some(into_module(test_module_name()).expect("valid module")),
                optional: false,
            }),
        )
        .expect_err("required missing hook should report an install error");

        assert_eq!(
            result.to_string(),
            format!(
                "required hook missing-required-test failed: function '{}' was not found in module '{}'",
                "DefinitelyMissingExport",
                test_module_name()
            )
        );
    }
}
