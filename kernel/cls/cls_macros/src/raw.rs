//! Module for user types implementing `cls::Raw`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Type};

/// Returns the methods for CPU locals implementing `cls::Raw`.
///
/// Note that unsigned integers implement `cls::Raw`.
pub(crate) fn methods(name: &Ident, ty: &Type) -> TokenStream {
    quote! {
        /// Replaces the contained value with `value`, and returns the old
        /// contained value.
        #[inline]
        pub fn replace(&self, value: #ty) -> #ty {
            #[cfg(target_arch = "x86_64")]
            {
                let mut raw = unsafe { ::cls::Raw::into_raw(value) };
                unsafe {
                    ::core::arch::asm!(
                        concat!("xchg {}, gs:[{}]"),
                        inout(reg) raw,
                        sym #name,
                        options(nostack),
                    )
                };

                // SAFETY: [{ptr}] contained a `u64` returned by `Raw::into_raw`
                // that has not been converted back into #ty.
                unsafe { ::cls::Raw::from_raw(raw) }
            }
            #[cfg(target_arch = "aarch64")]
            {
                let raw_in = ::cls::Raw::into_raw(value);
                let raw_out;

                unsafe {
                    ::core::arch::asm!(
                        "2:",
                        // Load value.
                        "mrs {tp_1}, tpidr_el1",
                        concat!("add {ptr}, {tp_1}, {offset}"),
                        "ldxr {raw_out}, [{ptr}]",

                        // Make sure task wasn't migrated between msr and ldxr.
                        "mrs {tp_2}, tpidr_el1",
                        "cmp {tp_1}, {tp_2}",
                        "b.ne 2b",

                        // Store value.
                        "stxr {cond:w}, {raw_in}, [{ptr}]",

                        // Make sure task wasn't migrated between ldxr and stxr.
                        "cbnz {cond}, 2b",

                        tp_1 = out(reg) _,
                        ptr = out(reg) _,
                        offset = sym #name,
                        raw_out = out(reg) raw_out,
                        tp_2 = out(reg) _,
                        raw_in = in(reg) raw_in,
                        cond = out(reg) _,
                        options(nostack),
                    )
                };

                // SAFETY: [{ptr}] contained a `u64` returned by `Raw::into_raw`
                // that has not been converted back into #ty.
                unsafe { ::cls::Raw::from_raw(raw_out) }
            }
        }

        /// Sets the contained value.
        #[inline]
        pub fn set(&self, value: #ty) {
            self.replace(value);
        }
    }
}
