//! Stack trace (backtrace) functionality using frame pointers.
//! 
//! There are two main ways of obtaining stack traces:
//! 1. Using the frame pointer register to find the previous stack frame.
//! 2. Using DWARF debugging information to understand the layout of each stack frame.
//! 
//! We support both ways, but prefer #2 because it doesn't suffer from the
//! compatibility and performance drawbacks of #1. 
//! See the `stack_trace` crate for the #2 functionality. 
//! 
//! This crate offers support for #1. 
//! The advantage of using this is that it doesn't require any significant dependencies.
//! However, this crate's frame pointer-based stack traces are only available
//! when the compiler has been configured to emit frame pointers,
//! which in Rust is achieved via the `-C force-frame-pointers=yes` rust flags option.

#![no_std]

// This entire crate depends upon the `frame_pointers` config option.
#[macro_use] extern crate cfg_if;
cfg_if! { if #[cfg(frame_pointers)] {

extern crate alloc;
extern crate memory;

use memory::{PageTable, VirtualAddress};


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
/// * `max_recursion`: an optional maximum number of stack frames to recurse up the call stack.
///   If not provided, the default maximum will be `64` call stack frames.
/// 
#[inline(never)]
pub fn stack_trace_using_frame_pointers(
    current_page_table: &PageTable,
    on_each_stack_frame: &mut dyn FnMut(usize, VirtualAddress) -> bool,
    max_recursion: Option<usize>,
) -> Result<(), &'static str> {

    let mut rbp: usize;
    // SAFE: just reading current register value
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
    }

    for _i in 0 .. max_recursion.unwrap_or(64) {
        // the stack contains the return address (of the caller) right before the current frame pointer
        if let Some(rip_ptr) = rbp.checked_add(core::mem::size_of::<usize>()) {
            if let (Some(rbp_vaddr), Some(rip_ptr)) = (VirtualAddress::new(rbp), VirtualAddress::new(rip_ptr)) {
                if current_page_table.translate(rbp_vaddr).is_some() && current_page_table.translate(rip_ptr).is_some() {
                    // SAFE: the address was checked above using page table walks
                    let rip = unsafe { *(rip_ptr.value() as *const usize) };
                    if rip == 0 {
                        return Ok(());
                    }
                    let rip = VirtualAddress::new(rip).ok_or("instruction pointer value was an invalid virtual address")?;
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
    Err("reached maximum recursion depth of call stack frames")
}


// // snippet to get the current instruction pointer RIP, stack pointer RSP, and RBP
// let mut rbp: usize;
// let mut rsp: usize;
// let mut rip: usize;
// unsafe {
//     // On x86 you cannot directly read the value of the instruction pointer (RIP),
//     // so we use a trick that exploits RIP-relateive addressing to read the current value of RIP (also gets RBP and RSP)
//     llvm_asm!("lea $0, [rip]" : "=r"(rip), "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
// }
// debug!("register values: RIP: {:#X}, RSP: {:#X}, RBP: {:#X}", rip, rsp, rbp);
// let _curr_instruction_pointer = VirtualAddress::new_canonical(rip);

}} // end of cfg_if block
