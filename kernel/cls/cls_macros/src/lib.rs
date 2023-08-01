//! Exports the [`cpu_local`] macro.

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
    _init: Expr,
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
        let _init: Expr = input.parse()?;
        input.parse::<Token![;]>()?;
        Ok(CpuLocal {
            attributes,
            visibility,
            name,
            ty,
            _init,
        })
    }
}

/// A macro for declaring CPU-local variables.
///
/// Variables must be an unsigned integer, bar `u128`.
///
/// The initialisation expression has no effect; to set the initial value,
/// `per_cpu::PerCpuData::new` must be modified.
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
        _init,
    } = parse_macro_input!(input as CpuLocal);

    let attributes = attributes.iter().map(|attribute| quote! { #attribute });

    let struct_name = Ident::new(
        &format!("CPU_LOCAL_{name}").to_case(Case::Pascal),
        Span::call_site().into(),
    );

    let ((x64_asm_width, x64_reg_class), (aarch64_reg_modifier, aarch64_instr_width)) =
        match ty.to_token_stream().to_string().as_ref() {
            "u8" => (("byte", quote! { reg_byte }), (":w", "b")),
            "u16" => (("word", quote! { reg }), (":w", "w")),
            "u32" => (("dword", quote! { reg }), (":w", "")),
            "u64" => (("qword", quote! { reg }), ("", "")),
            _ => {
                Diagnostic::spanned(ty.span().unwrap(), Level::Error, "invalid type")
                    .help("CPU locals only support these types: `u8`, `u16`, `u32`, `u64`")
                    .emit();
                return TokenStream::new();
            }
        };

    let x64_width_modifier = format!("{x64_asm_width} ptr ");
    let x64_cls_location = format!("gs:[{offset}]");

    quote! {
        #(#attributes)*
        #[thread_local]
        // #[link_section = ".cls"]
        #visibility static #name: #struct_name = #struct_name;

        #[repr(transparent)]
        #[non_exhaustive]
        #visibility struct #struct_name;

        impl #struct_name {
            #[inline]
            pub fn load(&self) -> #ty {
                #[cfg(target_arch = "x86_64")]
                {
                    let ret;
                    unsafe {
                        ::core::arch::asm!(
                            ::core::concat!("mov {}, ", #x64_cls_location),
                            out(#x64_reg_class) ret,
                            options(preserves_flags, nostack),
                        )
                    };
                    ret
                }
                #[cfg(target_arch = "aarch64")]
                {
                    let ret;
                    unsafe {
                        ::core::arch::asm!(
                            "2:",
                            // Load value.
                            "mrs {tp_1}, tpidr_el1",
                            concat!(
                                "ldr", #aarch64_instr_width,
                                " {ret", #aarch64_reg_modifier,"},",
                                " [{tp_1},#", stringify!(#offset), "]",
                            ),

                            // Make sure task wasn't migrated between mrs and ldr.
                            "mrs {tp_2}, tpidr_el1",
                            "cmp {tp_1}, {tp_2}",
                            "b.ne 2b",

                            tp_1 = out(reg) _,
                            ret = out(reg) ret,
                            tp_2 = out(reg) _,

                            options(nostack),
                        )
                    };
                    ret
                }
            }

            #[inline]
            pub fn fetch_add(&self, mut operand: #ty) -> #ty {
                #[cfg(target_arch = "x86_64")]
                {
                    unsafe {
                        ::core::arch::asm!(
                            ::core::concat!("xadd ", #x64_width_modifier, #x64_cls_location, ", {}"),
                            inout(#x64_reg_class) operand,
                            options(nostack),
                        )
                    };
                    operand
                }
                #[cfg(target_arch = "aarch64")]
                {
                    let ret;
                    unsafe {
                        ::core::arch::asm!(
                            "2:",
                            // Load value.
                            "mrs {tp_1}, tpidr_el1",
                            concat!("add {ptr}, {tp_1}, ", stringify!(#offset)),
                            concat!("ldxr", #aarch64_instr_width, " {value", #aarch64_reg_modifier,"}, [{ptr}]"),

                            // Make sure task wasn't migrated between msr and ldxr.
                            "mrs {tp_2}, tpidr_el1",
                            "cmp {tp_1}, {tp_2}",
                            "b.ne 2b",

                            // Compute and store value (reuse tp_1 register).
                            "add {tp_1}, {value}, {operand}",
                            concat!("stxr", #aarch64_instr_width, " {cond:w}, {tp_1", #aarch64_reg_modifier,"}, [{ptr}]"),

                            // Make sure task wasn't migrated between ldxr and stxr.
                            "cbnz {cond}, 2b",

                            tp_1 = out(reg) ret,
                            ptr = out(reg) _,
                            value = out(reg) ret,
                            tp_2 = out(reg) _,
                            operand = in(reg) operand,
                            cond = out(reg) _,

                            options(nostack),
                        )
                    };
                    ret
                }
            }

            #[inline]
            pub fn fetch_sub(&self, mut operand: #ty) -> #ty {
                operand = operand.overflowing_neg().0;
                self.fetch_add(operand)
            }
        }
    }
    .into()
}
