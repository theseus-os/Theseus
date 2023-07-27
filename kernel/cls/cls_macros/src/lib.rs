//! Exports the [`cpu_local`] macro.

#![feature(proc_macro_diagnostic, proc_macro_span, let_chains)]

mod int;
mod raw;

use convert_case::{Case, Casing};
use proc_macro::{Diagnostic, Level, Span, TokenStream};
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Attribute, Expr, Ident, Token, Type, Visibility,
};

struct CpuLocal {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    ty: Type,
    init: Expr,
}

impl Parse for CpuLocal {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;
        let visibility: Visibility = input.parse()?;
        input.parse::<Token![static]>()?;
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        input.parse::<Token![=]>()?;
        let init: Expr = input.parse()?;
        input.parse::<Token![;]>()?;
        Ok(CpuLocal {
            attributes,
            visibility,
            name,
            ty,
            init,
        })
    }
}

/// A macro for declaring CPU-local variables.
///
/// Variables can either be an unsigned integer, or a custom type implementing
/// `cls::RawRepresentation`.
#[proc_macro_attribute]
pub fn cpu_local(args: TokenStream, input: TokenStream) -> TokenStream {
    if !args.is_empty() {
        Diagnostic::spanned(
            Span::call_site(),
            Level::Error,
            "malformed `cpu_local` attribute input",
        )
        .help("must be of the form `#[cpu_local]`")
        .emit();
        return TokenStream::new();
    }

    let CpuLocal {
        attributes,
        visibility,
        name,
        ty,
        init,
    } = parse_macro_input!(input as CpuLocal);

    let attributes = attributes.iter().map(|attribute| quote! { #attribute });

    let struct_name = Ident::new(
        &format!("CPU_LOCAL_{name}").to_case(Case::Pascal),
        Span::call_site().into(),
    );

    let int_methods = int::methods(&name, &ty).unwrap_or_else(proc_macro2::TokenStream::new);
    let raw_methods = raw::methods(&name, &ty);

    quote! {
        #(#attributes)*
        #[thread_local]
        #[link_section = ".cls"]
        #visibility static #name: #struct_name = #struct_name::__new(#init);

        #[repr(transparent)]
        #[non_exhaustive]
        #visibility struct #struct_name {
            inner: ::core::cell::UnsafeCell<#ty>,
        }

        impl #struct_name {
            #[doc(hidden)]
            const fn __new(init: #ty) -> Self {
                Self {
                    inner: ::core::cell::UnsafeCell::new(init),
                }
            }

            #int_methods
            #raw_methods
        }
    }
    .into()
}
