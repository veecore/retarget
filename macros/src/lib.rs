//! Proc-macro entrypoints for the hook attribute surface.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("retarget only supports macOS and Windows");

mod args;
mod callable;
mod expand;
mod support;

use crate::args::{
    parse_hook_args, parse_hook_com_args, parse_hook_com_impl_args, parse_hook_objc_args,
    parse_hook_objc_impl_args, parse_hook_observe_args, parse_hook_observer_args,
};
use crate::expand::{
    expand_hook, expand_hook_com, expand_hook_com_impl, expand_hook_objc, expand_hook_objc_impl,
    expand_hook_observe, expand_hook_observer,
};
use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, ItemImpl, Result, parse_macro_input};

/// Parses one function attribute and expands it through the provided callback.
fn expand_fn_attr<A>(
    attr: TokenStream,
    item: TokenStream,
    parse_args: fn(TokenStream) -> Result<A>,
    expand: fn(A, ItemFn) -> Result<proc_macro2::TokenStream>,
) -> TokenStream {
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

#[proc_macro_attribute]
/// Expands `#[hook::c(...)]` / `#[hook::function(...)]` function hooks.
pub fn hook_c(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_args, expand_hook)
}

#[proc_macro_attribute]
/// Expands `#[hook::objc::class(...)]` by forcing the Objective-C kind to class.
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
/// Expands `#[hook::objc::instance(...)]` by forcing the Objective-C kind to instance.
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
/// Expands `#[hook::objc::methods(...)]` impl blocks.
pub fn hook_objc_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_impl_attr(attr, item, parse_hook_objc_impl_args, expand_hook_objc_impl)
}

#[proc_macro_attribute]
/// Expands one interception-observer function.
pub fn hook_observer(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_observer_args, expand_hook_observer)
}

#[proc_macro_attribute]
/// Expands one interception override declaration.
pub fn hook_observe(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_observe_args, expand_hook_observe)
}

#[proc_macro_attribute]
/// Expands one COM hook function.
pub fn hook_com(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_fn_attr(attr, item, parse_hook_com_args, expand_hook_com)
}

#[proc_macro_attribute]
/// Expands one impl block containing COM hook methods.
pub fn hook_com_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_impl_attr(attr, item, parse_hook_com_impl_args, expand_hook_com_impl)
}
