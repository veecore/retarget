//! Shared parsing and block rewriting for hook callables.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Expr, FnArg, Ident, ImplItemFn, ItemFn, Pat, Result, ReturnType, Token, Type};

/// One parsed hook callable plus the signature metadata expansion needs.
pub(crate) struct HookCallableMeta {
    /// The original function or impl method being expanded.
    callable: HookCallable,
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
    fn parse(callable: HookCallable) -> Result<Self> {
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
    pub(crate) fn inject_interception_recorder(&mut self, hook_id: &Expr) -> Result<()> {
        inject_interception_recorder(self.callable.block_mut(), hook_id)
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
fn inject_interception_recorder(block: &mut syn::Block, hook_id: &Expr) -> Result<()> {
    let original_stmts = block.stmts.clone();
    rewrite_block(
        block,
        quote!({
            ::retarget::__macro_support::record_interception(#hook_id);
            #(#original_stmts)*
        }),
    )
}

/// Replaces one block with newly generated block tokens.
fn rewrite_block(block: &mut syn::Block, tokens: TokenStream2) -> Result<()> {
    let new_block: syn::Block = syn::parse2(tokens)?;
    *block = new_block;
    Ok(())
}
