// Copyright 2017 Kevin Boos. 
// Licensed under the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// This file may not be copied, modified, or distributed
// except according to those terms.


#![no_std]

#![feature(alloc)]
#![feature(asm)]
#![feature(used)]
#![feature(core_intrinsics)]


extern crate alloc;
#[macro_use] extern crate log;


extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate dfqueue; // decoupled, fault-tolerant queue

extern crate input_event_types; // a temporary way to use input_event_manager types 
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
extern crate driver_init;
extern crate e1000;
extern crate window_manager;
extern crate scheduler;
#[macro_use] extern crate input_event_manager;
extern crate exceptions_full;


#[cfg(target_feature = "sse2")]
extern crate simd_test;


// Here, we add pub use statements for any function or data that we want to export from the nano_core
// and make visible/accessible to other modules that depend on nano_core functions.
// Or, just make the modules public above. Basically, they need to be exported from the nano_core like a regular library would.

use alloc::arc::Arc;
use alloc::{String, Vec};
use core::fmt;
use core::ops::DerefMut;
use core::sync::atomic::spin_loop_hint;
use memory::{MemoryManagementInfo, MappedPages, PageTable};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use irq_safety::{MutexIrqSafe, enable_interrupts};




/// the callback use in the logger crate for mirroring log functions to the input_event_manager
pub fn mirror_to_vga_cb(_color: logger::LogColor, prefix: &'static str, args: fmt::Arguments) {
    println!("{} {}", prefix, args);
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
    #[cfg(feature = "mirror_serial")]
    {
        // enable mirroring of serial port logging outputs to VGA buffer (for real hardware)
        logger::mirror_to_vga(mirror_to_vga_cb);
    }
    // at this point, we no longer *need* to use println_raw, because we can see the logs,
    // either from the serial port on an emulator, or because they're mirrored to the VGA buffer on real hardware.

    // calculate TSC period and initialize it
    // not strictly necessary, but more accurate if we do it early on before interrupts, multicore, and multitasking
    let _tsc_freq = tsc::get_tsc_frequency()?;
    // info!("TSC frequency calculated: {}", _tsc_freq);

    // now we initialize early driver stuff, like APIC/ACPI
    let madt_iter = driver_init::early_init(kernel_mmi_ref.lock().deref_mut())?;

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

    // initialize the kernel input_event_manager
    let input_event_queue_producer = input_event_manager::init()?;

    // initialize the rest of our drivers
    driver_init::init(input_event_queue_producer)?;
    
    // boot up the other cores (APs)
    let ap_count = acpi::madt::handle_ap_cores(madt_iter, kernel_mmi_ref.clone(), ap_start_realmode_begin, ap_start_realmode_end)?;
    info!("Finished handling and booting up all {} AP cores.", ap_count);

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


    // //init frame_buffer
    // let rs = frame_buffer::init();
    // if rs.is_ok() {
    //     trace!("frame_buffer initialized.");
    // } else {
    //     debug!("nano_core::nano_core_start: {}", rs.unwrap_err());
    // }
    // let rs = frame_buffer_3d::init();
    // if rs.is_ok() {
    //     trace!("frame_buffer initialized.");
    // } else {
    //     debug!("nano_core::nano_core_start: {}", rs.unwrap_err());
    // }


    // testing nic
    // TODO: remove this (@Ramla)
    if false {
        use e1000::test_nic_driver::test_nic_driver;
        spawn::spawn_kthread(test_nic_driver, None, String::from("test_nic_driver"), None)?;
    }  

    //test window manager
    if false {
        use window_manager::test_window_manager;
        spawn::spawn_kthread(test_window_manager::test_cursor, None, String::from("test_cursor"), None).unwrap();
        spawn::spawn_kthread(test_window_manager::test_draw, None, String::from("test_draw"), None).unwrap();
    }

    // create and jump to the first userspace thread
    if false {
        debug!("trying to jump to userspace");
        let module = memory::get_module("__u_test_program").ok_or("Error: no userspace modules named '__u_test_program' found!")?;
        spawn::spawn_userspace(module, Some(String::from("test_program_1")))?;
    }

    if false {
        debug!("trying to jump to userspace 2nd time");
        let module = memory::get_module("__u_test_program").ok_or("Error: no userspace modules named '__u_test_program' found!")?;
        spawn::spawn_userspace(module, Some(String::from("test_program_2")))?;
    }

    // create and jump to a userspace thread that tests syscalls
    if false {
        debug!("trying out a system call module");
        let module = memory::get_module("__u_syscall_send").ok_or("Error: no module named '__u_syscall_send' found!")?;
        spawn::spawn_userspace(module, None)?;
    }

    // a second duplicate syscall test user task
    if false {
        debug!("trying out a receive system call module");
        let module = memory::get_module("__u_syscall_receive").ok_or("Error: no module named '__u_syscall_receive' found!")?;
        spawn::spawn_userspace(module, None)?;
    }


    #[cfg(target_feature = "sse2")]
    {
        spawn::spawn_kthread(simd_test::test1, (), String::from("simd_test_1"), None).unwrap();
        spawn::spawn_kthread(simd_test::test2, (), String::from("simd_test_2"), None).unwrap();
        spawn::spawn_kthread(simd_test::test3, (), String::from("simd_test_3"), None).unwrap();
        
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
