//! Hook expansion logic shared by the proc-macro entrypoints.

use crate::args::{HookArgs, HookComArgs, HookComImplArgs, HookObjcArgs, HookObjcImplArgs};
use crate::callable::HookCallableMeta;
use crate::observe::{inject_observe_dispatch, take_observe_args};
use crate::support::{
    FunctionLikeHook, attr_path_ends_with, derive_c_hook_name_expr, derive_com_field_ident,
    derive_com_method_name_expr, derive_function_hook_name_expr, derive_hook_id_expr,
    derive_impl_hook_id_expr, derive_objc_hook_name_expr, emit_function_like_hook,
    module_slice_expr, normalize_function_target_expr, require_arg, required_image_expr,
    required_objc_method_expr, required_symbol_expr, try_function_expr, type_last_ident,
};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Expr, Ident, ImplItem, ImplItemFn, ItemFn, ItemImpl, Result, Type};

// Free-function hook expansion

/// Expands one free-function hook into the generated install/accessor items.
pub(crate) fn expand_hook(args: HookArgs, mut function: ItemFn) -> Result<TokenStream2> {
    // Strip any standalone `#[hook::observe(...)]` before we parse the callable
    // itself so the rest of expansion can treat observation as just another
    // optional rewrite step.
    let observe = take_observe_args(&mut function.attrs)?;
    let mut meta = HookCallableMeta::parse_function(function)?;

    let original_accessor = args.original;
    let function_target =
        normalize_function_target_expr(args.function, args.image, args.symbol, &meta.ident)?;
    let name = if let Some(name) = args.name {
        name
    } else {
        derive_function_hook_name_expr(&function_target, &meta.ident)?
    };
    let optional = args
        .optional
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(false));
    let fallback = args
        .fallback
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(core::default::Default::default()));

    let fn_ident = meta.ident.clone();
    let arg_tys = meta.arg_tys.clone();
    let ret_ty = meta.ret_ty.clone();
    let unsafety = meta.unsafety;
    let abi = meta.abi.clone();
    let hook_id = derive_hook_id_expr(&fn_ident);
    let function_value = try_function_expr(function_target.clone());

    let fn_ty_ident = format_ident!("__retarget_c_hook_ty_{}", fn_ident);
    let fallback_ident = format_ident!("__retarget_c_hook_fallback_{}", fn_ident);
    let original_ident = format_ident!("__retarget_c_hook_original_{}", fn_ident);
    let original_lock_ident = format_ident!("__RETARGET_C_HOOK_ORIGINAL_{}", fn_ident);
    let intercept_once_ident = format_ident!("__RETARGET_C_HOOK_INTERCEPT_{}", fn_ident);
    let accessor_ident = format_ident!("__retarget_c_hook_get_original_{}", fn_ident);
    let install_ident = format_ident!("__retarget_c_hook_install_{}", fn_ident);
    let hook_def_ident = format_ident!("__RETARGET_C_HOOK_DEF_{}", fn_ident);
    let replacement_value = quote!(#fn_ident as #fn_ty_ident);
    let accessor_item = original_accessor.map(|accessor_ident| {
        quote! {
            #[allow(dead_code)]
            fn #accessor_ident() -> Option<#fn_ty_ident> {
                #original_lock_ident.get().copied()
            }
        }
    });

    // Rewrite the user body in two passes: first add `forward!()`, then splice
    // in any requested observation at the function head.
    meta.inject_forward_helper(&fn_ty_ident, &original_ident)?;
    inject_observe_dispatch(
        meta.block_mut(),
        &hook_id,
        &intercept_once_ident,
        observe.as_ref(),
        &fn_ident,
    )?;

    let install_body = quote! {
        if #original_lock_ident.get().is_some() {
            return Ok(());
        }

        // The generated installer keeps just enough metadata for consistent
        // optional/required install handling; it does not carry target details
        // beyond what this specific install path needs.
        let spec = ::retarget::__macro_support::HookSpec {
            name: #name,
            optional: #optional,
        };

        let target = match #function_value {
            Ok(target) => target,
            Err(error) => {
                return ::retarget::__macro_support::finish_install(&spec, Err(error));
            }
        };

        let original = unsafe { target.replace_with(#replacement_value) }
            .map_err(|error| std::io::Error::other(format!(
                "required hook {} failed: {}",
                spec.name,
                error,
            )))?;
        let _ = #original_lock_ident.set(original);
        Ok(())
    };

    Ok(emit_function_like_hook(FunctionLikeHook {
        input: meta.into_tokens(),
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
        extra_items: accessor_item.unwrap_or_default(),
    }))
}

/// Expands one free-function Objective-C hook into the generated install/accessor items.
pub(crate) fn expand_hook_objc(args: HookObjcArgs, mut function: ItemFn) -> Result<TokenStream2> {
    let observe = take_observe_args(&mut function.attrs)?;
    let mut meta = HookCallableMeta::parse_function(function)?;

    let original_accessor = args.original;
    let class = require_arg(args.class, &meta.ident, "missing required `class` argument")?;
    let selector = require_arg(
        args.selector,
        &meta.ident,
        "missing required `selector` argument",
    )?;
    let name = if let Some(name) = args.name {
        name
    } else {
        derive_objc_hook_name_expr(&class, &selector, &meta.ident)?
    };
    let kind = args.kind.unwrap_or_else(|| {
        syn::parse_quote!(::retarget::__macro_support::ObjcMethodKind::Instance)
    });
    let optional = args
        .optional
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(false));
    let fallback = args
        .fallback
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(core::default::Default::default()));

    let fn_ident = meta.ident.clone();
    let arg_tys = meta.arg_tys.clone();
    let ret_ty = meta.ret_ty.clone();
    let unsafety = meta.unsafety;
    let abi = meta.abi.clone();
    let hook_id = derive_hook_id_expr(&fn_ident);
    let method_value = required_objc_method_expr(kind.clone(), class.clone(), selector.clone());

    let fn_ty_ident = format_ident!("__retarget_objc_hook_ty_{}", fn_ident);
    let fallback_ident = format_ident!("__retarget_objc_hook_fallback_{}", fn_ident);
    let original_ident = format_ident!("__retarget_objc_hook_original_{}", fn_ident);
    let original_lock_ident = format_ident!("__RETARGET_OBJC_HOOK_ORIGINAL_{}", fn_ident);
    let intercept_once_ident = format_ident!("__RETARGET_OBJC_HOOK_INTERCEPT_{}", fn_ident);
    let install_ident = format_ident!("__retarget_objc_hook_install_{}", fn_ident);
    let hook_def_ident = format_ident!("__RETARGET_OBJC_HOOK_DEF_{}", fn_ident);

    meta.inject_forward_helper(&fn_ty_ident, &original_ident)?;
    inject_observe_dispatch(
        meta.block_mut(),
        &hook_id,
        &intercept_once_ident,
        observe.as_ref(),
        &fn_ident,
    )?;

    let accessor_item = original_accessor.map(|accessor_ident| {
        quote! {
            #[allow(dead_code)]
            fn #accessor_ident() -> Option<#fn_ty_ident> {
                #original_lock_ident.get().copied()
            }
        }
    });
    let input = meta.into_tokens();
    let fallback_params = arg_tys.iter().map(|ty| quote! { _: #ty });

    Ok(quote! {
        #input

        // Objective-C free-function hooks are still emitted through the same
        // function-like scaffold; this hand-written branch only exists because
        // the method resolution/install path is different from plain functions.
        #[allow(non_camel_case_types)]
        type #fn_ty_ident = #unsafety #abi fn(#(#arg_tys),*) -> #ret_ty;

        #[allow(non_snake_case)]
        #unsafety #abi fn #fallback_ident(#(#fallback_params),*) -> #ret_ty {
            #fallback
        }

        #[allow(non_upper_case_globals)]
        static #original_lock_ident: std::sync::OnceLock<#fn_ty_ident> =
            std::sync::OnceLock::new();

        #[allow(non_upper_case_globals)]
        static #intercept_once_ident: std::sync::OnceLock<()> = std::sync::OnceLock::new();

        #[allow(non_snake_case)]
        #[inline]
        fn #original_ident() -> #fn_ty_ident {
            #original_lock_ident
                .get()
                .copied()
                .unwrap_or(#fallback_ident)
        }

        #accessor_item

        #[allow(non_snake_case)]
        fn #install_ident() -> std::io::Result<()> {
            if #original_lock_ident.get().is_some() {
                return Ok(());
            }

            let spec = ::retarget::__macro_support::HookSpec {
                name: #name,
                optional: #optional,
            };
            let method = match #method_value {
                Ok(method) => method,
                Err(error) => {
                    return ::retarget::__macro_support::finish_install(&spec, Err(error));
                }
            };

            let original = unsafe { method.replace_with(#fn_ident as #fn_ty_ident) };
            let _ = #original_lock_ident.set(original);
            Ok(())
        }

        #[allow(non_upper_case_globals)]
        #[::retarget::__macro_support::distributed_slice(::retarget::__macro_support::HOOKS)]
        static #hook_def_ident: ::retarget::__macro_support::HookDef =
            ::retarget::__macro_support::HookDef {
                install: #install_ident,
            };
    })
}

/// Expands one free-function COM hook into the generated install/accessor items.
pub(crate) fn expand_hook_com(args: HookComArgs, mut function: ItemFn) -> Result<TokenStream2> {
    let observe = take_observe_args(&mut function.attrs)?;
    let mut meta = HookCallableMeta::parse_function(function)?;

    let name = com_hook_name_expr(&args, &meta.ident, None)?;
    let symbol_value = com_hook_symbol_expr(&args, &name);
    let optional = args
        .optional
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(false));
    let fallback = args
        .fallback
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(core::default::Default::default()));

    let fn_ident = meta.ident.clone();
    let arg_tys = meta.arg_tys.clone();
    let ret_ty = meta.ret_ty.clone();
    let unsafety = meta.unsafety;
    let abi = meta.abi.clone();
    let hook_id = derive_hook_id_expr(&fn_ident);

    let fn_ty_ident = format_ident!("__retarget_com_hook_ty_{}", fn_ident);
    let fallback_ident = format_ident!("__retarget_com_hook_fallback_{}", fn_ident);
    let original_ident = format_ident!("__retarget_com_hook_original_{}", fn_ident);
    let original_lock_ident = format_ident!("__RETARGET_WINDOWS_HOOK_ORIGINAL_{}", fn_ident);
    let intercept_once_ident = format_ident!("__RETARGET_COM_HOOK_INTERCEPT_{}", fn_ident);
    let install_ident = format_ident!("__retarget_com_hook_install_{}", fn_ident);
    let hook_def_ident = format_ident!("__RETARGET_COM_HOOK_DEF_{}", fn_ident);
    let accessor_ident = args
        .original
        .clone()
        .unwrap_or_else(|| format_ident!("__retarget_com_hook_get_original_{}", fn_ident));
    let replacement_value = quote!(#fn_ident as #fn_ty_ident);
    let resolve_value = com_hook_resolve_expr(&args, None, &symbol_value, &fn_ty_ident);

    meta.inject_forward_helper(&fn_ty_ident, &original_ident)?;
    inject_observe_dispatch(
        meta.block_mut(),
        &hook_id,
        &intercept_once_ident,
        observe.as_ref(),
        &fn_ident,
    )?;

    let install_body = com_install_body(ComInstallPlan {
        args: &args,
        name,
        symbol_value,
        optional,
        original_lock_ident: original_lock_ident.clone(),
        replacement_value,
        resolve_value,
    });

    Ok(emit_function_like_hook(FunctionLikeHook {
        input: meta.into_tokens(),
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
        extra_items: TokenStream2::new(),
    }))
}

// Impl-block hook expansion

/// Expands one impl block containing COM hook methods.
pub(crate) fn expand_hook_com_impl(
    args: HookComImplArgs,
    mut input: ItemImpl,
) -> Result<TokenStream2> {
    if input.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &input.self_ty,
            "hook_com_impl only supports inherent impl blocks",
        ));
    }

    let self_ty = (*input.self_ty).clone();
    let type_ident = type_last_ident(&self_ty).unwrap_or_else(|| format_ident!("impl_type"));
    let mut generated = Vec::new();
    let mut saw_methods = false;

    for item in &mut input.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        saw_methods = true;

        // `#[hook::com_impl]` makes the per-method `#[hook::com(...)]`
        // attribute optional. If it is absent we still expand the method using
        // name/default inference from the impl context.
        let mut hook_attr = None;
        method.attrs.retain(|attr| {
            if attr_path_ends_with(attr, "com") {
                if hook_attr.is_none() {
                    hook_attr = Some(attr.clone());
                }
                return false;
            }
            true
        });

        let hook_args = match hook_attr {
            Some(hook_attr) => match &hook_attr.meta {
                syn::Meta::List(list) => {
                    crate::args::parse_hook_com_args(list.tokens.clone().into())
                }
                syn::Meta::Path(_) => {
                    crate::args::parse_hook_com_args(proc_macro::TokenStream::new())
                }
                syn::Meta::NameValue(_) => Err(syn::Error::new_spanned(
                    &hook_attr,
                    "hook::com expects a list of arguments",
                )),
            }?,
            None => HookComArgs::default(),
        };

        let expansion =
            expand_hook_com_method(&args, hook_args, method.clone(), &self_ty, &type_ident)?;
        *method = expansion.method;
        generated.push(expansion.generated);
    }

    if !saw_methods {
        return Err(syn::Error::new_spanned(
            &input.self_ty,
            "hook_com_impl requires at least one method",
        ));
    }

    Ok(quote! {
        #input
        #(#generated)*
    })
}

/// Expands one impl block containing Objective-C hook methods.
pub(crate) fn expand_hook_objc_impl(
    args: HookObjcImplArgs,
    mut input: ItemImpl,
) -> Result<TokenStream2> {
    if input.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &input.self_ty,
            "hook_objc_impl only supports inherent impl blocks",
        ));
    }

    let self_ty = (*input.self_ty).clone();
    let type_ident = type_last_ident(&self_ty).unwrap_or_else(|| format_ident!("impl_type"));
    let mut generated = Vec::new();
    let mut saw_hook_methods = false;

    for item in &mut input.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        // Objective-C impl hooks are different: only methods with one of the
        // explicit kind attrs participate because the method kind is part of
        // the hook target itself.
        let hook_args = match parse_objc_method_hook_attr(method)? {
            Some(hook_args) => hook_args,
            None => continue,
        };

        saw_hook_methods = true;
        let expansion =
            expand_hook_objc_method(&args, hook_args, method.clone(), &self_ty, &type_ident)?;
        *method = expansion.method;
        generated.push(expansion.generated);
    }

    if !saw_hook_methods {
        return Err(syn::Error::new_spanned(
            &input.self_ty,
            "hook_objc_impl requires at least one method annotated with #[hook::objc::instance(...)] or #[hook::objc::class(...)]",
        ));
    }

    Ok(quote! {
        #input
        #(#generated)*
    })
}

// Per-method expansion

/// The generated tokens for one expanded COM impl method.
struct HookComMethodExpansion {
    /// The rewritten original impl method.
    method: ImplItemFn,
    /// The generated helper items emitted beside the impl block.
    generated: TokenStream2,
}

/// The generated tokens for one expanded Objective-C impl method.
struct HookObjcMethodExpansion {
    /// The rewritten original impl method.
    method: ImplItemFn,
    /// The generated helper items emitted beside the impl block.
    generated: TokenStream2,
}

/// Expands one COM hook method inside one impl block.
fn expand_hook_com_method(
    impl_args: &HookComImplArgs,
    args: HookComArgs,
    mut method: ImplItemFn,
    self_ty: &Type,
    type_ident: &Ident,
) -> Result<HookComMethodExpansion> {
    let observe = take_observe_args(&mut method.attrs)?;
    let mut meta = HookCallableMeta::parse_method(method)?;

    let field_ident = args
        .field
        .clone()
        .unwrap_or_else(|| derive_com_field_ident(&meta.ident));
    let name = com_hook_name_expr(&args, &meta.ident, Some((impl_args, &field_ident)))?;
    let symbol_value = com_hook_symbol_expr(&args, &name);
    let optional = args
        .optional
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(false));
    let fallback = args
        .fallback
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(core::default::Default::default()));

    let fn_ident = meta.ident.clone();
    let arg_tys = meta.arg_tys.clone();
    let ret_ty = meta.ret_ty.clone();
    let unsafety = meta.unsafety;
    let abi = meta.abi.clone();
    let hook_id = derive_impl_hook_id_expr(self_ty, &fn_ident);
    let type_ident_snake = type_ident.to_string().to_ascii_lowercase();

    let fn_ty_ident = format_ident!("__retarget_com_hook_ty_{}_{}", type_ident, fn_ident);
    let fallback_ident = format_ident!("__retarget_com_hook_fallback_{}_{}", type_ident, fn_ident);
    let original_ident = format_ident!("__retarget_com_hook_original_{}_{}", type_ident, fn_ident);
    let original_lock_ident = format_ident!(
        "__RETARGET_WINDOWS_HOOK_ORIGINAL_{}_{}",
        type_ident,
        fn_ident
    );
    let intercept_once_ident =
        format_ident!("__RETARGET_COM_HOOK_INTERCEPT_{}_{}", type_ident, fn_ident);
    let install_ident = format_ident!("__retarget_com_hook_install_{}_{}", type_ident, fn_ident);
    let hook_def_ident = format_ident!("__RETARGET_COM_HOOK_DEF_{}_{}", type_ident, fn_ident);
    let accessor_ident = args.original.clone().unwrap_or_else(|| {
        format_ident!(
            "__retarget_com_hook_get_original_{}_{}",
            type_ident_snake,
            fn_ident
        )
    });
    let replacement_value = quote!(<#self_ty>::#fn_ident as #fn_ty_ident);
    let resolve_value = com_hook_resolve_expr(
        &args,
        Some((impl_args, &field_ident)),
        &symbol_value,
        &fn_ty_ident,
    );

    // For impl methods the generated replacement value points back at the
    // inherent method item, but the body rewriting is otherwise identical to
    // the free-function case.
    meta.inject_forward_helper(&fn_ty_ident, &original_ident)?;
    inject_observe_dispatch(
        meta.block_mut(),
        &hook_id,
        &intercept_once_ident,
        observe.as_ref(),
        &fn_ident,
    )?;

    let install_body = com_install_body(ComInstallPlan {
        args: &args,
        name,
        symbol_value,
        optional,
        original_lock_ident: original_lock_ident.clone(),
        replacement_value,
        resolve_value,
    });

    let method = meta.into_method();
    let generated = emit_function_like_hook(FunctionLikeHook {
        input: quote!(#method),
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
        extra_items: TokenStream2::new(),
    });

    Ok(HookComMethodExpansion { method, generated })
}

/// Expands one Objective-C hook method inside one impl block.
fn expand_hook_objc_method(
    impl_args: &HookObjcImplArgs,
    args: HookObjcArgs,
    mut method: ImplItemFn,
    self_ty: &Type,
    type_ident: &Ident,
) -> Result<HookObjcMethodExpansion> {
    let observe = take_observe_args(&mut method.attrs)?;
    let mut meta = HookCallableMeta::parse_method(method)?;

    let class = args.class.clone().or_else(|| impl_args.class.clone()).ok_or_else(|| {
        syn::Error::new_spanned(
            &meta.ident,
            "missing required `class` argument; supply it on #[hook::objc::methods(...)] or this method",
        )
    })?;
    let selector = args
        .selector
        .clone()
        .unwrap_or_else(|| default_selector_expr(&meta.ident));
    let kind = args.kind.clone().ok_or_else(|| {
        syn::Error::new_spanned(
            &meta.ident,
            "missing Objective-C method kind; annotate this method with #[hook::objc::instance(...)] or #[hook::objc::class(...)]",
        )
    })?;
    let original_accessor = args.original;
    let name = if let Some(name) = args.name {
        name
    } else {
        derive_objc_hook_name_expr(&class, &selector, &meta.ident)?
    };
    let optional = args
        .optional
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(false));
    let fallback = args
        .fallback
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(core::default::Default::default()));

    let fn_ident = meta.ident.clone();
    let arg_tys = meta.arg_tys.clone();
    let ret_ty = meta.ret_ty.clone();
    let unsafety = meta.unsafety;
    let abi = meta.abi.clone();
    let hook_id = derive_impl_hook_id_expr(self_ty, &fn_ident);
    let type_ident_snake = type_ident.to_string().to_ascii_lowercase();
    let method_value = required_objc_method_expr(kind, class, selector);

    let fn_ty_ident = format_ident!("__retarget_objc_hook_ty_{}_{}", type_ident, fn_ident);
    let fallback_ident = format_ident!("__retarget_objc_hook_fallback_{}_{}", type_ident, fn_ident);
    let original_ident = format_ident!("__retarget_objc_hook_original_{}_{}", type_ident, fn_ident);
    let original_lock_ident =
        format_ident!("__RETARGET_OBJC_HOOK_ORIGINAL_{}_{}", type_ident, fn_ident);
    let intercept_once_ident =
        format_ident!("__RETARGET_OBJC_HOOK_INTERCEPT_{}_{}", type_ident, fn_ident);
    let install_ident = format_ident!("__retarget_objc_hook_install_{}_{}", type_ident, fn_ident);
    let hook_def_ident = format_ident!("__RETARGET_OBJC_HOOK_DEF_{}_{}", type_ident, fn_ident);
    let accessor_ident = original_accessor.unwrap_or_else(|| {
        format_ident!(
            "__retarget_objc_hook_get_original_{}_{}",
            type_ident_snake,
            fn_ident
        )
    });

    meta.inject_forward_helper(&fn_ty_ident, &original_ident)?;
    inject_observe_dispatch(
        meta.block_mut(),
        &hook_id,
        &intercept_once_ident,
        observe.as_ref(),
        &fn_ident,
    )?;

    let install_body = quote! {
        if #original_lock_ident.get().is_some() {
            return Ok(());
        }

        let spec = ::retarget::__macro_support::HookSpec {
            name: #name,
            optional: #optional,
        };
        let method = match #method_value {
            Ok(method) => method,
            Err(error) => {
                return ::retarget::__macro_support::finish_install(&spec, Err(error));
            }
        };

        let original = unsafe { method.replace_with(<#self_ty>::#fn_ident as #fn_ty_ident) };
        let _ = #original_lock_ident.set(original);
        Ok(())
    };

    let method = meta.into_method();
    let generated = emit_function_like_hook(FunctionLikeHook {
        input: quote!(#method),
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
        extra_items: TokenStream2::new(),
    });

    Ok(HookObjcMethodExpansion { method, generated })
}

// COM-specific install helpers

/// Derives the user-facing hook name for one COM hook.
fn com_hook_name_expr(
    args: &HookComArgs,
    ident: &Ident,
    method_context: Option<(&HookComImplArgs, &Ident)>,
) -> Result<Expr> {
    if let Some(name) = &args.name {
        return Ok(name.clone());
    }
    if let Some(symbol) = &args.symbol {
        return derive_c_hook_name_expr(symbol, ident);
    }
    if let Some((impl_args, field_ident)) = method_context
        && let Some(interface) = impl_args.interface.as_ref()
    {
        return derive_com_method_name_expr(interface, field_ident, ident);
    }
    Err(syn::Error::new_spanned(
        ident,
        "missing required `name` argument when `symbol` is omitted",
    ))
}

/// Chooses the symbol expression used by one COM hook.
fn com_hook_symbol_expr(args: &HookComArgs, name: &Expr) -> Expr {
    args.symbol
        .clone()
        .map(required_symbol_expr)
        .unwrap_or_else(|| required_symbol_expr(name.clone()))
}

/// Builds the preferred resolution expression for one COM hook.
fn com_hook_resolve_expr(
    args: &HookComArgs,
    method_context: Option<(&HookComImplArgs, &Ident)>,
    symbol_value: &Expr,
    fn_ty_ident: &Ident,
) -> TokenStream2 {
    if let Some(resolve) = args.resolve.clone() {
        // The user supplied an explicit resolution path, so we treat it as the
        // highest-priority way to discover the original function pointer.
        return quote!({
            let value = (#resolve);
            value
                .map(::retarget::__macro_support::into_function)
                .transpose()
        });
    }

    if let Some((impl_args, field_ident)) = method_context
        && let (Some(interface), Some(instance)) =
            (impl_args.interface.as_ref(), impl_args.instance.as_ref())
    {
        // When COM impl context gives us a live interface pointer, we can pull
        // the original method directly from that vtable slot instead of doing a
        // later symbol lookup.
        return quote! {
            ::std::ptr::NonNull::new((#instance) as *mut std::ffi::c_void)
                .map(|interface| unsafe {
                    ::retarget::__macro_support::windows::com::interface_method::<#interface, #fn_ty_ident>(
                        interface,
                        |vtbl| vtbl.#field_ident,
                    )
                })
                .flatten()
                .map(::retarget::__macro_support::into_function)
                .transpose()
        };
    }

    quote! {
        Ok(#symbol_value.resolve_in_modules(resolve_images).ok())
    }
}

/// The normalized inputs shared by COM install-body generation.
struct ComInstallPlan<'a> {
    /// The original parsed COM hook arguments.
    args: &'a HookComArgs,
    /// The user-facing hook name.
    name: Expr,
    /// The resolved symbol expression for diagnostics and fallback resolution.
    symbol_value: Expr,
    /// The optional-install flag expression.
    optional: Expr,
    /// The storage cell for the original implementation.
    original_lock_ident: Ident,
    /// The generated typed replacement expression.
    replacement_value: TokenStream2,
    /// The generated preferred resolution path.
    resolve_value: TokenStream2,
}

/// Builds the common install body shared by free-function and impl-method COM hooks.
fn com_install_body(plan: ComInstallPlan<'_>) -> TokenStream2 {
    let ComInstallPlan {
        args,
        name,
        symbol_value,
        optional,
        original_lock_ident,
        replacement_value,
        resolve_value,
    } = plan;
    let image_inner = args.image.clone().map(required_image_expr);
    let install_images = module_slice_expr(&args.imports, image_inner.as_ref());
    let resolve_images = module_slice_expr(&args.resolve_images, image_inner.as_ref());

    quote! {
        if #original_lock_ident.get().is_some() {
            return Ok(());
        }

        let spec = ::retarget::__macro_support::HookSpec {
            name: #name,
            optional: #optional,
        };
        let install_images: &[::retarget::__macro_support::Module] = #install_images;
        let resolve_images: &[::retarget::__macro_support::Module] = #resolve_images;

        // Prefer the already-resolved original when we can get it cheaply.
        // Fall back to symbol lookup only when that first path yields nothing.
        let original = match #resolve_value {
            Ok(Some(target)) => unsafe { target.replace_with(#replacement_value) },
            Ok(None) => {
                let target = match #symbol_value.resolve_in_modules(install_images) {
                    Ok(target) => target,
                    Err(error) => {
                        return ::retarget::__macro_support::finish_install(
                            &spec,
                            Err(error),
                        );
                    }
                };
                unsafe { target.replace_with(#replacement_value) }
            }
            Err(error) => {
                return ::retarget::__macro_support::finish_install(&spec, Err(error));
            }
        }
        .map_err(|error| std::io::Error::other(format!(
            "required hook {} failed: {}",
            spec.name,
            error,
        )))?;

        let _ = #original_lock_ident.set(original);
        Ok(())
    }
}

// Objective-C-specific helpers

/// Uses the Rust method name as the default Objective-C selector.
fn default_selector_expr(ident: &Ident) -> Expr {
    let selector = syn::LitStr::new(&ident.to_string(), ident.span());
    syn::parse_quote!(#selector)
}

/// Pulls one Objective-C hook attribute off an impl method and normalizes its kind.
fn parse_objc_method_hook_attr(method: &mut ImplItemFn) -> Result<Option<HookObjcArgs>> {
    let mut hook_attr = None;
    method.attrs.retain(|attr| {
        // The impl-level Objective-C surface is split into `instance` and
        // `class`, but the downstream parser only wants one normalized `kind`.
        let kind: Option<Expr> = if attr_path_ends_with(attr, "instance") {
            Some(syn::parse_quote!(
                ::retarget::__macro_support::ObjcMethodKind::Instance
            ))
        } else if attr_path_ends_with(attr, "class") {
            Some(syn::parse_quote!(
                ::retarget::__macro_support::ObjcMethodKind::Class
            ))
        } else {
            None
        };

        if let Some(kind) = kind {
            if hook_attr.is_none() {
                hook_attr = Some((attr.clone(), kind));
            }
            return false;
        }

        true
    });

    let Some((hook_attr, kind)) = hook_attr else {
        return Ok(None);
    };

    let mut combined = match &hook_attr.meta {
        syn::Meta::List(list) => list.tokens.clone(),
        syn::Meta::Path(_) => TokenStream2::new(),
        syn::Meta::NameValue(_) => {
            return Err(syn::Error::new_spanned(
                &hook_attr,
                "hook::objc::{instance,class} expects a list of arguments",
            ));
        }
    };
    if !combined.is_empty() {
        combined.extend(quote!(,));
    }
    combined.extend(quote!(kind = #kind));

    crate::args::parse_hook_objc_args(combined.into()).map(Some)
}
