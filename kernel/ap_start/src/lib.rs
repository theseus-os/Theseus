//! Routines for booting up secondary CPU cores, 
//! aka application processors (APs) on x86_64.
//! 

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};
use log::{error, info};
use cpu::CpuId;
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
static AP_STACKS: MutexIrqSafe<BTreeMap<u32, NoDrop<Stack>>> = MutexIrqSafe::new(BTreeMap::new());

/// Insert a new stack that was allocated for the AP with the given `cpu_id`.
pub fn insert_ap_stack(cpu_id: u32 , stack: Stack) {
    AP_STACKS.lock().insert(cpu_id, NoDrop::new(stack));
}


/// Entry to rust for an AP.
/// The arguments must match the invocation order in "ap_boot.asm"
pub fn kstart_ap(
    processor_id: u8,
    cpu_id: CpuId,
    _stack_start: VirtualAddress,
    _stack_end: VirtualAddress,
    nmi_lint: u8,
    nmi_flags: u16,
) -> ! {
    info!("Booting AP: proc: {}, CPU: {}, stack: {:#X} to {:#X}, nmi_lint: {}, nmi_flags: {:#X}",
        processor_id, cpu_id, _stack_start, _stack_end, nmi_lint, nmi_flags
    );

    // set a flag telling the BSP that this AP has entered Rust code
    AP_READY_FLAG.store(true, Ordering::SeqCst);

    // The early TLS image has already been initialized by the bootstrap CPU,
    // so all we need to do here is to reload it on this CPU.
    early_tls::reload();

    // get the stack that was allocated for us (this AP) by the BSP.
    let this_ap_stack = AP_STACKS.lock().remove(&cpu_id.value())
        .unwrap_or_else(|| panic!("BUG: kstart_ap(): couldn't get stack created for CPU {}", cpu_id));

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
    let _idt = interrupts::init_ap(cpu_id, double_fault_stack.top_unusable(), privilege_stack.top_unusable())
        .expect("kstart_ap(): failed to initialize interrupts!");

    // Initialize this CPU's Local APIC such that we can use everything that depends on APIC IDs.
    // This must be done before initializing task spawning, because that relies on the ability to
    // enable/disable preemption, which is partially implemented by the Local APIC.
    LocalApic::init(
        &mut kernel_mmi_ref.lock().page_table,
        processor_id,
        Some(cpu_id.value()),
        false,
        nmi_lint,
        nmi_flags,
    ).unwrap();

    // Now that the Local APIC has been initialized for this CPU, we can initialize the
    // task management subsystem and create the idle task for this CPU.
    let bootstrap_task = spawn::init(kernel_mmi_ref.clone(), cpu_id, this_ap_stack).unwrap();
    spawn::create_idle_task().unwrap();

    // The PAT must be initialized explicitly on every CPU,
    // but it is not a fatal error if it doesn't exist.
    if page_attribute_table::init().is_err() {
        error!("This CPU does not support the Page Attribute Table");
    }

    info!("Initialization complete on CPU {}. Enabling interrupts...", cpu_id);
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
        error!("BUG: ap_start::kstart_ap(): CPU {} bootstrap task was rescheduled after being dead!", cpu_id);
    }
}