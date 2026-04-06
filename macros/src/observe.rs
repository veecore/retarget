//! Shared observer and observe expansion helpers.

use crate::args::{HookObserveArgs, HookObserverArgs, parse_hook_observe_args_tokens};
use crate::support::{attr_path_ends_with, interception_mode_tokens, require_arg};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Expr, ExprLit, Ident, ItemFn, Lit, Meta, Result, Type};

// Observer/observe entrypoints

/// Expands one interception observer definition.
pub(crate) fn expand_hook_observer(
    args: HookObserverArgs,
    function: ItemFn,
) -> Result<TokenStream2> {
    let fn_ident = function.sig.ident.clone();
    let default = require_arg(
        args.default,
        &fn_ident,
        "missing required `default` argument",
    )?;
    let default_mode = interception_mode_tokens(&default, &fn_ident)?;
    let observer_arg_ty = observer_arg_ty(&function)?;

    Ok(quote! {
        #function

        // Keep the observer contract entirely local to the user's module:
        // observed hooks read this default directly instead of consulting any
        // runtime registry.
        #[doc(hidden)]
        pub const __RETARGET_INTERCEPTION_DEFAULT_MODE: ::retarget::InterceptionMode =
            #default_mode;

        // The injected hook code calls this symbol directly. Making it a typed
        // function pointer keeps the generated call site simple while still
        // letting normal Rust type checking validate the observer signature.
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const __retarget_interception_observe: fn(#observer_arg_ty) = #fn_ident;
    })
}

/// Returns the single observer argument type after validating the signature shape.
fn observer_arg_ty(function: &ItemFn) -> Result<Type> {
    let inputs: Vec<&syn::FnArg> = function.sig.inputs.iter().collect();
    match inputs.as_slice() {
        [syn::FnArg::Typed(arg)] => Ok((*arg.ty).clone()),
        _ => Err(syn::Error::new_spanned(
            &function.sig.inputs,
            "hook::observer expects exactly one argument",
        )),
    }
}

/// Re-emits one observe helper attribute as an inert marker so the hook macro can
/// still recover it if this attribute expands first.
pub(crate) fn expand_hook_observe(args: HookObserveArgs, function: ItemFn) -> Result<TokenStream2> {
    let marker = syn::LitStr::new(
        &format!("__retarget_observe({})", observe_marker_tokens(&args)),
        proc_macro2::Span::call_site(),
    );

    Ok(quote! {
        // `hook::observe` is a standalone proc-macro attribute, so it cannot
        // directly hand structured state to the later hook expansion. We stash
        // the parsed tokens in one inert doc attribute and recover them when
        // `hook::c` / `hook::com` / `hook::objc` rewrites the callable.
        #[doc = #marker]
        #function
    })
}

// Observe extraction and body injection

/// Pulls one optional `#[hook::observe(...)]` declaration off one callable's attributes.
pub(crate) fn take_observe_args(
    attrs: &mut Vec<syn::Attribute>,
) -> Result<Option<HookObserveArgs>> {
    let mut observe = None;
    let mut index = 0;

    while index < attrs.len() {
        let parsed = if attr_path_ends_with(&attrs[index], "observe") {
            Some(parse_observe_attr(&attrs[index])?)
        } else {
            parse_observe_marker_attr(&attrs[index])?
        };

        if let Some(parsed) = parsed {
            if observe.replace(parsed).is_some() {
                return Err(syn::Error::new_spanned(
                    &attrs[index],
                    "duplicate `hook::observe` attribute",
                ));
            }
            attrs.remove(index);
            continue;
        }

        index += 1;
    }

    Ok(observe)
}

/// Injects one observe dispatch at the start of the callable body.
pub(crate) fn inject_observe_dispatch(
    block: &mut syn::Block,
    hook_id: &Expr,
    intercept_once_ident: &Ident,
    observe: Option<&HookObserveArgs>,
    ident: &Ident,
) -> Result<()> {
    let Some(observe) = observe else {
        return Ok(());
    };

    let observe_mode = observe
        .mode
        .as_ref()
        .map(|mode| interception_mode_tokens(mode, ident))
        .transpose()?;
    let mode = observe_mode.unwrap_or_else(|| quote!(__RETARGET_INTERCEPTION_DEFAULT_MODE));
    let dispatch_value = match observe.value.as_ref() {
        Some(value) => quote! {
            ::retarget::Signal {
                event: __retarget_interception_event,
                value: (#value),
            }
        },
        None => quote!(__retarget_interception_event),
    };
    let original_stmts = block.stmts.clone();

    rewrite_block(
        block,
        quote!({
            let __retarget_interception_mode = #mode;
            let __retarget_emit_interception = || {
                let __retarget_interception_event = ::retarget::InterceptionHit {
                    hook_id: #hook_id,
                    mode: __retarget_interception_mode,
                    at: ::retarget::__macro_support::interception_time(),
                };
                __retarget_interception_observe(#dispatch_value);
            };

            // Run observation at the function head so the observer sees the hit
            // even if the hook body returns early or panics later. The once
            // gate is local to each generated hook, which keeps FirstHit
            // bookkeeping out of any shared interception runtime.
            match __retarget_interception_mode {
                ::retarget::InterceptionMode::Off => {}
                ::retarget::InterceptionMode::FirstHit => {
                    if #intercept_once_ident.set(()).is_ok() {
                        __retarget_emit_interception();
                    }
                }
                ::retarget::InterceptionMode::EveryHit => __retarget_emit_interception(),
            }

            #(#original_stmts)*
        }),
    )
}

// Observe parsing helpers

/// Parses one direct `#[hook::observe(...)]` helper attribute.
fn parse_observe_attr(attr: &syn::Attribute) -> Result<HookObserveArgs> {
    match &attr.meta {
        Meta::List(list) => parse_hook_observe_args_tokens(list.tokens.clone()),
        Meta::Path(_) => Ok(HookObserveArgs::default()),
        Meta::NameValue(_) => Err(syn::Error::new_spanned(
            attr,
            "hook::observe expects a parenthesized argument list",
        )),
    }
}

/// Parses one inert marker emitted when `#[hook::observe(...)]` expands before a hook macro.
fn parse_observe_marker_attr(attr: &syn::Attribute) -> Result<Option<HookObserveArgs>> {
    if !attr.path().is_ident("doc") {
        return Ok(None);
    }

    let Meta::NameValue(value) = &attr.meta else {
        return Ok(None);
    };
    let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = &value.value
    else {
        return Ok(None);
    };
    let value = value.value();
    let Some(tokens) = value
        .strip_prefix("__retarget_observe(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Ok(None);
    };
    // Re-parse the exact tokens that `expand_hook_observe` serialized above so
    // the hook macro sees the same `HookObserveArgs` whether it read the
    // original attribute directly or this inert marker.
    let tokens: proc_macro2::TokenStream =
        syn::parse_str(tokens).map_err(|error| syn::Error::new_spanned(attr, error))?;
    parse_hook_observe_args_tokens(tokens).map(Some)
}

/// Builds the inert internal marker emitted by `#[hook::observe(...)]`.
fn observe_marker_tokens(args: &HookObserveArgs) -> TokenStream2 {
    let value = args.value.as_ref().map(|value| quote!(value = #value));
    let mode = args.mode.as_ref().map(|mode| quote!(mode = #mode));
    match (value, mode) {
        (Some(value), Some(mode)) => quote!(#value, #mode),
        (Some(value), None) => quote!(#value),
        (None, Some(mode)) => quote!(#mode),
        (None, None) => TokenStream2::new(),
    }
}

// Low-level rewriting helpers

/// Replaces one block body with parsed generated tokens.
fn rewrite_block(block: &mut syn::Block, tokens: TokenStream2) -> Result<()> {
    *block = syn::parse2(tokens)?;
    Ok(())
}
