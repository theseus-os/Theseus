//! This crate contains structures and routines for context switching 
//! when AVX/SSE extensions are enabled. 

#![no_std]
#![feature(asm, naked_functions)]

#[macro_use] extern crate context_switch_regular;
#[macro_use] extern crate context_switch_sse;

use context_switch_regular::ContextRegular;


/// The registers saved before a context switch and restored after a context switch.
#[repr(C, packed)]
pub struct ContextAVX {
    // The order of the registers here MUST MATCH the order of 
    // registers popped in the context_switch() function below. 
    ymm15: [u128; 2],
    ymm14: [u128; 2],   
    ymm13: [u128; 2],   
    ymm12: [u128; 2],   
    ymm11: [u128; 2],   
    ymm10: [u128; 2],   
    ymm9:  [u128; 2],   
    ymm8:  [u128; 2],   
    ymm7:  [u128; 2],   
    ymm6:  [u128; 2],   
    ymm5:  [u128; 2],   
    ymm4:  [u128; 2],   
    ymm3:  [u128; 2],   
    ymm2:  [u128; 2],   
    ymm1:  [u128; 2],   
    ymm0:  [u128; 2], 
    regular: ContextRegular,
}

impl ContextAVX {
    /// Creates a new ContextAVX struct that will cause the
    /// AVX-enabled Task containing it to begin its execution at the given `rip`.
    pub fn new(rip: usize) -> ContextAVX {
        ContextAVX {
            ymm15: [0, 0],
            ymm14: [0, 0],   
            ymm13: [0, 0],   
            ymm12: [0, 0],   
            ymm11: [0, 0],   
            ymm10: [0, 0],   
            ymm9:  [0, 0],   
            ymm8:  [0, 0],   
            ymm7:  [0, 0],   
            ymm6:  [0, 0],   
            ymm5:  [0, 0],   
            ymm4:  [0, 0],   
            ymm3:  [0, 0],   
            ymm2:  [0, 0],   
            ymm1:  [0, 0],   
            ymm0:  [0, 0],   
            regular: ContextRegular::new(rip),
        }
    }
}


/// An assembly macro for saving regular x86_64 registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_avx {
    () => (
        asm!("
            # save all of the ymm registers (for AVX)
            # each register is 32 bytes, and there are 16 of them
            lea rsp, [rsp - 32*16]
            vmovupd [rsp + 32*0],  ymm0   # push ymm0
            vmovupd [rsp + 32*1],  ymm1   # push ymm1
            vmovupd [rsp + 32*2],  ymm2   # push ymm2
            vmovupd [rsp + 32*3],  ymm3   # push ymm3
            vmovupd [rsp + 32*4],  ymm4   # push ymm4
            vmovupd [rsp + 32*5],  ymm5   # push ymm5
            vmovupd [rsp + 32*6],  ymm6   # push ymm6
            vmovupd [rsp + 32*7],  ymm7   # push ymm7
            vmovupd [rsp + 32*8],  ymm8   # push ymm8
            vmovupd [rsp + 32*9],  ymm9   # push ymm9
            vmovupd [rsp + 32*10], ymm10  # push ymm10
            vmovupd [rsp + 32*11], ymm11  # push ymm11
            vmovupd [rsp + 32*12], ymm12  # push ymm12
            vmovupd [rsp + 32*13], ymm13  # push ymm13
            vmovupd [rsp + 32*14], ymm14  # push ymm14
            vmovupd [rsp + 32*15], ymm15  # push ymm15
            "
            : : : "memory" : "intel", "volatile"
        );
    );
}


/// An assembly macro for saving regular x86_64 registers.
/// by pushing them onto the stack.
#[macro_export]
macro_rules! restore_registers_avx {
    () => (
        asm!("
            # restore all of the ymm registers
            vmovupd ymm15, [rsp + 32*15]   # pop ymm15
            vmovupd ymm14, [rsp + 32*14]   # pop ymm14
            vmovupd ymm13, [rsp + 32*13]   # pop ymm13
            vmovupd ymm12, [rsp + 32*12]   # pop ymm12
            vmovupd ymm11, [rsp + 32*11]   # pop ymm11
            vmovupd ymm10, [rsp + 32*10]   # pop ymm10
            vmovupd ymm9,  [rsp + 32*9]    # pop ymm9
            vmovupd ymm8,  [rsp + 32*8]    # pop ymm8
            vmovupd ymm7,  [rsp + 32*7]    # pop ymm7
            vmovupd ymm5,  [rsp + 32*5]    # pop ymm5
            vmovupd ymm6,  [rsp + 32*6]    # pop ymm6
            vmovupd ymm4,  [rsp + 32*4]    # pop ymm4
            vmovupd ymm3,  [rsp + 32*3]    # pop ymm3
            vmovupd ymm2,  [rsp + 32*2]    # pop ymm2
            vmovupd ymm1,  [rsp + 32*1]    # pop ymm1
            vmovupd ymm0,  [rsp + 32*0]    # pop ymm0
            lea rsp, [rsp + 32*16]
            "
            : : : "memory" : "intel", "volatile"
        );
    );
}


/// Switches context from an AVX Task to another AVX Task.
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
pub unsafe fn context_switch_avx() {
    // Since this is a naked function that expects its arguments in two registers,
    // you CANNOT place any log statements or other instructions here,
    // or at any point before, in between, or after the following macros.
    save_registers_regular!();
    save_registers_avx!();
    switch_stacks!();
    restore_registers_avx!();
    restore_registers_regular!();
}
