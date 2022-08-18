//! Support for broadcasting and handling TLB shootdown IPIs. 

#![no_std]

// #[macro_use] extern crate log;
extern crate irq_safety;
extern crate memory;
extern crate apic;
extern crate x86_64;
extern crate pause;


use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use irq_safety::{hold_interrupts, RwLockIrqSafe};
use memory::PageRange;
use apic::{LocalApic, get_my_apic, core_count, LapicIpiDestination};
use pause::spin_loop_hint;


/// The number of remaining cores that still need to handle the curerent TLB shootdown IPI
pub static TLB_SHOOTDOWN_IPI_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The lock that makes sure only one set of TLB shootdown IPIs is concurrently happening
pub static TLB_SHOOTDOWN_IPI_LOCK: AtomicBool = AtomicBool::new(false);
/// The range of pages for a TLB shootdown IPI.
pub static TLB_SHOOTDOWN_IPI_PAGES: RwLockIrqSafe<Option<PageRange>> = RwLockIrqSafe::new(None);


/// Initializes data, functions, and structures for the TLB shootdown. 
/// TODO: redesign this, it's weird and silly just to set one callback.
pub fn init() {
    memory::set_broadcast_tlb_shootdown_cb(broadcast_tlb_shootdown);
}


/// Broadcasts TLB shootdown IPI to all other AP cores.
/// Do not invoke this directly, but rather pass it as a callback to the memory subsystem,
/// which will invoke it as needed (on remap/unmap operations).
fn broadcast_tlb_shootdown(pages_to_invalidate: PageRange) {
    if let Some(my_lapic) = get_my_apic() {
        // info!("broadcast_tlb_shootdown():  AP {}, vaddrs: {:?}", my_lapic.read().apic_id, virtual_addresses);
        send_tlb_shootdown_ipi(&mut my_lapic.write(), pages_to_invalidate);
    }
}


/// Handles a TLB shootdown ipi by flushing the `VirtualAddress`es 
/// covered by the given range of `pages_to_invalidate`.
/// 
/// There is no need to invoke this directly, it will be called by an IPI interrupt handler.
pub fn handle_tlb_shootdown_ipi(pages_to_invalidate: PageRange) {
    // trace!("handle_tlb_shootdown_ipi(): AP {}, pages: {:?}", apic::get_my_apic_id(), pages_to_invalidate);

    for page in pages_to_invalidate {
        x86_64::instructions::tlb::flush(x86_64::VirtAddr::new(page.start_address().value() as u64));
    }
    TLB_SHOOTDOWN_IPI_COUNT.fetch_sub(1, Ordering::SeqCst);
}


/// Sends an IPI to all other cores (except me) to trigger 
/// a TLB flush of the given pages' virtual addresses.
pub fn send_tlb_shootdown_ipi(my_lapic: &mut LocalApic, pages_to_invalidate: PageRange) {        
    // skip sending IPIs if there are no other cores running
    let core_count = core_count();
    if core_count <= 1 {
        return;
    }

    // trace!("send_tlb_shootdown_ipi(): from AP {}, core_count: {}, {:?}", my_lapic.apic_id, core_count, pages_to_invalidate);

    // interrupts must be disabled here, because this IPI sequence must be fully synchronous with other cores,
    // and we wouldn't want this core to be interrupted while coordinating IPI responses across multiple cores.
    let _held_ints = hold_interrupts(); 

    // acquire lock
    // TODO: add timeout!!
    let mut old_lock_val = TLB_SHOOTDOWN_IPI_LOCK.load(Ordering::Relaxed);
    loop {
        match TLB_SHOOTDOWN_IPI_LOCK.compare_exchange_weak(old_lock_val, true, Ordering::AcqRel, Ordering::Relaxed) { 
            Ok(_) => break,
            Err(v) => old_lock_val = v,
        }
        spin_loop_hint();
    }

    *TLB_SHOOTDOWN_IPI_PAGES.write() = Some(pages_to_invalidate);
    TLB_SHOOTDOWN_IPI_COUNT.store(core_count - 1, Ordering::SeqCst); // -1 to exclude this core 

    // let's try to use NMI instead, since it will interrupt everyone forcibly and result in the fastest handling
    my_lapic.send_nmi_ipi(LapicIpiDestination::AllButMe); // send IPI to all other cores but this one

    // wait for all other cores to handle this IPI
    // it must be a blocking, synchronous operation to ensure stale TLB entries don't cause problems
    // TODO: add timeout!!
    while TLB_SHOOTDOWN_IPI_COUNT.load(Ordering::Relaxed) > 0 { 
        spin_loop_hint();
    }

    // clear TLB shootdown data
    *TLB_SHOOTDOWN_IPI_PAGES.write() = None;

    // release lock
    TLB_SHOOTDOWN_IPI_LOCK.store(false, Ordering::Release); 
}
