//! Attribute-argument parsing for the hook proc macros.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::parse::{Parse, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, ExprArray, ExprAssign, ExprPath, Ident, Path, Result, Token, Type};

// Shared parser macros

/// Parses one comma-separated nested-meta list into one typed args struct.
macro_rules! parse_nested_args {
    ($attr:expr, $args:ident, $unsupported:literal, {
        $($name:expr => $parser:ident($slot:expr)),* $(,)?
    }) => {{
        // Most hook attrs use the normal `name = value` nested-meta shape, so
        // this macro lets the individual parsers stay declarative instead of
        // repeating the same `ParseNestedMeta` boilerplate.
        let parser = syn::meta::parser(|meta| {
            $(
                if meta.path.is_ident($name) {
                    $parser(&meta, $slot, $name)?;
                    return Ok(());
                }
            )*
            Err(meta.error($unsupported))
        });
        parser.parse($attr)?;
        Ok($args)
    }};
}

/// Declares one hook-argument struct plus its matching parser.
macro_rules! define_hook_args {
    (
        $struct_name:ident => $parse_fn:ident, $unsupported:literal {
            $($field:ident : $ty:ty => $parser:ident),* $(,)?
        }
    ) => {
        #[derive(Default)]
        pub(crate) struct $struct_name {
            $(pub(crate) $field: $ty,)*
        }

        pub(crate) fn $parse_fn(attr: TokenStream) -> Result<$struct_name> {
            let mut args = $struct_name::default();
            // These attrs are all "bag of named options" parsers; expansion is
            // what later decides which combinations are actually meaningful.
            parse_nested_args!(attr, args, $unsupported, {
                $(stringify!($field) => $parser(&mut args.$field)),*
            })
        }
    };
}

// Hook argument models and top-level parsers

#[derive(Default)]
pub(crate) struct HookArgs {
    pub(crate) name: Option<Expr>,
    pub(crate) function: Option<Expr>,
    pub(crate) optional: Option<Expr>,
    pub(crate) fallback: Option<Expr>,
}

pub(crate) fn parse_hook_args(attr: TokenStream) -> Result<HookArgs> {
    let entries = Punctuated::<Expr, Token![,]>::parse_terminated.parse(attr)?;
    let mut args = HookArgs::default();
    let mut saw_named = false;

    for entry in entries {
        match entry {
            Expr::Assign(assign) => {
                saw_named = true;
                parse_named_hook_arg(&mut args, assign)?;
            }
            value => {
                // `#[hook::c("symbol", optional = true)]` is allowed, but once
                // we have crossed into named args we keep the rest named so the
                // grammar stays easy to reason about.
                if saw_named {
                    return Err(syn::Error::new_spanned(
                        value,
                        "positional hook target must come before named arguments",
                    ));
                }
                if args.function.replace(value).is_some() {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "duplicate `function` argument",
                    ));
                }
            }
        }
    }

    Ok(args)
}

define_hook_args!(HookObjcArgs => parse_hook_objc_args, "unsupported hook_objc argument" {
    original: Option<Ident> => parse_slot,
    name: Option<Expr> => parse_slot,
    class: Option<Expr> => parse_slot,
    selector: Option<Expr> => parse_slot,
    kind: Option<Expr> => parse_slot,
    image: Option<Expr> => parse_slot,
    optional: Option<Expr> => parse_slot,
    fallback: Option<Expr> => parse_slot,
});

define_hook_args!(HookObjcImplArgs => parse_hook_objc_impl_args, "unsupported hook_objc_impl argument" {
    class: Option<Expr> => parse_slot,
    image: Option<Expr> => parse_slot,
});

define_hook_args!(HookComArgs => parse_hook_com_args, "unsupported hook_com argument" {
    original: Option<Ident> => parse_slot,
    name: Option<Expr> => parse_slot,
    field: Option<Ident> => parse_slot,
    symbol: Option<Expr> => parse_slot,
    resolve: Option<Expr> => parse_slot,
    image: Option<Expr> => parse_slot,
    optional: Option<Expr> => parse_slot,
    fallback: Option<Expr> => parse_slot,
    imports: Vec<Expr> => parse_expr_array_slot,
    resolve_images: Vec<Expr> => parse_expr_array_slot,
});

define_hook_args!(HookComImplArgs => parse_hook_com_impl_args, "unsupported hook_com_impl argument" {
    interface: Option<Type> => parse_slot,
    instance: Option<Expr> => parse_slot,
});

define_hook_args!(HookObserverArgs => parse_hook_observer_args, "unsupported hook_observer argument" {
    default: Option<Path> => parse_slot,
});

/// Parsed arguments for `#[hook::observe(...)]`.
#[derive(Clone, Default)]
pub(crate) struct HookObserveArgs {
    /// Optional typed observation payload expression.
    pub(crate) value: Option<Expr>,
    /// Optional interception mode override path.
    pub(crate) mode: Option<Path>,
}

/// Parses `#[hook::observe(...)]` for either a typed payload, a mode override, both, or neither.
pub(crate) fn parse_hook_observe_args(attr: TokenStream) -> Result<HookObserveArgs> {
    parse_hook_observe_args_tokens(attr.into())
}

/// Parses `#[hook::observe(...)]` arguments from one token stream.
pub(crate) fn parse_hook_observe_args_tokens(attr: TokenStream2) -> Result<HookObserveArgs> {
    let entries = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(attr)?;
    let mut args = HookObserveArgs::default();
    let mut saw_named = false;

    for entry in entries {
        match entry {
            Expr::Assign(assign) => {
                saw_named = true;
                parse_named_observe_arg(&mut args, assign)?;
            }
            value => {
                // The positional observe grammar intentionally stays tiny:
                // one mode path, one payload expression, or both. We detect the
                // known mode spellings first and treat everything else as the
                // typed payload value.
                if saw_named {
                    return Err(syn::Error::new_spanned(
                        value,
                        "positional observe value must come before named arguments",
                    ));
                }
                if let Some(mode) = interception_mode_path(&value) {
                    if args.mode.replace(mode).is_some() {
                        return Err(syn::Error::new_spanned(value, "duplicate `mode` argument"));
                    }
                    continue;
                }
                if args.value.replace(value).is_some() {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "duplicate `value` argument",
                    ));
                }
            }
        }
    }

    Ok(args)
}

/// Returns the requested interception mode path when the expression names a supported mode.
fn interception_mode_path(expr: &Expr) -> Option<Path> {
    let Expr::Path(ExprPath {
        qself: None, path, ..
    }) = expr
    else {
        return None;
    };
    // Accept fully qualified spellings like `retarget::intercept::EveryHit`
    // while still rejecting arbitrary paths that are not one of the known
    // interception modes.
    let mode_ident = path.segments.last()?.ident.to_string();
    matches!(mode_ident.as_str(), "Off" | "FirstHit" | "EveryHit").then(|| path.clone())
}

// Shared parsing helpers

/// Parses one `name = value` style hook argument for `#[hook::c(...)]`.
fn parse_named_hook_arg(args: &mut HookArgs, assign: ExprAssign) -> Result<()> {
    let name = assign_name(&assign)?;
    let value = *assign.right;

    match name.as_str() {
        "name" => set_expr_slot(&mut args.name, value, &assign.left, &name),
        "function" => set_expr_slot(&mut args.function, value, &assign.left, &name),
        "optional" => set_expr_slot(&mut args.optional, value, &assign.left, &name),
        "fallback" => set_expr_slot(&mut args.fallback, value, &assign.left, &name),
        _ => Err(syn::Error::new_spanned(
            assign.left,
            "unsupported hook argument",
        )),
    }
}

/// Parses one `name = value` style observe argument for `#[hook::observe(...)]`.
fn parse_named_observe_arg(args: &mut HookObserveArgs, assign: ExprAssign) -> Result<()> {
    let name = assign_name(&assign)?;
    let value = *assign.right;

    match name.as_str() {
        "value" => set_expr_slot(&mut args.value, value, &assign.left, &name),
        "mode" => {
            // Keep mode parsing aligned between positional and named forms so
            // `#[hook::observe(Mode::FirstHit)]` and
            // `#[hook::observe(mode = Mode::FirstHit)]` normalize the same way.
            let mode = interception_mode_path(&value).ok_or_else(|| {
                syn::Error::new_spanned(
                    value,
                    "unsupported interception mode; expected `Off`, `FirstHit`, or `EveryHit`",
                )
            })?;
            set_path_slot(&mut args.mode, mode, &assign.left, &name)
        }
        _ => Err(syn::Error::new_spanned(
            assign.left,
            "unsupported observe argument",
        )),
    }
}

/// Parses one single-assignment nested-meta value into one optional slot.
fn parse_slot<T: Parse>(
    meta: &syn::meta::ParseNestedMeta,
    slot: &mut Option<T>,
    name: &str,
) -> Result<()> {
    let value: T = meta.value()?.parse()?;
    if slot.replace(value).is_some() {
        return Err(duplicate_arg(meta, name));
    }
    Ok(())
}

/// Parses one bracketed expression list into one vector-valued slot.
fn parse_expr_array_slot(
    meta: &syn::meta::ParseNestedMeta,
    slot: &mut Vec<Expr>,
    name: &str,
) -> Result<()> {
    if !slot.is_empty() {
        return Err(duplicate_arg(meta, name));
    }
    let value: ExprArray = meta.value()?.parse()?;
    slot.extend(value.elems);
    Ok(())
}

/// Extracts the assigned argument name from one `name = value` expression.
fn assign_name(assign: &ExprAssign) -> Result<String> {
    match assign.left.as_ref() {
        Expr::Path(ExprPath {
            qself: None, path, ..
        }) if path.segments.len() == 1 => Ok(path.segments[0].ident.to_string()),
        _ => Err(syn::Error::new_spanned(
            &assign.left,
            "unsupported hook argument",
        )),
    }
}

/// Stores one expression in one optional slot, reporting duplicates consistently.
fn set_expr_slot(slot: &mut Option<Expr>, value: Expr, span: &Expr, name: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(syn::Error::new_spanned(
            span,
            format!("duplicate `{name}` argument"),
        ));
    }
    Ok(())
}

/// Stores one path in one optional slot, reporting duplicates consistently.
fn set_path_slot(slot: &mut Option<Path>, value: Path, span: &Expr, name: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(syn::Error::new_spanned(
            span,
            format!("duplicate `{name}` argument"),
        ));
    }
    Ok(())
}

/// Builds the shared duplicate-argument parse error.
fn duplicate_arg(meta: &syn::meta::ParseNestedMeta, name: &str) -> syn::Error {
    meta.error(format!("duplicate `{name}` argument"))
}
