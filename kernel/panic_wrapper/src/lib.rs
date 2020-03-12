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
extern crate stack_trace;
extern crate stack_trace_frame_pointers;
extern crate fault_log;

use core::panic::PanicInfo;
// use alloc::string::String;
use memory::VirtualAddress;
use task::{KillReason, PanicInfoOwned};
use fault_log::add_panic_entry;

/// Performs the standard panic handling routine, which involves the following:
/// 
/// * Invoking the current `Task`'s `panic_handler` routine, if it has registered one.
/// * Printing a backtrace of the call stack.
/// * Finally, it performs stack unwinding of this `Task'`s stack and kills it.
/// 
/// Returns `Ok(())` if everything ran successfully, and `Err` otherwise.
pub fn panic_wrapper(panic_info: &PanicInfo) -> Result<(), &'static str> {
    trace!("at top of panic_wrapper: {:?}", panic_info);

    add_panic_entry();

    // print a stack trace
    let stack_trace_result = {
        // By default, we use DWARF-based debugging stack traces
        #[cfg(not(frame_pointers))] {
            error!("------------------ Stack Trace (DWARF) ---------------------------");
            stack_trace::stack_trace(
                &|stack_frame, stack_frame_iter| {
                    let symbol_offset = stack_frame_iter.namespace().get_section_containing_address(
                        VirtualAddress::new_canonical(stack_frame.call_site_address() as usize),
                        false
                    ).map(|(sec, offset)| (sec.name.clone(), offset));
                    if let Some((symbol_name, offset)) = symbol_offset {
                        error!("  {:>#018X} in {} + {:#X}", stack_frame.call_site_address(), symbol_name, offset);
                    } else {
                        error!("  {:>#018X} in ??", stack_frame.call_site_address());
                    }
                    true
                },
                None,
            )
        }
        #[cfg(frame_pointers)] {
            error!("------------------ Stack Trace (frame pointers) ------------------");
            let curr_task = task::get_my_current_task().ok_or("get_my_current_task() failed")?;
            let namespace = curr_task.get_namespace();
            let (mmi_ref, app_crate_ref) = { 
                let t = curr_task.lock();
                (t.mmi.clone(), t.app_crate.clone())
            };
            let mmi = mmi_ref.lock();

            use core::ops::Deref;
            stack_trace_frame_pointers::stack_trace_using_frame_pointers(
                &mmi.page_table,
                &mut |_frame_pointer, instruction_pointer: VirtualAddress| {
                    let symbol_offset = namespace.get_section_containing_address(instruction_pointer, app_crate_ref.as_deref().map(|acr| acr.deref()), false)
                        .map(|(sec, offset)| (sec.name.clone(), offset));
                    if let Some((symbol_name, offset)) = symbol_offset {
                        error!("  {:>#018X} in {} + {:#X}", instruction_pointer, symbol_name, offset);
                    } else {
                        error!("  {:>#018X} in ??", instruction_pointer);
                    }
                    true
                },
                None,
            )
        }
    };
    match stack_trace_result {
        Ok(()) => error!("  Beginning of stack"),
        Err(e) => error!("  {}", e),
    }
    error!("------------------------------------------------------------------");

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
        match unwind::start_unwinding(cause, 5) {
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
