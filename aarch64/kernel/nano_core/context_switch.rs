use core::mem::size_of;

use log::{info, error};

use memory::{MappedPages, PageTable};
use pte_flags::PteFlags;

use context_switch_regular::context_switch_regular;

static mut PREV_STACK: usize = 0xdeadbeef;

/// Sample Task Function
///
/// It simply re-triggers a context switch to return to the original main task.
pub extern "C" fn landing_pad() {
    let main_task_stack = unsafe { PREV_STACK };

    info!("[in landing_pad]");
    info!("main_task_stack: 0x{:x}", main_task_stack);
    info!("switching back to the main task");
    switch_to_task(main_task_stack);

    error!("This should never be printed");
    loop {}
}

/// Utility function to switch to another task
///
/// The task's stack pointer is required.
pub fn switch_to_task(new_stack: usize) {
    unsafe {
        let prev_stack_ptr = (&mut PREV_STACK) as *mut usize;
        context_switch_regular(prev_stack_ptr, new_stack);
    }
}

/// Utility function which allocates a 16-page long
/// initial task stack; you have to give it a function
/// pointer (such as the address of `landing_pad`)
/// which will be executed if you use
/// `context_switch_regular` with that new stack.
pub fn create_stack(
    page_table: &mut PageTable,
    start_address: usize,
    pages: usize,
) -> Result<(MappedPages, usize), &'static str> {
    // x30 stores the return address
    // it's used by the `ret` instruction
    let artificial_ctx_src = [
        // [lower address]
        // [returned stack pointer]

                    0, 0, //  x2,  x3,
                    0, 0, //  x4,  x5,
                    0, 0, //  x6,  x7,
                    0, 0, //  x8,  x9,
                    0, 0, // x10, x11,
                    0, 0, // x12, x13,
                    0, 0, // x14, x15,
                    0, 0, // x16, x17,
                    0, 0, // x18, x19,
                    0, 0, // x20, x21,
                    0, 0, // x22, x23,
                    0, 0, // x24, x25,
                    0, 0, // x26, x27,
                    0, 0, // x28, x29,
        start_address, 0, // x30, <nothing; kept for alignment>

        // [higher address]
        // [top of stack]
    ];

    let stack = page_allocator::allocate_pages(pages).ok_or("couldn't allocate new stack")?;
    let mut stack = page_table.map_allocated_pages(stack, PteFlags::WRITABLE | PteFlags::NOT_EXECUTABLE)?;

    let stack_ptr;
    {
        let stack: &mut [usize] = stack.as_slice_mut(0, pages * (4096 / size_of::<usize>()))?;

        // inserting an artificial SavedContext
        // at the top of the stack
        let offset = stack.len() - artificial_ctx_src.len();
        let artificial_ctx_dst = &mut stack[offset..];
        stack_ptr = artificial_ctx_dst.as_ptr() as usize;
        artificial_ctx_dst.copy_from_slice(artificial_ctx_src.as_slice());
    }

    Ok((stack, stack_ptr))
}