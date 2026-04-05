//! Attribute-argument parsing for the hook proc macros.

use proc_macro::TokenStream;
use syn::parse::{Parse, Parser};
use syn::{Expr, ExprArray, Ident, Result, Type};

/// Parses one comma-separated nested-meta list into one typed args struct.
macro_rules! parse_nested_args {
    ($attr:expr, $args:ident, $unsupported:literal, {
        $($name:expr => $parser:ident($slot:expr)),* $(,)?
    }) => {{
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
            parse_nested_args!(attr, args, $unsupported, {
                $(stringify!($field) => $parser(&mut args.$field)),*
            })
        }
    };
}

define_hook_args!(HookArgs => parse_hook_args, "unsupported hook argument" {
    name: Option<Expr> => parse_slot,
    function: Option<Expr> => parse_slot,
    optional: Option<Expr> => parse_slot,
    fallback: Option<Expr> => parse_slot,
});

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
    default: Option<Ident> => parse_slot,
});

/// Parsed arguments for `#[hook::observe(...)]`.
pub(crate) struct HookObserveArgs {
    /// The requested interception mode identifier.
    pub(crate) mode: Ident,
}

/// Parses the single interception-mode identifier accepted by `#[hook::observe(...)]`.
pub(crate) fn parse_hook_observe_args(attr: TokenStream) -> Result<HookObserveArgs> {
    let mode: Ident = syn::parse(attr)?;
    Ok(HookObserveArgs { mode })
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

/// Builds the shared duplicate-argument parse error.
fn duplicate_arg(meta: &syn::meta::ParseNestedMeta, name: &str) -> syn::Error {
    meta.error(format!("duplicate `{name}` argument"))
}
