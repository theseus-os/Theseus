//! Provides types and simple routines for handling panics.
//! This is similar to what's found in Rust's `core::panic` crate, 
//! but is much simpler and doesn't require lifetimes 
//! (although it does require alloc types like String).
//! 
#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mod_mgmt;
extern crate task;
extern crate unwind;
extern crate stack_trace;
extern crate stack_trace_frame_pointers;
extern crate fault_log;

use core::panic::PanicInfo;
// use alloc::string::String;
use memory::VirtualAddress;
use task::{KillReason, PanicInfoOwned};
use fault_log::log_panic_entry;

/// Performs the standard panic handling routine, which involves the following:
/// 
/// * Invoking the current `Task`'s `kill_handler` routine, if it has registered one.
/// * Printing a backtrace of the call stack.
/// * Finally, it performs stack unwinding of this `Task'`s stack and kills it.
/// 
/// Returns `Ok(())` if everything ran successfully, and `Err` otherwise.
pub fn panic_wrapper(panic_info: &PanicInfo) -> Result<(), &'static str> {
    trace!("at top of panic_wrapper: {:?}", panic_info);
    log_panic_entry (panic_info);
    // fault_log::print_fault_log();

    // print a stack trace
    let stack_trace_result = {
        // By default, we use DWARF-based debugging stack traces
        #[cfg(not(frame_pointers))] {
            error!("------------------ Stack Trace (DWARF) ---------------------------");
            stack_trace::stack_trace(
                &mut |stack_frame, stack_frame_iter| {
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
            let namespace = task::get_my_current_task()
                .map(|t| t.get_namespace())
                .or_else(|| mod_mgmt::get_initial_kernel_namespace())
                .ok_or("couldn't get current task's or default namespace")?;
            let mmi_ref = task::get_my_current_task()
                .map(|t| t.mmi.clone())
                .or_else(|| memory::get_kernel_mmi_ref())
                .ok_or("couldn't get current task's or default kernel MMI")?;
            let mmi = mmi_ref.lock();

            stack_trace_frame_pointers::stack_trace_using_frame_pointers(
                &mmi.page_table,
                &mut |_frame_pointer, instruction_pointer: VirtualAddress| {
                    let symbol_offset = namespace.get_section_containing_address(instruction_pointer, false)
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

    // Call this task's kill handler, if it has one.
    if let Some(ref kh_func) = task::take_kill_handler() {
        debug!("Found kill handler callback to invoke in Task {:?}", task::get_my_current_task());
        kh_func(&KillReason::Panic(PanicInfoOwned::from(panic_info)));
    } else {
        debug!("No kill handler callback in Task {:?}", task::get_my_current_task());
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
