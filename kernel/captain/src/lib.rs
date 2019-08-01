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
#![feature(asm)]
#![feature(core_intrinsics)]


extern crate alloc;
#[macro_use] extern crate log;

extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate dfqueue; // decoupled, fault-tolerant queue

extern crate event_types; // a temporary way to use input_event_manager types 
extern crate logger;
extern crate memory; // the virtual memory subsystem 
extern crate apic; 
extern crate mod_mgmt;
extern crate spawn;
extern crate tsc;
extern crate task; 
extern crate interrupts;
extern crate acpi;
extern crate device_manager;
extern crate e1000;
extern crate window_manager;
extern crate scheduler;
extern crate frame_buffer;
#[cfg(mirror_log_to_vga)] #[macro_use] extern crate print;
extern crate input_event_manager;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[cfg(test_network)] extern crate exceptions_full;
extern crate network_manager;
extern crate pause;

#[cfg(simd_personality)] extern crate simd_personality;



use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::DerefMut;
use memory::{VirtualAddress, MemoryManagementInfo, MappedPages};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use irq_safety::{MutexIrqSafe, enable_interrupts};
use pause::spin_loop_hint;



#[cfg(mirror_log_to_vga)]
/// the callback use in the logger crate for mirroring log functions to the input_event_manager
pub fn mirror_to_vga_cb(_color: &logger::LogColor, prefix: &'static str, args: core::fmt::Arguments) {
    println!("{}{}", prefix, args);
}



/// Initialize the Captain, which is the main module that steers the ship of Theseus. 
/// This does all the rest of the module loading and initialization so that the OS 
/// can continue running and do actual useful work.
pub fn init(
    kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, 
    identity_mapped_pages: Vec<MappedPages>,
    bsp_stack_bottom: VirtualAddress,
    bsp_stack_top: VirtualAddress,
    ap_start_realmode_begin: VirtualAddress,
    ap_start_realmode_end: VirtualAddress,
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
    device_manager::early_init(kernel_mmi_ref.lock().deref_mut())?;

    // initialize the rest of the BSP's interrupt stuff, including TSS & GDT
    let (double_fault_stack, privilege_stack) = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(1).ok_or("could not allocate double fault stack")?,
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).ok_or("could not allocate privilege stack")?,
        )
    };
    let idt = interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable())?;
    
    // init other featureful (non-exception) interrupt handlers
    // interrupts::init_handlers_pic();
    interrupts::init_handlers_apic();
    
    // get BSP's apic id
    let bsp_apic_id = apic::get_bsp_id().ok_or("captain::init(): Coudln't get BSP's apic_id!")?;

    // create the initial `Task`, i.e., task_zero
    spawn::init(kernel_mmi_ref.clone(), bsp_apic_id, bsp_stack_bottom, bsp_stack_top)?;

    // after we've initialized the task subsystem, we can use better exception handlers
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    exceptions_full::init(idt);
    
    // boot up the other cores (APs)
    let ap_count = multicore_bringup::handle_ap_cores(kernel_mmi_ref.clone(), ap_start_realmode_begin, ap_start_realmode_end)?;
    info!("Finished handling and booting up all {} AP cores.", ap_count);

    // //init frame_buffer
    let rs = frame_buffer::init();
    match rs {
        Ok(_) => {
            trace!("Frame_buffer initialized successfully.");
        }
        Err(err) => { 
            error!("captain::init(): failed to initialize frame_buffer");
            return Err(err);
        }
    }

    // initialize the input event manager, which will start the default terminal 
    let input_event_queue_producer = input_event_manager::init()?;

    // initialize the rest of our drivers
    device_manager::init(input_event_queue_producer)?;

    task_fs::init()?;


    // before we jump to userspace, we need to unmap the identity-mapped section of the kernel's page tables, at PML4[0]
    // unmap the kernel's original identity mapping (including multiboot2 boot_info) to clear the way for userspace mappings
    // we cannot do this until we have booted up all the APs
    drop(identity_mapped_pages);
    {
        // for i in 0 .. 512 { 
        //     debug!("P4[{:03}] = {:#X}", i, active_table.p4().get_entry_value(i));
        // }
        // clear the 0th P4 entry, which covers any existing identity mappings
        kernel_mmi_ref.lock().page_table.p4_mut().clear_entry(0); 
    }
    
    // create a SIMD personality
    #[cfg(simd_personality)]
    {
        let simd_ext = task::SimdExt::SSE;
        warn!("SIMD_PERSONALITY FEATURE ENABLED, creating a new personality with {:?}!", simd_ext);
        spawn::KernelTaskBuilder::new(simd_personality::setup_simd_personality, simd_ext)
            .name(alloc::string::String::from("setup_simd_personality"))
            .spawn()?;
    }


    info!("captain::init(): initialization done! Enabling interrupts and entering Task 0's idle loop...");
    enable_interrupts();
    scheduler::schedule();
    // NOTE: DO NOT PUT ANY CODE BELOW THIS POINT, AS IT SHOULD NEVER RUN!
    // (unless there are no other tasks available to run on the BSP core, which never happens)
    

    loop { 
        spin_loop_hint();
        // TODO: put this core into a low-power state
        // TODO: exit this loop cleanly upon a shutdown signal
    }
}
