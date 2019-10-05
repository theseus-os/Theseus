//! Stack trace (backtrace) functionality.
//! 
//! There are two main ways of obtaining stack traces:
//! 1. Using the frame pointer register to find the previous stack frame.
//! 2. Using DWARF debugging information to understand the layout of each stack frame.
//! 
//! We support both ways, but prefer #2 because it doesn't suffer from the
//! compatibility and performance drawbacks of #1.
//! 
//! Note that frame pointer-based stack traces are only available 
//! 
//! The functions below offer convenient ways to obtain a stack trace,
//! iterate over the trace, print the trace, etc. 

#![no_std]
#![feature(asm)]

extern crate alloc;
// #[macro_use] extern crate log;
extern crate memory;
extern crate task;
extern crate unwind;

// use alloc::string::String;
use memory::{PageTable, VirtualAddress};
// use task::{KillReason, PanicInfoOwned};



/// Get a stack trace using the frame pointer registers (RBP on x86_64).
/// This function is only available if the compiler was configured to use frame pointers.
/// Using frame pointers to navigate up the call stack can only provide very basic information,
/// i.e., the frame pointer register value and the instruction pointer of the call site. 
/// 
/// For additional, fuller information about each stack frame, use the stack frame iterator
/// based on DWARF debug info.
/// 
/// # Arguments
/// * `current_page_table`: a reference to the active `PageTable`,
/// * `on_each_stack_frame`: the function that will be called for each stack frame in the call stack.
///   The function is called with two arguments: 
///   (1) the frame pointer register value and (2) the instruction pointer 
///   at that point in the call stack; the latter is useful for symbol resolution.
///   The function should return `true` if it wants to continue iterating up the call stack,
///   or `false` if it wants the iteration to stop.
/// 
#[cfg(frame_pointers)]
#[inline(never)]
pub fn stack_trace_using_frame_pointers(
    current_page_table: &PageTable,
    on_each_stack_frame: &mut dyn FnMut(usize, VirtualAddress) -> bool,
) -> Result<(), &'static str> {

    let mut rbp: usize;
    // SAFE: just reading current register value
    unsafe {
        asm!("" : "={rbp}"(rbp) : : "memory" : "intel", "volatile");
    }

    // set a recursion maximum of 64 stack frames
    for _i in 0..64 {
        // the stack contains the return address (of the caller) right before the current frame pointer
        if let Some(rip_ptr) = rbp.checked_add(core::mem::size_of::<usize>()) {
            if let (Ok(rbp_vaddr), Ok(rip_ptr)) = (VirtualAddress::new(rbp), VirtualAddress::new(rip_ptr)) {
                if current_page_table.translate(rbp_vaddr).is_some() && current_page_table.translate(rip_ptr).is_some() {
                    // SAFE: the address was checked above using page table walks
                    let rip = unsafe { *(rip_ptr.value() as *const usize) };
                    if rip == 0 {
                        return Ok(());
                    }
                    let rip = VirtualAddress::new(rip).map_err(|_| "instruction pointer value was an invalid virtual address")?;
                    let keep_going = on_each_stack_frame(rbp, rip);
                    if !keep_going { 
                        return Ok(());
                    }
                    
                    // move up the call stack to the previous frame
                    // SAFE: the address was checked above using page tables
                    rbp = unsafe { *(rbp as *const usize) };
                } else {
                    return Err("guard page");
                }
            } else {
                return Err("frame pointer value in RBP was an invalid virtual address");
            }
        } else {
            return Err("frame pointer value in RBP was too large and overflowed.");
        }
    }
    Err("reached maximum depth of 64 call stack frames")
}


// // snippet to get the current instruction pointer RIP, stack pointer RSP, and RBP
// let mut rbp: usize;
// let mut rsp: usize;
// let mut rip: usize;
// unsafe {
//     // On x86 you cannot directly read the value of the instruction pointer (RIP),
//     // so we use a trick that exploits RIP-relateive addressing to read the current value of RIP (also gets RBP and RSP)
//     asm!("lea $0, [rip]" : "=r"(rip), "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
// }
// debug!("register values: RIP: {:#X}, RSP: {:#X}, RBP: {:#X}", rip, rsp, rbp);
// let _curr_instruction_pointer = VirtualAddress::new_canonical(rip);
