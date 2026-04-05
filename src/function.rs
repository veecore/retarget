//! Opaque function, module, and symbol targets used by hook installation.

use crate::error::{InvalidName, expect_utf8, write_invalid_name};
use crate::imp::function::{
    loaded_module_by_name, module_name_from_handle, prime_module_by_name, replace_function,
    resolve_function_address, resolve_symbol_global, resolve_symbol_in_module,
};
use std::error::Error;
use std::ffi::{CStr, CString, NulError, c_void};
use std::fmt;
use std::io;
use std::ptr::NonNull;

mod private {
    /// Seals the hook-function trait to supported function pointer forms.
    pub trait Sealed {}
}

/// One typed function pointer that can be used as one hook target or replacement.
pub trait FunctionPointer: private::Sealed + Copy {}

macro_rules! impl_hook_function_signature {
    ($($arg:ident),* ) => {
        impl_hook_function_signature!("C", $($arg),*);
        #[cfg(target_os = "windows")]
        impl_hook_function_signature!("system", $($arg),*);
        impl_hook_function_signature!("Rust", $($arg),*);
    };
    ($abi:literal, $($arg:ident),* ) => {
        impl<Ret, $($arg),*> private::Sealed for unsafe extern $abi fn($($arg),*) -> Ret {}
        impl<Ret, $($arg),*> FunctionPointer for unsafe extern $abi fn($($arg),*) -> Ret {}

        impl<Ret, $($arg),*> private::Sealed for extern $abi fn($($arg),*) -> Ret {}
        impl<Ret, $($arg),*> FunctionPointer for extern $abi fn($($arg),*) -> Ret {}
    };
}

impl_hook_function_signature!();
impl_hook_function_signature!(A);
impl_hook_function_signature!(A, B);
impl_hook_function_signature!(A, B, C);
impl_hook_function_signature!(A, B, C, D);
impl_hook_function_signature!(A, B, C, D, E);
impl_hook_function_signature!(A, B, C, D, E, F);
impl_hook_function_signature!(A, B, C, D, E, F, G);
impl_hook_function_signature!(A, B, C, D, E, F, G, H);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J, K);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J, K, L);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J, K, L, M);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J, K, L, M, O);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J, K, L, M, O, P);
impl_hook_function_signature!(A, B, C, D, E, F, G, H, I, J, K, L, M, O, P, Q);

/// One resolved hookable function target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Function {
    /// Resolved raw function pointer.
    resolved: NonNull<c_void>,
    /// Resolved owning module.
    module: Module,
    /// Retained symbol identity.
    symbol: Symbol,
}

impl Function {
    /// Returns the owning module.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Returns the retained symbol identity.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns this function target as one typed function pointer.
    ///
    /// The caller must ensure `T` matches the resolved function ABI and
    /// signature.
    ///
    /// # Safety
    ///
    /// `T` must exactly match the ABI and signature of the resolved function.
    pub unsafe fn resolve<T: FunctionPointer>(&self) -> T {
        unsafe { crate::imp::ptr_to_fn(self.resolved) }
    }

    /// Replaces this function target with one typed replacement implementation.
    ///
    /// The caller must ensure `T` matches the resolved function ABI and
    /// signature.
    ///
    /// # Safety
    ///
    /// `T` must exactly match the ABI and signature of both the target and the
    /// replacement function.
    pub unsafe fn replace_with<T: FunctionPointer>(
        &self,
        replacement: T,
    ) -> Result<T, FunctionReplaceError> {
        let original = replace_function(self.resolved, crate::imp::fn_to_ptr(replacement))
            .map_err(|source| FunctionReplaceError::new(self.clone(), source))?;
        Ok(unsafe { crate::imp::ptr_to_fn(original) })
    }

    /// Builds one already resolved function target from typed parts.
    pub(crate) fn from_resolved_parts(
        resolved: NonNull<c_void>,
        module: Module,
        symbol: Symbol,
    ) -> Self {
        Self {
            resolved,
            module,
            symbol,
        }
    }

    /// Replaces this retained symbol identity.
    #[allow(dead_code)]
    #[cfg(target_os = "windows")]
    pub(crate) fn with_symbol(mut self, symbol: Symbol) -> Self {
        self.symbol = symbol;
        self
    }
}

impl fmt::Display for Function {
    /// Formats one resolved function target for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' in module '{}'", self.symbol, self.module)
    }
}

/// One typed function-replacement failure.
#[derive(Debug)]
pub struct FunctionReplaceError {
    /// The function target that could not be replaced.
    target: Function,
    /// The underlying platform failure.
    source: io::Error,
}

impl FunctionReplaceError {
    /// Builds one replacement error for one resolved [`Function`].
    fn new(target: Function, source: io::Error) -> Self {
        Self { target, source }
    }
}

impl fmt::Display for FunctionReplaceError {
    /// Formats one function-replacement error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to replace function {}: {}",
            self.target, self.source
        )
    }
}

impl Error for FunctionReplaceError {
    /// Returns the underlying source error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// One resolved module handle retained for lookup and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Module {
    /// Resolved raw module handle.
    resolved: NonNull<c_void>,
    /// Retained module name for diagnostics and scoped symbol lookup.
    name: CString,
}

impl Module {
    /// Returns this module name as UTF-8 text.
    pub fn name(&self) -> &str {
        expect_utf8(&self.name)
    }

    /// Resolves one symbol within this module.
    pub fn resolve<S: IntoSymbol>(&self, symbol: S) -> Result<Function, FunctionError> {
        let symbol = symbol.into_symbol().map_err(FunctionError::from)?;
        symbol.resolve_in(self)
    }

    /// Returns the resolved raw module handle.
    pub(crate) fn resolved(&self) -> NonNull<c_void> {
        self.resolved
    }

    /// Builds one already resolved module from one handle and retained name.
    pub(crate) fn from_resolved_parts(resolved: NonNull<c_void>, name: CString) -> Self {
        Self { resolved, name }
    }
}

impl fmt::Display for Module {
    /// Formats one resolved module name for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One exported symbol name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(CString);

impl Symbol {
    /// Returns this symbol name as UTF-8 text.
    pub fn name(&self) -> &str {
        expect_utf8(&self.0)
    }

    /// Resolves this symbol from the process-global namespace.
    pub fn resolve(&self) -> Result<Function, FunctionError> {
        let resolved = resolve_symbol_global(self.name_c_str())
            .ok_or_else(|| FunctionError::function_not_found(None, self.clone()))?;
        Ok(Function::from_resolved_parts(
            resolved.symbol,
            Module::from_resolved_parts(resolved.module, resolved.module_name),
            self.clone(),
        ))
    }

    /// Resolves this symbol inside one specific module.
    pub fn resolve_in(&self, module: &Module) -> Result<Function, FunctionError> {
        let resolved = resolve_symbol_in_module(self.name_c_str(), module.resolved())
            .ok_or_else(|| FunctionError::function_not_found(Some(module.clone()), self.clone()))?;
        Ok(Function::from_resolved_parts(
            resolved,
            module.clone(),
            self.clone(),
        ))
    }

    /// Resolves this symbol against one module list and then the process-global namespace.
    pub fn resolve_in_modules(&self, modules: &[Module]) -> Result<Function, FunctionError> {
        for module in modules {
            if let Some(address) = resolve_symbol_in_module(self.name_c_str(), module.resolved()) {
                return Ok(Function::from_resolved_parts(
                    address,
                    module.clone(),
                    self.clone(),
                ));
            }
        }

        self.resolve()
    }

    /// Returns this symbol name as a C string.
    pub(crate) fn name_c_str(&self) -> &CStr {
        &self.0
    }

    /// Builds one symbol from one already validated C string.
    pub(crate) fn from_cstring(value: CString) -> Self {
        Self(value)
    }
}

impl fmt::Display for Symbol {
    /// Formats one symbol identity for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One typed module-target conversion error.
#[derive(Debug)]
pub struct ModuleError {
    /// Private module-target conversion details.
    imp: ModuleErrorImpl,
}

/// Private module-target conversion details.
#[derive(Debug)]
enum ModuleErrorImpl {
    /// The provided module name contained one interior NUL byte.
    InvalidName {
        /// The captured invalid module input.
        input: InvalidName,
    },
    /// The requested module was not loaded in the current process.
    NotFound {
        /// The requested module name.
        name: CString,
    },
    /// The requested module could not be loaded into the current process.
    LoadFailed {
        /// The requested module name.
        name: CString,
        /// The underlying operating-system failure.
        source: io::Error,
    },
    /// The crate could not recover module information from one resolved value.
    Unavailable {
        /// The underlying operating-system failure.
        source: io::Error,
    },
}

impl ModuleError {
    /// Returns whether this error only indicates absence.
    pub(crate) fn is_absent(&self) -> bool {
        matches!(&self.imp, ModuleErrorImpl::NotFound { .. })
    }

    /// Builds one typed invalid-module-name error.
    pub(crate) fn invalid_name(source: NulError) -> Self {
        Self {
            imp: ModuleErrorImpl::InvalidName {
                input: InvalidName::from_nul_error(source),
            },
        }
    }

    /// Builds one typed missing-module error.
    pub(crate) fn module_not_found(module_name: CString) -> Self {
        Self {
            imp: ModuleErrorImpl::NotFound { name: module_name },
        }
    }

    /// Builds one typed module-load failure.
    pub(crate) fn module_load_failed(module_name: CString, source: io::Error) -> Self {
        Self {
            imp: ModuleErrorImpl::LoadFailed {
                name: module_name,
                source,
            },
        }
    }

    /// Builds one typed module-recovery error for one resolved value.
    pub(crate) fn module_unavailable(source: io::Error) -> Self {
        Self {
            imp: ModuleErrorImpl::Unavailable { source },
        }
    }
}

impl fmt::Display for ModuleError {
    /// Formats one module-target conversion error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.imp {
            ModuleErrorImpl::InvalidName { input } => write_invalid_name(f, "module", input),
            ModuleErrorImpl::NotFound { name } => write!(
                f,
                "module '{}' is not loaded in the current process",
                expect_utf8(name)
            ),
            ModuleErrorImpl::LoadFailed { name, source } => {
                write!(f, "failed to load module '{}': {source}", expect_utf8(name))
            }
            ModuleErrorImpl::Unavailable { source } => {
                write!(
                    f,
                    "failed to recover module information from the provided value: {source}"
                )
            }
        }
    }
}

impl Error for ModuleError {
    /// Returns the underlying source error when one exists.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.imp {
            ModuleErrorImpl::LoadFailed { source, .. } => Some(source),
            ModuleErrorImpl::Unavailable { source } => Some(source),
            ModuleErrorImpl::InvalidName { .. } | ModuleErrorImpl::NotFound { .. } => None,
        }
    }
}

/// One typed symbol-target conversion error.
#[derive(Debug)]
pub struct SymbolError {
    /// Private symbol-target conversion details.
    imp: SymbolErrorImpl,
}

/// Private symbol-target conversion details.
#[derive(Debug)]
enum SymbolErrorImpl {
    /// The provided symbol name contained one interior NUL byte.
    InvalidName {
        /// The captured invalid symbol input.
        input: InvalidName,
    },
}

impl SymbolError {
    /// Builds one typed invalid-symbol-name error.
    pub(crate) fn invalid_name(source: NulError) -> Self {
        Self {
            imp: SymbolErrorImpl::InvalidName {
                input: InvalidName::from_nul_error(source),
            },
        }
    }
}

impl fmt::Display for SymbolError {
    /// Formats one symbol-target conversion error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.imp {
            SymbolErrorImpl::InvalidName { input } => write_invalid_name(f, "symbol", input),
        }
    }
}

impl Error for SymbolError {
    /// Returns the underlying source error when one exists.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.imp {
            SymbolErrorImpl::InvalidName { .. } => None,
        }
    }
}

/// One typed exported-function target conversion error.
#[derive(Debug)]
pub struct FunctionError {
    /// Private exported-function target conversion details.
    imp: FunctionErrorImpl,
}

/// Private exported-function target conversion details.
#[derive(Debug)]
enum FunctionErrorImpl {
    /// One nested module-target conversion failure.
    Module(ModuleError),
    /// One nested symbol-target conversion failure.
    Symbol(SymbolError),
    /// The final function lookup step did not resolve one symbol in one target scope.
    Resolve {
        /// The requested module when one was specified.
        module: Option<Module>,
        /// The missing symbol identity.
        symbol: Symbol,
    },
}

impl FunctionError {
    /// Returns whether this error only indicates target absence.
    pub(crate) fn is_absent(&self) -> bool {
        match &self.imp {
            FunctionErrorImpl::Module(error) => error.is_absent(),
            FunctionErrorImpl::Symbol(_) => false,
            FunctionErrorImpl::Resolve { .. } => true,
        }
    }

    /// Builds one typed unresolved-function error.
    pub(crate) fn function_not_found(module: Option<Module>, symbol: Symbol) -> Self {
        Self {
            imp: FunctionErrorImpl::Resolve { module, symbol },
        }
    }
}

impl fmt::Display for FunctionError {
    /// Formats one function-target conversion error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.imp {
            FunctionErrorImpl::Module(error) => error.fmt(f),
            FunctionErrorImpl::Symbol(error) => error.fmt(f),
            FunctionErrorImpl::Resolve {
                module: Some(module),
                symbol,
            } => write!(
                f,
                "function '{}' was not found in module '{}'",
                symbol, module
            ),
            FunctionErrorImpl::Resolve {
                module: None,
                symbol,
            } => write!(
                f,
                "function '{}' was not found in the process-global symbol space",
                symbol
            ),
        }
    }
}

impl Error for FunctionError {
    /// Returns the underlying source error when one exists.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.imp {
            FunctionErrorImpl::Module(error) => Some(error as &(dyn Error + 'static)),
            FunctionErrorImpl::Symbol(error) => Some(error as &(dyn Error + 'static)),
            FunctionErrorImpl::Resolve { .. } => None,
        }
    }
}

impl From<ModuleError> for FunctionError {
    /// Maps one module-target conversion error into one function-target error.
    fn from(error: ModuleError) -> Self {
        Self {
            imp: FunctionErrorImpl::Module(error),
        }
    }
}

impl From<SymbolError> for FunctionError {
    /// Maps one symbol-target conversion error into one function-target error.
    fn from(error: SymbolError) -> Self {
        Self {
            imp: FunctionErrorImpl::Symbol(error),
        }
    }
}

/// Converts one supported function target into one [`Function`].
pub trait IntoFunction {
    /// Converts this value into one resolved function pointer.
    fn into_function(self) -> Result<Function, FunctionError>;
}

/// Converts one supported module value into one [`Module`].
pub trait IntoModule {
    /// Converts this value into one resolved module handle.
    fn into_module(self) -> Result<Module, ModuleError>;
}

/// Converts one supported symbol value into one [`Symbol`].
pub trait IntoSymbol {
    /// Converts this value into one symbol name.
    fn into_symbol(self) -> Result<Symbol, SymbolError>;
}

/// Converts one supported target value into one [`Function`].
pub fn into_function<T: IntoFunction>(value: T) -> Result<Function, FunctionError> {
    value.into_function()
}

/// Converts one supported module value into one [`Module`].
pub fn into_module<T: IntoModule>(value: T) -> Result<Module, ModuleError> {
    value.into_module()
}

/// Converts one supported symbol value into one [`Symbol`].
pub fn into_symbol<T: IntoSymbol>(value: T) -> Result<Symbol, SymbolError> {
    value.into_symbol()
}

/// Builds one [`Function`] from one already resolved address.
fn function_from_address(resolved: NonNull<c_void>) -> Result<Function, FunctionError> {
    let resolved = resolve_function_address(resolved).map_err(ModuleError::module_unavailable)?;
    Ok(Function::from_resolved_parts(
        resolved.symbol,
        Module::from_resolved_parts(resolved.module, resolved.module_name),
        Symbol::from_cstring(resolved.symbol_name),
    ))
}

/// Conversion trait impls for high-level function targets.
mod impls {
    use super::*;

    impl IntoFunction for Function {
        /// Returns the same function target unchanged.
        fn into_function(self) -> Result<Function, FunctionError> {
            Ok(self)
        }
    }

    impl<T: FunctionPointer> IntoFunction for T {
        /// Wraps one typed function pointer after recovering its owning module.
        fn into_function(self) -> Result<Function, FunctionError> {
            function_from_address(crate::imp::fn_to_ptr(self))
        }
    }

    impl IntoFunction for &str {
        /// Resolves one borrowed global function symbol.
        fn into_function(self) -> Result<Function, FunctionError> {
            CString::new(self)
                .map_err(SymbolError::invalid_name)?
                .into_function()
        }
    }

    impl IntoFunction for String {
        /// Resolves one owned global function symbol.
        fn into_function(self) -> Result<Function, FunctionError> {
            CString::new(self)
                .map_err(SymbolError::invalid_name)?
                .into_function()
        }
    }

    impl IntoFunction for &CStr {
        /// Resolves one borrowed global function symbol.
        fn into_function(self) -> Result<Function, FunctionError> {
            self.to_owned().into_function()
        }
    }

    impl IntoFunction for CString {
        /// Resolves one owned global function symbol.
        fn into_function(self) -> Result<Function, FunctionError> {
            Symbol::from_cstring(self).resolve()
        }
    }

    impl<I, S> IntoFunction for (I, S)
    where
        I: IntoModule,
        S: IntoSymbol,
    {
        /// Resolves one function symbol inside one specific module.
        fn into_function(self) -> Result<Function, FunctionError> {
            let module = self.0.into_module().map_err(FunctionError::from)?;
            module.resolve(self.1)
        }
    }

    impl IntoFunction for NonNull<c_void> {
        /// Wraps one raw non-null function address after recovering its owning module.
        fn into_function(self) -> Result<Function, FunctionError> {
            function_from_address(self)
        }
    }

    impl IntoFunction for *mut c_void {
        /// Wraps one raw mutable function address after validating it is non-null.
        fn into_function(self) -> Result<Function, FunctionError> {
            let resolved = NonNull::new(self).ok_or_else(|| {
                FunctionError::from(ModuleError::module_unavailable(io::Error::other(
                    "null function pointers cannot be resolved",
                )))
            })?;
            function_from_address(resolved)
        }
    }

    impl IntoFunction for *const c_void {
        /// Wraps one raw const function address after validating it is non-null.
        fn into_function(self) -> Result<Function, FunctionError> {
            (self as *mut c_void).into_function()
        }
    }

    impl IntoModule for Module {
        /// Returns the same module unchanged.
        fn into_module(self) -> Result<Module, ModuleError> {
            Ok(self)
        }
    }

    impl IntoModule for &str {
        /// Resolves one borrowed module path or module name.
        fn into_module(self) -> Result<Module, ModuleError> {
            CString::new(self)
                .map_err(ModuleError::invalid_name)?
                .into_module()
        }
    }

    impl IntoModule for String {
        /// Resolves one owned module path or module name.
        fn into_module(self) -> Result<Module, ModuleError> {
            CString::new(self)
                .map_err(ModuleError::invalid_name)?
                .into_module()
        }
    }

    impl IntoModule for &CStr {
        /// Resolves one borrowed module path or module name.
        fn into_module(self) -> Result<Module, ModuleError> {
            self.to_owned().into_module()
        }
    }

    impl IntoModule for CString {
        /// Resolves one owned module path or module name.
        fn into_module(self) -> Result<Module, ModuleError> {
            let resolved = if let Some(module) = loaded_module_by_name(&self) {
                module
            } else if cfg!(feature = "auto-prime") {
                prime_module_by_name(&self)
                    .map_err(|source| ModuleError::module_load_failed(self.clone(), source))?
            } else {
                return Err(ModuleError::module_not_found(self));
            };
            let name = module_name_from_handle(resolved).unwrap_or(self);
            Ok(Module::from_resolved_parts(resolved, name))
        }
    }

    impl IntoModule for NonNull<c_void> {
        /// Wraps one already resolved module handle after recovering one diagnostic name.
        fn into_module(self) -> Result<Module, ModuleError> {
            let name = module_name_from_handle(self).map_err(ModuleError::module_unavailable)?;
            Ok(Module::from_resolved_parts(self, name))
        }
    }

    impl IntoSymbol for Symbol {
        /// Returns the same symbol unchanged.
        fn into_symbol(self) -> Result<Symbol, SymbolError> {
            Ok(self)
        }
    }

    impl IntoSymbol for &str {
        /// Wraps one borrowed symbol name.
        fn into_symbol(self) -> Result<Symbol, SymbolError> {
            CString::new(self)
                .map(Symbol::from_cstring)
                .map_err(SymbolError::invalid_name)
        }
    }

    impl IntoSymbol for String {
        /// Wraps one owned symbol name.
        fn into_symbol(self) -> Result<Symbol, SymbolError> {
            CString::new(self)
                .map(Symbol::from_cstring)
                .map_err(SymbolError::invalid_name)
        }
    }

    impl IntoSymbol for &CStr {
        /// Wraps one borrowed symbol name.
        fn into_symbol(self) -> Result<Symbol, SymbolError> {
            Ok(Symbol::from_cstring(self.to_owned()))
        }
    }

    impl IntoSymbol for CString {
        /// Wraps one owned symbol name.
        fn into_symbol(self) -> Result<Symbol, SymbolError> {
            Ok(Symbol::from_cstring(self))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{into_function, into_module, into_symbol};
    use std::ffi::c_void;

    #[cfg(target_os = "windows")]
    type TestFunction = unsafe extern "system" fn() -> u32;

    #[cfg(target_os = "macos")]
    type TestFunction = unsafe extern "C" fn() -> i32;

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

    #[cfg(target_os = "windows")]
    fn assert_raw_pointer_symbol_name(name: &str, raw: std::ptr::NonNull<c_void>) {
        let _ = raw;
        assert_eq!(name, test_symbol_name());
    }

    #[cfg(target_os = "macos")]
    fn assert_raw_pointer_symbol_name(name: &str, raw: std::ptr::NonNull<c_void>) {
        assert!(!name.is_empty());
        assert_ne!(name, format!("0x{:x}", raw.as_ptr() as usize));
    }

    #[test]
    fn resolves_modules_from_strings() {
        let module = into_module(test_module_name()).expect("valid module");
        assert_eq!(module.name(), test_module_name());
    }

    #[test]
    fn preserves_existing_modules() {
        let module = into_module(test_module_name()).expect("valid module");
        assert_eq!(into_module(module.clone()).expect("valid module"), module);
    }

    #[test]
    fn wraps_strings_as_symbols() {
        let symbol = into_symbol(test_symbol_name()).expect("valid symbol");
        assert_eq!(symbol.name(), test_symbol_name());
        assert_eq!(symbol.to_string(), test_symbol_name());
    }

    #[test]
    fn resolves_global_functions_from_strings() {
        let function = into_function(test_symbol_name()).expect("valid function");
        assert_eq!(function.symbol().name(), test_symbol_name());
        assert!(!function.module().name().is_empty());
    }

    #[test]
    fn resolves_global_functions_from_symbols() {
        let symbol = into_symbol(test_symbol_name()).expect("valid symbol");
        let function = symbol.resolve().expect("valid function");

        assert_eq!(function.symbol(), &symbol);
        assert!(!function.module().name().is_empty());
    }

    #[test]
    fn resolves_scoped_functions_from_modules() {
        let module = into_module(test_module_name()).expect("valid module");
        let function = module.resolve(test_symbol_name()).expect("valid function");

        assert_eq!(function.module(), &module);
        assert_eq!(function.symbol().name(), test_symbol_name());
        assert_eq!(module.to_string(), test_module_name());
        assert_eq!(
            function.to_string(),
            format!(
                "'{}' in module '{}'",
                test_symbol_name(),
                test_module_name()
            )
        );
    }

    #[test]
    fn resolves_typed_functions_from_function_targets() {
        let function = into_function(test_symbol_name()).expect("valid function");
        let resolved: TestFunction = unsafe { function.resolve() };
        let value = unsafe { resolved() };

        assert_ne!(value, 0);
    }

    #[test]
    fn wraps_typed_function_pointers_as_functions() {
        let original = into_function(test_symbol_name()).expect("valid function");
        let typed: TestFunction = unsafe { original.resolve() };
        let wrapped = into_function(typed).expect("typed function pointer");

        assert_eq!(
            crate::imp::fn_to_ptr(typed),
            crate::imp::fn_to_ptr(unsafe { wrapped.resolve::<TestFunction>() })
        );
        assert!(!wrapped.module().name().is_empty());
    }

    #[test]
    fn wraps_raw_function_pointers_as_functions() {
        let original = into_function(test_symbol_name()).expect("valid function");
        let raw = crate::imp::fn_to_ptr(unsafe { original.resolve::<TestFunction>() });
        let wrapped = into_function(raw).expect("raw function pointer");

        assert_eq!(wrapped.module(), original.module());
        assert_raw_pointer_symbol_name(wrapped.symbol().name(), raw);
    }

    #[test]
    fn preserves_existing_symbols() {
        let symbol = into_symbol(test_symbol_name()).expect("valid symbol");
        assert_eq!(into_symbol(symbol.clone()).expect("valid symbol"), symbol);
    }

    #[test]
    fn preserves_existing_functions() {
        let function = into_function(test_symbol_name()).expect("valid function");
        assert_eq!(
            into_function(function.clone()).expect("valid function"),
            function
        );
    }

    #[test]
    fn resolves_scoped_functions_from_existing_types() {
        let module = into_module(test_module_name()).expect("valid module");
        let symbol = into_symbol(test_symbol_name()).expect("valid symbol");
        let function = into_function((module.clone(), symbol.clone())).expect("valid function");

        assert_eq!(function.module(), &module);
        assert_eq!(function.symbol(), &symbol);
    }
}
