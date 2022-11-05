//! Routines for booting up secondary CPU cores, 
//! aka application processors (APs) on x86_64.
//! 

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate memory;
extern crate stack;
extern crate interrupts;
extern crate spawn;
extern crate scheduler;
extern crate kernel_config;
extern crate apic;
extern crate no_drop;

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};
use irq_safety::{enable_interrupts, MutexIrqSafe};
use memory::{VirtualAddress, get_kernel_mmi_ref};
use stack::Stack;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use apic::LocalApic;
use no_drop::NoDrop;

/// An atomic flag used for synchronizing progress between the BSP 
/// and the AP that is currently being booted.
/// False means the AP hasn't started or hasn't yet finished booting.
pub static AP_READY_FLAG: AtomicBool = AtomicBool::new(false);

/// Temporary storage for transferring allocated `Stack`s from 
/// the main bootstrap processor (BSP) to the AP processor being booted in `kstart_ap()` below.
static AP_STACKS: MutexIrqSafe<BTreeMap<u8, NoDrop<Stack>>> = MutexIrqSafe::new(BTreeMap::new());

/// Insert a new stack that was allocated for the AP with the given `apic_id`.
pub fn insert_ap_stack(apic_id: u8, stack: Stack) {
    AP_STACKS.lock().insert(apic_id, NoDrop::new(stack));
}


/// Entry to rust for an AP.
/// The arguments must match the invocation order in "ap_boot.asm"
pub fn kstart_ap(
    processor_id: u8,
    apic_id: u8,
    _stack_start: VirtualAddress,
    _stack_end: VirtualAddress,
    nmi_lint: u8,
    nmi_flags: u16,
) -> ! {
    info!("Booting AP: proc: {}, apic: {}, stack: {:#X} to {:#X}, nmi_lint: {}, nmi_flags: {:#X}",
        processor_id, apic_id, _stack_start, _stack_end, nmi_lint, nmi_flags
    );

    // set a flag telling the BSP that this AP has entered Rust code
    AP_READY_FLAG.store(true, Ordering::SeqCst);

    // get the stack that was allocated for us (this AP) by the BSP.
    let this_ap_stack = AP_STACKS.lock().remove(&apic_id)
        .unwrap_or_else(|| panic!("BUG: kstart_ap(): couldn't get stack created for AP with apic_id: {}", apic_id));

    // initialize interrupts (including TSS/GDT) for this AP
    let kernel_mmi_ref = get_kernel_mmi_ref().expect("kstart_ap(): kernel_mmi ref was None");
    let (double_fault_stack, privilege_stack) = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            stack::alloc_stack(KERNEL_STACK_SIZE_IN_PAGES, &mut kernel_mmi.page_table)
                .expect("kstart_ap(): could not allocate double fault stack"),
            stack::alloc_stack(1, &mut kernel_mmi.page_table)
                .expect("kstart_ap(): could not allocate privilege stack"),
        )
    };
    let _idt = interrupts::init_ap(apic_id, double_fault_stack.top_unusable(), privilege_stack.top_unusable())
        .expect("kstart_ap(): failed to initialize interrupts!");

    // Initialize this CPU's Local APIC such that we can use everything that depends on APIC IDs.
    // This must be done before initializing task spawning, because that relies on the ability to
    // enable/disable preemption, which is partially implemented by the Local APIC.
    LocalApic::init(
        &mut kernel_mmi_ref.lock().page_table,
        processor_id,
        Some(apic_id),
        false,
        nmi_lint,
        nmi_flags,
    ).unwrap();

    // Now that the Local APIC has been initialized for this CPU, we can initialize the
    // task management subsystem and create the idle task for this CPU.
    let bootstrap_task = spawn::init(kernel_mmi_ref.clone(), apic_id, this_ap_stack).unwrap();
    spawn::create_idle_task().unwrap();

    info!("Initialization complete on AP core {}. Enabling interrupts...", apic_id);
    // The following final initialization steps are important, and order matters:
    // 1. Drop any other local stack variables that still exist.
    // (currently nothing else needs to be dropped)
    // 2. "Finish" this bootstrap task, indicating it has exited and no longer needs to run.
    bootstrap_task.finish();
    // 3. Enable interrupts such that other tasks can be scheduled in.
    enable_interrupts();
    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************

    scheduler::schedule();
    loop { 
        error!("BUG: ap_start::kstart_ap(): CPU {} bootstrap task was rescheduled after being dead!", apic_id);
    }
}