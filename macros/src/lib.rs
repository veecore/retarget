//! Proc-macro entrypoints for the public hook attribute surface.
//!
//! These docs describe the macros as users see them through `retarget::hook`.
//! Generated helper names and anything under `retarget::__macro_support` are
//! implementation details and should not be used directly.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("retarget only supports macOS and Windows");

mod args;
mod callable;
mod expand;
mod observe;
mod support;

use crate::args::{
    parse_hook_args, parse_hook_com_args, parse_hook_com_impl_args, parse_hook_objc_args,
    parse_hook_objc_impl_args, parse_hook_observe_args, parse_hook_observer_args,
};
use crate::expand::{
    expand_hook, expand_hook_com, expand_hook_com_impl, expand_hook_objc, expand_hook_objc_impl,
};
use crate::observe::{expand_hook_observe, expand_hook_observer};
use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, ItemImpl, Result, parse_macro_input};

// Shared entrypoint plumbing

/// Parses one function attribute and expands it through the provided callback.
fn expand_fn_attr<A>(
    attr: TokenStream,
    item: TokenStream,
    parse_args: fn(TokenStream) -> Result<A>,
    expand: fn(A, ItemFn) -> Result<proc_macro2::TokenStream>,
) -> TokenStream {
    // Keep the proc-macro entrypoints intentionally thin: each one just parses
    // attribute arguments plus the annotated item, then hands the real work to
    // the shared expansion layer.
    let args = match parse_args(attr) {
        Ok(args) => args,
        Err(err) => return err.to_compile_error().into(),
    };
    let function = parse_macro_input!(item as ItemFn);
    match expand(args, function) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Parses one impl-block attribute and expands it through the provided callback.
fn expand_impl_attr<A>(
    attr: TokenStream,
    item: TokenStream,
    parse_args: fn(TokenStream) -> Result<A>,
    expand: fn(A, ItemImpl) -> Result<proc_macro2::TokenStream>,
) -> TokenStream {
    // Impl-block hooks follow the same split as free-function hooks; the only
    // difference is the outer item kind that gets parsed before expansion.
    let args = match parse_args(attr) {
        Ok(args) => args,
        Err(err) => return err.to_compile_error().into(),
    };
    let input = parse_macro_input!(item as ItemImpl);
    match expand(args, input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

// Public proc-macro entrypoints

#[proc_macro_attribute]
/// Declares one exported-function hook.
///
/// This is the cross-platform entrypoint for hooking named exported functions.
/// The target can be written in a few equivalent styles:
///
/// - `#[hook::c]` to use the Rust function name as the symbol name
/// - `#[hook::c("symbol")]` for one explicit process-global symbol
/// - `#[hook::c(("module", "symbol"))]` for one module-scoped symbol
/// - `#[hook::c(function = ..., optional = ..., fallback = ...)]` for the
///   fully named form
///
/// # Warnings
///
/// The Rust signature must exactly match the hooked function's ABI and
/// argument layout. A mismatched signature is undefined behavior.
pub fn hook_c(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_args, expand_hook)
}

#[proc_macro_attribute]
/// Declares one Objective-C class-method hook.
///
/// This is the same as `#[hook::objc(..., kind = Class)]`, but with the method
/// kind fixed by the attribute itself.
pub fn hook_objc_class(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut combined = proc_macro2::TokenStream::from(attr);
    if !combined.is_empty() {
        combined.extend(quote!(,));
    }
    combined.extend(quote!(
        kind = ::retarget::__macro_support::ObjcMethodKind::Class
    ));
    expand_fn_attr(
        combined.into(),
        item,
        parse_hook_objc_args,
        expand_hook_objc,
    )
}

#[proc_macro_attribute]
/// Declares one Objective-C instance-method hook.
///
/// This is the same as `#[hook::objc(..., kind = Instance)]`, but with the
/// method kind fixed by the attribute itself.
pub fn hook_objc_instance(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut combined = proc_macro2::TokenStream::from(attr);
    if !combined.is_empty() {
        combined.extend(quote!(,));
    }
    combined.extend(quote!(
        kind = ::retarget::__macro_support::ObjcMethodKind::Instance
    ));
    expand_fn_attr(
        combined.into(),
        item,
        parse_hook_objc_args,
        expand_hook_objc,
    )
}

#[proc_macro_attribute]
/// Groups Objective-C hooks inside one inherent impl block.
///
/// Only methods annotated with `#[hook::objc::instance(...)]` or
/// `#[hook::objc::class(...)]` participate in hook generation.
pub fn hook_objc_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_impl_attr(attr, item, parse_hook_objc_impl_args, expand_hook_objc_impl)
}

#[proc_macro_attribute]
/// Declares one local interception observer.
///
/// The observer must be a free function with exactly one argument. That
/// argument is usually either `retarget::intercept::Event` or
/// `retarget::intercept::Signal<T>`.
///
/// The required `default = ...` argument provides the mode used by bare
/// `#[hook::observe]` annotations in the same module.
///
/// # Warnings
///
/// Observed hooks currently need to live in the same module as the observer.
pub fn hook_observer(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_observer_args, expand_hook_observer)
}

#[proc_macro_attribute]
/// Opts one hook into observation.
///
/// Forms:
///
/// - `#[hook::observe]` uses the observer's default mode
/// - `#[hook::observe(Mode::EveryHit)]` overrides just the mode
/// - `#[hook::observe(MySignal::Thing)]` emits one typed payload
/// - `#[hook::observe(MySignal::Thing, mode = Mode::EveryHit)]` supplies both
///
/// This attribute is intentionally small: it only controls whether one hook
/// emits observation events and what typed payload, if any, gets passed to the
/// observer.
///
/// # Warnings
///
/// This must be paired with `#[hook::observer(...)]` in the same module. Any
/// visible generated helper names involved in that plumbing are internal and
/// may change without notice.
pub fn hook_observe(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_observe_args, expand_hook_observe)
}

#[proc_macro_attribute]
/// Declares one COM hook function.
///
/// This is the low-level COM entrypoint for cases where you already know the
/// symbol or resolution strategy. For grouped, interface-oriented COM hooks,
/// prefer `#[hook::com_impl(...)]`.
pub fn hook_com(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_com_args, expand_hook_com)
}

#[proc_macro_attribute]
/// Groups COM hooks inside one inherent impl block.
///
/// Method names default to PascalCase COM field names, so a Rust method like
/// `present` naturally targets `Present` unless you override it with
/// `#[hook::com(field = ...)]`.
///
/// # Warnings
///
/// These methods are not real Rust receivers over the hooked object. The first
/// argument still needs to model the foreign receiver explicitly, usually as a
/// raw pointer.
pub fn hook_com_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_impl_attr(attr, item, parse_hook_com_impl_args, expand_hook_com_impl)
}
