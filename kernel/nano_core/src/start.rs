use core::sync::atomic::{AtomicBool, Ordering};

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

/// Entry to rust for an AP
pub unsafe extern fn kstart_ap(args_ptr: *const KernelArgsAp) -> ! {
    let processor_id: u8 = {
        let args = &*args_ptr;
        let processor_id = args.processor_id as u8; // originally a u8 in MadtLocalApic
        let apic_id = args.apic_id as u8;  // originally a u8 in MadtLocalApic
        let flags = args.flags as u32;  // originally a u32 in MadtLocalApic
        let bsp_table = args.page_table as usize;
        let stack_start = args.stack_start as usize;
        let stack_end = args.stack_end as usize;

        println_unsafe!("kstart_ap: {:?}", args);
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

        processor_id
    };

    // while ! BSP_READY.load(Ordering::SeqCst) {
    //     interrupt::pause();
    // }

    loop { }
    // ::kmain_ap(processor_id);
}