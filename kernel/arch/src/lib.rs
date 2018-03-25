#![no_std]
#![feature(asm)]

// #[macro_use] extern crate log;
extern crate gdt;

use gdt::{AvailableSegmentSelector, get_segment_selector};


/// Transitions the currently-running Task from kernel space to userspace.
/// Thus, it should be called from a userspace-ready task wrapper, i.e., `userspace_wrapper()`. 
/// Unsafe because both the stack_ptr and the function_ptr must be valid!
pub unsafe fn jump_to_userspace(stack_ptr: usize, function_ptr: usize) {
    
    // Steps to jumping to userspace:
    // 1) push stack segment selector (ss), i.e., the user_data segment selector
    // 2) push the userspace stack pointer
    // 3) push rflags, the control flags we wish to use
    // 4) push the code segment selector (cs), i.e., the user_code segment selector
    // 5) push the instruction pointer (rip) for the start of userspace, e.g., the function pointer
    // 6) set all other segment registers (ds, es, fs, gs) to the user_data segment, same as (ss)
    // 7) issue iret to return to userspace

    // debug!("Jumping to userspace with stack_ptr: {:#x} and function_ptr: {:#x}",
    //                   stack_ptr, function_ptr);
    // debug!("stack: {:#x} {:#x} func: {:#x}", *(stack_ptr as *const usize), *((stack_ptr - 8) as *const usize), 
    //                 *(function_ptr as *const usize));



    let ss: u16 = get_segment_selector(AvailableSegmentSelector::UserData64).0;
    let cs: u16 = get_segment_selector(AvailableSegmentSelector::UserCode64).0;
    
    // interrupts must be enabled in the rflags for the new userspace task
    let rflags: usize = 1 << 9; // just set the interrupt bit, not the IOPL 
    
    // debug!("jump_to_userspace: rflags = {:#x}, userspace interrupts: {}", rflags, rflags & 0x200 == 0x200);



    asm!("mov ds, $0" : : "r"(ss) : "memory" : "intel", "volatile");
    asm!("mov es, $0" : : "r"(ss) : "memory" : "intel", "volatile");
    //asm!("mov fs, $0" : : "r"(ss) : "memory" : "intel", "volatile");


    asm!("push $0" : : "r"(ss as usize) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(stack_ptr) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(rflags) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(cs as usize) : "memory" : "intel", "volatile");
    asm!("push $0" : : "r"(function_ptr) : "memory" : "intel", "volatile");
    
    // Optionally, we can push arguments onto the stack here too.

    // final step, use iret instruction to jump to Ring 3
    asm!("iretq" : : : "memory" : "intel", "volatile");
}



#[inline(always)]
pub fn pause() {
    unsafe { asm!("pause" : : : : "intel", "volatile"); }
}
