//! Support for broadcasting and handling TLB shootdown IPIs. 

#![no_std]


extern crate alloc;
#[macro_use] extern crate lazy_static;
// #[macro_use] extern crate log;
extern crate irq_safety;
extern crate memory;
extern crate apic;
extern crate x86_64;


use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering, spin_loop_hint};
use alloc::vec::Vec;
use irq_safety::{hold_interrupts, RwLockIrqSafe};
use memory::VirtualAddress;
use apic::{LocalApic, get_my_apic, get_lapics, LapicIpiDestination};


/// The IRQ number used for IPIs
pub const TLB_SHOOTDOWN_IPI_IRQ: u8 = 0x40;
/// The number of remaining cores that still need to handle the curerent TLB shootdown IPI
pub static TLB_SHOOTDOWN_IPI_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The lock that makes sure only one set of TLB shootdown IPIs is concurrently happening
pub static TLB_SHOOTDOWN_IPI_LOCK: AtomicBool = AtomicBool::new(false);
lazy_static! {
    /// The virtual addresses used for TLB shootdown IPIs
    pub static ref TLB_SHOOTDOWN_IPI_VIRTUAL_ADDRESSES: RwLockIrqSafe<Vec<VirtualAddress>> = 
        RwLockIrqSafe::new(Vec::new());
}


/// Initializes data, functions, and structures for the TLB shootdown. 
/// TODO: redesign this, it's weird and silly just to set one callback.
pub fn init() {
    memory::set_broadcast_tlb_shootdown_cb(broadcast_tlb_shootdown);
}


/// Broadcasts TLB shootdown IPI to all other AP cores.
/// Do not invoke this directly, but rather pass it as a callback to the memory subsystem,
/// which will invoke it as needed (on remap/unmap operations).
fn broadcast_tlb_shootdown(virtual_addresses: Vec<VirtualAddress>) {
    if let Some(my_lapic) = get_my_apic() {
        // info!("broadcast_tlb_shootdown():  AP {}, vaddrs: {:?}", my_lapic.read().apic_id, virtual_addresses);
        send_tlb_shootdown_ipi(&mut my_lapic.write(), virtual_addresses);
    }
}


/// Handles a TLB shootdown ipi by flushing the `VirtualAddress`es 
/// currently stored in `TLB_SHOOTDOWN_IPI_VIRTUAL_ADDRESSES`.
/// DO not invoke this directly, it will be called by an IPI interrupt handler.
pub fn handle_tlb_shootdown_ipi(virtual_addresses: &[VirtualAddress]) {
    // let apic_id = get_my_apic_id().unwrap_or(0xFF);
    // trace!("handle_tlb_shootdown_ipi(): AP {}, vaddrs: {:?}", apic_id, virtual_addresses);

    for vaddr in virtual_addresses {
        x86_64::instructions::tlb::flush(x86_64::VirtualAddress(vaddr.value()));
    }
    TLB_SHOOTDOWN_IPI_COUNT.fetch_sub(1, Ordering::SeqCst);
}


/// Sends an IPI to all other cores (except me) to trigger 
/// a TLB flush of the given `VirtualAddress`es
pub fn send_tlb_shootdown_ipi(my_lapic: &mut LocalApic, virtual_addresses: Vec<VirtualAddress>) {        
    // skip sending IPIs if there are no other cores running
    let core_count = get_lapics().iter().count();
    if core_count <= 1 {
        return;
    }

    // trace!("send_tlb_shootdown_ipi(): from AP {}, core_count: {}, {:?}", self.apic_id, core_count, virtual_addresses);

    // interrupts must be disabled here, because this IPI sequence must be fully synchronous with other cores,
    // and we wouldn't want this core to be interrupted while coordinating IPI responses across multiple cores.
    let _held_ints = hold_interrupts(); 

    // acquire lock
    // TODO: add timeout!!
    while TLB_SHOOTDOWN_IPI_LOCK.compare_and_swap(false, true, Ordering::SeqCst) { 
        spin_loop_hint();
    }

    *TLB_SHOOTDOWN_IPI_VIRTUAL_ADDRESSES.write() = virtual_addresses;
    TLB_SHOOTDOWN_IPI_COUNT.store(core_count - 1, Ordering::SeqCst); // -1 to exclude this core 

    // let's try to use NMI instead, since it will interrupt everyone forcibly and result in the fastest handling
    my_lapic.send_nmi_ipi(LapicIpiDestination::AllButMe); // send IPI to all other cores but this one

    // wait for all other cores to handle this IPI
    // it must be a blocking, synchronous operation to ensure stale TLB entries don't cause problems
    // TODO: add timeout!!
    while TLB_SHOOTDOWN_IPI_COUNT.load(Ordering::SeqCst) > 0 { 
        spin_loop_hint();
    }

    // clear TLB shootdown data
    TLB_SHOOTDOWN_IPI_VIRTUAL_ADDRESSES.write().clear();

    // release lock
    TLB_SHOOTDOWN_IPI_LOCK.store(false, Ordering::SeqCst); 
}
