//! The main initialization routine and setup logic of the OS. 
//!
//! The `captain` steers the ship of Theseus, meaning that it contains basic logic 
//! for initializing all of the other crates in the proper order and with
//! the proper flow of data between them.
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
//! At the end, the `captain` enables interrupts to allow the system to schedule in other tasks.

#![no_std]

extern crate alloc;

use core::ops::DerefMut;
use log::{error, info};
use memory::{EarlyIdentityMappedPages, MmiRef, PhysicalAddress, VirtualAddress};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use irq_safety::enable_interrupts;
use stack::Stack;
use no_drop::NoDrop;


#[cfg(mirror_log_to_vga)]
mod mirror_log_callbacks {
    /// The callback for use in the logger crate to mirror log functions to the early VGA screen.
    pub(crate) fn mirror_to_early_vga(args: core::fmt::Arguments) {
        vga_buffer::println_raw!("{}", args);
    }

    /// The callback for use in the logger crate to mirror log functions to the
    /// fully-featured terminal-based display.
    pub(crate) fn mirror_to_terminal(args: core::fmt::Arguments) {
        app_io::println!("{}", args);
    }
}


/// Initialize the Captain, which is the main crate that "steers the ship" of Theseus. 
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
/// * `ap_gdt`: the virtual address of the GDT created for the AP's realmode boot code.
/// * `rsdp_address`: the physical address of the RSDP (an ACPI table pointer),
///    if available and provided by the bootloader.
pub fn init(
    kernel_mmi_ref: MmiRef,
    identity_mapped_pages: NoDrop<EarlyIdentityMappedPages>,
    bsp_initial_stack: NoDrop<Stack>,
    ap_start_realmode_begin: VirtualAddress,
    ap_start_realmode_end: VirtualAddress,
    ap_gdt: VirtualAddress,
    rsdp_address: Option<PhysicalAddress>,
) -> Result<(), &'static str> {
    #[cfg(mirror_log_to_vga)] {
        // Enable early mirroring of logger output to VGA buffer (for real hardware)
        logger::set_log_mirror_function(mirror_log_callbacks::mirror_to_early_vga);
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
        ap_gdt,
        Some(kernel_config::display::FRAMEBUFFER_MAX_RESOLUTION),
    )?;
    let cpu_count = ap_count + 1;
    info!("Finished handling and booting up all {} AP cores; {} total CPUs are running.", ap_count, cpu_count);

    #[cfg(mirror_log_to_vga)] {
        // Currently, handling the AP cores also siwtches the graphics mode
        // (from text mode VGA to a graphical framebuffer).
        // Thus, we can now use enable the function that mirrors logger output to the terminal.
        logger::set_log_mirror_function(mirror_log_callbacks::mirror_to_terminal);
    }

    // Now that other CPUs are fully booted, init TLB shootdowns,
    // which rely on Local APICs to broadcast an IPI to all running CPUs.
    tlb_shootdown::init();
    
    // Initialize the per-core heaps.
    multiple_heaps::switch_to_multiple_heaps()?;
    info!("Initialized per-core heaps");

    #[cfg(feature = "uefi")] {
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
        log::warn!("SIMD_PERSONALITY FEATURE ENABLED, creating a new personality with {:?}!", simd_ext);
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
