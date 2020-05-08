#![no_std]
#![feature(asm)]

#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate memory;
extern crate interrupts;
extern crate spawn;
extern crate scheduler;
extern crate kernel_config;
extern crate apic;
extern crate tlb_shootdown;

use core::sync::atomic::{AtomicBool, Ordering};
use irq_safety::{enable_interrupts, RwLockIrqSafe};
use memory::{VirtualAddress, get_kernel_mmi_ref};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use apic::{LocalApic, get_lapics};


/// An atomic flag used for synchronizing progress between the BSP 
/// and the AP that is currently being booted.
/// False means the AP hasn't started or hasn't yet finished booting.
pub static AP_READY_FLAG: AtomicBool = AtomicBool::new(false);


/// Entry to rust for an AP.
/// The arguments must match the invocation order in "ap_boot.asm"
pub fn kstart_ap(processor_id: u8, apic_id: u8, 
                 stack_start: VirtualAddress, stack_end: VirtualAddress,
                 nmi_lint: u8, nmi_flags: u16) -> ! 
{
    info!("Booted AP: proc: {}, apic: {}, stack: {:#X} to {:#X}, nmi_lint: {}, nmi_flags: {:#X}", 
        processor_id, apic_id, stack_start, stack_end, nmi_lint, nmi_flags
    );

    // set a flag telling the BSP that this AP has entered Rust code
    AP_READY_FLAG.store(true, Ordering::SeqCst);


    // initialize interrupts (including TSS/GDT) for this AP
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("kstart_ap(): kernel_mmi ref was None");
    let (double_fault_stack, privilege_stack) = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("kstart_ap(): could not allocate double fault stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("kstart_ap(): could not allocate privilege stack"),
        )
    };
    let _idt = interrupts::init_ap(apic_id, double_fault_stack.top_unusable(), privilege_stack.top_unusable())
        .expect("kstart_ap(): failed to initialize interrupts!");

    let bootstrap_task = spawn::init(kernel_mmi_ref.clone(), apic_id, stack_start, stack_end).unwrap();

    // as a final step, init this apic as a new LocalApic, and add it to the list of all lapics.
    // we do this last (after all other initialization) in order to prevent this lapic
    // from prematurely receiving IPIs or being used in other ways,
    // and also to ensure that if this apic fails to init, it's not accidentally used as a functioning apic in the list.
    let lapic = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        LocalApic::new(&mut kernel_mmi.page_table, processor_id, apic_id, false, nmi_lint, nmi_flags)
            .expect("kstart_ap(): failed to create LocalApic")
    };
    get_lapics().insert(apic_id, RwLockIrqSafe::new(lapic));
    tlb_shootdown::init();

    info!("Initialization complete on AP core {}. Spawning idle task...", apic_id);
    spawn::create_idle_task(Some(apic_id)).unwrap();

    // Now that we've created a new idle task for this core, we can drop ourself's bootstrapped task.
    drop(bootstrap_task);
    // Before we finish initialization, drop any other local stack variables that still exist.
    //   (currently none others to drop)    

    enable_interrupts();
    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************

    scheduler::schedule();
    loop { 
        error!("BUG: ap_start::kstart_ap(): AP's bootstrap task was rescheduled after being dead!");
    }
}