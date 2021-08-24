//! Abstractions for deferred interrupt handler tasks.
//!
//! Deferred interrupt handler tasks are similar to the concept of
//! "top half" and "bottom half" interrupt handlers in other OSes,
//! in which the top half is the short, latency-sensitive function
//! that runs immediately when the interrupt request is serviced,
//! while the bottom half is the more complex function that runs 
//! in a deferred manner to handle longer operations.
//! Other terminology is often used, including "first-level" and
//! "second-level" interrupt handlers, or "hard" and "soft" interrupt handlers.
//!
//! That being said, this implementation of deferred interrupt handler tasks
//! differs from both tasklets and workqueues in Linux.
//! We also do not use the "top half" or "bottom half" terminology
//! because it is confusing and difficult to remember which is which.
//! Instead, we refer to the first latency-sensitive part as the
//! *interrupt handler* and the second later part as the *deferred task*.
//! The "interrupt handler" runs immediately (in a synchronous fashion) 
//! when the interrupt occurs, while the deferred task runs asynchronously
//! at some time in the future, ideally as soon as possible.
//! 
//! The general idea is that an interrupt handler should be short
//! and do the minimum amount of work possible in order to keep 
//! the system responsive, because all (or most) other interrupts
//! are typically disabled while the interrupt handler executes to completion.
//! Thus, most of the work should be deferred until later, such that the
//! interrupt handler itself only does a couple of quick things:
//!  * Notifies the deferred task that work is ready to be done,
//!    optionally providing details about what work it needs to do,
//!  * Acknowledges the interrupt such that the hardware knows it was handled.  
//!
//! The deferred handler task is tied directly to a single interrupt handler 
//! in a 1-to-1 manner at the time of creation. 
//! Therefore, it is both efficient and easy to use. 
//! In the simplest of cases, such as a serial port device, the interrupt handler
//! only needs to mark the deferred task as unblocked (runnable)
//! and then acknowledge the interrupt. 
//! No other data exchange is needed between the interrupt handler and the 
//! deferred task.
//! For more complicated cases, the interrupt handler may need to do a minimal
//! amount of bookkeeping tasks (such as advancing a ringbuffer index)
//! and potentially send some information about what the deferred task should do.
//! It is typically best to use a lock-free queue or an interrupt-safe mutex
//! to share such information between the interrupt handler and deferred task.
//!

#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(never_type)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate mpmc;
extern crate x86_64;
extern crate task;
extern crate spawn;
extern crate scheduler;
#[macro_use] extern crate debugit;
extern crate async_channel;
extern crate interrupts;


use alloc::string::String;
use task::{TaskRef, get_my_current_task};
use x86_64::structures::idt::{ExceptionStackFrame};



pub type InterruptHandlerFunction = x86_64::structures::idt::HandlerFunc;


pub enum InterruptRegistrationError {
    AlreadyTaken {
        irq: u8,
        existing_handler_address: u64
    },
    SpawnFail(&'static str),
}


/// Registers an interrupt handler and a deferred task to handle
/// the longer-running tasks related to that interrupt in an asynchronous context.
///
/// The function fails if the interrupt number is already in use. 
/// 
/// # Arguments 
/// * `interrupt_num`: the interrupt number (IRQ vector) that is being requested.
/// * `interrupt_handler`: the handler to be registered,
///    which will be invoked when the interrupt occurs.
/// * `deferred_interrupt_action`: the closure/function callback that will be invoked
///    in an asynchronous manner after the `interrupt_handler` runs. 
/// * `deferred_action_argument`: the argument that will be passed to the above
///    `deferred_interrupt_action` function.
/// 
/// TODO: further document how the `deferred_interrupt_action` is actually invoked in a loop
///       and what happens in between each invocation.
///       Essentially, the returned `Task` runs an infinite loop that will sleep until notified
///       by the given `interrupt_handler`. 
///       When woken up, the task loop invokes the `deferred_interrupt_action`
///       and then puts itself back to sleep (blocking itself) when the `deferred_interrupt_action`
///       is complete and returns. 
///       This avoids the need for the `deferred_interrupt_action` to manually handle repeated calls
///       in and amongst the sleep/wake behavior.
///
///
/// It is the caller's responsibility to notify or otherwise wake up the deferred interrupt task
/// in the given `interrupt_handler` (or elsewhere, arbitrarily). 
/// WIthout doing this, the `deferred_interrupt_action` will never be invoked.
/// The returned [`TaskRef`] is useful for doing this, as you can `unblock` it when it needs to run,
/// e.g., when an interrupt has occurred.
///
/// # Return
/// * `Ok(TaskRef)` if successfully registered, in which the returned task is the long-running
///    loop that repeatedly invokes the given `deferred_interrupt_action`.
/// * `Err(existing_handler_address)` if the given `interrupt_num` was already in use.
pub fn register_interrupt_handler<DIA, Arg, Success, Failure, S>(
    interrupt_number: u8,
    interrupt_handler: InterruptHandlerFunction,
    deferred_interrupt_action: DIA,
    deferred_action_argument: Arg,
    deferred_task_name: Option<S>,
) -> Result<TaskRef, InterruptRegistrationError> 
    where DIA: Fn(&Arg) -> Result<Success, Failure> + Send + 'static,
          Arg: Send + 'static,
          S: Into<String>,
{
    // First, attempt to register the interrupt handler.
    interrupts::register_interrupt(interrupt_number, interrupt_handler)
        .map_err(|existing_handler_address| {
            error!("Interrupt number {:#X} was already taken by handler at {:#X}! Sharing IRQs is currently unsupported.",
                interrupt_number, existing_handler_address
            );
            InterruptRegistrationError::AlreadyTaken {
                irq: interrupt_number,
                existing_handler_address,
            }
        })?;

    // Spawn the deferred task, which should be initially blocked from running.
    // It will be unblocked by the interrupt handler whenever it needs to run.
    let mut tb = spawn::new_task_builder(
        deferred_task_entry_point::<DIA, Arg, Success, Failure>,
        (deferred_interrupt_action, deferred_action_argument),
    ).block();
    if let Some(name) = deferred_task_name {
        tb = tb.name(name.into());
    }
    tb.spawn().map_err(|e| InterruptRegistrationError::SpawnFail(e))
}



/// The entry point for a new deferred interrupt task.
///
/// Note: we could use restartable tasks for this, but the current requirement
/// of the function itself and its arguments being `Clone`able may be overly restrictive. 
fn deferred_task_entry_point<DIA, Arg, Success, Failure>(
    (deferred_interrupt_action, deferred_action_argument): (DIA, Arg),
) -> ! 
    where DIA: Fn(&Arg) -> Result<Success, Failure>,
          Arg: Send + 'static,
{
    let curr_task = get_my_current_task().expect("BUG: deferred_task_entry_point: couldn't get current task.");
    trace!("Entered {:?}:\n\t action: {:?}\n\t arg:    {:?}", 
        curr_task.name, debugit!(deferred_interrupt_action), debugit!(deferred_action_argument)
    );

    loop {
        let _res = deferred_interrupt_action(&deferred_action_argument);
        // Note: here, upon failure, we could return from this loop task entirely instead of just logging the error.
        // Or, we could accept a boolean/cfg that determines whether we should bail or continue looping.
        
        // match _res {
        //     Ok(success) => debug!("Deferred interrupt action returned success: {:?}", debugit!(success)),
        //     Err(failure) => error!("Deferred interrupt action returned failure: {:?}", debugit!(failure)),
        // }
        
        curr_task.block();
        scheduler::schedule();
    }
}


extern "x86-interrupt" fn test_interrupt_handler<const IRQ: u8>(_stack_frame: &mut ExceptionStackFrame) {
    trace!("top of test_interrupt_handler for IRQ {:#X}", IRQ);
    
}
