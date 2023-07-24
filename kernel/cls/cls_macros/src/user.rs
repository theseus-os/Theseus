use proc_macro2::TokenStream;
use quote::quote;
use syn::{LitInt, Type};

pub(crate) fn methods(ty: &Type, offset: &LitInt) -> TokenStream {
    let x64_cls_location = format!("gs:[{offset}]");

    quote! {
        #[cfg(target_arch = "x86_64")]
        pub fn replace(&self, value: #ty) -> #ty {
            let mut raw = unsafe { ::cls::RawRepresentation::into_raw(value) };
            unsafe {
                ::core::arch::asm!(
                    concat!("xchg {}, ", #x64_cls_location),
                    inout(reg) raw,
                )
            };
            unsafe { ::cls::RawRepresentation::from_raw(raw) }
        }

        #[cfg(target_arch = "aarch64")]
        pub fn replace(&self, value: #ty) -> #ty {
            let raw_in = unsafe { ::cls::RawRepresentation::into_raw(value) };
            let raw_out;

            unsafe {
                ::core::arch::asm!(
                    "1:",
                    // Load value.
                    "mrs {tp_1}, tpidr_el1",
                    concat!("add {ptr}, {tp_1}, ", stringify!(#offset)),
                    "ldxr {raw_out}, [{ptr}]",

                    // Make sure task wasn't migrated between msr and ldxr.
                    "mrs {tp_2}, tpidr_el1",
                    "cmp {tp_1}, {tp_2}",
                    "b.ne 1b",

                    // Store value.
                    "stxr {cond:w}, {raw_in}, [{ptr}]",

                    // Make sure task wasn't migrated between ldxr and stxr.
                    "cbnz {cond}, 1b",

                    tp_1 = out(reg) _,
                    ptr = out(reg) _,
                    raw_out = out(reg) raw_out,
                    tp_2 = out(reg) _,
                    raw_in = in(reg) raw_in,
                    cond = out(reg) _,
                )
            };
            unsafe { ::cls::RawRepresentation::from_raw(raw_out) }
        }

        pub fn set(&self, value: #ty) {
            self.replace(value);
        }
    }
}
