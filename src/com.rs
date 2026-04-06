//! High-level Windows COM hook targets.
//!
//! These types model the COM-specific part of hook resolution: one live
//! interface pointer, one vtable slot, and the resolved function that lives
//! there.

use crate::error::expect_utf8;
use crate::function::{
    Function, FunctionError, FunctionPointer, FunctionReplaceError, Symbol, into_function,
};
use std::ffi::{CStr, CString, c_void};
use std::ptr::NonNull;

/// One resolved COM method target.
///
/// This is the resolved form of a COM hook target: the crate has already read
/// one vtable slot from one live interface instance and turned it into a
/// normal [`Function`] for replacement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComMethod {
    /// The method descriptor used to resolve this target.
    id: ComMethodId,
    /// The resolved function target.
    function: Function,
}

impl ComMethod {
    /// Returns the retained COM method descriptor.
    pub fn id(&self) -> &ComMethodId {
        &self.id
    }

    /// Returns the resolved function target.
    pub fn function(&self) -> &Function {
        &self.function
    }

    /// Replaces this resolved COM method target with one typed replacement.
    ///
    /// On success, the returned function pointer is the original vtable entry.
    ///
    /// # Safety
    ///
    /// `T` must exactly match the ABI and signature of both the target method
    /// and the replacement implementation.
    pub unsafe fn replace_with<T: FunctionPointer>(
        &self,
        replacement: T,
    ) -> Result<T, FunctionReplaceError> {
        unsafe { self.function.replace_with(replacement) }
    }
}

/// One COM method descriptor.
///
/// This is the COM equivalent of one selector-like identity: it tells the
/// crate which vtable slot to read from one live interface instance. Optional
/// names exist only for diagnostics.
///
/// # Warnings
///
/// `slot_index` is a raw zero-based vtable slot index. `retarget` does not
/// validate that it matches the intended interface method beyond checking that
/// the slot can be read.
///
/// Equality and hashing use only the slot index. Optional names are retained
/// for diagnostics and do not participate.
#[derive(Debug, Clone)]
pub struct ComMethodId {
    /// Zero-based vtable slot index.
    slot_index: usize,
    /// Optional interface name used only for diagnostics.
    interface_name: Option<CString>,
    /// Optional method name used only for diagnostics.
    method_name: Option<CString>,
}

impl ComMethodId {
    /// Builds one unnamed COM method descriptor from one zero-based slot index.
    pub fn new(slot_index: usize) -> Self {
        Self {
            slot_index,
            interface_name: None,
            method_name: None,
        }
    }

    /// Builds one named COM method descriptor from one zero-based slot index.
    pub fn named(slot_index: usize, interface_name: CString, method_name: CString) -> Self {
        Self::new(slot_index)
            .with_interface_name(interface_name)
            .with_method_name(method_name)
    }

    /// Returns this zero-based vtable slot index.
    pub fn slot_index(&self) -> usize {
        self.slot_index
    }

    /// Returns this optional interface name as UTF-8 text.
    pub fn interface_name(&self) -> Option<&str> {
        self.interface_name.as_deref().map(expect_utf8)
    }

    /// Returns this optional method name as UTF-8 text.
    pub fn method_name(&self) -> Option<&str> {
        self.method_name.as_deref().map(expect_utf8)
    }

    /// Attaches one retained interface name used only for diagnostics.
    pub fn with_interface_name(mut self, interface_name: CString) -> Self {
        self.interface_name = Some(interface_name);
        self
    }

    /// Attaches one retained method name used only for diagnostics.
    pub fn with_method_name(mut self, method_name: CString) -> Self {
        self.method_name = Some(method_name);
        self
    }

    /// Resolves this COM method descriptor on one live interface instance.
    ///
    /// This is the step that turns one "slot on one interface instance" into a
    /// concrete hookable function target.
    ///
    /// # Safety
    ///
    /// `instance` must point to one live COM interface instance whose vtable is
    /// large enough to contain this slot.
    pub unsafe fn resolve_on<I: IntoComInstance>(
        &self,
        instance: I,
    ) -> Result<ComMethod, ComError> {
        let instance = instance.into_com_instance()?;
        let slot = unsafe { crate::imp::com::vtable_slot(instance.as_raw(), self.slot_index) }
            .ok_or_else(|| ComError::slot_unavailable(self.clone()))?;
        let function = into_function(slot)
            .map(|function| match &self.method_name {
                Some(method_name) => {
                    function.with_symbol(Symbol::from_cstring(method_name.clone()))
                }
                None => function,
            })
            .map_err(|source| ComError::function(self.clone(), source))?;
        Ok(ComMethod {
            id: self.clone(),
            function,
        })
    }
}

/// One live COM interface instance used to discover one concrete implementation.
///
/// This wrapper is non-owning. It does not call `AddRef` and it does not manage
/// the lifetime of the underlying COM object for you.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComInstance {
    /// Raw non-null COM interface pointer.
    raw: NonNull<c_void>,
}

impl ComInstance {
    /// Wraps one raw COM interface pointer.
    ///
    /// # Safety
    ///
    /// `raw` must point to one live COM interface instance.
    pub unsafe fn from_raw(raw: NonNull<c_void>) -> Self {
        Self { raw }
    }

    /// Wraps one interface pointer written through one out-parameter.
    ///
    /// # Safety
    ///
    /// `out` must point to one writable out-parameter location that now stores
    /// one live COM interface pointer.
    pub unsafe fn from_out_ptr(out: *mut *mut c_void) -> Result<Self, ComError> {
        let raw =
            unsafe { crate::imp::com::out_ptr_value(out) }.ok_or_else(ComError::null_instance)?;
        Ok(Self { raw })
    }

    /// Returns this COM interface pointer as one raw pointer.
    pub fn as_raw(&self) -> NonNull<c_void> {
        self.raw
    }

    /// Resolves one method descriptor on this COM interface instance.
    ///
    /// # Safety
    ///
    /// This instance must remain live while the method slot is being read.
    pub unsafe fn resolve<M: IntoComMethodId>(&self, method: M) -> Result<ComMethod, ComError> {
        let method = method.into_com_method_id()?;
        unsafe { method.resolve_on(*self) }
    }
}

/// One COM target construction failure.
#[derive(Debug)]
pub struct ComError {
    /// Private COM conversion details.
    imp: ComErrorImpl,
}

/// Private COM conversion details.
#[derive(Debug)]
enum ComErrorImpl {
    /// The provided COM interface pointer was null.
    NullInstance,
    /// The requested method slot was unavailable on the provided interface.
    MissingMethod {
        /// The requested method descriptor.
        method: ComMethodId,
    },
    /// The resolved method pointer could not be wrapped as one [`Function`].
    Function {
        /// The requested method descriptor.
        method: ComMethodId,
        /// The underlying function-resolution failure.
        source: FunctionError,
    },
}

impl ComError {
    /// Builds one typed null-instance error.
    fn null_instance() -> Self {
        Self {
            imp: ComErrorImpl::NullInstance,
        }
    }

    /// Builds one typed missing-method error.
    fn slot_unavailable(method: ComMethodId) -> Self {
        Self {
            imp: ComErrorImpl::MissingMethod { method },
        }
    }

    /// Builds one typed function-resolution failure for one COM method descriptor.
    fn function(method: ComMethodId, source: FunctionError) -> Self {
        Self {
            imp: ComErrorImpl::Function { method, source },
        }
    }
}

/// Converts one supported COM method target into one resolved [`ComMethod`].
pub trait IntoComMethod {
    /// Converts this value into one resolved COM method target.
    ///
    /// Common inputs are an existing [`ComMethod`] or a `(instance, method)`
    /// pair where `method` can itself be one [`ComMethodId`] or slot index.
    ///
    /// # Safety
    ///
    /// Any interface instance embedded in this value must point to one live
    /// COM interface whose vtable can be read safely.
    unsafe fn into_com_method(self) -> Result<ComMethod, ComError>;
}

/// Converts one supported COM method-descriptor value into one [`ComMethodId`].
pub trait IntoComMethodId {
    /// Converts this value into one COM method descriptor.
    ///
    /// Common inputs are a raw `usize` slot index or one prebuilt
    /// [`ComMethodId`].
    fn into_com_method_id(self) -> Result<ComMethodId, ComError>;
}

/// Converts one supported COM interface value into one [`ComInstance`].
pub trait IntoComInstance {
    /// Converts this value into one non-null COM interface instance wrapper.
    ///
    /// Common inputs are raw pointers, [`NonNull`] pointers, existing
    /// [`ComInstance`] values, and `windows_core::Interface` values.
    fn into_com_instance(self) -> Result<ComInstance, ComError>;
}

/// Converts one supported COM method target into one resolved [`ComMethod`].
///
/// This is a convenience wrapper around [`IntoComMethod::into_com_method`].
///
/// # Safety
///
/// Any interface instance embedded in `value` must point to one live COM
/// interface whose vtable can be read safely.
pub unsafe fn into_com_method<T: IntoComMethod>(value: T) -> Result<ComMethod, ComError> {
    unsafe { value.into_com_method() }
}

/// Converts one supported COM method-descriptor value into one [`ComMethodId`].
pub fn into_com_method_id<T: IntoComMethodId>(value: T) -> Result<ComMethodId, ComError> {
    value.into_com_method_id()
}

/// Converts one supported COM interface value into one [`ComInstance`].
pub fn into_com_instance<T: IntoComInstance>(value: T) -> Result<ComInstance, ComError> {
    value.into_com_instance()
}

/// Standard-library trait impls used by the public COM target and error models.
mod std_impls {
    use super::*;
    use std::error::Error;
    use std::fmt;
    use std::hash::{Hash, Hasher};

    impl fmt::Display for ComMethod {
        /// Formats one resolved COM method target for diagnostics.
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.id.fmt(f)
        }
    }

    impl AsRef<Function> for ComMethod {
        /// Borrows the resolved function target.
        fn as_ref(&self) -> &Function {
            self.function()
        }
    }

    impl PartialEq for ComMethodId {
        fn eq(&self, other: &Self) -> bool {
            self.slot_index == other.slot_index
        }
    }

    impl Eq for ComMethodId {}

    impl Hash for ComMethodId {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.slot_index.hash(state);
        }
    }

    impl fmt::Display for ComMethodId {
        /// Formats one COM method descriptor for diagnostics.
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match (self.interface_name(), self.method_name()) {
                (Some(interface_name), Some(method_name)) => {
                    write!(f, "{}::{}", interface_name, method_name)
                }
                (None, Some(method_name)) => {
                    write!(f, "slot {} ('{}')", self.slot_index, method_name)
                }
                (Some(interface_name), None) => {
                    write!(f, "{}::slot({})", interface_name, self.slot_index)
                }
                (None, None) => write!(f, "slot {}", self.slot_index),
            }
        }
    }

    impl fmt::Display for ComError {
        /// Formats one COM target construction failure for diagnostics.
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.imp {
                ComErrorImpl::NullInstance => {
                    f.write_str("null COM interface pointers cannot be resolved")
                }
                ComErrorImpl::MissingMethod { method } => write!(
                    f,
                    "COM method {} could not be resolved on the provided interface instance",
                    method
                ),
                ComErrorImpl::Function { method, source } => {
                    write!(f, "failed to resolve COM method {}: {}", method, source)
                }
            }
        }
    }

    impl Error for ComError {
        /// Returns the underlying source error when one exists.
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match &self.imp {
                ComErrorImpl::NullInstance | ComErrorImpl::MissingMethod { .. } => None,
                ComErrorImpl::Function { source, .. } => Some(source),
            }
        }
    }
}

/// Conversion trait impls for high-level COM targets.
mod impls {
    use super::*;
    use crate::function::IntoFunction;
    use windows_core::Interface;

    impl IntoFunction for ComMethod {
        /// Returns the resolved function target behind this COM method.
        fn into_function(self) -> Result<Function, FunctionError> {
            Ok(self.function)
        }
    }

    impl IntoComMethod for ComMethod {
        /// Returns the same resolved COM method target unchanged.
        unsafe fn into_com_method(self) -> Result<ComMethod, ComError> {
            Ok(self)
        }
    }

    impl<I, M> IntoComMethod for (I, M)
    where
        I: IntoComInstance,
        M: IntoComMethodId,
    {
        /// Resolves one COM method descriptor on one COM interface instance.
        unsafe fn into_com_method(self) -> Result<ComMethod, ComError> {
            let method = self.1.into_com_method_id()?;
            unsafe { method.resolve_on(self.0) }
        }
    }

    impl IntoComMethodId for ComMethodId {
        /// Returns the same COM method descriptor unchanged.
        fn into_com_method_id(self) -> Result<ComMethodId, ComError> {
            Ok(self)
        }
    }

    impl IntoComMethodId for usize {
        /// Wraps one zero-based vtable slot index as one COM method descriptor.
        fn into_com_method_id(self) -> Result<ComMethodId, ComError> {
            Ok(ComMethodId::new(self))
        }
    }

    impl IntoComMethodId for (usize, CString, CString) {
        /// Builds one named COM method descriptor from one slot and retained names.
        fn into_com_method_id(self) -> Result<ComMethodId, ComError> {
            Ok(ComMethodId::named(self.0, self.1, self.2))
        }
    }

    impl IntoComMethodId for (usize, &CStr, &CStr) {
        /// Builds one named COM method descriptor from one slot and borrowed names.
        fn into_com_method_id(self) -> Result<ComMethodId, ComError> {
            Ok(ComMethodId::named(
                self.0,
                self.1.to_owned(),
                self.2.to_owned(),
            ))
        }
    }

    impl IntoComInstance for ComInstance {
        /// Returns the same COM interface instance unchanged.
        fn into_com_instance(self) -> Result<ComInstance, ComError> {
            Ok(self)
        }
    }

    impl IntoComInstance for NonNull<c_void> {
        /// Wraps one non-null raw COM interface pointer.
        fn into_com_instance(self) -> Result<ComInstance, ComError> {
            Ok(unsafe { ComInstance::from_raw(self) })
        }
    }

    impl IntoComInstance for *mut c_void {
        /// Wraps one raw mutable COM interface pointer after validating it is non-null.
        fn into_com_instance(self) -> Result<ComInstance, ComError> {
            NonNull::new(self)
                .ok_or_else(ComError::null_instance)
                .map(|raw| unsafe { ComInstance::from_raw(raw) })
        }
    }

    impl IntoComInstance for *const c_void {
        /// Wraps one raw const COM interface pointer after validating it is non-null.
        fn into_com_instance(self) -> Result<ComInstance, ComError> {
            (self as *mut c_void).into_com_instance()
        }
    }

    impl<T: Interface> IntoComInstance for &T {
        /// Wraps one borrowed Windows COM interface value after validating it is non-null.
        fn into_com_instance(self) -> Result<ComInstance, ComError> {
            self.as_raw().into_com_instance()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ComMethod, ComMethodId};
    use crate::function::{Function, Module, Symbol};
    use std::collections::HashSet;
    use std::ffi::{CString, c_void};
    use std::ptr::NonNull;

    fn fake_ptr(value: usize) -> NonNull<c_void> {
        NonNull::new(value as *mut c_void).expect("non-null test pointer")
    }

    #[test]
    fn method_ids_ignore_diagnostic_names_in_equality_and_hashing() {
        let unnamed = ComMethodId::new(3);
        let named = ComMethodId::named(
            3,
            CString::new("IUnknown").expect("valid c string"),
            CString::new("QueryInterface").expect("valid c string"),
        );

        assert_eq!(unnamed, named);
        assert_eq!(HashSet::from([unnamed, named]).len(), 1);
    }

    #[test]
    fn resolved_methods_ignore_diagnostic_names_in_equality() {
        let first = ComMethod {
            id: ComMethodId::new(7),
            function: unsafe {
                Function::from_resolved_parts(
                    fake_ptr(0x1111),
                    Module::from_resolved_parts(
                        fake_ptr(0x2222),
                        CString::new("first.dll").expect("valid c string"),
                    ),
                    Symbol::from_cstring(CString::new("FirstName").expect("valid c string")),
                )
            },
        };
        let second = ComMethod {
            id: ComMethodId::named(
                7,
                CString::new("IMyInterface").expect("valid c string"),
                CString::new("RenamedMethod").expect("valid c string"),
            ),
            function: unsafe {
                Function::from_resolved_parts(
                    fake_ptr(0x1111),
                    Module::from_resolved_parts(
                        fake_ptr(0x2222),
                        CString::new("second.dll").expect("valid c string"),
                    ),
                    Symbol::from_cstring(CString::new("SecondName").expect("valid c string")),
                )
            },
        };

        assert_eq!(first, second);
    }
}
