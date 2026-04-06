//! Opaque Objective-C hook targets used by method installation on macOS.
//!
//! These types model Objective-C hook identity as `(class, selector, kind)`
//! and then resolve that identity into one replaceable runtime method.

use crate::FunctionPointer;
use crate::error::{InvalidName, expect_utf8, write_invalid_name};
use crate::imp::objc::{replace_method, resolve_class, resolve_method};
use std::error::Error;
use std::ffi::c_void;
use std::ffi::{CStr, CString, NulError};
use std::fmt;
use std::ptr::NonNull;

/// One Objective-C method target prepared for installation.
///
/// This is the fully resolved Objective-C hook target: the class and selector
/// have already been looked up in the runtime and tied to either the instance
/// or class method namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjcMethod {
    /// Resolved runtime method pointer.
    resolved: NonNull<c_void>,
    /// The owning Objective-C class.
    class: ObjcClass,
    /// The target selector identity.
    selector: ObjcSelector,
    /// Whether this target is one instance or class method.
    kind: ObjcMethodKind,
}

impl ObjcMethod {
    /// Builds one instance-method target from one resolved class and selector.
    pub fn instance(class: ObjcClass, selector: ObjcSelector) -> Result<Self, ObjcMethodError> {
        class.instance_method(selector)
    }

    /// Builds one class-method target from one resolved class and selector.
    pub fn class(class: ObjcClass, selector: ObjcSelector) -> Result<Self, ObjcMethodError> {
        class.class_method(selector)
    }

    /// Returns the owning class.
    pub fn class_ref(&self) -> &ObjcClass {
        &self.class
    }

    /// Returns the selector identity.
    pub fn selector(&self) -> &ObjcSelector {
        &self.selector
    }

    /// Returns whether this target is one instance or class method.
    pub fn kind(&self) -> ObjcMethodKind {
        self.kind
    }

    /// Returns whether this is one instance-method target.
    pub fn is_instance(&self) -> bool {
        matches!(self.kind, ObjcMethodKind::Instance)
    }

    /// Returns whether this is one class-method target.
    pub fn is_class(&self) -> bool {
        matches!(self.kind, ObjcMethodKind::Class)
    }

    /// Replaces this Objective-C method implementation with one typed replacement.
    ///
    /// The caller must ensure `T` matches the Objective-C method ABI and
    /// signature.
    ///
    /// On success, the returned function pointer is the original method
    /// implementation that was replaced.
    ///
    /// # Safety
    ///
    /// `T` must exactly match the ABI and signature of both the target method
    /// and the replacement implementation.
    pub unsafe fn replace_with<T: FunctionPointer>(&self, replacement: T) -> T {
        let original = replace_method(self.resolved, crate::imp::fn_to_ptr(replacement));
        unsafe { crate::imp::ptr_to_fn(original) }
    }

    /// Resolves one concrete Objective-C method from typed public identity.
    fn resolve(
        class: ObjcClass,
        selector: ObjcSelector,
        kind: ObjcMethodKind,
    ) -> Result<Self, ObjcMethodError> {
        let resolved = resolve_method(
            class.resolved(),
            selector.name_c_str(),
            matches!(kind, ObjcMethodKind::Instance),
        )
        .ok_or_else(|| ObjcMethodError::resolve(class.clone(), selector.clone(), kind))?;

        Ok(Self {
            resolved,
            class,
            selector,
            kind,
        })
    }
}

impl fmt::Display for ObjcMethod {
    /// Formats one Objective-C method target for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.class, self.selector)
    }
}

/// Describes whether one Objective-C hook targets one instance or class method.
///
/// Objective-C keeps instance methods and class methods in different method
/// tables, so this is part of the hook target's identity rather than just
/// metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjcMethodKind {
    /// One instance method implementation.
    Instance,
    /// One class method implementation.
    Class,
}

impl fmt::Display for ObjcMethodKind {
    /// Formats one Objective-C method kind for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjcMethodKind::Instance => f.write_str("instance"),
            ObjcMethodKind::Class => f.write_str("class"),
        }
    }
}

/// One Objective-C class identifier.
///
/// This value preserves the user-facing class identity and one resolved runtime
/// class pointer.
///
/// Use [`ObjcClass::instance_method`] or [`ObjcClass::class_method`] to turn a
/// class into one concrete hook target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjcClass {
    /// Resolved runtime class pointer.
    resolved: NonNull<c_void>,
    /// Retained Objective-C class name for diagnostics.
    name: CString,
}

impl ObjcClass {
    /// Returns this Objective-C class name as UTF-8 text.
    pub fn name(&self) -> &str {
        expect_utf8(&self.name)
    }

    /// Resolves one instance method on this class.
    pub fn instance_method<S: IntoObjcSelector>(
        &self,
        selector: S,
    ) -> Result<ObjcMethod, ObjcMethodError> {
        let selector = selector
            .into_objc_selector()
            .map_err(ObjcMethodError::from)?;
        ObjcMethod::resolve(self.clone(), selector, ObjcMethodKind::Instance)
    }

    /// Resolves one class method on this class.
    pub fn class_method<S: IntoObjcSelector>(
        &self,
        selector: S,
    ) -> Result<ObjcMethod, ObjcMethodError> {
        let selector = selector
            .into_objc_selector()
            .map_err(ObjcMethodError::from)?;
        ObjcMethod::resolve(self.clone(), selector, ObjcMethodKind::Class)
    }

    /// Returns the resolved runtime class pointer.
    pub(crate) fn resolved(&self) -> NonNull<c_void> {
        self.resolved
    }
}

impl fmt::Display for ObjcClass {
    /// Formats one Objective-C class identity for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One Objective-C selector identifier.
///
/// Selectors intentionally stay identity-like. The eventual hook target is the
/// resolved method found from one class, one selector, and one method kind.
///
/// A selector by itself is not enough to install a hook; it must still be
/// paired with a class and method kind.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjcSelector(CString);

impl ObjcSelector {
    /// Returns this Objective-C selector name as UTF-8 text.
    pub fn name(&self) -> &str {
        expect_utf8(&self.0)
    }

    /// Returns this Objective-C selector name as a C string.
    pub(crate) fn name_c_str(&self) -> &CStr {
        &self.0
    }
}

impl fmt::Display for ObjcSelector {
    /// Formats one Objective-C selector identity for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One typed Objective-C class conversion failure.
#[derive(Debug)]
pub struct ObjcClassError {
    /// Private Objective-C class conversion details.
    imp: ObjcClassErrorImpl,
}

/// Private Objective-C class conversion details.
#[derive(Debug)]
enum ObjcClassErrorImpl {
    /// The provided Objective-C class name contained one interior NUL byte.
    InvalidName {
        /// The captured invalid class input.
        input: InvalidName,
    },
    /// The requested Objective-C class was not present in the runtime.
    NotFound {
        /// The requested Objective-C class name.
        name: CString,
    },
}

impl ObjcClassError {
    /// Returns whether this error only indicates absence.
    pub(crate) fn is_absent(&self) -> bool {
        matches!(&self.imp, ObjcClassErrorImpl::NotFound { .. })
    }

    /// Builds one typed invalid-class-name error.
    pub(crate) fn invalid_name(source: NulError) -> Self {
        Self {
            imp: ObjcClassErrorImpl::InvalidName {
                input: InvalidName::from_nul_error(source),
            },
        }
    }

    /// Builds one typed missing-class error.
    pub(crate) fn not_found(class_name: CString) -> Self {
        Self {
            imp: ObjcClassErrorImpl::NotFound { name: class_name },
        }
    }
}

impl fmt::Display for ObjcClassError {
    /// Formats one Objective-C class conversion error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.imp {
            ObjcClassErrorImpl::InvalidName { input } => {
                write_invalid_name(f, "Objective-C class", input)
            }
            ObjcClassErrorImpl::NotFound { name } => write!(
                f,
                "Objective-C class '{}' was not found in the runtime",
                expect_utf8(name)
            ),
        }
    }
}

impl Error for ObjcClassError {
    /// Returns the underlying source error when one exists.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.imp {
            ObjcClassErrorImpl::InvalidName { .. } | ObjcClassErrorImpl::NotFound { .. } => None,
        }
    }
}

/// One typed Objective-C selector conversion failure.
#[derive(Debug)]
pub struct ObjcSelectorError {
    /// Private Objective-C selector conversion details.
    imp: ObjcSelectorErrorImpl,
}

/// Private Objective-C selector conversion details.
#[derive(Debug)]
enum ObjcSelectorErrorImpl {
    /// The provided Objective-C selector name contained one interior NUL byte.
    InvalidName {
        /// The captured invalid selector input.
        input: InvalidName,
    },
}

impl ObjcSelectorError {
    /// Builds one typed invalid-selector-name error.
    pub(crate) fn invalid_name(source: NulError) -> Self {
        Self {
            imp: ObjcSelectorErrorImpl::InvalidName {
                input: InvalidName::from_nul_error(source),
            },
        }
    }
}

impl fmt::Display for ObjcSelectorError {
    /// Formats one Objective-C selector conversion error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.imp {
            ObjcSelectorErrorImpl::InvalidName { input } => {
                write_invalid_name(f, "Objective-C selector", input)
            }
        }
    }
}

impl Error for ObjcSelectorError {
    /// Returns the underlying source error when one exists.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.imp {
            ObjcSelectorErrorImpl::InvalidName { .. } => None,
        }
    }
}

/// One typed Objective-C method conversion failure.
#[derive(Debug)]
pub struct ObjcMethodError {
    /// Private Objective-C method conversion details.
    imp: ObjcMethodErrorImpl,
}

/// Private Objective-C method conversion details.
#[derive(Debug)]
enum ObjcMethodErrorImpl {
    /// One nested class conversion failure.
    Class(ObjcClassError),
    /// One nested selector conversion failure.
    Selector(ObjcSelectorError),
    /// The final Objective-C method lookup step did not resolve one method.
    Resolve {
        /// The resolved class identity used for lookup.
        class: ObjcClass,
        /// The selector identity used for lookup.
        selector: ObjcSelector,
        /// Whether the lookup targeted one instance or class method.
        kind: ObjcMethodKind,
    },
}

impl ObjcMethodError {
    /// Returns whether this error only indicates target absence.
    pub(crate) fn is_absent(&self) -> bool {
        match &self.imp {
            ObjcMethodErrorImpl::Class(error) => error.is_absent(),
            ObjcMethodErrorImpl::Selector(_) => false,
            ObjcMethodErrorImpl::Resolve { .. } => true,
        }
    }

    /// Builds one typed unresolved-method error.
    pub(crate) fn resolve(class: ObjcClass, selector: ObjcSelector, kind: ObjcMethodKind) -> Self {
        Self {
            imp: ObjcMethodErrorImpl::Resolve {
                class,
                selector,
                kind,
            },
        }
    }
}

impl fmt::Display for ObjcMethodError {
    /// Formats one Objective-C method conversion error for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.imp {
            ObjcMethodErrorImpl::Class(error) => error.fmt(f),
            ObjcMethodErrorImpl::Selector(error) => error.fmt(f),
            ObjcMethodErrorImpl::Resolve {
                class,
                selector,
                kind,
            } => write!(
                f,
                "Objective-C {} method '{}' was not found on class '{}'",
                kind, selector, class
            ),
        }
    }
}

impl Error for ObjcMethodError {
    /// Returns the underlying source error when one exists.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.imp {
            ObjcMethodErrorImpl::Class(error) => Some(error),
            ObjcMethodErrorImpl::Selector(error) => Some(error),
            ObjcMethodErrorImpl::Resolve { .. } => None,
        }
    }
}

impl From<ObjcClassError> for ObjcMethodError {
    /// Maps one class conversion failure into one method conversion failure.
    fn from(error: ObjcClassError) -> Self {
        Self {
            imp: ObjcMethodErrorImpl::Class(error),
        }
    }
}

impl From<ObjcSelectorError> for ObjcMethodError {
    /// Maps one selector conversion failure into one method conversion failure.
    fn from(error: ObjcSelectorError) -> Self {
        Self {
            imp: ObjcMethodErrorImpl::Selector(error),
        }
    }
}

/// Converts one supported value into one resolved [`ObjcMethod`].
///
/// Common inputs are an existing [`ObjcMethod`] or one tuple-like identity that
/// combines a class, selector, and method kind through the provided impls.
pub trait IntoObjcMethod {
    /// Converts this value into one resolved Objective-C method target.
    fn into_objc_method(self) -> Result<ObjcMethod, ObjcMethodError>;
}

/// Converts one supported value into one [`ObjcClass`].
///
/// Common inputs are existing [`ObjcClass`] values and class names as Rust or C
/// strings.
pub trait IntoObjcClass {
    /// Converts this value into one resolved Objective-C class.
    fn into_objc_class(self) -> Result<ObjcClass, ObjcClassError>;
}

/// Converts one supported value into one [`ObjcSelector`].
///
/// Common inputs are existing [`ObjcSelector`] values and selector names as
/// Rust or C strings.
pub trait IntoObjcSelector {
    /// Converts this value into one Objective-C selector identifier.
    fn into_objc_selector(self) -> Result<ObjcSelector, ObjcSelectorError>;
}

/// Converts one supported value into one resolved [`ObjcMethod`].
///
/// This is a convenience wrapper around [`IntoObjcMethod::into_objc_method`].
pub fn into_objc_method<T: IntoObjcMethod>(value: T) -> Result<ObjcMethod, ObjcMethodError> {
    value.into_objc_method()
}

/// Converts one supported value into one [`ObjcClass`].
///
/// This is a convenience wrapper around [`IntoObjcClass::into_objc_class`].
pub fn into_objc_class<T: IntoObjcClass>(value: T) -> Result<ObjcClass, ObjcClassError> {
    value.into_objc_class()
}

/// Converts one supported value into one [`ObjcSelector`].
///
/// This is a convenience wrapper around [`IntoObjcSelector::into_objc_selector`].
pub fn into_objc_selector<T: IntoObjcSelector>(
    value: T,
) -> Result<ObjcSelector, ObjcSelectorError> {
    value.into_objc_selector()
}

/// Conversion trait impls for high-level Objective-C targets.
mod impls {
    use super::*;
    use crate::function::{IntoSymbol, Symbol, SymbolError};

    impl IntoSymbol for ObjcSelector {
        /// Reuses one Objective-C selector as one symbol-like name.
        fn into_symbol(self) -> Result<Symbol, SymbolError> {
            Ok(Symbol::from_cstring(self.0))
        }
    }

    impl IntoObjcClass for ObjcClass {
        /// Returns the same class identifier unchanged.
        fn into_objc_class(self) -> Result<ObjcClass, ObjcClassError> {
            Ok(self)
        }
    }

    impl IntoObjcClass for &str {
        /// Resolves one borrowed Objective-C class name.
        fn into_objc_class(self) -> Result<ObjcClass, ObjcClassError> {
            CString::new(self)
                .map_err(ObjcClassError::invalid_name)?
                .into_objc_class()
        }
    }

    impl IntoObjcClass for String {
        /// Resolves one owned Objective-C class name.
        fn into_objc_class(self) -> Result<ObjcClass, ObjcClassError> {
            CString::new(self)
                .map_err(ObjcClassError::invalid_name)?
                .into_objc_class()
        }
    }

    impl IntoObjcClass for &CStr {
        /// Resolves one borrowed Objective-C class name.
        fn into_objc_class(self) -> Result<ObjcClass, ObjcClassError> {
            self.to_owned().into_objc_class()
        }
    }

    impl IntoObjcClass for CString {
        /// Resolves one owned Objective-C class name.
        fn into_objc_class(self) -> Result<ObjcClass, ObjcClassError> {
            let class =
                resolve_class(&self).ok_or_else(|| ObjcClassError::not_found(self.clone()))?;
            Ok(ObjcClass {
                resolved: class,
                name: self,
            })
        }
    }

    impl IntoObjcSelector for ObjcSelector {
        /// Returns the same selector identifier unchanged.
        fn into_objc_selector(self) -> Result<ObjcSelector, ObjcSelectorError> {
            Ok(self)
        }
    }

    impl IntoObjcSelector for &str {
        /// Wraps one borrowed selector name.
        fn into_objc_selector(self) -> Result<ObjcSelector, ObjcSelectorError> {
            CString::new(self)
                .map(ObjcSelector)
                .map_err(ObjcSelectorError::invalid_name)
        }
    }

    impl IntoObjcSelector for String {
        /// Wraps one owned selector name.
        fn into_objc_selector(self) -> Result<ObjcSelector, ObjcSelectorError> {
            CString::new(self)
                .map(ObjcSelector)
                .map_err(ObjcSelectorError::invalid_name)
        }
    }

    impl IntoObjcSelector for &CStr {
        /// Wraps one borrowed selector name.
        fn into_objc_selector(self) -> Result<ObjcSelector, ObjcSelectorError> {
            Ok(ObjcSelector(self.to_owned()))
        }
    }

    impl IntoObjcSelector for CString {
        /// Wraps one owned selector name.
        fn into_objc_selector(self) -> Result<ObjcSelector, ObjcSelectorError> {
            Ok(ObjcSelector(self))
        }
    }

    impl IntoObjcMethod for ObjcMethod {
        /// Returns the same Objective-C method target unchanged.
        fn into_objc_method(self) -> Result<ObjcMethod, ObjcMethodError> {
            Ok(self)
        }
    }

    impl<C, S> IntoObjcMethod for (C, S, ObjcMethodKind)
    where
        C: IntoObjcClass,
        S: IntoObjcSelector,
    {
        /// Resolves one Objective-C method from one class, selector, and method kind.
        fn into_objc_method(self) -> Result<ObjcMethod, ObjcMethodError> {
            let class = self.0.into_objc_class().map_err(ObjcMethodError::from)?;
            let selector = self.1.into_objc_selector().map_err(ObjcMethodError::from)?;
            ObjcMethod::resolve(class, selector, self.2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ObjcMethod, ObjcMethodKind, into_objc_class, into_objc_method, into_objc_selector,
    };
    use crate::function::into_symbol;

    #[test]
    fn resolves_classes_from_strings() {
        let class = into_objc_class("NSObject").expect("valid class");
        assert_eq!(class.name(), "NSObject");
        assert_eq!(class.to_string(), "NSObject");
    }

    #[test]
    fn wraps_strings_as_selectors() {
        let selector = into_objc_selector("frontmostApplication").expect("valid selector");
        assert_eq!(selector.name(), "frontmostApplication");
        assert_eq!(selector.to_string(), "frontmostApplication");
    }

    #[test]
    fn reuses_selectors_as_symbols() {
        let symbol = into_symbol("frontmostApplication").expect("valid symbol");
        let selector_symbol =
            into_symbol(into_objc_selector("frontmostApplication").expect("valid selector"))
                .expect("valid selector symbol");
        assert_eq!(selector_symbol, symbol);
        assert_eq!(selector_symbol.name(), "frontmostApplication");
    }

    #[test]
    fn reports_missing_classes_with_typed_error() {
        let error = into_objc_class("__DefinitelyMissingObjcClass").expect_err("missing class");
        assert_eq!(
            error.to_string(),
            "Objective-C class '__DefinitelyMissingObjcClass' was not found in the runtime"
        );
    }

    #[test]
    fn builds_instance_methods_from_class_and_selector() {
        let class = into_objc_class("NSObject").expect("valid class");
        let selector = into_objc_selector("description").expect("valid selector");
        let method = ObjcMethod::instance(class.clone(), selector.clone()).expect("valid method");

        assert!(method.is_instance());
        assert!(!method.is_class());
        assert_eq!(method.class_ref(), &class);
        assert_eq!(method.selector(), &selector);
        assert_eq!(method.to_string(), "NSObject::description");
        let _resolved = method.resolved;
    }

    #[test]
    fn builds_instance_methods_from_class_helpers() {
        let class = into_objc_class("NSObject").expect("valid class");
        let method = class
            .instance_method("description")
            .expect("valid instance method");

        assert!(method.is_instance());
        assert_eq!(method.class_ref(), &class);
    }

    #[test]
    fn builds_class_methods_from_class_and_selector() {
        let class = into_objc_class("NSObject").expect("valid class");
        let selector = into_objc_selector("new").expect("valid selector");
        let method = ObjcMethod::class(class.clone(), selector.clone()).expect("valid method");

        assert!(method.is_class());
        assert!(!method.is_instance());
        assert_eq!(method.class_ref(), &class);
        assert_eq!(method.selector(), &selector);
        assert_eq!(method.to_string(), "NSObject::new");
        let _resolved = method.resolved;
    }

    #[test]
    fn builds_class_methods_from_class_helpers() {
        let class = into_objc_class("NSObject").expect("valid class");
        let method = class.class_method("new").expect("valid class method");

        assert!(method.is_class());
        assert_eq!(method.class_ref(), &class);
    }

    #[test]
    fn resolves_methods_from_tuples() {
        let method = into_objc_method(("NSObject", "description", ObjcMethodKind::Instance))
            .expect("valid tuple method");

        assert!(method.is_instance());
        assert_eq!(method.to_string(), "NSObject::description");
    }

    #[test]
    fn reports_missing_instance_methods_with_typed_error() {
        let class = into_objc_class("NSObject").expect("valid class");
        let selector = into_objc_selector("__definitelyMissingSelector").expect("valid selector");
        let error = ObjcMethod::instance(class, selector).expect_err("missing method");

        assert_eq!(
            error.to_string(),
            "Objective-C instance method '__definitelyMissingSelector' was not found on class 'NSObject'"
        );
        assert!(error.is_absent());
    }

    #[test]
    fn reports_missing_class_methods_with_typed_error() {
        let class = into_objc_class("NSObject").expect("valid class");
        let selector = into_objc_selector("__definitelyMissingSelector").expect("valid selector");
        let error = ObjcMethod::class(class, selector).expect_err("missing method");

        assert_eq!(
            error.to_string(),
            "Objective-C class method '__definitelyMissingSelector' was not found on class 'NSObject'"
        );
        assert!(error.is_absent());
    }

    #[test]
    fn reports_invalid_class_names_lossily() {
        let error = into_objc_class("Definitely\0InvalidObjcClass")
            .expect_err("interior nul byte must fail");
        assert_eq!(
            error.to_string(),
            "invalid Objective-C class name 'Definitely\\0InvalidObjcClass': interior NUL at byte 10"
        );
    }

    #[test]
    fn reports_invalid_selector_names_lossily() {
        let error = into_objc_selector("Definitely\0InvalidSelector")
            .expect_err("interior nul byte must fail");
        assert_eq!(
            error.to_string(),
            "invalid Objective-C selector name 'Definitely\\0InvalidSelector': interior NUL at byte 10"
        );
    }
}
