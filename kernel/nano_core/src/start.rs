use core::sync::atomic::{AtomicBool, Ordering};
use memory;
use memory::{VirtualAddress};
use interrupts;
use task;

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

    // initialize interrupts by using the same IDT and interrupt handlers for all APs (will this work? maybe)
    let mut kernel_mmi_ref = memory::get_kernel_mmi_ref().expect("couldn't get kernel_mmi_ref");
    let (double_fault_stack, privilege_stack) = { 
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(1).expect("could not allocate double fault stack"),
            kernel_mmi.alloc_stack(4).expect("could not allocate privilege stack"),
        )
    };
    interrupts::init_ap(apic_id, double_fault_stack.top_unusable(), privilege_stack.top_unusable()); 
    
    task::init(kernel_mmi_ref, apic_id, stack_start, stack_end);

    // init AP as a new local APIC
    let mut lapics_locked = ::interrupts::apic::get_lapics();
    lapics_locked.insert(apic_id, ::interrupts::apic::LocalApic::new(processor_id, apic_id, flags, false));
    
    // TODO: FIXME: process NmiInterruptLapic entries in the MADT 

    // set a flag telling the BSP that this AP has finished initializing
    AP_READY_FLAG.store(true, Ordering::SeqCst); // must be Sequential Consistency because the BSP is polling it in a while loop


    // while ! BSP_READY.load(Ordering::SeqCst) {
    //     interrupt::pause();
    // }

    loop { }
    // ::kmain_ap(apic_id);
}