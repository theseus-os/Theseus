//! Support for broadcasting and handling TLB shootdown IPIs. 

#![no_std]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use irq_safety::hold_interrupts;
use memory::PageRange;
use cpu::cpu_count;
use core::hint::spin_loop;
use sync_irq::IrqSafeRwLock;

#[cfg(target_arch = "x86_64")]
use memory_x86_64::tlb_flush_virt_addr;

#[cfg(target_arch = "aarch64")]
use memory_aarch64::tlb_flush_virt_addr;

/// The number of remaining CPUs that still need to handle the current TLB shootdown IPI.
static TLB_SHOOTDOWN_IPI_COUNT: AtomicU32 = AtomicU32::new(0);
/// This lock ensures only one round of TLB shootdown IPIs can occur concurrently.
static TLB_SHOOTDOWN_IPI_LOCK: AtomicBool = AtomicBool::new(false);
/// The range of virtual pages to be flushed for a TLB shootdown IPI.
static TLB_SHOOTDOWN_IPI_PAGES: IrqSafeRwLock<Option<PageRange>> = IrqSafeRwLock::new(None);


/// Initializes data, functions, and structures for the TLB shootdown. 
pub fn init() {
    memory::set_broadcast_tlb_shootdown_cb(broadcast_tlb_shootdown);

    #[cfg(target_arch = "aarch64")]
    interrupts::setup_ipi_handler(tlb_shootdown_ipi_handler, interrupts::TLB_SHOOTDOWN_IPI).unwrap();
}

/// Handles a TLB shootdown IPI requested by another CPU.
///
/// There is no need to invoke this directly, it will be called by an IPI interrupt handler.
///
/// ## Return
/// Returns `true` if virtual addresses were actually flushed, `false` otherwise.
pub fn handle_tlb_shootdown_ipi() -> bool {
    let pages_to_invalidate = TLB_SHOOTDOWN_IPI_PAGES.read().clone();
    if let Some(pages) = pages_to_invalidate {
        // log::trace!("handle_tlb_shootdown_ipi(): CPU {}, pages: {:?}", apic::current_cpu(), pages);
        for page in pages {
            tlb_flush_virt_addr(page.start_address());
        }
        TLB_SHOOTDOWN_IPI_COUNT.fetch_sub(1, Ordering::Relaxed);
        true
    } else {
        false
    }
}


/// Broadcasts a TLB shootdown IPI to all other CPUs, causing them to flush (invalidate)
/// the given virtual pages in their TLBs.
///
/// This is invoked by the memory subsystem as needed, e.g., on remap/unmap operations.
fn broadcast_tlb_shootdown(pages_to_invalidate: PageRange) {        
    // skip sending IPIs if there are no other cores running
    let cpu_count = cpu_count();
    if cpu_count <= 1 {
        return;
    }

    if false {
        log::trace!("send_tlb_shootdown_ipi(): from CPU {:?}, cpu_count: {}, {:?}", cpu::current_cpu(), cpu_count, pages_to_invalidate);
    }

    // interrupts must be disabled here, because this IPI sequence must be fully synchronous with other cores,
    // and we wouldn't want this core to be interrupted while coordinating IPI responses across multiple cores.
    let _held_ints = hold_interrupts();

    // acquire lock
    // TODO: add timeout!!
    loop {
        if TLB_SHOOTDOWN_IPI_LOCK.compare_exchange_weak(
            false,
            true,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok() {
            break;
        }
        spin_loop();
    }

    *TLB_SHOOTDOWN_IPI_PAGES.write() = Some(pages_to_invalidate);
    TLB_SHOOTDOWN_IPI_COUNT.store(cpu_count - 1, Ordering::Relaxed); // -1 to exclude this core 

    #[cfg(target_arch = "x86_64")] {
        let my_lapic = apic::get_my_apic()
            .expect("BUG: broadcast_tlb_shootdown(): couldn't get LocalApic");

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
    let expected = handle_tlb_shootdown_ipi();
    assert!(expected, "Unexpected TLB Shootdown IPI!");
    interrupts::EoiBehaviour::HandlerDidNotSendEoi
}
