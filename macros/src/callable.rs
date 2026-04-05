//! Shared parsing and block rewriting for hook callables.

use crate::args::{HookObserveArgs, parse_hook_observe_args_tokens};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    Expr, ExprLit, FnArg, Ident, ImplItemFn, ItemFn, Lit, Meta, Pat, Result, ReturnType, Token,
    Type,
};

/// One parsed hook callable plus the signature metadata expansion needs.
pub(crate) struct HookCallableMeta {
    /// The original function or impl method being expanded.
    callable: HookCallable,
    /// Optional shared observation metadata pulled off helper attributes.
    observe: Option<HookObserveArgs>,
    /// The callable name.
    pub(crate) ident: Ident,
    /// The typed argument list extracted from the signature.
    pub(crate) arg_tys: Vec<Type>,
    /// The argument identifiers used by the generated `forward!()` helper.
    pub(crate) arg_idents: Vec<Ident>,
    /// The normalized return type tokens.
    pub(crate) ret_ty: TokenStream2,
    /// The required `unsafe` marker from the source signature.
    pub(crate) unsafety: syn::token::Unsafe,
    /// The required `extern` ABI from the source signature.
    pub(crate) abi: syn::Abi,
}

impl HookCallableMeta {
    /// Parses one free function hook definition.
    pub(crate) fn parse_function(function: ItemFn) -> Result<Self> {
        Self::parse(HookCallable::Function(function))
    }

    /// Parses one impl-method hook definition.
    pub(crate) fn parse_method(method: ImplItemFn) -> Result<Self> {
        Self::parse(HookCallable::Method(method))
    }

    /// Validates one hook signature and extracts the metadata expansion needs.
    fn parse(mut callable: HookCallable) -> Result<Self> {
        let observe = take_observe_args(callable.attrs_mut())?;
        let sig = callable.sig();
        let ident = sig.ident.clone();

        let Some(abi) = sig.abi.clone() else {
            return Err(syn::Error::new_spanned(
                sig.fn_token,
                "hook functions must declare an extern ABI",
            ));
        };
        let Some(unsafety) = sig.unsafety else {
            return Err(syn::Error::new_spanned(
                sig.fn_token,
                "hook functions must be unsafe",
            ));
        };
        if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
            return Err(syn::Error::new_spanned(
                &sig.generics,
                "hook functions cannot be generic",
            ));
        }
        if sig.variadic.is_some() {
            return Err(syn::Error::new_spanned(
                &sig.variadic,
                "hook functions cannot be variadic",
            ));
        }

        let (arg_tys, arg_idents) = collect_arg_meta(&sig.inputs)?;
        let ret_ty = match &sig.output {
            ReturnType::Default => quote!(()),
            ReturnType::Type(_, ty) => quote!(#ty),
        };

        Ok(Self {
            callable,
            observe,
            ident,
            arg_tys,
            arg_idents,
            ret_ty,
            unsafety,
            abi,
        })
    }

    /// Injects the generated `forward!()` helper macro into the callable body.
    pub(crate) fn inject_forward_helper(
        &mut self,
        fn_ty_ident: &Ident,
        original_ident: &Ident,
    ) -> Result<()> {
        inject_forward_helper(
            self.callable.block_mut(),
            fn_ty_ident,
            original_ident,
            &self.arg_idents,
        )
    }

    /// Injects interception bookkeeping at the start of the callable body.
    pub(crate) fn inject_interception_recorder(
        &mut self,
        hook_id: &Expr,
        intercept_once_ident: &Ident,
    ) -> Result<()> {
        let observe_value = self
            .observe
            .as_ref()
            .and_then(|observe| observe.value.clone());
        inject_interception_recorder(
            self.callable.block_mut(),
            hook_id,
            intercept_once_ident,
            observe_value.as_ref(),
        )
    }

    /// Returns the optional interception mode override requested by `#[hook::observe(...)]`.
    pub(crate) fn observe_mode(&self) -> Option<&syn::Path> {
        self.observe
            .as_ref()
            .and_then(|observe| observe.mode.as_ref())
    }

    /// Returns the optional typed observation payload expression.
    pub(crate) fn observe_value(&self) -> Option<&Expr> {
        self.observe
            .as_ref()
            .and_then(|observe| observe.value.as_ref())
    }

    /// Re-emits the wrapped callable as tokens after mutation.
    pub(crate) fn into_tokens(self) -> TokenStream2 {
        self.callable.into_tokens()
    }

    /// Returns the wrapped impl method after mutation.
    pub(crate) fn into_method(self) -> ImplItemFn {
        match self.callable {
            HookCallable::Method(method) => method,
            HookCallable::Function(_) => unreachable!("hook method metadata must hold one method"),
        }
    }
}

/// One hook callable that can be either a free function or an impl method.
enum HookCallable {
    /// One free function hook definition.
    Function(ItemFn),
    /// One impl method hook definition.
    Method(ImplItemFn),
}

impl HookCallable {
    /// Borrows the underlying function signature regardless of callable kind.
    fn sig(&self) -> &syn::Signature {
        match self {
            HookCallable::Function(function) => &function.sig,
            HookCallable::Method(method) => &method.sig,
        }
    }

    /// Borrows the mutable block body regardless of callable kind.
    fn block_mut(&mut self) -> &mut syn::Block {
        match self {
            HookCallable::Function(function) => function.block.as_mut(),
            HookCallable::Method(method) => &mut method.block,
        }
    }

    /// Borrows the mutable outer attributes regardless of callable kind.
    fn attrs_mut(&mut self) -> &mut Vec<syn::Attribute> {
        match self {
            HookCallable::Function(function) => &mut function.attrs,
            HookCallable::Method(method) => &mut method.attrs,
        }
    }

    /// Re-emits the callable regardless of whether it started as a function or method.
    fn into_tokens(self) -> TokenStream2 {
        match self {
            HookCallable::Function(function) => quote!(#function),
            HookCallable::Method(method) => quote!(#method),
        }
    }
}

/// Collects the typed argument metadata needed by expansion.
fn collect_arg_meta(inputs: &Punctuated<FnArg, Token![,]>) -> Result<(Vec<Type>, Vec<Ident>)> {
    let args: Vec<(Type, Ident)> = inputs
        .iter()
        .map(collect_typed_arg_meta)
        .collect::<Result<_>>()?;
    Ok(args.into_iter().unzip())
}

/// Extracts one `(Type, Ident)` pair from one typed argument.
fn collect_typed_arg_meta(arg: &FnArg) -> Result<(Type, Ident)> {
    match arg {
        FnArg::Receiver(_) => Err(syn::Error::new_spanned(
            arg,
            "hook functions cannot take self",
        )),
        FnArg::Typed(pat) => Ok(((*pat.ty).clone(), typed_arg_ident(&pat.pat)?)),
    }
}

/// Extracts the identifier pattern required by generated forwarding code.
fn typed_arg_ident(pat: &Pat) -> Result<Ident> {
    match pat {
        Pat::Ident(pat_ident) => Ok(pat_ident.ident.clone()),
        _ => Err(syn::Error::new_spanned(
            pat,
            "hook arguments must be identifiers",
        )),
    }
}

/// Injects the generated `forward!()` helper into one callable body.
fn inject_forward_helper(
    block: &mut syn::Block,
    fn_ty_ident: &Ident,
    original_ident: &Ident,
    arg_idents: &[Ident],
) -> Result<()> {
    let original_stmts = block.stmts.clone();
    rewrite_block(
        block,
        quote!({
            let original = || -> #fn_ty_ident {
                #original_ident()
            };
            #[allow(unused_macros)]
            macro_rules! forward {
                () => {
                    unsafe { original()(#(#arg_idents),*) }
                };
            }
            #(#original_stmts)*
        }),
    )
}

/// Injects interception recording into one callable body.
fn inject_interception_recorder(
    block: &mut syn::Block,
    hook_id: &Expr,
    intercept_once_ident: &Ident,
    observe_value: Option<&Expr>,
) -> Result<()> {
    let original_stmts = block.stmts.clone();
    let observe_stmt = match observe_value {
        Some(value) => quote! {
            if let Some(__retarget_interception_event) =
                ::retarget::__macro_support::next_interception(#hook_id, &#intercept_once_ident)
            {
                ::retarget::__macro_support::dispatch_signal(
                    __retarget_interception_event,
                    (#value),
                );
            }
        },
        None => quote! {
            if let Some(__retarget_interception_event) =
                ::retarget::__macro_support::next_interception(#hook_id, &#intercept_once_ident)
            {
                ::retarget::__macro_support::dispatch_interception(__retarget_interception_event);
            }
        },
    };
    rewrite_block(
        block,
        quote!({
            #observe_stmt
            #(#original_stmts)*
        }),
    )
}

/// Pulls one optional `#[hook::observe(...)]` declaration off one callable's attributes.
fn take_observe_args(attrs: &mut Vec<syn::Attribute>) -> Result<Option<HookObserveArgs>> {
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

/// Parses one direct `#[hook::observe(...)]` helper attribute.
fn parse_observe_attr(attr: &syn::Attribute) -> Result<HookObserveArgs> {
    match &attr.meta {
        Meta::List(list) => parse_hook_observe_args_tokens(list.tokens.clone()),
        Meta::Path(_) => Err(syn::Error::new_spanned(
            attr,
            "hook::observe expects a payload expression, an interception mode, or both",
        )),
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
    let tokens: proc_macro2::TokenStream =
        syn::parse_str(tokens).map_err(|error| syn::Error::new_spanned(attr, error))?;
    parse_hook_observe_args_tokens(tokens).map(Some)
}

/// Checks whether one attribute path ends with the requested segment.
fn attr_path_ends_with(attr: &syn::Attribute, expected: &str) -> bool {
    attr.path()
        .segments
        .last()
        .map(|segment| segment.ident == expected)
        .unwrap_or(false)
}

/// Replaces one block with newly generated block tokens.
fn rewrite_block(block: &mut syn::Block, tokens: TokenStream2) -> Result<()> {
    let new_block: syn::Block = syn::parse2(tokens)?;
    *block = new_block;
    Ok(())
}
