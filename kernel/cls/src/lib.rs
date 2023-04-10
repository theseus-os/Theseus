#![feature(proc_macro_diagnostic)]

use proc_macro::{Diagnostic, Level, TokenStream};
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    Expr, Ident, Token, Type, Visibility,
};

#[derive(Debug)]
struct Static {
    visibility: Visibility,
    ident: Ident,
    ty: Type,
    expression: Expr,
}

impl Parse for Static {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let visibility = input.parse()?;
        <Token![static]>::parse(input)?;
        let ident = input.parse()?;
        <Token![:]>::parse(input)?;
        let ty = input.parse()?;
        <Token![=]>::parse(input)?;
        let expression = input.parse()?;
        <Token![;]>::parse(input)?;

        Ok(Static {
            visibility,
            ident,
            ty,
            expression,
        })
    }
}

#[proc_macro_attribute]
pub fn cpu_local(_: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Static);

    let visibility = input.visibility;
    let ident = input.ident;
    let ty = input.ty;
    let expression = input.expression;

    // TODO: Support non-zeroed initialisation states.
    if expression.to_token_stream().to_string() != "0" {
        Diagnostic::spanned(
            vec![expression.span().unwrap()],
            Level::Error,
            "only zeroed initialisation states supported",
        )
        .emit();
    }

    quote! {
        #[link_section = "cls"]
        #visibility static #ident: #ident = #ident {
            __private_field: (),
        };

        #[allow(missing_copy_implementations)]
        #[allow(non_camel_case_types)]
        #[allow(dead_code)]
        #visibility struct #ident {
            __private_field: (),
        }

        // TODO: Unique name so that multiple CPU locals in same namespace don't clash.
        mod reloc {
            extern "C" {
                static #ident: &'static ();
            }
        }

        impl ::core::ops::Deref for #ident {
            type Target = #ty;

            fn deref(&self) -> &Self::Target {
                let target: *const #ty;

                unsafe {
                    ::core::arch::asm!(
                        "mov {gs}, qword ptr gs:[0]",
                        "lea {target}, [{gs} + {relocation}]",
                        target = out(reg) target,
                        gs = out(reg) _,
                        // TODO: Do we need an extern?
                        relocation = sym #ident,
                        // TODO: Add flags.
                    )
                };

                unsafe { &* target }
            }
        }
    }
    .into()
}
