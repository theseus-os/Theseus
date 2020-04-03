//! The heap initialization functions, both for the initial heap and the per-core heaps.

#![feature(const_in_array_repeat_expressions)]
#![no_std]

#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate irq_safety;
extern crate multiple_heaps;
extern crate apic;

use memory::{
    MappedPages, PageRange, VirtualAddress, AreaFrameAllocator, PageTable
};
use kernel_config::memory::{KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START, MAX_HEAPS};
use irq_safety::MutexIrqSafe;
use core::ops::DerefMut;
use multiple_heaps::{HEAP_FLAGS};
use apic::max_apic_id;


/// Initializes the heap with the inital kernel heap size
pub fn initialize_heap(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable) -> Result<MappedPages, &'static str> {
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE;

    // map the heap pages
    let mut allocator = allocator_mutex.lock();
    let pages = PageRange::from_virt_addr(VirtualAddress::new_canonical(heap_start), heap_initial_size);
    let heap_mp = page_table.map_pages(pages, HEAP_FLAGS, allocator.deref_mut())?;
    
    // actual heap initialization occurs here
    multiple_heaps::init_initial_allocator(heap_start, heap_initial_size)?; 

    debug!("mapped and initialized the heap");
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    Ok(heap_mp)
}


pub fn initialize_per_core_heaps () -> Result<(), &'static str> {
    let max_apic_id = max_apic_id()? as usize;
    assert!(MAX_HEAPS >= max_apic_id);

    for id in 0..=max_apic_id {
        multiple_heaps::init_per_core_heap(id)?;
    }
    Ok(())       
}

