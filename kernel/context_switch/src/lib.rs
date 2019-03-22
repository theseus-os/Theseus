//! This is a wrapper crate around all other context switch implementation crates
//! that helps to manage the complex configuration options involving SIMD and personalities.

#![no_std]
#![feature(asm, naked_functions)]

#[macro_use] extern crate cfg_if;


// If `simd_personality` is enabled, all of the `context_switch*` implementation crates are simultaneously enabled,
// in order to allow choosing one of them based on the configuration options of each Task (SIMD, regular, etc).
// If `simd_personality` is NOT enabled, then we use the context_switch routine that matches the actual build target. 
cfg_if! {
    if #[cfg(simd_personality)] {
        #[macro_use] extern crate context_switch_sse;
        #[macro_use] extern crate context_switch_regular;

        pub use context_switch_sse::*;
        pub use context_switch_regular::*;


        /// Switches context from a regular Task to an SSE Task.
        /// 
        /// # Arguments
        /// * First argument  (put in `rdi`): mutable pointer to the previous task's stack pointer
        /// * Second argument (put in `rsi`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        /// Also, it must be a naked function, so there cannot be regular arguments passed into it.
        /// Instead, the caller of this function must place the first argument into the `rdi` register
        /// and the second argument into the `rsi` register right before invoking this function.
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_regular_to_sse() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            switch_stacks!();
            restore_registers_sse!();
            restore_registers_regular!();
        }


        /// Switches context from an SSE Task to a regular Task.
        /// 
        /// # Arguments
        /// * First argument  (put in `rdi`): mutable pointer to the previous task's stack pointer
        /// * Second argument (put in `rsi`): the value of the next task's stack pointer
        /// 
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks. 
        /// Also, it must be a naked function, so there cannot be regular arguments passed into it.
        /// Instead, the caller of this function must place the first argument into the `rdi` register
        /// and the second argument into the `rsi` register right before invoking this function.
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_sse_to_regular() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            save_registers_sse!();
            switch_stacks!();
            restore_registers_regular!();
        }
    }

    else if #[cfg(target_feature = "avx")] {
        // this WILL deprecate `if #[cfg(simd_personality)]`
        // so, don't worry about duplicated code
        #[macro_use] extern crate context_switch_sse;
        #[macro_use] extern crate context_switch_regular;
        #[macro_use] extern crate context_switch_avx;

        pub use context_switch_sse::*;
        pub use context_switch_regular::*;
        pub use context_switch_avx::*;

        /// Switches context from a regular Task to an SSE Task.
        ///
        /// # Arguments
        /// * First argument  (put in `rdi`): mutable pointer to the previous task's stack pointer
        /// * Second argument (put in `rsi`): the value of the next task's stack pointer
        ///
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks.
        /// Also, it must be a naked function, so there cannot be regular arguments passed into it.
        /// Instead, the caller of this function must place the first argument into the `rdi` register
        /// and the second argument into the `rsi` register right before invoking this function.
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_regular_to_sse() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            switch_stacks!();
            restore_registers_sse!();
            restore_registers_regular!();
        }


        /// Switches context from an SSE Task to a regular Task.
        ///
        /// # Arguments
        /// * First argument  (put in `rdi`): mutable pointer to the previous task's stack pointer
        /// * Second argument (put in `rsi`): the value of the next task's stack pointer
        ///
        /// # Safety
        /// This function is unsafe because it changes the content on both task's stacks.
        /// Also, it must be a naked function, so there cannot be regular arguments passed into it.
        /// Instead, the caller of this function must place the first argument into the `rdi` register
        /// and the second argument into the `rsi` register right before invoking this function.
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_sse_to_regular() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            save_registers_sse!();
            switch_stacks!();
            restore_registers_regular!();
        }

        // REGULAR -> AVX
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_regular_to_avx() {
            save_registers_regular!();
            switch_stacks!();
            restore_registers_avx!();
            restore_registers_regular!();
        }

        // SSE -> AVX
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_sse_to_avx() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            save_registers_sse!();
            switch_stacks!();
            restore_registers_avx!();
            restore_registers_regular!();
        }

        // AVX -> REGULAR
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_avx_to_regular() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            save_registers_avx!();
            switch_stacks!();
            restore_registers_regular!();
        }

        // AVX -> SSE
        #[naked]
        #[inline(never)]
        pub unsafe fn context_switch_avx_to_sse() {
            // Since this is a naked function that expects its arguments in two registers,
            // you CANNOT place any log statements or other instructions here,
            // or at any point before, in between, or after the following macros.
            save_registers_regular!();
            save_registers_avx!();
            switch_stacks!();
            restore_registers_sse!();
            restore_registers_regular!();
        }

        pub use context_switch_avx::context_switch_avx as context_switch;   // Does anybody use this?
    }

    else if #[cfg(target_feature = "sse2")] {
        // this actually covers SSE, SSE2, SSE3, SSE4
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


cfg_if! {
    if #[cfg(simd_personality)] {

        

    }
}
