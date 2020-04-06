//! The heap initialization functions, both for the initial heap and the multiple heaps.

#![feature(const_in_array_repeat_expressions)]
#![no_std]

#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate irq_safety;
extern crate heap;
extern crate apic;
extern crate multiple_heaps;


use memory::{AreaFrameAllocator, PageTable};
use kernel_config::memory::{KERNEL_HEAP_INITIAL_SIZE_PAGES, KERNEL_HEAP_START};
use irq_safety::MutexIrqSafe;
use apic::get_lapics;


/// Creates and initializes the first system heap which will be used until the multiple heaps are initialized.
pub fn initialize_heap(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable) -> Result<(), &'static str> {
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE_PAGES;
    
    // actual heap initialization occurs here
    heap::init_initial_allocator(allocator_mutex, page_table, heap_start, heap_initial_size)?; 

    debug!("mapped and initialized the initial heap");
    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs

    Ok(())
}

/// Completes the final steps needed to switch over to using multiple heaps.
/// Redirects the heap allocation and deallocation functions to the multiple heaps, and merges the initial heap into
/// one of the new heaps. After this point the initial heap is empty and will not be used again.
pub fn multiple_heaps_ready_to_use() -> Result<(), &'static str> {
    heap::set_alloc_function(multiple_heaps::allocate);
    heap::set_dealloc_function(multiple_heaps::deallocate);

    let initial_heap = heap::remove_initial_allocator();
    multiple_heaps::merge_initial_heap(initial_heap)
}

/// Initializes the multiple heaps using the apic id as the key, which is mapped to a heap id.
/// If we want to change the value the heap id is based on, we would substitute 
/// the lapic iterator with an iterator containing the desired keys.
pub fn initialize_multiple_heaps () -> Result<(), &'static str> {
    let lapics = get_lapics();

    for lapic in lapics.iter() {
        let lapic = lapic.1;
        let apic_id = lapic.read().apic_id;
        multiple_heaps::init_individual_heap(apic_id as usize)?;
    }

    Ok(())       
}

