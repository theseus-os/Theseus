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

use log::{error, info};
use memory::{EarlyIdentityMappedPages, MmiRef, PhysicalAddress};
use irq_safety::enable_interrupts;
use stack::Stack;
use no_drop::NoDrop;

#[cfg(target_arch = "x86_64")]
use {
    core::ops::DerefMut,
    kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES,
};

#[cfg(all(mirror_log_to_vga, target_arch = "x86_64"))]
mod mirror_log_callbacks {
    /// The callback for use in the logger crate to mirror log functions to the early VGA screen.
    pub(crate) fn mirror_to_early_vga(args: core::fmt::Arguments) {
        early_printer::println!("{}", args);
    }

    /// The callback for use in the logger crate to mirror log functions to the
    /// fully-featured terminal-based display.
    pub(crate) fn mirror_to_terminal(args: core::fmt::Arguments) {
        app_io::println!("{}", args);
    }
}

/// Items that must be held until the end of [`init()`] and should be dropped after.
pub struct DropAfterInit {
    pub identity_mappings: NoDrop<EarlyIdentityMappedPages>,
}
impl DropAfterInit {
    fn drop_all(self) {
        drop(self.identity_mappings.into_inner());
    }
}

pub use multicore_bringup::MulticoreBringupInfo;

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
/// * `multicore_info`: information needed to bring up secondary CPUs.
/// * `rsdp_address`: the physical address of the RSDP (an ACPI table pointer),
///    if available and provided by the bootloader.
#[cfg_attr(target_arch = "aarch64", allow(unreachable_code, unused_variables))]
pub fn init(
    kernel_mmi_ref: MmiRef,
    bsp_initial_stack: NoDrop<Stack>,
    drop_after_init: DropAfterInit,
    multicore_info: MulticoreBringupInfo,
    rsdp_address: Option<PhysicalAddress>,
) -> Result<(), &'static str> {
    #[cfg(all(mirror_log_to_vga, target_arch = "x86_64"))] {
        // Enable early mirroring of logger output to VGA buffer (for real hardware)
        logger::set_log_mirror_function(mirror_log_callbacks::mirror_to_early_vga);
    }

    // calculate TSC period and initialize it
    // not strictly necessary, but more accurate if we do it early on before interrupts, multicore, and multitasking
    #[cfg(target_arch = "x86_64")]
    if let Some(period) = tsc::get_tsc_period() {
        time::register_clock_source::<tsc::Tsc>(period);
    } else {
        log::warn!("Couldn't get TSC period");
    }

    // now we initialize early driver stuff, like APIC/ACPI
    // arch-gate: device_manager currently detects PCI & PS2 devices,
    // which are unsupported on aarch64 at this point
    #[cfg(target_arch = "x86_64")]
    device_manager::early_init(rsdp_address, kernel_mmi_ref.lock().deref_mut())?;

    // initialize the rest of the BSP's interrupt stuff, including TSS & GDT
    // arch-gate: the IDT & special stacks are x86_64 specific
    #[cfg(target_arch = "x86_64")]
    let idt = {
        // does nothing at the moment on x86_64
        interrupt_controller::init()?;

        let (double_fault_stack, privilege_stack) = {
            let mut kernel_mmi = kernel_mmi_ref.lock();
            (
                stack::alloc_stack(KERNEL_STACK_SIZE_IN_PAGES, &mut kernel_mmi.page_table)
                    .ok_or("could not allocate double fault stack")?,
                stack::alloc_stack(1, &mut kernel_mmi.page_table)
                    .ok_or("could not allocate privilege stack")?,
            )
        };
        interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable())?
    };

    #[cfg(target_arch = "aarch64")] {
        // Initialize the GIC
        interrupt_controller::init()?;

        interrupts::init()?;

        // register BSP CpuId
        cpu::register_cpu(true)?;
    }
    
    // get BSP's CPU ID
    let bsp_id = cpu::bootstrap_cpu().ok_or("captain::init(): couldn't get ID of bootstrap CPU!")?;
    cls_allocator::reload_current_cpu();

    // Initialize the scheduler and create the initial `Task`,
    // which is bootstrapped from this current execution context.
    scheduler::init()?;
    let bootstrap_task = spawn::init(kernel_mmi_ref.clone(), bsp_id, bsp_initial_stack)?;
    info!("Created initial bootstrap task: {:?}", bootstrap_task);

    // after we've initialized the task subsystem, we can use better exception handlers
    // arch-gate: aarch64 simply logs exceptions and crash; porting exceptions_full
    // hasn't been done yet
    #[cfg(target_arch = "x86_64")]
    exceptions_full::init(idt);
    
    // boot up the other cores (APs)
    let ap_count = multicore_bringup::handle_ap_cores(
        &kernel_mmi_ref,
        multicore_info,
    )?;

    let cpu_count = ap_count + 1;
    info!("Finished booting all {} AP cores; {} total CPUs are running.", ap_count, cpu_count);
    info!("Proceeding with system initialization, please wait...");

    // arch-gate: no framebuffer support on aarch64 at the moment
    #[cfg(all(mirror_log_to_vga, target_arch = "x86_64"))] {
        // Currently, handling the AP cores also siwtches the graphics mode
        // (from text mode VGA to a graphical framebuffer).
        // Thus, we can now use enable the function that mirrors logger output to the terminal.
        logger::set_log_mirror_function(mirror_log_callbacks::mirror_to_terminal);
    }

    // Now that other CPUs are fully booted, init TLB shootdowns,
    // which rely on Local APICs to broadcast an IPI to all running CPUs.
    tlb_shootdown::init();
    
    // Initialize the per-core heaps.
    // arch-gate: no multicore support on aarch64 at the moment
    #[cfg(target_arch = "x86_64")] {
        multiple_heaps::switch_to_multiple_heaps()?;
        info!("Initialized per-core heaps");
    }

    // Initialize the window manager, and also the PAT, if available.
    // The PAT supports write-combining caching of graphics video memory for better performance
    // and must be initialized explicitly on every CPU, 
    // but it is not a fatal error if it doesn't exist.
    #[cfg(target_arch = "x86_64")]
    if page_attribute_table::init().is_err() {
        error!("This CPU does not support the Page Attribute Table");
    }

    // arch-gate: no windowing/input support on aarch64 at the moment
    #[cfg(target_arch = "x86_64")]
    match window_manager::init() {
        Ok((key_producer, mouse_producer)) => {
            device_manager::init(key_producer, mouse_producer)?;
        },
        Err(error) => {
            error!("Failed to init window manager (expected if using nographic): {error}");
        }
    }

    #[cfg(target_arch = "aarch64")]
    device_manager::init()?;

    task_fs::init()?;

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

    // Now that key subsystems are initialized, we can:
    // 1. Drop the items that needed to be held through initialization,
    drop_after_init.drop_all();

    // 2. Spawn various system tasks/daemons,
    console::start_connection_detection()?;

    // 3. Start the first application(s).
    first_application::start()?;

    info!("captain::init(): initialization done! Spawning an idle task on BSP core {} and enabling interrupts...", bsp_id);
    // The following final initialization steps are important, and order matters:
    // 1. Drop any other local stack variables that still exist.
    drop(kernel_mmi_ref);
    // 2. Cleanup bootstrap tasks, which handles this one and all other APs' bootstrap tasks.
    spawn::cleanup_bootstrap_tasks(cpu_count)?;
    // 3. "Finish" this bootstrap task, indicating it has exited and no longer needs to run.
    bootstrap_task.finish();
    // 4. Enable interrupts such that other tasks can be scheduled in.
    enable_interrupts();
    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************

    scheduler::schedule();
    loop { 
        error!("BUG: captain::init(): captain's bootstrap task was rescheduled after being dead!");
    }
}
