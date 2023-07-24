use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{LitInt, Type};

pub(crate) fn methods(ty: &Type, offset: &LitInt) -> Option<TokenStream> {
    let ((x64_asm_width, x64_reg_class), (aarch64_reg_modifier, aarch64_instr_width)) =
        match ty.to_token_stream().to_string().as_ref() {
            "u8" => (("byte", quote! { reg_byte }), (":w", "b")),
            "u16" => (("word", quote! { reg }), (":w", "w")),
            "u32" => (("dword", quote! { reg }), (":w", "")),
            "u64" => (("qword", quote! { reg }), ("", "")),
            _ => return None,
        };

    let x64_width_modifier = format!("{x64_asm_width} ptr ");
    let x64_cls_location = format!("gs:[{offset}]");

    Some(quote! {
        #[cfg(target_arch = "x86_64")]
        #[inline]
        pub fn load(&self) -> #ty {
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

        #[cfg(target_arch = "x86_64")]
        #[inline]
        pub fn increment(&self) {
            unsafe {
                ::core::arch::asm!(
                    ::core::concat!("inc ", #x64_width_modifier, #x64_cls_location),
                    options(preserves_flags, nostack),
                )
            };
        }

        #[cfg(target_arch = "x86_64")]
        #[inline]
        pub fn decrement(&self) {
            unsafe {
                ::core::arch::asm!(
                    ::core::concat!("dec ", #x64_width_modifier, #x64_cls_location),
                    options(preserves_flags, nostack),
                )
            };
        }

        #[cfg(target_arch = "aarch64")]
        #[inline]
        pub fn load(&self) -> #ty {
            let ret;
            unsafe {
                ::core::arch::asm!(
                    "1:",
                    // Load value.
                    "mrs {tp_1}, tpidr_el1",
                    concat!(
                        "ldr", #aarch64_instr_width,
                        " {ret", #aarch64_reg_modifier,"},",
                        " [{tp_1},#", stringify!(#offset), "]",
                    ),

                    // Make sure task wasn't migrated between mrs and ldxr.
                    "mrs {tp_2}, tpidr_el1",
                    "cmp {tp_1}, {tp_2}",
                    "b.ne 1b",

                    tp_1 = out(reg) _,
                    ret = out(reg) ret,
                    tp_2 = out(reg) _,
                )
            };
            ret
        }

        #[cfg(target_arch = "aarch64")]
        #[inline]
        pub fn add(&self, operand: #ty) {
            unsafe {
                ::core::arch::asm!(
                    "1:",
                    // Load value.
                    // TODO: Can we add offset and load in one instruction?
                    "mrs {tp_1}, tpidr_el1",
                    concat!("add {ptr}, {tp_1}, ", stringify!(#offset)),
                    concat!("ldxr", #aarch64_instr_width, " {value", #aarch64_reg_modifier,"}, [{ptr}]"),

                    // Make sure task wasn't migrated between msr and ldxr.
                    "mrs {tp_2}, tpidr_el1",
                    "cmp {tp_1}, {tp_2}",
                    "b.ne 1b",

                    // Compute and store value.
                    "add {value}, {value}, {operand}",
                    concat!("stxr", #aarch64_instr_width, " {cond:w}, {value", #aarch64_reg_modifier,"}, [{ptr}]"),

                    // Make sure task wasn't migrated between ldxr and stxr.
                    "cbnz {cond}, 1b",

                    tp_1 = out(reg) _,
                    ptr = out(reg) _,
                    value = out(reg) _,
                    tp_2 = out(reg) _,
                    operand = in(reg) operand,
                    cond = out(reg) _,

                    options(nostack),
                )
            };
        }

        #[cfg(target_arch = "aarch64")]
        #[inline]
        pub fn sub(&self, operand: #ty) {
            unsafe {
                ::core::arch::asm!(
                    "1:",
                    // Load value.
                    // TODO: Can we add offset and load in one instruction?
                    "mrs {tp_1}, tpidr_el1",
                    concat!("add {ptr}, {tp_1}, ", stringify!(#offset)),
                    concat!("ldxr", #aarch64_instr_width, " {value", #aarch64_reg_modifier,"}, [{ptr}]"),

                    // Make sure task wasn't migrated between msr and ldxr.
                    "mrs {tp_2}, tpidr_el1",
                    "cmp {tp_1}, {tp_2}",
                    "b.ne 1b",

                    // Compute and store value.
                    "sub {value}, {value}, {operand}",
                    concat!("stxr", #aarch64_instr_width, " {cond:w}, {value", #aarch64_reg_modifier,"}, [{ptr}]"),

                    // Make sure task wasn't migrated between ldxr and stxr.
                    "cbnz {cond}, 1b",

                    tp_1 = out(reg) _,
                    ptr = out(reg) _,
                    value = out(reg) _,
                    tp_2 = out(reg) _,
                    operand = in(reg) operand,
                    cond = out(reg) _,

                    options(nostack),
                )
            };
        }

        #[cfg(target_arch = "aarch64")]
        #[inline]
        pub fn increment(&self) {
            self.add(1);
        }

        #[cfg(target_arch = "aarch64")]
        #[inline]
        pub fn decrement(&self) {
            self.sub(1);
        }
    })
}
