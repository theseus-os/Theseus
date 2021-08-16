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

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate mpmc;
extern crate x86_64;
extern crate task;
extern crate spawn;
extern crate async_channel;
extern crate interrupts;


use alloc::string::String;
use task::TaskRef;
use x86_64::structures::idt::{Idt, LockedIdt, ExceptionStackFrame, HandlerFunc};



pub type InterruptHandlerFunction = x86_64::structures::idt::HandlerFunc;


pub fn register_interrupt_handler<DF, S>(
    interrupt_number: u8,
    interrupt_handler_function: InterruptHandlerFunction,
    deferred_task_function: DF,
    deferred_task_name: Option<S>,
) -> Result<TaskRef, &'static str> 
    where DF: Fn(()),
          S: Into<String>,
{
    // First, attempt to register the interrupt handler.
    interrupts::register_interrupt(interrupt_number, interrupt_handler_function)?;

    // Spawn the deferred task, which should be initially blocked from running.
    // It will be unblocked by the interrupt handler whenever it needs to run.
    let mut tb = spawn::new_task_builder(deferred_task_function, ())
        .block();
    if let Some(name) = deferred_task_name {
        tb = tb.name(name.into());
    }
    tb.spawn()
}



/*
mod test {
    use crate::*;

    static 

    pub fn test() {

    }


    pub extern "x86-interrupt" fn test_interrupt_handler<F, R>(_stack_frame: &mut ExceptionStackFrame) 
        where F: Fn() -> Option<R>
    {


        debug!("Hello from test_interrupt_handler");
    }
}
*/
