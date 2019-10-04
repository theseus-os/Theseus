//! Provides types and simple routines for handling panics.
//! This is similar to what's found in Rust's `core::panic` crate, 
//! but is much simpler and doesn't require lifetimes 
//! (although it does require alloc types like String).
//! 
#![no_std]
#![feature(asm)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate task;
extern crate unwind;

use core::panic::PanicInfo;
use alloc::string::String;
use memory::{PageTable, VirtualAddress};
use task::{KillReason, PanicInfoOwned};

/// Performs the standard panic handling routine, which involves the following:
/// 
/// * Invoking the current `Task`'s `panic_handler` routine, if it has registered one.
/// * Printing a backtrace of the call stack.
/// * Finally, it performs stack unwinding of this `Task'`s stack and kills it.
/// 
/// Returns `Ok(())` if everything ran successfully, and `Err` otherwise.
pub fn panic_wrapper(panic_info: &PanicInfo) -> Result<(), &'static str> {
    trace!("at top of panic_wrapper: {:?}", panic_info);

    // print a stack trace
    {
        let curr_task = task::get_my_current_task().ok_or("get_my_current_task() failed")?;
        let namespace = curr_task.get_namespace();
        let (mmi_ref, app_crate_ref, _is_idle_task) = { 
            let t = curr_task.lock();
            (t.mmi.clone(), t.app_crate.clone(), t.is_an_idle_task)
        };

        // // dump some info about the loaded app crate
        // if let Some(ref app_crate) = app_crate_ref {
        //     let krate = app_crate.lock_as_ref();
        //     trace!("============== Crate {} =================", krate.crate_name);
        //     for s in krate.sections.values() {
        //         trace!("   {:?}", &*s.lock());
        //     }
        // }

        #[cfg(frame_pointers)]
        stack_trace_using_frame_pointer(
            &mmi_ref.lock().page_table,
            &|instruction_pointer: VirtualAddress| {
                namespace.get_section_containing_address(instruction_pointer, app_crate_ref.as_ref(), false)
                    .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
            },
        );
    }

    // Call this task's panic handler, if it has one.
    // Note that we must consume and drop the Task's panic handler BEFORE that Task can possibly be dropped.
    // This is because if the app sets a panic handler that is a closure/function in the text section of the app itself,
    // then after the app crate is released the panic handler will be dropped AFTER the app crate has been freed.
    // When it tries to drop the task's panic handler, causes a page fault because the text section of the app crate has been unmapped.
    {
        let panic_handler = task::get_my_current_task().and_then(|t| t.take_panic_handler());
        if let Some(ref ph_func) = panic_handler {
            debug!("Found panic handler callback to invoke in Task {:?}", task::get_my_current_task());
            ph_func(panic_info);
        }
        else {
            debug!("No panic handler callback in Task {:?}", task::get_my_current_task());
        }
    }

    // Start the unwinding process
    {
        let cause = KillReason::Panic(PanicInfoOwned::from(panic_info));
        match unwind::start_unwinding(cause) {
            Ok(_) => {
                warn!("BUG: start_unwinding() returned an Ok() value, which is unexpected because it means no unwinding actually occurred. Task: {:?}.", task::get_my_current_task());
                Ok(())
            }
            Err(e) => {
                error!("Task {:?} was unable to start unwinding procedure, error: {}.", task::get_my_current_task(), e);
                Err(e)
            }
        }
    }
    
    // if !is_idle_task {
    //     // kill the offending task (the current task)
    //     error!("Killing panicked task \"{}\"", curr_task.lock().name);
    //     curr_task.kill(KillReason::Panic(PanicInfoOwned::from(panic_info)))?;
    //     runqueue::remove_task_from_all(curr_task)?;
    //     Ok(())
    // }
    // else {
    //     Err("")
    // }
}




/// Get a stack trace using the frame pointer register (RBP on x86_64). 
/// If the compiler didn't emit frame pointers, then this function will not work.
/// 
/// This was adapted from Redox's stack trace implementation.
#[cfg(frame_pointers)]
#[inline(never)]
pub fn stack_trace_using_frame_pointer(
    current_page_table: &PageTable,
    addr_to_symbol: &dyn Fn(VirtualAddress) -> Option<(String, usize)>
) {
    // SAFETY: pointers are checked 
    // get the stack base pointer
    let mut rbp: usize;
    let mut rsp: usize;
    unsafe {
        asm!("" : "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
    }

    error!("STACK TRACE: RBP: {:>#018X}, RSP: {:>#018X}", rbp, rsp);
    if rbp == 0 {
        error!("Frame pointers have been omitted in this build. \
            Stack tracing/unwinding cannot be performed because we don't yet \
            support using DWARF .debug_* sections to backtrace the stack. \
            Make sure that the rustc option '-C force-frame-pointers=yes' is used."
        );
    }
    // set a recursion maximum of 64 stack frames
    for _frame in 0..64 {
        if let Some(rip_rbp) = rbp.checked_add(core::mem::size_of::<usize>()) {
            if let (Ok(rbp_vaddr), Ok(rip_rbp_vaddr)) = (VirtualAddress::new(rbp), VirtualAddress::new(rip_rbp)) {
                if current_page_table.translate(rbp_vaddr).is_some() && current_page_table.translate(rip_rbp_vaddr).is_some() {
                    let rip = unsafe { *(rip_rbp as *const usize) };
                    if rip == 0 {
                        error!("  {:>#018X}: BEGINNING OF STACK", rbp);
                        break;
                    }
                    if let Some((symbol_name, offset)) = addr_to_symbol(VirtualAddress::new_canonical(rip)) {
                        error!("  {:>#018X}: {:>#018X} in {} + {:#X}", rbp, rip, symbol_name, offset);
                    } else {
                        error!("  {:>#018X}: {:>#018X} in ??", rbp, rip);
                    }
                    // move up the call stack to the previous frame
                    rbp = unsafe { *(rbp as *const usize) };
                } else {
                    error!("  {:>#018X}: GUARD PAGE", rbp);
                    break;
                }
            } else {
                error!(" {:>#018X}: INVALID VIRTUAL ADDRESS in RBP", rbp);
                break;
            }
        } else {
            error!("  {:>#018X}: RBP OVERFLOW", rbp);
        }
    }
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
