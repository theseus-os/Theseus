//! Support for broadcasting and handling TLB shootdown IPIs. 

#![no_std]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use irq_safety::{hold_interrupts, RwLockIrqSafe};
use memory::PageRange;
use cpu::cpu_count;
use core::hint::spin_loop;

#[cfg(target_arch = "x86_64")]
use memory_x86_64::tlb_flush_virt_addr;

#[cfg(target_arch = "aarch64")]
use memory_aarch64::tlb_flush_virt_addr;

/// The number of remaining cores that still need to handle the current TLB shootdown IPI
pub static TLB_SHOOTDOWN_IPI_COUNT: AtomicU32 = AtomicU32::new(0);
/// The lock that makes sure only one set of TLB shootdown IPIs is concurrently happening
pub static TLB_SHOOTDOWN_IPI_LOCK: AtomicBool = AtomicBool::new(false);
/// The range of pages for a TLB shootdown IPI.
pub static TLB_SHOOTDOWN_IPI_PAGES: RwLockIrqSafe<Option<PageRange>> = RwLockIrqSafe::new(None);


/// Initializes data, functions, and structures for the TLB shootdown. 
/// TODO: redesign this, it's weird and silly just to set one callback.
pub fn init() {
    memory::set_broadcast_tlb_shootdown_cb(broadcast_tlb_shootdown);

    #[cfg(target_arch = "aarch64")]
    interrupts::init_ipi(tlb_shootdown_ipi_handler, interrupts::TLB_SHOOTDOWN_IPI).unwrap();
}

/// Handles a TLB shootdown ipi by flushing the `VirtualAddress`es 
/// covered by the given range of `pages_to_invalidate`.
/// 
/// There is no need to invoke this directly, it will be called by an IPI interrupt handler.
pub fn handle_tlb_shootdown_ipi(pages_to_invalidate: PageRange) {
    // log::trace!("handle_tlb_shootdown_ipi(): AP {}, pages: {:?}", apic::current_cpu(), pages_to_invalidate);

    for page in pages_to_invalidate {
        tlb_flush_virt_addr(page.start_address());
    }

    TLB_SHOOTDOWN_IPI_COUNT.fetch_sub(1, Ordering::SeqCst);
}


/// Broadcasts TLB shootdown IPI to all other AP cores.
///
/// Do not invoke this directly, but rather pass it as a callback to the memory subsystem,
/// which will invoke it as needed (on remap/unmap operations).
///
/// Sends an IPI to all other cores (except me) to trigger 
/// a TLB flush of the given pages' virtual addresses.
fn broadcast_tlb_shootdown(pages_to_invalidate: PageRange) {        
    // skip sending IPIs if there are no other cores running
    let cpu_count = cpu_count();
    if cpu_count <= 1 {
        return;
    }

    // log::trace!("send_tlb_shootdown_ipi(): cpu_count: {}, {:?}", cpu_count, pages_to_invalidate);

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
        spin_loop();
    }

    *TLB_SHOOTDOWN_IPI_PAGES.write() = Some(pages_to_invalidate);
    TLB_SHOOTDOWN_IPI_COUNT.store(cpu_count - 1, Ordering::SeqCst); // -1 to exclude this core 

    #[cfg(target_arch = "x86_64")] {
        let my_lapic = apic::get_my_apic().expect("If there are more than one CPU ready, this one should be registered");

        // use NMI, since it will interrupt everyone forcibly and result in the fastest handling
        my_lapic.write().send_nmi_ipi(apic::LapicIpiDestination::AllButMe); // send IPI to all other cores but this one
    }

    #[cfg(target_arch = "aarch64")]
    interrupts::send_ipi_to_all_other_cpus(interrupts::TLB_SHOOTDOWN_IPI);

    // wait for all other cores to handle this IPI
    // it must be a blocking, synchronous operation to ensure stale TLB entries don't cause problems
    // TODO: add timeout!!
    while TLB_SHOOTDOWN_IPI_COUNT.load(Ordering::Relaxed) > 0 { 
        spin_loop();
    }

    // clear TLB shootdown data
    *TLB_SHOOTDOWN_IPI_PAGES.write() = None;

    // release lock
    TLB_SHOOTDOWN_IPI_LOCK.store(false, Ordering::Release); 
}

/// Interrupt Handler for TLB Shootdowns on aarch64
#[cfg(target_arch = "aarch64")]
extern "C" fn tlb_shootdown_ipi_handler(_exc: &interrupts::ExceptionContext) -> interrupts::EoiBehaviour {
    if let Some(pages_to_invalidate) = TLB_SHOOTDOWN_IPI_PAGES.read().clone() {
        // trace!("nmi_handler (AP {})", cpu::current_cpu());
        handle_tlb_shootdown_ipi(pages_to_invalidate);
    } else {
        panic!("Unexpected TLB Shootdown IPI!");
    }

    interrupts::EoiBehaviour::CallerMustSignalEoi
}
