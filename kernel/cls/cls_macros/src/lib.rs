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
    Attribute, Expr, Ident, LitBool, Path, Token, Type, Visibility,
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

struct Args {
    cls_dependency: bool,
    stores_guard: Option<Type>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut cls_dependency = true;
        let mut stores_guard = None;

        while let Ok(name) = input.parse::<Path>() {
            input.parse::<Token![=]>()?;

            if name.is_ident("cls_dep") {
                cls_dependency = input.parse::<LitBool>()?.value();
            } else if name.is_ident("stores_guard") {
                stores_guard = Some((name, input.parse::<Type>()?));
            } else {
                Diagnostic::spanned(
                    name.span().unwrap(),
                    Level::Error,
                    format!("invalid argument `{}`", name.to_token_stream()),
                )
                .help("valid arguments are: `cls_dep`, `stores_guard`")
                .emit();
                return Err(syn::Error::new(name.span(), ""));
            }
            let _ = input.parse::<Token![,]>();
        }

        if let Some((ref name, ref value)) = stores_guard && !cls_dependency {
            let span = name.span().join(value.span()).unwrap().unwrap();
            Diagnostic::spanned(
                span,
                Level::Error,
                "`stores_guard` requires `cls_dep`",
            )
            .emit();
            return Err(syn::Error::new(name.span(), ""));
        }

        Ok(Self {
            cls_dependency,
            stores_guard: stores_guard.map(|tuple| tuple.1),
        })
    }
}

/// A macro for declaring CPU-local variables.
///
/// Variables must be an unsigned integer, bar `u128`.
///
/// The initialisation expression has no effect; to set the initial value,
/// `per_cpu::PerCpuData::new` must be modified.
///
/// # Arguments
///
/// The macro supports additional named arguments defined after the offset (e.g.
/// `#[cpu_local(0, cls_dep = false)]`):
/// - `cls_dep`: Whether to define methods that depend on `cls` indirectly
///   adding a dependency on `preemption` and `irq_safety`. This is only really
///   useful for CPU locals defined in `preemption` to avoid a circular
///   dependency. Defaults to true.
/// - `stores_guard`: If defined, must be set to either `HeldInterrupts` or
///   `PreemptionGuard` and signifies that the CPU local has the type
///   `Option<Guard>`. This option defines special methods that use the guard
///   being switched into the CPU local, rather than an additional guard
///   parameter, as proof that the CPU local can be safely accessed.
#[proc_macro_attribute]
pub fn cpu_local(args: TokenStream, input: TokenStream) -> TokenStream {
    let Args {
        cls_dependency,
        stores_guard,
    } = match syn::parse(args) {
        Ok(args) => args,
        Err(error) => {
            if error.to_string() == "" {
                // We've already emmited an error diagnostic.
                return TokenStream::new();
            } else {
                Diagnostic::spanned(error.span().unwrap(), Level::Error, error.to_string()).emit();
                return TokenStream::new();
            }
        }
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

    let offset_expr = cls_offset_expr(&name);
    let ref_expr = quote! {
        {
            let offset = #offset_expr;
            #[cfg(target_arch = "x86_64")]
            let mut ptr: u64 = {
                use cls::__private::x86_64::registers::segmentation::{GS, Segment64};
                let gs = GS::read_base().as_u64();

                let (value, overflow_occured) = gs.overflowing_add(offset);
                assert!(overflow_occured, "overflow did not occur");

                value
            };
            #[cfg(target_arch = "aarch64")]
            let mut ptr = {
                use cls::__private::tock_registers::interfaces::Readable;
                let tpidr_el1 = cls::__private::cortex_a::registers::TPIDR_EL1.get();

                let (value, overflow_occured) = tpidr_el1.overflowing_add(offset);
                assert!(overflow_occured, "overflow did not occur");

                value
            };
            // log::info!("ptr after asm: {ptr:0x?}");
            unsafe { &mut*(ptr as *mut #ty) }
        }
    };

    let cls_dep_functions = if cls_dependency {
        let guarded_functions = quote! {
            #[inline]
            pub fn replace_guarded<G>(&self, mut value: #ty, guard: &G) -> #ty
            where
                G: ::cls::CpuAtomicGuard,
            {
                let rref = #ref_expr;
                ::core::mem::swap(rref, &mut value);
                value
            }

            #[inline]
            pub fn set_guarded<G>(&self, mut value: #ty, guard: &G)
            where
                G: ::cls::CpuAtomicGuard,
            {
                self.replace_guarded(value, guard);
            }
        };

        if let Some(guard_type) = stores_guard {
            quote! {
                #guarded_functions

                #[inline]
                pub fn replace(&self, guard: #guard_type) -> #ty {
                    // Check that the guard type matches the type of the static.
                    // https://github.com/rust-lang/rust/issues/20041#issuecomment-820911297
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
                        T: ::cls::CpuAtomicGuard
                    {}
                    implements_guard_trait::<#guard_type>();

                    // ::log::debug!("guard: {guard:?}");
                    let rref = #ref_expr;
                    // ::log::debug!("ptr to CLS guard: {:0x?}", rref as *const _);
                    // ::log::debug!("bytes at CLS guard: {:0x?}", unsafe { core::slice::from_raw_parts(rref as *const _ as *const u8, 8) });
                    let mut guard = Some(guard);
                    ::core::mem::swap(rref, &mut guard);
                    // log::info!("replaced guard: {guard:?}");

                    guard
                }

                #[inline]
                pub fn set(&self, mut guard: #guard_type) {
                    self.replace(guard);
                }
            }
        } else {
            quote! {
                #guarded_functions

                #[inline]
                pub fn replace(&self, value: #ty) -> #ty {
                    // TODO: Should this ever be `disable_interrupts` instead?
                    let guard = ::cls::__private::preemption::hold_preemption();
                    self.replace_guarded(value, &guard)
                }

                #[inline]
                pub fn set(&self, value: #ty) {
                    // TODO: Should this ever be `disable_interrupts` instead?
                    let guard = ::cls::__private::preemption::hold_preemption();
                    self.set_guarded(value, &guard);
                }

            }
        }
    } else {
        proc_macro2::TokenStream::new()
    };

    let int_functions = int::int_functions(&ty, &name);

    quote! {
        #(#attributes)*
        #[thread_local]
        #[link_section = ".cls"]
        #[used]
        #visibility static #name: #struct_name = #struct_name {
            __inner: #init,
        };

        #[repr(transparent)]
        #[doc(hidden)]
        #visibility struct #struct_name {
            __inner: #ty,
        }

        impl #struct_name {
            #int_functions
            #cls_dep_functions
        }
    }
    .into()
}

fn cls_offset_expr(name: &Ident) -> proc_macro2::TokenStream {
    quote! {
        {
            extern "C" {
                static CLS_SIZE: u8;
                static TLS_SIZE: u8;
            }

            // TODO: Open an issue in rust repo? We aren't actually doing anything unsafe.
            // SAFETY: We don't access the extern static.
            let cls_size = unsafe { ::core::ptr::addr_of!(CLS_SIZE) } as u64;
            // SAFETY: We don't access the extern static.
            let tls_size = unsafe { ::core::ptr::addr_of!(TLS_SIZE) } as u64;

            #[cfg(target_arch = "x86_64")]
            {
                let temp: u64;
                unsafe {
                    ::core::arch::asm!(
                        "lea {temp}, [{cls}@TPOFF + {tls_size} + 0x1000]",
                        temp = lateout(reg) temp,
                        cls = sym #name,
                        tls_size = in(reg) tls_size,
                    )
                };
                let offset = (cls_size - temp).wrapping_neg();
                offset
            }
            #[cfg(target_arch = "aarch64")]
            {
                todo!();
                0u64
            }
        }
    }
}
