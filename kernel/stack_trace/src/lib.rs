//! Stack trace (backtrace) functionality using DWARF debugging information.
//! 
//! There are two main ways of obtaining stack traces:
//! 1. Using the frame pointer register to find the previous stack frame.
//! 2. Using DWARF debugging information to understand the layout of each stack frame.
//! 
//! We support both ways, but prefer #2 because it doesn't suffer from the
//! compatibility and performance drawbacks of #1. 
//! See the `stack_trace_frame_pointers` crate for the #1 functionality. 
//! 
//! This crate offers support for #2. 
//! The advantage of using this is that it should always be possible
//! regardless of how the compiler was configured.
//! However, this crate requires more dependencies, and if object files have been 
//! specifically stripped of DWARF info, then we won't be able to recurse up the call stack.
//! 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate mod_mgmt;
extern crate task;
extern crate unwind;
extern crate fallible_iterator;

pub use mod_mgmt::{CrateNamespace, StrongSectionRef};
pub use task::get_my_current_task;

use unwind::{StackFrame, StackFrameIter};
use fallible_iterator::FallibleIterator;


/// Get a stack trace using the default stack tracer based on DWARF debug info. 
/// 
/// # Arguments
/// * `on_each_stack_frame`: (a mutable reference to) the function that will be called 
///   for each stack frame in the call stack.
/// 
///   The function is called with two arguments: 
///   1. a `StackFrame` instance that contains information about that frame, 
///   2. a reference to the current `StackFrameIter`, which can be used to obtain
///   register values that existed at this frame in the call stack.
/// 
///   The function should return `true` if it wants to continue iterating up the call stack,
///   or `false` if it wants the iteration to stop.
/// * `max_recursion`: an optional maximum number of stack frames to recurse up the call stack.
/// 
/// # Examples
/// Typical usage would involve using the stack frame's call site address to print out 
/// a standard backtrace of the call stack, as such:
/// ```
/// stack_trace(
///     &mut |stack_frame, _stack_frame_iter| {
///         println!("{:>#018X}", stack_frame.call_site_address());
///         true // keep iterating
///     },
///     None,
/// );
/// ```
#[inline(never)]
pub fn stack_trace(
    on_each_stack_frame: &mut dyn FnMut(StackFrame, &StackFrameIter) -> bool,
    max_recursion: Option<usize>,
) -> Result<(), &'static str> {
    let max_recursion = max_recursion.unwrap_or(usize::MAX);

    unwind::invoke_with_current_registers(&mut |registers| {
        let namespace = task::get_my_current_task()
            .map(|t| t.get_namespace())
            .or_else(|| mod_mgmt::get_initial_kernel_namespace())
            .ok_or("couldn't get current task's or default namespace")?;
        let mut stack_frame_iter = StackFrameIter::new(namespace.clone(), registers);

        // iterate over each frame in the call stack
        let mut i = 0;
        while let Some(frame) = stack_frame_iter.next()? {
            let keep_going = on_each_stack_frame(frame, &stack_frame_iter);
            if !keep_going {
                return Ok(());
            }
            i += 1;
            if i == max_recursion {
                trace!("stack_trace(): reached maximum recursion depth of {} stack frames", max_recursion);
                return Err("reached recursion limit for stack frames");
            }
        }
        Ok(())
    })
}
