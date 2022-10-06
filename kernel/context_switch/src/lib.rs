//! This is a wrapper crate around all other context switch implementation crates
//! that helps to manage the complex configuration options involving SIMD and personalities.

#![no_std]
#![feature(naked_functions)]

#[macro_use] extern crate cfg_if;

// If `simd_personality` is enabled, all of the `context_switch*` implementation crates are simultaneously enabled,
// in order to allow choosing one of them based on the configuration options of each Task (SIMD, regular, etc).
// If `simd_personality` is NOT enabled, then we use the context_switch routine that matches the actual build target. 
cfg_if! {
    if #[cfg(simd_personality)] {
        extern crate context_switch_regular;
        extern crate context_switch_sse;
        extern crate context_switch_avx;

        use core::arch::asm;
        pub use context_switch_sse::*;
        pub use context_switch_regular::*;
        pub use context_switch_avx::*;

        /// Switches context from a regular Task to an SSE Task.
        /// 
        /// # Arguments
        /// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
        /// * Second argument (in `RSI`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        #[naked]
        pub unsafe extern "C" fn context_switch_regular_to_sse(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here
            // before, in between, or after anything below.
            asm!(
                save_registers_regular!(),
                switch_stacks!(),
                restore_registers_sse!(),
                restore_registers_regular!(),
                options(noreturn)
            );
        }


        /// Switches context from an SSE Task to a regular Task.
        ///
        /// # Arguments
        /// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
        /// * Second argument (in `RSI`): the value of the next task's stack pointer
        ///
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks.
        #[naked]
        pub unsafe extern "C" fn context_switch_sse_to_regular(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here
            // before, in between, or after anything below.
            asm!(
                save_registers_regular!(),
                save_registers_sse!(),
                switch_stacks!(),
                restore_registers_regular!(),
                options(noreturn)
            );
        }

        /// Switches context from a regular Task to an AVX Task.
        /// 
        /// # Arguments
        /// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
        /// * Second argument (in `RSI`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        #[naked]
        pub unsafe extern "C" fn context_switch_regular_to_avx(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here
            // before, in between, or after anything below.
            asm!(
                save_registers_regular!(),
                switch_stacks!(),
                restore_registers_avx!(),
                restore_registers_regular!(),
                options(noreturn)
            );
        }

        /// Switches context from an SSE Task to an AVX regular Task.
        /// 
        /// # Arguments
        /// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
        /// * Second argument (in `RSI`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        #[naked]
        pub unsafe extern "C" fn context_switch_sse_to_avx(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here
            // before, in between, or after anything below.
            asm!(
                save_registers_regular!(),
                save_registers_sse!(),
                switch_stacks!(),
                restore_registers_avx!(),
                restore_registers_regular!(),
                options(noreturn)
            );
        }

        /// Switches context from an AVX Task to a regular Task.
        /// 
        /// # Arguments
        /// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
        /// * Second argument (in `RSI`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        #[naked]
        pub unsafe extern "C" fn context_switch_avx_to_regular(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here
            // before, in between, or after anything below.
            asm!(
                save_registers_regular!(),
                save_registers_avx!(),
                switch_stacks!(),
                restore_registers_regular!(),
                options(noreturn)
            );
        }

        /// Switches context from an AVX Task to an SSE Task.
        /// 
        /// # Arguments
        /// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
        /// * Second argument (in `RSI`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        #[naked]
        pub unsafe extern "C" fn context_switch_avx_to_sse(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here
            // before, in between, or after anything below.
            asm!(
                save_registers_regular!(),
                save_registers_avx!(),
                switch_stacks!(),
                restore_registers_sse!(),
                restore_registers_regular!(),
                options(noreturn)
            );
        }
    }

    // BELOW HERE: simd_personality is disabled, and we just need to re-export a single Context & context_switch.
    // Because "else-if" statements are executed top-down, we need to put newer standards (supersets) before older standards (subsets).
    // For example, AVX512 is above AVX2, which is above AVX, which is above SSE, which is above regular (non-SIMD).

    else if #[cfg(target_feature = "avx")] {
        extern crate context_switch_avx;
        pub use context_switch_avx::ContextAVX as Context;
        pub use context_switch_avx::context_switch_avx as context_switch;
    }

    else if #[cfg(target_feature = "sse2")] {
        // this crate covers SSE, SSE2, SSE3, SSE4, but we're only currently using it for SSE2
        extern crate context_switch_sse;
        pub use context_switch_sse::ContextSSE as Context;
        pub use context_switch_sse::context_switch_sse as context_switch;
    }

    else {
        // this covers only the default x86_64 registers
        extern crate context_switch_regular;
        pub use context_switch_regular::ContextRegular as Context;
        pub use context_switch_regular::context_switch_regular as context_switch;
    }
}
