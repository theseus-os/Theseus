#![feature(proc_macro_diagnostic, proc_macro_span, let_chains)]

use convert_case::{Case, Casing};
use proc_macro::{Diagnostic, Level, Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    Attribute, Expr, Ident, LitInt, Token, Type, Visibility,
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

#[proc_macro_attribute]
pub fn cpu_local(args: TokenStream, input: TokenStream) -> TokenStream {
    // if !args.is_empty() {
    //     Diagnostic::spanned(
    //         Span::call_site(),
    //         Level::Error,
    //         "malformed `cpu_local` attribute input",
    //     )
    //     .help("must be of the form `#[cpu_local]`")
    //     .emit();
    //     return TokenStream::new();
    // }

    let offset = if let Ok(i) = syn::parse::<LitInt>(args.clone()) {
        i
    } else {
        let span = args
            .into_iter()
            .map(|tt| tt.span())
            .reduce(|a, b| a.join(b).unwrap())
            .unwrap_or_else(Span::call_site);
        Diagnostic::spanned(span, Level::Error, "invalid offset").emit();
        return TokenStream::new();
    };

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

    let (asm_width, reg_class) = match ty.to_token_stream().to_string().as_ref() {
        "u8" => ("byte", quote! { reg_byte }),
        "u16" => ("word", quote! { reg }),
        "u32" => ("dword", quote! { reg }),
        "u64" => ("qword", quote! { reg }),
        _ => {
            Diagnostic::spanned(ty.span().unwrap(), Level::Error, "invalid type")
                .help("CPU locals only support these types: `u8`, `u16`, `u32`, `u64`")
                .emit();
            return TokenStream::new();
        }
    };

    let unary_functions = unary_functions(asm_width, offset.clone());

    quote! {
        #(#attributes)*
        #[thread_local]
        // #[link_section = ".cls"]
        #visibility static #name: #struct_name = #struct_name {
            _inner: ::core::cell::UnsafeCell::new(#init),
        };

        #[repr(transparent)]
        #[non_exhaustive]
        #visibility struct #struct_name {
            _inner: ::core::cell::UnsafeCell<#ty>,
        }

        impl #struct_name {
            #unary_functions

            #[inline(always)]
            pub fn load(&self) -> #ty {
                let ret;
                unsafe {
                    ::core::arch::asm!(
                        concat!("mov {}, gs:[", stringify!(#offset), "]"),
                        out(#reg_class) ret,
                        options(preserves_flags, nostack),
                    )
                };
                ret
            }

            // #[inline(always)]
            // pub fn exchange(&self, other: #ty) -> #ty {
            //     use crate::RawRepresentation;
            //     let mut raw = other.into_raw();
            //     unsafe {
            //         ::core::arch::asm!("xchg {} gs:{}", inout(reg) raw, sym #name);
            //     }
            //     RawRepresentation::from_raw(raw)
            // }
        }
    }
    .into()
}

fn unary_functions(asm_width: &'static str, offset: LitInt) -> proc_macro2::TokenStream {
    const UNARY_OPERATORS: [(&str, &str); 2] = [("increment", "inc"), ("decrement", "dec")];

    let functions = UNARY_OPERATORS.iter().map(|(rust_name, asm_name)| {
        let rust_name = Ident::new(rust_name, Span::call_site().into());
        let asm_string = format!("{asm_name} {asm_width} ptr gs:[{offset}]");

        quote! {
            #[inline(always)]
            pub fn #rust_name(&self) {
                unsafe { ::core::arch::asm!(#asm_string, options(preserves_flags, nostack)) };
            }
        }
    });

    functions.collect()
}
