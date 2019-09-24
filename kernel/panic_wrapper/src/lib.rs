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
extern crate apic;
extern crate task;
extern crate runqueue;

use core::panic::PanicInfo;
use alloc::string::String;
use memory::{PageTable, VirtualAddress};
use task::{KillReason, PanicInfoOwned};

/// Performs the standard panic handling routine, which involves the following:
/// 
/// * Printing a basic panic message.
/// * Getting the current `Task`.
/// * Printing a backtrace of the call stack.
/// * Invoking the current `Task`'s `panic_handler` routine, if it has registered one.
/// * If there is no registered panic handler, then it prints a standard message.
/// * Finally, it kills the panicked `Task`.
/// 
pub fn panic_wrapper(panic_info: &PanicInfo) -> Result<(), &'static str> {
    // Stuff for unwinding here, nothing working yet
    // let curr_task_locked = curr_task.lock();
    // let my_crate = curr_task_locked.app_crate.as_ref().expect("unwind_test's app_crate was None");
    // for sec_ref in my_crate.lock_as_ref().sections.values() {
    //     let sec = sec_ref.lock();
    //     trace!("    section {:?}, vaddr: {:#X}, size: {:#X}", sec.name, sec.virt_addr(), sec.size);
    // }
    // namespace.handle_eh_frame(&my_crate, true).expect("handle_eh_frame failed");


    trace!("at top of panic_wrapper: {:?}", panic_info);

    let apic_id = apic::get_my_apic_id();

    // get current task to see if it has a panic_handler
    let curr_task = task::get_my_current_task().ok_or("get_my_current_task() failed")?;
    let namespace = curr_task.get_namespace();
    let (mmi_ref, app_crate_ref, is_idle_task) = { 
        let t = curr_task.lock();
        (t.mmi.clone(), t.app_crate.clone(), t.is_an_idle_task)
    };
    // We should ensure that the lock on the curr_task isn't held here,
    // in order to allow the panic handler and other functions below to acquire it. 

    // print a stack trace
    stack_trace(
        &mmi_ref.lock().page_table,
        &|instruction_pointer: VirtualAddress| {
            namespace.get_containing_section(instruction_pointer, app_crate_ref.as_ref())
                .map(|(sec_ref, offset)| (sec_ref.lock().name.clone(), offset))
        },
    );
    
    // call this task's panic handler, if it has one. 
    let panic_handler = curr_task.take_panic_handler();
    if let Some(ref ph_func) = panic_handler {
        ph_func(&PanicInfoOwned::from(panic_info));
        error!("PANIC handled in task \"{}\" on core {:?}: {}", curr_task.lock().name, apic_id, panic_info);
    }
    else {
        error!("PANIC was unhandled in task \"{}\" on core {:?} at {}", curr_task.lock().name, apic_id, panic_info);
        // memory::stack_trace();
    }

    if !is_idle_task {
        // kill the offending task (the current task)
        error!("Killing panicked task \"{}\"", curr_task.lock().name);
        curr_task.kill(KillReason::Panic(PanicInfoOwned::from(panic_info)))?;
        runqueue::remove_task_from_all(curr_task)?;
        Ok(())
    }
    else {
        Err("")
    }
}




/// Get a stack trace, borrowed from Redox
#[inline(never)]
pub fn stack_trace(
    current_page_table: &PageTable,
    addr_to_symbol: &dyn Fn(VirtualAddress) -> Option<(String, usize)>) 
{
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
