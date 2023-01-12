use log::{info, error};
use core::mem::size_of;

use memory::{MappedPages, PageTable};
use kernel_config::memory::PAGE_SIZE;
use pte_flags::PteFlags;

use context_switch_regular::{context_switch_regular, ContextRegular};

static mut PREV_STACK: usize = 0xdeadbeef;

/// Sample Task Function
///
/// It simply re-triggers a context switch to return to the original main task.
pub extern "C" fn task_entrypoint() {
    let main_task_stack = unsafe { PREV_STACK };

    info!("[in task_entrypoint]");
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

/// Utility function which allocates an initial task
/// stack; you have to give it a function
/// pointer (such as the address of `task_entrypoint`)
/// which will be executed if you use
/// `context_switch_regular` with that new stack.
pub fn create_stack(
    page_table: &mut PageTable,
    start_address: extern "C" fn(),
    pages: usize,
) -> Result<(MappedPages, usize), &'static str> {
    let artificial_ctx_src = ContextRegular::new(start_address as *const () as _);

    let stack = page_allocator::allocate_pages(pages).ok_or("couldn't allocate new stack")?;
    let mut stack = page_table.map_allocated_pages(stack, PteFlags::WRITABLE | PteFlags::NOT_EXECUTABLE)?;

    let stack_ptr = {
        // inserting an artificial context
        // at the top of the stack
        let offset = pages * PAGE_SIZE - size_of::<ContextRegular>();
        let artificial_ctx_dst = stack.as_type_mut(offset)?;
        *artificial_ctx_dst = artificial_ctx_src;
        artificial_ctx_dst as *mut _ as usize
    };

    Ok((stack, stack_ptr))
}
