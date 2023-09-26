//! Exports the [`macro@cpu_local`] macro.

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
            let mut ptr = {
                use cls::__private::x86_64::registers::segmentation::{GS, Segment64};
                let gs = GS::read_base().as_u64();

                // If this CLS section was statically linked, its `offset` will be negative on x86_64 only.
                let (value, _) = gs.overflowing_add(offset);

                value
            };
            #[cfg(target_arch = "aarch64")]
            let mut ptr = {
                use cls::__private::tock_registers::interfaces::Readable;
                let tpidr_el1 = cls::__private::cortex_a::registers::TPIDR_EL1.get();

                tpidr_el1 + offset
            };
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

            #[inline]
            pub fn update_guarded<F, R, G>(&self, f: F, guard: &G) -> R
            where
                F: ::core::ops::FnOnce(&mut #ty) -> R,
                G: ::cls::CpuAtomicGuard,
            {
                let rref = #ref_expr;
                f(rref)
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

                    let rref = #ref_expr;
                    let mut guard = Some(guard);
                    ::core::mem::swap(rref, &mut guard);

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

                #[inline]
                pub fn update<F, R>(&self, f: F) -> R
                where
                    F: ::core::ops::FnOnce(&mut #ty) -> R,
                {
                    // TODO: Should this ever be `disable_interrupts` instead?
                    let guard = ::cls::__private::preemption::hold_preemption();
                    self.update_guarded(f, &guard)
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
            // See the crate-level docs in `cls` for an explanation of the implementation.

            #[cfg(target_arch = "x86_64")]
            {
                extern "C" {
                    static __THESEUS_CLS_SIZE: u8;
                    static __THESEUS_TLS_SIZE: u8;
                }

                // TODO: Open an issue in rust repo? We aren't actually doing anything unsafe.
                // SAFETY: We don't access the extern static.
                let cls_size = unsafe { ::core::ptr::addr_of!(__THESEUS_CLS_SIZE) } as u64;
                // SAFETY: We don't access the extern static.
                let tls_size = unsafe { ::core::ptr::addr_of!(__THESEUS_TLS_SIZE) } as u64;

                // On x86_64, `mod_mgmt` will set `__THESEUS_CLS_SIZE` and `__THESEUS_TLS_SIZE` to
                // `usize::MAX` in order to indicate dynamic loading/linking is being used.
                if cls_size == u64::MAX && tls_size == u64::MAX {
                    // The offset is always correct when dynamically linked (since we set it in `mod_mgmt`),
                    // so there is no need to modify the offset here, we can just use it directly.
                    let offset: u64;
                    unsafe {
                        ::core::arch::asm!(
                            "lea {offset}, [{cls}@TPOFF]",
                            offset = out(reg) offset,
                            cls = sym #name,
                            options(nomem, preserves_flags, nostack),
                        )
                    };
                    offset
                }
                
                // Otherwise, if the CLS section/symbol was statically linked,
                // we need to adjust the offset to account for the section's 4K page alignment
                // and the size of the preceding static TLS sections.
                // See the crate-level docs of `cls` for a detailed explanation of this.
                else {
                    // If the statically-linked base kernel image has no TLS sections,
                    // the `{cls}@TPOFF` expression alone correctly gives us the offset
                    // from the end of the `.cls` section.
                    if tls_size == 0 {
                        let offset: u64;
                        unsafe {
                            ::core::arch::asm!(
                                "lea {offset}, [{cls}@TPOFF]",
                                offset = out(reg) offset,
                                cls = sym #name,
                                options(nomem, preserves_flags, nostack),
                            )
                        };
                        offset
                    } else {
                        // The linker script aligns sections to page boundaries.
                        const ALIGNMENT: u64 = 1 << 12;
                        // TODO: Use `next_multiple_of(0x1000)` when stabilised.
                        let cls_start_to_tls_start = (cls_size + ALIGNMENT - 1) & !(ALIGNMENT - 1);

                        let from_cls_start: u64;
                        unsafe {
                            ::core::arch::asm!(
                                "lea {from_cls_start}, [{cls}@TPOFF + {tls_size} + {cls_start_to_tls_start}]",
                                from_cls_start = lateout(reg) from_cls_start,
                                cls = sym #name,
                                tls_size = in(reg) tls_size,
                                cls_start_to_tls_start = in(reg) cls_start_to_tls_start,
                                options(nomem, preserves_flags, nostack),
                            )
                        };
                        let offset = (cls_size - from_cls_start).wrapping_neg();
                        offset
                    }
                }
            }
            #[cfg(target_arch = "aarch64")]
            {
                extern "C" {
                    static __THESEUS_TLS_SIZE: u8;
                }

                // TODO: Open an issue in rust repo? We aren't actually doing anything unsafe.
                // SAFETY: We don't access the extern static.
                let tls_size = unsafe { ::core::ptr::addr_of!(__THESEUS_TLS_SIZE) } as u64;

                // The linker script aligns sections to page boundaries.
                const ALIGNMENT: u64 = 1 << 12;
                // TODO: Use `next_multiple_of(0x1000)` when stabilised.
                let tls_start_to_cls_start = (tls_size + ALIGNMENT - 1) & !(ALIGNMENT - 1);

                let mut offset = 0;
                unsafe {
                    // This will compile into something like
                    // ```armasm
                    // add {offset}, 0, #1, lsl #12
                    // add {offset}, {offset}, #0x10
                    // sub {offset}, {offset}, #1, lsl #12
                    // ```
                    // `#1, lsl #12` is `1 << 12 = 0x1000` which is the size of a single page.
                    //
                    // The first add instruction loads the upper 12 bits of the offset into
                    // `{offset}`, and the second add instruction loads the lower 12 bits. Then,
                    // since all CLS offsets on aarch64 are a page size larger than they are
                    // supposed to be, we subtract a page size. Realistically, we could just omit
                    // loading the upper 12 bits of the offset and the `sub` insntruction under the
                    // assumption that the CLS section will never be larger than a page. But that
                    // would lead to very cryptic bugs if we were to ever breach that limit and an
                    // extra `sub` instruction per CLS access isn't significant.
                    ::core::arch::asm!(
                        "add {offset}, {offset}, #:tprel_hi12:{cls}, lsl #12",
                        "add {offset}, {offset}, #:tprel_lo12_nc:{cls}",
                        "sub {offset}, {offset}, {tls_start_to_cls_start}",
                        offset = inout(reg) offset,
                        cls = sym #name,
                        tls_start_to_cls_start = in(reg) tls_start_to_cls_start,
                        options(nomem, preserves_flags, nostack),
                    )
                };
                offset
            }
        }
    }
}
