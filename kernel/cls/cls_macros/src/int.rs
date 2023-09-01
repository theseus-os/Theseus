use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{Ident, Type};

use crate::cls_offset_expr;

pub(crate) fn int_functions(ty: &Type, name: &Ident) -> Option<TokenStream> {
    let ((x86_64_asm_width, x86_64_reg_class), (aarch64_reg_modifier, aarch64_instr_width)) =
        match ty.to_token_stream().to_string().as_ref() {
            "u8" => (("byte", quote! { reg_byte }), (":w", "b")),
            "u16" => (("word", quote! { reg }), (":w", "w")),
            "u32" => (("dword", quote! { reg }), (":w", "")),
            "u64" => (("qword", quote! { reg }), ("", "")),
            _ => {
                return None;
            }
        };
    let x86_64_width_modifier = format!("{x86_64_asm_width} ptr ");
    let offset_expr = cls_offset_expr(name);

    Some(quote! {
        #[inline]
        pub fn load(&self) -> #ty {
            let offset = #offset_expr;
            #[cfg(target_arch = "x86_64")]
            {
                let ret;
                unsafe {
                    ::core::arch::asm!(
                        ::core::concat!("mov {ret}, gs:[{offset}]"),
                        ret = out(#x86_64_reg_class) ret,
                        offset = in(reg) offset,
                        options(readonly, preserves_flags, nostack),
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
                        "add {ptr}, {tp_1}, {offset}",
                        concat!(
                            "ldr", #aarch64_instr_width,
                            " {ret", #aarch64_reg_modifier,"},",
                            " [{ptr}]",
                        ),

                        // Make sure task wasn't migrated between mrs and ldr.
                        "mrs {tp_2}, tpidr_el1",
                        "cmp {tp_1}, {tp_2}",
                        "b.ne 2b",

                        tp_1 = out(reg) _,
                        ptr = out(reg) _,
                        offset = in(reg) offset,
                        ret = out(reg) ret,
                        tp_2 = out(reg) _,

                        options(readonly, nostack),
                    )
                };
                ret
            }
        }

        #[inline]
        pub fn fetch_add(&self, mut operand: #ty) -> #ty {
            let offset = #offset_expr;
            #[cfg(target_arch = "x86_64")]
            {
                unsafe {
                    ::core::arch::asm!(
                        ::core::concat!("xadd ", #x86_64_width_modifier, "gs:[{offset}], {operand}"),
                        offset = in(reg) offset,
                        operand = inout(#x86_64_reg_class) operand,
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
                        "add {ptr}, {tp_1}, {offset}",
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
                        offset = in(reg) offset,
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
    })
}
