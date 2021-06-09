use alloc_cortex_m::CortexMHeap;

#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

#[alloc_error_handler]
fn default_alloc_error_handler(_: core::alloc::Layout) -> ! {
    loop {}
}

pub fn init_memory_management(heap_start: usize, heap_end: usize) {
    // let heap_start_low_bound = cortex_m_rt::heap_start() as usize;
    // let heap_start = kernel_config::memory::KERNEL_HEAP_START;
    // let heap_end = heap_start + kernel_config::memory::KERNEL_HEAP_INITIAL_SIZE;
    // assert!(heap_start >= heap_start_low_bound);
    let heap_size = heap_end - heap_start;
    unsafe { ALLOCATOR.init(heap_start, heap_size); }
}
