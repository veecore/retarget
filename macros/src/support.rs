//! Shared expansion helpers used across the hook proc macros.

use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use syn::{Expr, ExprLit, Ident, Lit, LitStr, Path, Result, Type};

// Shared generated hook scaffold

/// One normalized description of the generated items for a function-like hook.
pub(crate) struct FunctionLikeHook {
    /// The original function or method item.
    pub(crate) input: TokenStream2,
    /// The generated function-pointer alias name.
    pub(crate) fn_ty_ident: Ident,
    /// The generated fallback implementation name.
    pub(crate) fallback_ident: Ident,
    /// The generated original-function accessor name.
    pub(crate) original_ident: Ident,
    /// The generated storage cell for the original implementation.
    pub(crate) original_lock_ident: Ident,
    /// The generated first-hit gate for interception callbacks.
    pub(crate) intercept_once_ident: Ident,
    /// The generated optional accessor function for callers.
    pub(crate) accessor_ident: Ident,
    /// The generated installer function name.
    pub(crate) install_ident: Ident,
    /// The generated distributed-slice hook definition name.
    pub(crate) hook_def_ident: Ident,
    /// The generated function-pointer argument types.
    pub(crate) arg_tys: Vec<Type>,
    /// The generated function-pointer return type.
    pub(crate) ret_ty: TokenStream2,
    /// The required `unsafe` marker from the source signature.
    pub(crate) unsafety: syn::token::Unsafe,
    /// The required `extern` ABI from the source signature.
    pub(crate) abi: syn::Abi,
    /// The generated fallback return expression.
    pub(crate) fallback: Expr,
    /// The generated installer body.
    pub(crate) install_body: TokenStream2,
    /// Extra generated items emitted beside the shared hook scaffolding.
    pub(crate) extra_items: TokenStream2,
}

/// Emits the shared generated items used by plain functions and impl methods.
pub(crate) fn emit_function_like_hook(hook: FunctionLikeHook) -> TokenStream2 {
    let FunctionLikeHook {
        input,
        fn_ty_ident,
        fallback_ident,
        original_ident,
        original_lock_ident,
        intercept_once_ident,
        accessor_ident,
        install_ident,
        hook_def_ident,
        arg_tys,
        ret_ty,
        unsafety,
        abi,
        fallback,
        install_body,
        extra_items,
    } = hook;
    let fallback_params = arg_tys.iter().map(|ty| quote! { _: #ty });

    quote! {
        #input

        // Normalize every hook body behind one concrete function-pointer alias.
        // That keeps the generated storage and `forward!()` plumbing identical
        // across exported functions, COM methods, and Objective-C methods.
        #[allow(non_camel_case_types)]
        type #fn_ty_ident = #unsafety #abi fn(#(#arg_tys),*) -> #ret_ty;

        #[allow(non_snake_case)]
        #unsafety #abi fn #fallback_ident(#(#fallback_params),*) -> #ret_ty {
            #fallback
        }

        #[allow(non_upper_case_globals)]
        static #original_lock_ident: std::sync::OnceLock<#fn_ty_ident> =
            std::sync::OnceLock::new();

        // First-hit interception bookkeeping is also local to each generated
        // hook; there is no shared observer runtime coordinating this.
        #[allow(non_upper_case_globals)]
        static #intercept_once_ident: std::sync::OnceLock<()> = std::sync::OnceLock::new();

        #[allow(dead_code, non_snake_case)]
        fn #accessor_ident() -> Option<#fn_ty_ident> {
            #original_lock_ident.get().copied()
        }

        #[allow(non_snake_case)]
        #[inline]
        fn #original_ident() -> #fn_ty_ident {
            #accessor_ident().unwrap_or(#fallback_ident)
        }

        #[allow(non_snake_case)]
        fn #install_ident() -> std::io::Result<()> {
            #install_body
        }

        #extra_items

        // Installation stays global through the distributed slice, but each
        // hook contributes exactly one tiny installer function.
        #[allow(non_upper_case_globals)]
        #[::retarget::__macro_support::distributed_slice(::retarget::__macro_support::HOOKS)]
        static #hook_def_ident: ::retarget::__macro_support::HookDef =
            ::retarget::__macro_support::HookDef {
                install: #install_ident,
            };
    }
}

// Shared validation and naming helpers

/// Requires one macro argument to be present.
pub(crate) fn require_arg<T>(value: Option<T>, span: &impl ToTokens, message: &str) -> Result<T> {
    value.ok_or_else(|| syn::Error::new_spanned(span, message))
}

/// Maps one interception-mode path to its runtime enum value.
pub(crate) fn interception_mode_tokens(mode: &Path, ident: &Ident) -> Result<TokenStream2> {
    let Some(mode_ident) = mode.segments.last().map(|segment| &segment.ident) else {
        return Err(syn::Error::new_spanned(
            mode,
            format!(
                "unsupported interception mode for `{ident}`; expected `Off`, `FirstHit`, or `EveryHit`"
            ),
        ));
    };

    match mode_ident.to_string().as_str() {
        "Off" => Ok(quote!(::retarget::InterceptionMode::Off)),
        "FirstHit" => Ok(quote!(::retarget::InterceptionMode::FirstHit)),
        "EveryHit" => Ok(quote!(::retarget::InterceptionMode::EveryHit)),
        _ => Err(syn::Error::new_spanned(
            mode,
            format!(
                "unsupported interception mode for `{ident}`; expected `Off`, `FirstHit`, or `EveryHit`"
            ),
        )),
    }
}

/// Builds the stable hook identifier used by free-function hooks.
pub(crate) fn derive_hook_id_expr(ident: &Ident) -> Expr {
    syn::parse_quote!(concat!(module_path!(), "::", stringify!(#ident)))
}

/// Builds the stable hook identifier used by impl-method hooks.
pub(crate) fn derive_impl_hook_id_expr(self_ty: &Type, ident: &Ident) -> Expr {
    syn::parse_quote!(concat!(
        module_path!(),
        "::",
        stringify!(#self_ty),
        "::",
        stringify!(#ident)
    ))
}

/// Uses one string literal directly when available, otherwise falls back to the item name.
pub(crate) fn derive_c_hook_name_expr(symbol: &Expr, span: &impl ToTokens) -> Result<Expr> {
    if let Some(symbol) = expr_lit_str(symbol) {
        Ok(syn::parse_quote!(#symbol))
    } else {
        Ok(syn::parse_quote!(stringify!(#span)))
    }
}

/// Derives a human-readable function hook name from one function target expression.
pub(crate) fn derive_function_hook_name_expr(
    function: &Expr,
    span: &impl ToTokens,
) -> Result<Expr> {
    // Prefer the actual target symbol when it is available literally so error
    // messages talk about the hooked symbol instead of the Rust helper name.
    if let Some(symbol) = expr_lit_str(function) {
        return Ok(syn::parse_quote!(#symbol));
    }

    if let Expr::Tuple(tuple) = function
        && tuple.elems.len() == 2
        && let Some(symbol) = tuple.elems.iter().nth(1).and_then(expr_lit_str)
    {
        return Ok(syn::parse_quote!(#symbol));
    }

    Ok(syn::parse_quote!(stringify!(#span)))
}

/// Uses the Rust function name as the default exported-function target.
pub(crate) fn default_function_target_expr(ident: &Ident) -> Expr {
    let symbol = LitStr::new(&ident.to_string(), ident.span());
    syn::parse_quote!(#symbol)
}

/// Derives a human-readable Objective-C hook name from class and selector expressions.
pub(crate) fn derive_objc_hook_name_expr(
    class: &Expr,
    selector: &Expr,
    span: &impl ToTokens,
) -> Result<Expr> {
    match (expr_lit_str(class), expr_lit_str(selector)) {
        (Some(class), Some(selector)) => Ok(syn::parse_quote!(concat!(#class, "::", #selector))),
        _ => Ok(syn::parse_quote!(stringify!(#span))),
    }
}

/// Derives one COM method display name from one interface type and vtable field.
pub(crate) fn derive_com_method_name_expr(
    interface: &Type,
    field: &Ident,
    span: &impl ToTokens,
) -> Result<Expr> {
    let Some(interface_ident) = type_last_ident(interface) else {
        return Err(syn::Error::new_spanned(
            span,
            "could not infer interface name; supply `name = ...`",
        ));
    };
    Ok(syn::parse_quote!(concat!(
        stringify!(#interface_ident),
        "::",
        stringify!(#field)
    )))
}

/// Converts one snake_case Rust method name into one PascalCase COM vtable field name.
pub(crate) fn derive_com_field_ident(ident: &Ident) -> Ident {
    let mut output = String::new();
    for part in ident.to_string().split('_').filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.push(first.to_ascii_uppercase());
            output.extend(chars);
        }
    }
    format_ident!("{}", output)
}

// Generated target conversion helpers

/// Builds the module slice used for install-time or resolve-time image lists.
pub(crate) fn module_slice_expr(images: &[Expr], fallback_image: Option<&Expr>) -> TokenStream2 {
    // COM hooks can probe one set of images and install against another. This
    // helper keeps that "slice literal or empty slice" codegen centralized.
    if !images.is_empty() {
        quote! {
            &[#(::retarget::__macro_support::into_module(#images).expect("hook module must not contain NUL")),*]
        }
    } else if let Some(image) = fallback_image {
        quote! { &[#image] }
    } else {
        quote! { &[] }
    }
}

/// Converts one required image expression into one infallible generated module expression.
pub(crate) fn required_image_expr(image: Expr) -> Expr {
    syn::parse_quote!(::retarget::__macro_support::into_module(#image).expect("hook module target must resolve"))
}

/// Converts one required symbol expression into one infallible generated symbol expression.
pub(crate) fn required_symbol_expr(symbol: Expr) -> Expr {
    syn::parse_quote!(::retarget::__macro_support::into_symbol(#symbol).expect("hook symbol must be valid"))
}

/// Converts one generated function target expression through the public high-level API.
pub(crate) fn try_function_expr(function: Expr) -> Expr {
    syn::parse_quote!(::retarget::__macro_support::into_function(#function))
}

/// Builds the generated Objective-C method resolution expression.
pub(crate) fn required_objc_method_expr(kind: Expr, class: Expr, selector: Expr) -> Expr {
    // This stays expression-shaped so the caller can splice it directly into
    // generated install code and still reuse the public conversion helpers for
    // diagnostics.
    syn::parse_quote!({
        match ::retarget::__macro_support::into_objc_class(#class) {
            Ok(class) => match ::retarget::__macro_support::into_objc_selector(#selector) {
                Ok(selector) => match #kind {
                    ::retarget::__macro_support::ObjcMethodKind::Instance => {
                        ::retarget::__macro_support::ObjcMethod::instance(class, selector)
                    }
                    ::retarget::__macro_support::ObjcMethodKind::Class => {
                        ::retarget::__macro_support::ObjcMethod::class(class, selector)
                    }
                },
                Err(error) => Err(::retarget::__macro_support::ObjcMethodError::from(error)),
            },
            Err(error) => Err(::retarget::__macro_support::ObjcMethodError::from(error)),
        }
    })
}

// Low-level syntax helpers

/// Checks whether one attribute path ends with the requested segment.
pub(crate) fn attr_path_ends_with(attr: &syn::Attribute, expected: &str) -> bool {
    attr.path()
        .segments
        .last()
        .map(|segment| segment.ident == expected)
        .unwrap_or(false)
}

/// Extracts one string literal from one expression when possible.
fn expr_lit_str(expr: &Expr) -> Option<LitStr> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Some(value.clone()),
        _ => None,
    }
}

/// Returns the final path segment identifier from one type path.
pub(crate) fn type_last_ident(ty: &Type) -> Option<Ident> {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.clone()),
        _ => None,
    }
}
