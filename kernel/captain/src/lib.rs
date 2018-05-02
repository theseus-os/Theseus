// Copyright 2017 Kevin Boos. 
// Licensed under the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// This file may not be copied, modified, or distributed
// except according to those terms.


#![no_std]

#![feature(alloc)]
#![feature(asm)]
#![feature(used)]

extern crate alloc;
#[macro_use] extern crate log;


extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate dfqueue; // decoupled, fault-tolerant queue

#[cfg(feature = "loadable")]
#[macro_use] extern crate vga_buffer;
#[cfg(not(feature = "loadable"))]
extern crate vga_buffer;

extern crate console_types; // a temporary way to use console types 
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
extern crate console;

#[cfg(target_feature = "sse2")]
extern crate simd_test;

// temporarily moving these macros here because I'm not sure if/how we can load macros from a crate at runtime
/// calls print!() with an extra "\n" at the end. 
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}

/// The main printing macro, which simply pushes an output event to the console's event queue. 
/// This ensures that only one thread (the console) ever accesses the UI, which right now is just the VGA buffer.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        use core::fmt::Write;
        use alloc::String;
        let mut s: String = String::new();
        match write!(&mut s, $($arg)*) {
            Ok(_) => { }
            Err(e) => error!("Writing to String in print!() macro failed, error: {}", e),
        }
        
        #[cfg(feature = "loadable")] {
            if let Some(section) = ::mod_mgmt::metadata::get_symbol("console::print_to_console").upgrade() {
                let vaddr = section.virt_addr();
                let print_func: fn(String) -> Result<(), &'static str> = unsafe { ::core::mem::transmute(vaddr) };
                let _ = print_func(s.clone());
            }
            else {
                // if console crate hasn't been loaded yet, write to the raw VGA buffer instead
                println_raw!("Couldn't get \"console::print_to_console\" symbol! Tried to print: {}", s);
                // error!("No \"console::print_to_console\" symbol in print!() macro! Printing: {}", s);
            }
        }
        #[cfg(not(feature = "loadable"))]
        {
            let _ = console::print_to_console(s);
        } 
    });
}


// Here, we add pub use statements for any function or data that we want to export from the nano_core
// and make visible/accessible to other modules that depend on nano_core functions.
// Or, just make the modules public above. Basically, they need to be exported from the nano_core like a regular library would.

use alloc::arc::Arc;
use alloc::{String, Vec};
use core::fmt;
use core::sync::atomic::spin_loop_hint;
use memory::{MemoryManagementInfo, MappedPages};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use irq_safety::{MutexIrqSafe, enable_interrupts};

#[cfg(feature = "loadable")] use task::Task;
#[cfg(feature = "loadable")] use memory::{VirtualAddress, ModuleArea};
#[cfg(feature = "loadable")] use console_types::ConsoleEvent;
#[cfg(feature = "loadable")] use dfqueue::DFQueueProducer;
#[cfg(feature = "loadable")] use irq_safety::RwLockIrqSafe;
#[cfg(feature = "loadable")] use acpi::madt::MadtIter;




/// the callback use in the logger crate for mirroring log functions to the console
pub fn mirror_to_vga_cb(_color: logger::LogColor, prefix: &'static str, args: fmt::Arguments) {
    println!("{} {}", prefix, args);
}



/// Initialize the Captain, which is the main module that steers the ship of Theseus. 
/// IT does all the rest of the module loading and initialization so that the OS 
/// can continue running and do actual useful work.
pub fn init(kernel_mmi_ref: Arc<MutexIrqSafe<MemoryManagementInfo>>, 
            identity_mapped_pages: Vec<MappedPages>,
            bsp_stack_bottom: usize, bsp_stack_top: usize,
            ap_start_realmode_begin: usize, ap_start_realmode_end: usize) 
{
	
    #[cfg(feature = "loadable")]
    {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_util").unwrap(), &mut kernel_mmi, false).unwrap();        
        mod_mgmt::load_kernel_crate(memory::get_module("__k_atomic").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_port_io").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_serial_port").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_irq_safety").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_bit_field").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_log").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_x86_64").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_pit_clock").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_tsc").unwrap(), &mut kernel_mmi, false).unwrap();
    }

    // calculate TSC period and initialize it
    // not strictly necessary, but more accurate if we do it early on before interrupts, multicore, and multitasking
    let _tsc_freq = {
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("tsc::get_tsc_frequency").upgrade().expect("tsc::get_tsc_frequency").virt_addr();
            let func: fn() -> Result<u64, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func()
        }
        #[cfg(not(feature = "loadable"))]
        {
            tsc::get_tsc_frequency()
        }   
    };


    // load the rest of our crate dependencies
    #[cfg(feature = "loadable")]
    {
        let mut kernel_mmi = kernel_mmi_ref.lock();

        mod_mgmt::load_kernel_crate(memory::get_module("__k_keycodes_ascii").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_console_types").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_keyboard").unwrap(), &mut kernel_mmi, false).unwrap();
        
        mod_mgmt::load_kernel_crate(memory::get_module("__k_spin").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_pci").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_ioapic").unwrap(), &mut kernel_mmi, false).unwrap();
        
        mod_mgmt::load_kernel_crate(memory::get_module("__k_raw_cpuid").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_apic").unwrap(), &mut kernel_mmi, false).unwrap();
    
        mod_mgmt::load_kernel_crate(memory::get_module("__k_tss").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_gdt").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_pic").unwrap(), &mut kernel_mmi, false).unwrap();

        mod_mgmt::load_kernel_crate(memory::get_module("__k_task").unwrap(), &mut kernel_mmi, false).unwrap(); 
        mod_mgmt::load_kernel_crate(memory::get_module("__k_scheduler").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_spawn").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_interrupts").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_vga_buffer").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_console").unwrap(), &mut kernel_mmi, false).unwrap();
        

        mod_mgmt::load_kernel_crate(memory::get_module("__k_dbus").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_syscall").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_ap_start").unwrap(), &mut kernel_mmi, false).unwrap();

        mod_mgmt::load_kernel_crate(memory::get_module("__k_acpi").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_e1000").unwrap(), &mut kernel_mmi, false).unwrap();
        mod_mgmt::load_kernel_crate(memory::get_module("__k_driver_init").unwrap(), &mut kernel_mmi, false).unwrap();
    }


    // now we initialize early driver stuff, like APIC/ACPI
    let madt_iter = {
        let mut kernel_mmi = kernel_mmi_ref.lock();

        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("driver_init::early_init").upgrade().expect("driver_init::early_init").virt_addr();
            let func: fn(&mut memory::MemoryManagementInfo) -> Result<MadtIter, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func(&mut kernel_mmi)
        }
        #[cfg(not(feature = "loadable"))]
        {
            driver_init::early_init(&mut kernel_mmi)
        }
    }.expect("Failed to get MADT (APIC) table iterator!");


    // initialize the rest of the BSP's interrupt stuff, including TSS & GDT
    let (double_fault_stack, privilege_stack, syscall_stack) = { 
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(1).expect("could not allocate double fault stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("could not allocate privilege stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("could not allocate syscall stack")
        )
    };

    #[cfg(feature = "loadable")]
    {
        let vaddr = mod_mgmt::metadata::get_symbol("interrupts::init").upgrade().expect("interrupts::init").virt_addr();
        let func: fn(usize, usize) -> Result<(), &'static str> = unsafe { ::core::mem::transmute(vaddr) };
        func(double_fault_stack.top_unusable(), privilege_stack.top_unusable()).expect("failed to initialize interrupts!");
    } 
    #[cfg(not(feature = "loadable"))] 
    {
        interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable()).expect("failed to initialize interrupts!");
    }
    

    // init other featureful (non-exception) interrupt handlers
    // interrupts::init_handlers_pic();
    #[cfg(feature = "loadable")] 
    {
        let vaddr = mod_mgmt::metadata::get_symbol("interrupts::init_handlers_apic").upgrade().expect("interrupts::init_handlers_apic").virt_addr();
        let func: fn() = unsafe { ::core::mem::transmute(vaddr) };
        func();
    } 
    #[cfg(not(feature = "loadable"))]
    {
        interrupts::init_handlers_apic();
    }

    // initialize the syscall subsystem
    #[cfg(feature = "loadable")]
    {
        let vaddr = mod_mgmt::metadata::get_symbol("syscall::init").upgrade().expect("syscall::init").virt_addr();
        let func: fn(usize) = unsafe { ::core::mem::transmute(vaddr) };
        func(syscall_stack.top_usable());
    }
    #[cfg(not(feature = "loadable"))]
    {
        syscall::init(syscall_stack.top_usable());
    }

    // get BSP's apic id
    let bsp_apic_id = {
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("apic::get_bsp_id").upgrade().expect("apic::get_bsp_id").virt_addr();
            let func: fn() -> Option<u8> = unsafe { ::core::mem::transmute(vaddr) };
            func().expect("captain::init(): Coudln't get BSP's apic_id!")
        }
        #[cfg(not(feature = "loadable"))]
        {
            apic::get_bsp_id().expect("captain::init(): Coudln't get BSP's apic_id!")
        }
    };
    
    
    // create the initial `Task`, i.e., task_zero
    #[cfg(feature = "loadable")] 
    {
        let vaddr = mod_mgmt::metadata::get_symbol("spawn::init").upgrade().expect("spawn::init").virt_addr();
        let func: fn(Arc<MutexIrqSafe<MemoryManagementInfo>>, u8, VirtualAddress, VirtualAddress) -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
        func(kernel_mmi_ref.clone(), bsp_apic_id, bsp_stack_bottom, bsp_stack_top).unwrap();
    } 
    #[cfg(not(feature = "loadable"))]
    {
        spawn::init(kernel_mmi_ref.clone(), bsp_apic_id, bsp_stack_bottom, bsp_stack_top).unwrap();
    }


    // initialize the kernel console
    let console_queue_producer = {
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("console::init").upgrade().expect("console::init").virt_addr();
            let func: fn() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func().expect("console::init() failed!")
        } 
        #[cfg(not(feature = "loadable"))]
        {
            console::init().expect("console::init() failed!")
        }
    };



    // initialize the rest of our drivers
    #[cfg(feature = "loadable")]
    {
        let vaddr = mod_mgmt::metadata::get_symbol("driver_init::init").upgrade().expect("driver_init::init").virt_addr();
        let func: fn(DFQueueProducer<ConsoleEvent>) -> Result<(), &'static str> = unsafe { ::core::mem::transmute(vaddr) };
        func(console_queue_producer).unwrap();
    }
    #[cfg(not(feature = "loadable"))]
    {
        driver_init::init(console_queue_producer).unwrap();
    }
    

    // boot up the other cores (APs)
    let ap_count = {
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("acpi::madt::handle_ap_cores").upgrade().expect("acpi::madt::handle_ap_cores").virt_addr();
            let func: fn(MadtIter, Arc<MutexIrqSafe<MemoryManagementInfo>>, usize, usize) -> Result<usize, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func(madt_iter, kernel_mmi_ref.clone(), ap_start_realmode_begin, ap_start_realmode_end)
                .expect("Error handling AP cores")
        }
        #[cfg(not(feature = "loadable"))]
        {
            acpi::madt::handle_ap_cores(madt_iter, kernel_mmi_ref.clone(), ap_start_realmode_begin, ap_start_realmode_end)
                .expect("Error handling AP cores")
        }
    };
    info!("Finished handling and booting up all {} AP cores.", ap_count);
    // assert!(apic::get_lapics().iter().count() == ap_count + 1, "SANITY CHECK FAILED: too many LocalApics in the list!");


    // before we jump to userspace, we need to unmap the identity-mapped section of the kernel's page tables, at PML4[0]
    // unmap the kernel's original identity mapping (including multiboot2 boot_info) to clear the way for userspace mappings
    // we cannot do this until we have booted up all the APs
    ::core::mem::drop(identity_mapped_pages);
    {
        use memory::PageTable;
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let ref mut kernel_page_table = kernel_mmi.page_table;
        
        match kernel_page_table {
            &mut PageTable::Active(ref mut active_table) => {
                // for i in 0 .. 512 { 
                //     debug!("P4[{:03}] = {:#X}", i, active_table.p4().get_entry_value(i));
                // }

                // clear the 0th P4 entry, which covers any outstanding identity mappings
                active_table.p4_mut().clear_entry(0); 
            }
            _ => { }
        }
    }


    if true {
        // #[cfg(feature = "loadable")]
        // {
        //     let vaddr = mod_mgmt::metadata::get_symbol("e1000::test_nic_driver::test_nic_driver").upgrade().expect("e1000::test_nic_driver::test_nic_driver").virt_addr();
        //     let func: fn(Option<u64>) = unsafe { ::core::mem::transmute(vaddr) };
        //     spawn::spawn_kthread(func, None, String::from("test_nic_driver")).unwrap();
        // }
        #[cfg(not(feature = "loadable"))]
        {
            use e1000::test_nic_driver::test_nic_driver;
            spawn::spawn_kthread(test_nic_driver, None, String::from("test_nic_driver"), None).unwrap();
        }
    }  

    //test window manager
    if false {
        #[cfg(not(feature = "loadable"))]
        {
            use window_manager::test_window_manager;
            spawn::spawn_kthread(test_window_manager::test_cursor, None, String::from("test_cursor"), None).unwrap();
            spawn::spawn_kthread(test_window_manager::test_draw, None, String::from("test_draw"), None).unwrap();

        }
    }

     //test window manager
    if true {
        #[cfg(not(feature = "loadable"))]
        {
            use window_manager::test_window_manager;
            spawn::spawn_kthread(test_window_manager::test_text, None, String::from("test_text"), None).unwrap();
        }
    }

    // create and jump to the first userspace thread
    if true
    {
        debug!("trying to jump to userspace");
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("spawn::spawn_userspace").upgrade().expect("spawn::spawn_userspace").virt_addr();
            let func: fn(&ModuleArea, Option<String>) -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func(module, Some(String::from("test_program_1"))).unwrap();
        }
        #[cfg(not(feature = "loadable"))]
        {
            spawn::spawn_userspace(module, Some(String::from("test_program_1"))).unwrap();
        }
    }

    if true
    {
        debug!("trying to jump to userspace 2nd time");
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("spawn::spawn_userspace").upgrade().expect("spawn::spawn_userspace").virt_addr();
            let func: fn(&ModuleArea, Option<String>) -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func(module, Some(String::from("test_program_2"))).unwrap();
        }
        #[cfg(not(feature = "loadable"))]
        {
            spawn::spawn_userspace(module, Some(String::from("test_program_2"))).unwrap();
        }
    }

    // create and jump to a userspace thread that tests syscalls
    if false
    {
        debug!("trying out a system call module");
        let module = memory::get_module("syscall_send").expect("Error: no module named 'syscall_send' found!");
        
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("spawn::spawn_userspace").upgrade().expect("spawn::spawn_userspace").virt_addr();
            let func: fn(&ModuleArea, Option<String>) -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func(module, None).unwrap();
        }
        #[cfg(not(feature = "loadable"))]
        {
            spawn::spawn_userspace(module, None).unwrap();
        }
    }

    // a second duplicate syscall test user task
    if false
    {
        debug!("trying out a receive system call module");
        let module = memory::get_module("syscall_receive").expect("Error: no module named 'syscall_receive' found!");
        
        #[cfg(feature = "loadable")]
        {
            let vaddr = mod_mgmt::metadata::get_symbol("spawn::spawn_userspace").upgrade().expect("spawn::spawn_userspace").virt_addr();
            let func: fn(&ModuleArea, Option<String>) -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            func(module, None).unwrap();
        }
    #[cfg(not(feature = "loadable"))]
        {
            spawn::spawn_userspace(module, None).unwrap();
        }
    }

    #[cfg(target_feature = "sse2")]
    {
        spawn::spawn_kthread(simd_test::test1, (), String::from("simd_test_1"), None).unwrap();
        spawn::spawn_kthread(simd_test::test2, (), String::from("simd_test_2"), None).unwrap();
        spawn::spawn_kthread(simd_test::test3, (), String::from("simd_test_3"), None).unwrap();
        
    }

    println!("initialization done! Enabling interrupts to schedule away from Task 0 ...");
    debug!("captain::init(): initialization done! Enabling interrupts and entering Task 0's idle loop...");
    enable_interrupts();

    // the below should never run unless there are no other tasks available to run on the BSP core
    
    loop { 
        spin_loop_hint();
        // TODO: exit this loop cleanly upon a shutdown signal
    }

}
