use core::sync::atomic::{AtomicBool, Ordering};
use memory::{VirtualAddress, get_kernel_mmi_ref};
use interrupts;
use syscall;
use task;
use BSP_READY_FLAG;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use drivers::acpi::madt::MadtIter;
use interrupts::apic::{LocalApic, get_lapics};
use spin::RwLock;

/// An atomic flag used for synchronizing progress between the BSP 
/// and the AP that is currently being booted.
/// False means the AP hasn't started or hasn't yet finished booting.
pub static AP_READY_FLAG: AtomicBool = AtomicBool::new(false);


/// Entry to rust for an AP.
/// The arguments must match the invocation order in "ap_boot.asm"
pub fn kstart_ap(processor_id: u8, apic_id: u8, flags: u32, 
                 stack_start: VirtualAddress, stack_end: VirtualAddress,
                 madt_iter: MadtIter) -> ! 
{
    info!("Booted AP: proc: {} apic: {} flags: {:#X} stack: {:#X} to {:#X}", processor_id, apic_id, flags, stack_start, stack_end);


    // set a flag telling the BSP that this AP has entered Rust code
    AP_READY_FLAG.store(true, Ordering::SeqCst); // must be Sequential Consistency because the BSP is polling it in a while loop

    // wait for the BSP to finish its initialization of system-wide things (like the IDT) before enabling interrupts 
    while ! BSP_READY_FLAG.load(Ordering::SeqCst) {
        ::arch::pause();
    }
    // NOTE: code below here depends on the BSP having inited the rest of the system-wide things first

    // initialize interrupts (including TSS/GDT) for this AP
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("kstart_ap: kernel_mmi ref was None");
    let (double_fault_stack, privilege_stack, syscall_stack) = { 

        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(1).expect("could not allocate double fault stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("could not allocate privilege stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("could not allocate syscall stack")
        )
    };
    interrupts::init_ap(apic_id, double_fault_stack.top_unusable(), privilege_stack.top_unusable())
                    .expect("failed to initialize interrupts!");

    syscall::init(syscall_stack.top_usable());

    task::init_ap(kernel_mmi_ref, apic_id, stack_start, stack_end).unwrap();

    // as a final step, init this apic as a new LocalApic, and add it to the list of all lapics.
    // we do this last (after all other initialization) in order to prevent this lapic
    // from prematurely receiving IPIs or being used in other ways,
    // and also to ensure that if this apic fails to init, it's not used as one apic in the list.
    let lapic = LocalApic::new(processor_id, apic_id, flags, false, madt_iter.clone());
    get_lapics().insert(apic_id, RwLock::new(lapic));

    interrupts::enable_interrupts();
    info!("Entering idle_task loop on AP {} with interrupts {}", apic_id, 
           if interrupts::interrupts_enabled() { "enabled" } else { "DISABLED!!! ERROR!" });

    loop { 
        schedule!();
        ::arch::pause();
    }
}