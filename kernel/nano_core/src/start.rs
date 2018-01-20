use core::sync::atomic::{AtomicBool, Ordering};
use memory::VirtualAddress;

/// An atomic flag used for synchronizing progress between the BSP 
/// and the AP that is currently being booted.
/// False means the AP hasn't started or hasn't yet finished booting.
pub static AP_READY_FLAG: AtomicBool = AtomicBool::new(false);


#[repr(packed)]
#[derive(Debug)]
pub struct KernelArgsAp {
    processor_id: u64,
    apic_id: u64,
    flags: u64,
    page_table: u64,
    stack_start: u64,
    stack_end: u64,
}

/// Entry to rust for an AP.
/// The arguments 
#[no_mangle]
pub unsafe fn kstart_ap(processor_id: u8, apic_id: u8, flags: u32, stack_start: VirtualAddress, stack_end: VirtualAddress) -> ! {

    info!("Booted AP: proc: {} apic: {} flags: {:#X} stack: {:#X} to {:#X}", processor_id, apic_id, flags, stack_start, stack_end);

    loop { }

    // Initialize paging
    // let tcb_offset = paging::init_ap(processor_id, bsp_table, stack_start, stack_end);

    // Set up GDT for AP
    // gdt::init(tcb_offset, stack_end);

    // Set up IDT for AP
    // idt::init();

    // init AP as a new local APIC
    let mut lapics_locked = ::interrupts::apic::get_lapics();
    lapics_locked.insert(processor_id, ::interrupts::apic::LocalApic::new(processor_id, apic_id, flags));

    // set a flag telling the BSP that this AP has finished initializing
    AP_READY_FLAG.store(true, Ordering::SeqCst); // must be Sequential Consistency because the BSP is polling it in a while loop


    // while ! BSP_READY.load(Ordering::SeqCst) {
    //     interrupt::pause();
    // }

    loop { }
    // ::kmain_ap(processor_id);
}