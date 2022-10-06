//! This crate contains structures and routines for context switching 
//! when AVX extensions are enabled. 

#![no_std]
#![feature(naked_functions)]

extern crate zerocopy;
#[macro_use] extern crate context_switch_regular;

use context_switch_regular::ContextRegular;
use zerocopy::FromBytes;


/// The registers saved before a context switch and restored after a context switch
/// for AVX-enabled Tasks.
///
/// Note: the order of the registers here MUST MATCH the order of 
/// registers popped in the [`restore_registers_avx!`] macro. 
#[derive(FromBytes)]
#[repr(C, packed)]
pub struct ContextAVX {
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


/// An assembly block for saving AVX registers
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_avx {
    () => (
        // Save all of the ymm registers (for AVX).
        // Each register is 32 bytes (256 bits), and there are 16 of them.
        r#"
            lea rsp, [rsp - 32*16]
            vmovups [rsp + 32*0],  ymm0   # push ymm0
            vmovups [rsp + 32*1],  ymm1   # push ymm1
            vmovups [rsp + 32*2],  ymm2   # push ymm2
            vmovups [rsp + 32*3],  ymm3   # push ymm3
            vmovups [rsp + 32*4],  ymm4   # push ymm4
            vmovups [rsp + 32*5],  ymm5   # push ymm5
            vmovups [rsp + 32*6],  ymm6   # push ymm6
            vmovups [rsp + 32*7],  ymm7   # push ymm7
            vmovups [rsp + 32*8],  ymm8   # push ymm8
            vmovups [rsp + 32*9],  ymm9   # push ymm9
            vmovups [rsp + 32*10], ymm10  # push ymm10
            vmovups [rsp + 32*11], ymm11  # push ymm11
            vmovups [rsp + 32*12], ymm12  # push ymm12
            vmovups [rsp + 32*13], ymm13  # push ymm13
            vmovups [rsp + 32*14], ymm14  # push ymm14
            vmovups [rsp + 32*15], ymm15  # push ymm15
        "#
    );
}


/// An assembly block for restoring AVX registers
/// by popping them off of the stack.
#[macro_export]
macro_rules! restore_registers_avx {
    () => (
        // restore all of the ymm registers
        r#"
            vmovups ymm15, [rsp + 32*15]   # pop ymm15
            vmovups ymm14, [rsp + 32*14]   # pop ymm14
            vmovups ymm13, [rsp + 32*13]   # pop ymm13
            vmovups ymm12, [rsp + 32*12]   # pop ymm12
            vmovups ymm11, [rsp + 32*11]   # pop ymm11
            vmovups ymm10, [rsp + 32*10]   # pop ymm10
            vmovups ymm9,  [rsp + 32*9]    # pop ymm9
            vmovups ymm8,  [rsp + 32*8]    # pop ymm8
            vmovups ymm7,  [rsp + 32*7]    # pop ymm7
            vmovups ymm5,  [rsp + 32*5]    # pop ymm5
            vmovups ymm6,  [rsp + 32*6]    # pop ymm6
            vmovups ymm4,  [rsp + 32*4]    # pop ymm4
            vmovups ymm3,  [rsp + 32*3]    # pop ymm3
            vmovups ymm2,  [rsp + 32*2]    # pop ymm2
            vmovups ymm1,  [rsp + 32*1]    # pop ymm1
            vmovups ymm0,  [rsp + 32*0]    # pop ymm0
            lea rsp, [rsp + 32*16]
        "#
    );
}


/// Switches context from an AVX Task to another AVX Task.
/// 
/// # Arguments
/// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
/// * Second argument (in `RSI`): the value of the next task's stack pointer
/// 
/// # Safety
/// This function is unsafe because it changes the content on both task's stacks. 
#[naked]
pub unsafe extern "C" fn context_switch_avx(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
    // Since this is a naked function that expects its arguments in two registers,
    // you CANNOT place any log statements or other instructions here
    // before, in between, or after anything below.
    core::arch::asm!(
        save_registers_regular!(),
        save_registers_avx!(),
        switch_stacks!(),
        restore_registers_avx!(),
        restore_registers_regular!(),
        options(noreturn)
    );
}
