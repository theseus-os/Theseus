use memory::{EntryFlags, allocate_pages_by_bytes, get_kernel_mmi_ref};
use stack::Stack;
use spawn;

pub fn init() -> ! {
    let kernel_mmi_ref = get_kernel_mmi_ref().unwrap();
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let stack_pages = allocate_pages_by_bytes(512).unwrap();
    let mapped_stack_pages = kernel_mmi.page_table.map_allocated_pages(stack_pages, EntryFlags::WRITABLE).unwrap();
    let stack = Stack::from_pages(mapped_stack_pages).unwrap();

    let _bootstrap_task = spawn::init(0, stack).unwrap();

    spawn::create_idle_task(Some(0)).unwrap();

    loop {}
}
