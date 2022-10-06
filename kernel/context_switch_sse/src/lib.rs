//! This crate contains structures and routines for context switching 
//! when SSE extensions are enabled. 

#![no_std]
#![feature(naked_functions)]

extern crate zerocopy;
#[macro_use] extern crate context_switch_regular;

use context_switch_regular::ContextRegular;
use zerocopy::FromBytes;


/// The registers saved before a context switch and restored after a context switch
/// for SSE-enabled Tasks.
///
/// Note: the order of the registers here MUST MATCH the order of 
/// registers popped in the [`restore_registers_sse!`] macro. 
#[derive(FromBytes)]
#[repr(C, packed)]
pub struct ContextSSE {
    xmm15: u128,
    xmm14: u128,   
    xmm13: u128,   
    xmm12: u128,   
    xmm11: u128,   
    xmm10: u128,   
    xmm9:  u128,   
    xmm8:  u128,   
    xmm7:  u128,   
    xmm6:  u128,   
    xmm5:  u128,   
    xmm4:  u128,   
    xmm3:  u128,   
    xmm2:  u128,   
    xmm1:  u128,   
    xmm0:  u128, 
    regular: ContextRegular,
}

impl ContextSSE {
    /// Creates a new ContextSSE struct that will cause the
    /// SSE-enabled Task containing it to begin its execution at the given `rip`.
    pub fn new(rip: usize) -> ContextSSE {
        ContextSSE {
            xmm15: 0,
            xmm14: 0,   
            xmm13: 0,   
            xmm12: 0,   
            xmm11: 0,   
            xmm10: 0,   
            xmm9:  0,   
            xmm8:  0,   
            xmm7:  0,   
            xmm6:  0,   
            xmm5:  0,   
            xmm4:  0,   
            xmm3:  0,   
            xmm2:  0,   
            xmm1:  0,   
            xmm0:  0,   
            regular: ContextRegular::new(rip),
        }
    }
}


/// An assembly block for saving SSE registers
/// by pushing them onto the stack.
#[macro_export]
macro_rules! save_registers_sse {
    () => (
        // Save all of the xmm registers (for SSE).
        // Each register is 16 bytes (128 bits), and there are 16 of them.
        r#"
            lea rsp, [rsp - 16*16]
            movdqu [rsp + 16*0],  xmm0   # push xmm0
            movdqu [rsp + 16*1],  xmm1   # push xmm1
            movdqu [rsp + 16*2],  xmm2   # push xmm2
            movdqu [rsp + 16*3],  xmm3   # push xmm3
            movdqu [rsp + 16*4],  xmm4   # push xmm4
            movdqu [rsp + 16*5],  xmm5   # push xmm5
            movdqu [rsp + 16*6],  xmm6   # push xmm6
            movdqu [rsp + 16*7],  xmm7   # push xmm7
            movdqu [rsp + 16*8],  xmm8   # push xmm8
            movdqu [rsp + 16*9],  xmm9   # push xmm9
            movdqu [rsp + 16*10], xmm10  # push xmm10
            movdqu [rsp + 16*11], xmm11  # push xmm11
            movdqu [rsp + 16*12], xmm12  # push xmm12
            movdqu [rsp + 16*13], xmm13  # push xmm13
            movdqu [rsp + 16*14], xmm14  # push xmm14
            movdqu [rsp + 16*15], xmm15  # push xmm15
        "#
    );
}


/// An assembly block for restoring SSE registers
/// by popping them off of the stack.
#[macro_export]
macro_rules! restore_registers_sse {
    () => (
        // restore all of the xmm registers
        r#"
            movdqu xmm15, [rsp + 16*15]   # pop xmm15
            movdqu xmm14, [rsp + 16*14]   # pop xmm14
            movdqu xmm13, [rsp + 16*13]   # pop xmm13
            movdqu xmm12, [rsp + 16*12]   # pop xmm12
            movdqu xmm11, [rsp + 16*11]   # pop xmm11
            movdqu xmm10, [rsp + 16*10]   # pop xmm10
            movdqu xmm9,  [rsp + 16*9]    # pop xmm9
            movdqu xmm8,  [rsp + 16*8]    # pop xmm8
            movdqu xmm7,  [rsp + 16*7]    # pop xmm7
            movdqu xmm5,  [rsp + 16*5]    # pop xmm5
            movdqu xmm6,  [rsp + 16*6]    # pop xmm6
            movdqu xmm4,  [rsp + 16*4]    # pop xmm4
            movdqu xmm3,  [rsp + 16*3]    # pop xmm3
            movdqu xmm2,  [rsp + 16*2]    # pop xmm2
            movdqu xmm1,  [rsp + 16*1]    # pop xmm1
            movdqu xmm0,  [rsp + 16*0]    # pop xmm0
            lea rsp, [rsp + 16*16]
        "#
    );
}


/// Switches context from an SSE Task to another SSE Task.
/// 
/// # Arguments
/// * First argument  (in `RDI`): mutable pointer to the previous task's stack pointer
/// * Second argument (in `RSI`): the value of the next task's stack pointer
/// 
/// # Safety
/// This function is unsafe because it changes the content on both task's stacks. 
#[naked]
pub unsafe extern "C" fn context_switch_sse(_prev_stack_pointer: *mut usize, _next_stack_pointer_value: usize) {
    // Since this is a naked function that expects its arguments in two registers,
    // you CANNOT place any log statements or other instructions here
    // before, in between, or after anything below.
    core::arch::asm!(
        save_registers_regular!(),
        save_registers_sse!(),
        switch_stacks!(),
        restore_registers_sse!(),
        restore_registers_regular!(),
        options(noreturn)
    );
}
