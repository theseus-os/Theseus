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
#![feature(alloc)]
#![feature(asm)]
#![feature(core_intrinsics)]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;


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
extern crate syscall;
extern crate interrupts;
extern crate acpi;
extern crate device_manager;
extern crate e1000;
extern crate window_manager;
extern crate scheduler;
extern crate frame_buffer;
#[cfg(mirror_log_to_vga)] #[macro_use] extern crate print;
extern crate input_event_manager;
#[cfg(test_network)] extern crate exceptions_full;
extern crate network_manager;

#[cfg(test_ota_update)] extern crate ota_update_client;
#[cfg(test_ota_update)] extern crate test_ota_update;

#[cfg(simd_personality)] extern crate simd_personality;



use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::DerefMut;
use core::sync::atomic::spin_loop_hint;
use memory::{MemoryManagementInfo, MappedPages, PageTable};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use irq_safety::{MutexIrqSafe, enable_interrupts};



#[cfg(mirror_log_to_vga)]
/// the callback use in the logger crate for mirroring log functions to the input_event_manager
pub fn mirror_to_vga_cb(_color: &logger::LogColor, prefix: &'static str, args: core::fmt::Arguments) {
    println!("{}{}", prefix, args);
}



/// Initialize the Captain, which is the main module that steers the ship of Theseus. 
/// This does all the rest of the module loading and initialization so that the OS 
/// can continue running and do actual useful work.
pub fn init(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, 
            identity_mapped_pages: Vec<MappedPages>,
            bsp_stack_bottom: usize, bsp_stack_top: usize,
            ap_start_realmode_begin: usize, ap_start_realmode_end: usize) 
            -> Result<(), &'static str>
{
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
    let madt_iter = device_manager::early_init(kernel_mmi_ref.lock().deref_mut())?;

    // initialize the rest of the BSP's interrupt stuff, including TSS & GDT
    let (double_fault_stack, privilege_stack, syscall_stack) = { 
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(1).ok_or("could not allocate double fault stack")?,
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).ok_or("could not allocate privilege stack")?,
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).ok_or("could not allocate syscall stack")?
        )
    };
    let idt = interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable())?;
    
    // init other featureful (non-exception) interrupt handlers
    // interrupts::init_handlers_pic();
    interrupts::init_handlers_apic();
    
    // initialize the syscall 
    syscall::init(syscall_stack.top_usable());

    // get BSP's apic id
    let bsp_apic_id = apic::get_bsp_id().ok_or("captain::init(): Coudln't get BSP's apic_id!")?;

    // create the initial `Task`, i.e., task_zero
    spawn::init(kernel_mmi_ref.clone(), bsp_apic_id, bsp_stack_bottom, bsp_stack_top)?;

    // after we've initialized the task subsystem, we can use better exception handlers
    exceptions_full::init(idt);
    
    // boot up the other cores (APs)
    let ap_count = acpi::madt::handle_ap_cores(madt_iter, kernel_mmi_ref.clone(), ap_start_realmode_begin, ap_start_realmode_end)?;
    info!("Finished handling and booting up all {} AP cores.", ap_count);

    // //init frame_buffer
    let rs = frame_buffer::init();
    match rs {
        Ok(_) => {
            trace!("Frame_buffer initialized successfully.");
        }
        Err(err) => { 
            println_raw!("captain::init(): failed to initialize frame_buffer");
            return Err(err);
        }
    }

    // initialize the input event manager, which will start the default terminal 
    let input_event_queue_producer = input_event_manager::init()?;

    // initialize the rest of our drivers
    device_manager::init(input_event_queue_producer)?;


    #[cfg(test_ota_update)]
    {
        if let Some(iface) = network_manager::NETWORK_INTERFACES.lock().iter().next().cloned() {
            spawn::KernelTaskBuilder::new(test_ota_update::simple_keyboard_swap, iface)
                .name(String::from("test_ota_update"))
                .spawn()?;
        } else {
            error!("captain: Couldn't test the OTA update functionality because no e1000 NIC exists.");
        }
    }


    // before we jump to userspace, we need to unmap the identity-mapped section of the kernel's page tables, at PML4[0]
    // unmap the kernel's original identity mapping (including multiboot2 boot_info) to clear the way for userspace mappings
    // we cannot do this until we have booted up all the APs
    ::core::mem::drop(identity_mapped_pages);
    {
        if let PageTable::Active(ref mut active_table) = kernel_mmi_ref.lock().page_table {
            // for i in 0 .. 512 { 
            //     debug!("P4[{:03}] = {:#X}", i, active_table.p4().get_entry_value(i));
            // }

            // clear the 0th P4 entry, which covers any existing identity mappings
            active_table.p4_mut().clear_entry(0); 
        }
        else {
            return Err("Couldn't get kernel's ActivePageTable to clear out identity mappings!");
        }
    }

    // create and jump to the first userspace thread
    #[cfg(spawn_userspace)] {
        debug!("trying to jump to userspace");
        let module = memory::get_module("u#test_program").ok_or("Error: no userspace modules named 'u#test_program' found!")?;
        spawn::spawn_userspace(module, Some(String::from("test_program_1")))?;
    }

    #[cfg(spawn_userspace)] {
        debug!("trying to jump to userspace 2nd time");
        let module = memory::get_module("u#test_program").ok_or("Error: no userspace modules named 'u#test_program' found!")?;
        spawn::spawn_userspace(module, Some(String::from("test_program_2")))?;
    }

    // create and jump to a userspace thread that tests syscalls
    #[cfg(spawn_userspace)] {
        debug!("trying out a system call module");
        let module = memory::get_module("u#syscall_send").ok_or("Error: no module named 'u#syscall_send' found!")?;
        spawn::spawn_userspace(module, None)?;
    }

    // a second duplicate syscall test user task
    #[cfg(spawn_userspace)] {
        debug!("trying out a receive system call module");
        let module = memory::get_module("u#syscall_receive").ok_or("Error: no module named 'u#syscall_receive' found!")?;
        spawn::spawn_userspace(module, None)?;
    }

    
    // create a SIMD personality
    #[cfg(simd_personality)]
    {
        warn!("SIMD_PERSONALTIY FEATURE ENABLED!");
        KernelTaskBuilder::new(simd_personality::setup_simd_personality, None)
            .name(String::from("setup_simd_personality"))
            .spawn()?;
    }

    info!("captain::init(): initialization done! Enabling interrupts and entering Task 0's idle loop...");
    enable_interrupts();
    // NOTE: do not put any code below this point, as it should never run
    // (unless there are no other tasks available to run on the BSP core, which doesnt happen)
    

    loop { 
        spin_loop_hint();
        // TODO: exit this loop cleanly upon a shutdown signal
    }
}
