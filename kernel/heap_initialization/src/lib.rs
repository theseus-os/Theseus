#![no_std]

#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate irq_safety;
extern crate multiple_heaps;

use memory::{
    MappedPages, PageRange, VirtualAddress, AreaFrameAllocator, PageTable, EntryFlags
};
use kernel_config::memory::{KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START};
use irq_safety::MutexIrqSafe;
use core::ops::DerefMut;


pub fn initialize_heap(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable) -> Result<MappedPages, &'static str> {
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE;

    // map the heap pages
    let heap_mapped_pages = map_heap_pages(allocator_mutex, page_table, heap_start, heap_initial_size)?;
    
    // actual heap initialization occurs here
    // this is where we would switch out the heap implementation 
    multiple_heaps::init(heap_start, heap_initial_size)?; 

    debug!("mapped and initialized the heap");
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    Ok(heap_mapped_pages)
}


pub fn map_heap_pages(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable, heap_start: usize, heap_initial_size: usize) 
-> Result<MappedPages, &'static str> 
{
    let mut allocator = allocator_mutex.lock();

    let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(heap_start), heap_initial_size);
    let heap_flags = EntryFlags::WRITABLE;
    let heap_mp = page_table.map_pages(pages, heap_flags, allocator.deref_mut())?;

    Ok(heap_mp)
}


