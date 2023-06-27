//! Provides types and simple routines for handling panics.
//! This is similar to what's found in Rust's `core::panic` crate, 
//! but is much simpler and doesn't require lifetimes 
//! (although it does require alloc types like String).
//! 
#![no_std]

extern crate alloc;

use core::panic::PanicInfo;
use log::{debug, trace};
use fault_log::log_panic_entry;
use task::{KillReason, PanicInfoOwned};

#[cfg(target_arch = "x86_64")]
use log::{error, warn};

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

    // Print a stack trace. Not yet supported on aarch64
    #[cfg(target_arch = "x86_64")] {
    let stack_trace_result = {
        // By default, we use DWARF-based debugging stack traces
        #[cfg(not(frame_pointers))] {
            error!("------------------ Stack Trace (DWARF) ---------------------------");
            stack_trace::stack_trace(
                &mut |stack_frame, stack_frame_iter| {
                    let symbol_offset = stack_frame_iter.namespace().get_section_containing_address(
                        memory::VirtualAddress::new_canonical(stack_frame.call_site_address() as usize),
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
            let (namespace, mmi_ref) = match task::with_current_task(|t|
                (t.get_namespace().clone(), t.mmi.clone())
            ) {
                Ok((ns, mmi)) => (ns, mmi),
                Err(_) => (
                    mod_mgmt::get_initial_kernel_namespace()
                        .ok_or("couldn't get current task's or default namespace")?
                        .clone(),
                    memory::get_kernel_mmi_ref()
                        .ok_or("couldn't get current task's or default kernel MMI")?
                        .clone(),
                )
            };
            let mmi = mmi_ref.lock();
            stack_trace_frame_pointers::stack_trace_using_frame_pointers(
                &mmi.page_table,
                &mut |_frame_pointer, instruction_pointer: memory::VirtualAddress| {
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
    }

    // Call this task's kill handler, if it has one.
    if let Some(ref kh_func) = task::take_kill_handler() {
        debug!("Found kill handler callback to invoke in Task {:?}", task::get_my_current_task());
        kh_func(&KillReason::Panic(PanicInfoOwned::from(panic_info)));
    } else {
        debug!("No kill handler callback in Task {:?}", task::get_my_current_task());
    }

    // Start the unwinding process. Not yet supported on aarch64
    #[cfg(not(target_arch = "x86_64"))] {
        Err("Unwinding is currently only supported on x86_64")
    }
    #[cfg(target_arch = "x86_64")]
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
}
