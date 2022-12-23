//! The main initialization routine and setup logic of the OS. 
//! 
//! The `captain` steers the ship of Theseus, meaning that it contains basic logic 
//! for initializing all of the other crates in the proper order and with the proper flow of data between them.
//! 
//! Currently, this is the default `captain` in Theseus, which does the following:
//! 
//! * Initializes ACPI and APIC to discover multicore and other hardware configuration,
//! * Sets up interrupt and exception handlers,
//! * Sets up basic device drivers,
//! * Spawns event handling threads,
//! * Initializes the window manager and graphics subsystem,
//! * etc. 
//! 
//! At the end, the `captain` must enable interrupts to allow the system to schedule other Tasks. 
//! It then falls into an idle loop that does nothing, and should never be scheduled in.
//!

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;

extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate dfqueue; // decoupled, fault-tolerant queue

extern crate logger;
extern crate memory; // the virtual memory subsystem 
extern crate no_drop;
extern crate stack;
extern crate cpu; 
extern crate mod_mgmt;
extern crate spawn;
extern crate tsc;
extern crate task; 
extern crate interrupts;
extern crate acpi;
extern crate device_manager;
extern crate e1000;
extern crate scheduler;
#[cfg(mirror_log_to_vga)] #[macro_use] extern crate app_io;
extern crate first_application;
extern crate exceptions_full;
extern crate network_manager;
extern crate window_manager;
extern crate tlb_shootdown;
extern crate multiple_heaps;
extern crate console;
#[cfg(simd_personality)] extern crate simd_personality;


use alloc::vec::Vec;
use core::ops::DerefMut;
use memory::{VirtualAddress, MappedPages, MmiRef};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use irq_safety::enable_interrupts;
use stack::Stack;
use no_drop::NoDrop;
use memory::PhysicalAddress;


#[cfg(mirror_log_to_vga)]
/// the callback use in the logger crate for mirroring log functions to the terminal
pub fn mirror_to_vga_cb(args: core::fmt::Arguments) {
    println!("{}", args);
}



/// Initialize the Captain, which is the main module that steers the ship of Theseus. 
/// 
/// This does the rest of the initialization procedures so that the OS 
/// can continue running and do actual useful work.
/// 
/// # Arguments
/// * `kernel_mmi_ref`: a reference to the kernel's memory management info.
/// * `identity_mapped_pages`: the memory containing the identity-mapped content,
///    which must not be dropped until all APs are finished booting.
/// * `bsp_initial_stack`: the stack currently in use for running this code,
///    which must not be dropped for the entire execution of the initial bootstrap task.
/// * `ap_start_realmode_begin`: the start bound (inclusive) of the AP's realmode boot code.
/// * `ap_start_realmode_end`: the end bound (exlusive) of the AP's realmode boot code.
pub fn init(
    kernel_mmi_ref: MmiRef,
    identity_mapped_pages: NoDrop<Vec<MappedPages>>,
    bsp_initial_stack: NoDrop<Stack>,
    ap_start_realmode_begin: VirtualAddress,
    ap_start_realmode_end: VirtualAddress,
    rsdp_address: Option<PhysicalAddress>,
) -> Result<(), &'static str> {
    #[cfg(mirror_log_to_vga)]
    {
        // enable mirroring of serial port logging outputs to VGA buffer (for real hardware)
        logger::mirror_to_vga(mirror_to_vga_cb);
    }

    // calculate TSC period and initialize it
    // not strictly necessary, but more accurate if we do it early on before interrupts, multicore, and multitasking
    let _tsc_freq = tsc::get_tsc_frequency()?;
    // info!("TSC frequency calculated: {}", _tsc_freq);

    // now we initialize early driver stuff, like APIC/ACPI
    device_manager::early_init(rsdp_address, kernel_mmi_ref.lock().deref_mut())?;

    // initialize the rest of the BSP's interrupt stuff, including TSS & GDT
    let (double_fault_stack, privilege_stack) = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            stack::alloc_stack(KERNEL_STACK_SIZE_IN_PAGES, &mut kernel_mmi.page_table)
                .ok_or("could not allocate double fault stack")?,
            stack::alloc_stack(1, &mut kernel_mmi.page_table)
                .ok_or("could not allocate privilege stack")?,
        )
    };
    let idt = interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable())?;
    
    // get BSP's apic id
    let bsp_apic_id = cpu::bootstrap_cpu().ok_or("captain::init(): couldn't get ID of bootstrap CPU!")?;

    // create the initial `Task`, which is bootstrapped from this execution context.
    let bootstrap_task = spawn::init(kernel_mmi_ref.clone(), bsp_apic_id, bsp_initial_stack)?;
    info!("Created initial bootstrap task: {:?}", bootstrap_task);

    // after we've initialized the task subsystem, we can use better exception handlers
    exceptions_full::init(idt);
    
    // boot up the other cores (APs)
    let ap_count = multicore_bringup::handle_ap_cores(
        &kernel_mmi_ref,
        ap_start_realmode_begin,
        ap_start_realmode_end,
        Some(kernel_config::display::FRAMEBUFFER_MAX_RESOLUTION),
    )?;
    let cpu_count = ap_count + 1;
    info!("Finished handling and booting up all {} AP cores; {} total CPUs are running.", ap_count, cpu_count);

    // Now that other CPUs are fully booted, init TLB shootdowns,
    // which rely on Local APICs to broadcast an IPI to all running CPUs.
    tlb_shootdown::init();
    
    // Initialize the per-core heaps.
    multiple_heaps::switch_to_multiple_heaps()?;
    info!("Initialized per-core heaps");

    #[cfg(feature = "uefi")]
    {
        log::error!("uefi boot cannot proceed as it is not fully implemented");
        loop {}
    }

    // Initialize the window manager, and also the PAT, if available.
    // The PAT supports write-combining caching of graphics video memory for better performance
    // and must be initialized explicitly on every CPU, 
    // but it is not a fatal error if it doesn't exist.
    if page_attribute_table::init().is_err() {
        error!("This CPU does not support the Page Attribute Table");
    }
    let (key_producer, mouse_producer) = window_manager::init()?;

    // initialize the rest of our drivers
    device_manager::init(key_producer, mouse_producer)?;
    task_fs::init()?;


    // We can drop and unmap the identity mappings after the initial bootstrap is complete.
    // We could probably do this earlier, but we definitely can't do it until after the APs boot.
    drop(identity_mapped_pages.into_inner());
    
    // create a SIMD personality
    #[cfg(simd_personality)] {
        #[cfg(simd_personality_sse)]
        let simd_ext = task::SimdExt::SSE;
        #[cfg(simd_personality_avx)]
        let simd_ext = task::SimdExt::AVX;
        warn!("SIMD_PERSONALITY FEATURE ENABLED, creating a new personality with {:?}!", simd_ext);
        spawn::new_task_builder(simd_personality::setup_simd_personality, simd_ext)
            .name(alloc::format!("setup_simd_personality_{:?}", simd_ext))
            .spawn()?;
    }

    // Now that key subsystems are initialized, we can spawn various system tasks/daemons
    // and then the first application(s).
    console::start_connection_detection()?;
    first_application::start()?;

    info!("captain::init(): initialization done! Spawning an idle task on BSP core {} and enabling interrupts...", bsp_apic_id);
    // The following final initialization steps are important, and order matters:
    // 1. Drop any other local stack variables that still exist.
    drop(kernel_mmi_ref);
    // 2. Create the idle task for this CPU.
    spawn::create_idle_task()?;
    // 3. Cleanup bootstrap tasks, which handles this one and all other APs' bootstrap tasks.
    spawn::cleanup_bootstrap_tasks(cpu_count as usize)?;
    // 4. "Finish" this bootstrap task, indicating it has exited and no longer needs to run.
    bootstrap_task.finish();
    // 5. Enable interrupts such that other tasks can be scheduled in.
    enable_interrupts();
    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************

    scheduler::schedule();
    loop { 
        error!("BUG: captain::init(): captain's bootstrap task was rescheduled after being dead!");
    }
}
