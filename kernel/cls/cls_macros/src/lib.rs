//! Exports the [`cpu_local`] macro.

#![feature(proc_macro_diagnostic, proc_macro_span, let_chains)]

mod int;

use convert_case::{Case, Casing};
use proc_macro::{Diagnostic, Level, Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    Attribute, Expr, Ident, LitBool, LitInt, Path, Token, Type, Visibility,
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

struct Args {
    offset: LitInt,
    cls_dependency: bool,
    stores_guard: Option<Type>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let offset = input.parse()?;
        let mut cls_dependency = true;
        let mut stores_guard = None;

        while input.parse::<Token![,]>().is_ok() {
            let name = input.parse::<Path>()?;
            input.parse::<Token![=]>()?;

            if name.is_ident("cls_dep") {
                cls_dependency = input.parse::<LitBool>()?.value();
            } else if name.is_ident("stores_guard") {
                stores_guard = Some(input.parse()?);
            } else {
                Diagnostic::spanned(
                    name.span().unwrap(),
                    Level::Error,
                    format!("invalid argument `{}`", name.to_token_stream()),
                )
                .help("valid arguments are: `cls_dep`")
                .emit();
                return Err(syn::Error::new(name.span(), ""));
            }
        }

        if stores_guard.is_some() && !cls_dependency {
            todo!();
            // Diagnostic::spanned(
            //     name.span().unwrap(),
            //     Level::Error,
            //     format!("invalid argument `{}`", name.to_token_stream()),
            // )
            // .help("valid arguments are: `cls_dep`")
            // .emit();
            // return Err(syn::Error::new(name.span(), ""));
        }

        Ok(Self {
            offset,
            cls_dependency,
            stores_guard,
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
    let Args {
        offset,
        cls_dependency,
        stores_guard,
    } = parse_macro_input!(args as Args);

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

    let ptr_expr = quote! {
        {
            let ptr: usize;
            #[cfg(target_arch = "x86_64")]
            {
                unsafe {
                    ::core::arch::asm!(
                        // TODO: rdgsbase
                        "rdgsbase {}",
                        out(reg) ptr,
                        options(nomem, preserves_flags, nostack),
                    )
                };
            }
            #[cfg(target_arch = "aarch64")]
            {
                unsafe {
                    ::core::arch::asm!(
                        "mrs {}, tpidr_el1",
                        out(reg) ptr,
                        options(nomem, preserves_flags, nostack),
                    )
                };
            }
            ptr + #offset
        }
    };

    let cls_crate_functions = if let Some(guard_type) = stores_guard {
        quote! {
            #[inline]
            pub fn replace(&self, guard: #guard_type) -> #ty {
                // Check that the guard type matches the type of the static.
                trait TyEq {}
                impl<T> TyEq for (T, T) {}
                fn ty_eq<A, B>()
                where
                    (A, B): TyEq
                {}
                ty_eq::<::core::option::Option<#guard_type>, #ty>();

                // Check that the guard type implements cls::Guard.
                fn implements_guard_trait<T>()
                where
                    T: ::cls::Guard
                {}
                implements_guard_trait::<#guard_type>();

                let mut guard = Some(guard);
                let ptr = #ptr_expr;

                let rref = unsafe { &mut*(ptr as *mut #ty) };
                ::core::mem::swap(rref, &mut guard);

                guard
            }

            #[inline]
            pub unsafe fn take(&self) -> #guard_type {
                let ptr = #ptr_expr;

                let mut ret = None;
                let rref = unsafe { &mut*(ptr as *mut #ty) };
                ::core::mem::swap(rref, &mut ret);

                ret.expect("wawawa")
            }

            #[inline]
            pub fn set(&self, mut guard: #guard_type) {
                self.replace(guard);
            }
        }
    } else if cls_dependency {
        quote! {
            #[inline]
            pub fn replace_guarded<G>(&self, mut value: #ty, guard: &G) -> #ty
            where
                G: ::cls::Guard,
            {
                let ptr = #ptr_expr;

                let rref = unsafe { &mut*(ptr as *mut #ty) };
                ::core::mem::swap(rref, &mut value);

                value
            }

            #[inline]
            pub fn set_guarded<G>(&self, mut value: #ty, guard: &G)
            where
                G: ::cls::Guard,
            {
                self.replace_guarded(value, guard);
            }
        }
    } else {
        proc_macro2::TokenStream::new()
    };

    let int_functions = int::int_functions(ty, offset).unwrap_or_default();

    quote! {
        #(#attributes)*
        #[thread_local]
        // #[link_section = ".cls"]
        #visibility static #name: #struct_name = #struct_name;

        #[repr(transparent)]
        #[non_exhaustive]
        #visibility struct #struct_name;

        impl #struct_name {
            #int_functions
            #cls_crate_functions

        }
    }
    .into()
}
