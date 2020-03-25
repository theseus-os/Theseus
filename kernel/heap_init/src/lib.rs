#![no_std]

#[macro_use] extern crate log;
extern crate heap_irq_safe;
#[macro_use] extern crate memory;
extern crate kernel_config;
extern crate irq_safety;

use heap_irq_safe::init;
use memory::{
    MappedPages, PageRange, VirtualAddress, VirtualMemoryArea, AreaFrameAllocator, PageTable, EntryFlags
};
use kernel_config::memory::{KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START};
use irq_safety::MutexIrqSafe;
use core::ops::DerefMut;

pub fn map_heap_pages(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable, heap_start: usize, heap_initial_size: usize) 
-> Result<(MappedPages, VirtualMemoryArea), &'static str> 
{
    let mut allocator = allocator_mutex.lock();

    let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(heap_start), heap_initial_size);
    let heap_flags = EntryFlags::WRITABLE;
    let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(VirtualAddress::new_canonical(heap_start), heap_initial_size, heap_flags, "Kernel Heap");
    let heap_mp = page_table.map_pages(pages, heap_flags, allocator.deref_mut())?;

    Ok((heap_mp, heap_vma))
}

pub fn initialize_heap(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable) -> Result<(MappedPages, VirtualMemoryArea), &'static str> {
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE;
    let (heap_mapped_pages, heap_vma) = map_heap_pages(allocator_mutex, page_table, heap_start, heap_initial_size)?;
    heap_irq_safe::init(heap_start, heap_initial_size);

    debug!("mapped and initialized the heap, VMA: {:?}", heap_vma);
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    Ok((heap_mapped_pages, heap_vma))
}
