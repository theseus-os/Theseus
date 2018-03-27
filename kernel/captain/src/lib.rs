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


extern crate console_types; // a temporary way to use console types 
extern crate logger;
extern crate memory; // the virtual memory subsystem 
extern crate apic; 
extern crate mod_mgmt;
extern crate arch; 
extern crate spawn;
extern crate tsc;
extern crate syscall;
extern crate interrupts;
extern crate acpi;
extern crate driver_init;
extern crate e1000;



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
        
        if let Some(section) = ::mod_mgmt::metadata::get_symbol("console::print_to_console").upgrade() {
            let vaddr = section.virt_addr();
            let print_func: fn(String) -> Result<(), &'static str> = unsafe { ::core::mem::transmute(vaddr) };
            if let Err(e) = print_func(s.clone()) {
                error!("print_to_console() in print!() macro failed, error: {}  Printing: {}", e, s);
            }
        }
        else {
            error!("No \"console::print_to_console\" symbol in print!() macro! Printing: {}", s);
        }
    });
}


// Here, we add pub use statements for any function or data that we want to export from the nano_core
// and make visible/accessible to other modules that depend on nano_core functions.
// Or, just make the modules public above. Basically, they need to be exported from the nano_core like a regular library would.

use alloc::arc::Arc;
use alloc::{String, Vec};
use core::fmt;
use memory::{MemoryManagementInfo, MappedPages};
use e1000::test_nic_driver::test_nic_driver;
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use dfqueue::DFQueueProducer;
use console_types::ConsoleEvent;
use irq_safety::{MutexIrqSafe, enable_interrupts, interrupts_enabled};


fn test_loop_1(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_1!");
    loop {
        let mut i: usize = 100000000; // usize::max_value();
        unsafe { asm!(""); }
        while i > 0 {
            i -= 1;
        }
        print!("1");

        let section = ::mod_mgmt::metadata::get_symbol("scheduler::schedule").upgrade().expect("failed to get scheduler::schedule symbol!");
        let schedule_func: fn() -> bool = unsafe { ::core::mem::transmute(section.virt_addr()) };
        schedule_func();
    }
}


fn test_loop_2(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_2!");
    loop {
        let mut i: usize = 100000000; // usize::max_value();
        unsafe { asm!(""); }
        while i > 0 {
            i -= 1;
        }
        print!("2");
        
        let section = ::mod_mgmt::metadata::get_symbol("scheduler::schedule").upgrade().expect("failed to get scheduler::schedule symbol!");
        let schedule_func: fn() -> bool = unsafe { ::core::mem::transmute(section.virt_addr()) };
        schedule_func();
    }
}


fn test_loop_3(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_3!");
    
    // {
    //     use memory::{PhysicalMemoryArea, FRAME_ALLOCATOR};
    //     let test_area = PhysicalMemoryArea::new(0xFFF7000, 0x10000, 1, 3);
    //     FRAME_ALLOCATOR.try().unwrap().lock().add_area(test_area, false).unwrap();
    // }

    loop {
        let mut i: usize = 100000000; // usize::max_value();
        while i > 0 {
            unsafe { asm!(""); }
            i -= 1;
            // if i % 3 == 0 {
            //     debug!("GOT FRAME: {:?}",  memory::allocate_frame()); // TODO REMOVE
            //     debug!("GOT FRAMES: {:?}", memory::allocate_frames(20)); // TODO REMOVE
            // }
        }
        print!("3");
        
        let section = ::mod_mgmt::metadata::get_symbol("scheduler::schedule").upgrade().expect("failed to get scheduler::schedule symbol!");
        let schedule_func: fn() -> bool = unsafe { ::core::mem::transmute(section.virt_addr()) };
        schedule_func();
    }
}




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
	
    // calculate TSC period and initialize it
    // not strictly necessary, but more accurate if we do it early on before interrupts, multicore, and multitasking
    let _tsc_freq = tsc::get_tsc_frequency();


    // parse our other loadable modules and their dependencies
    if true {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let _one      = mod_mgmt::load_kernel_crate(memory::get_module("__k_log").unwrap(), &mut kernel_mmi, false).unwrap();
        let _two      = mod_mgmt::load_kernel_crate(memory::get_module("__k_keycodes_ascii").unwrap(), &mut kernel_mmi, false).unwrap();
        let _three    = mod_mgmt::load_kernel_crate(memory::get_module("__k_console_types").unwrap(), &mut kernel_mmi, false).unwrap();
        let _four     = mod_mgmt::load_kernel_crate(memory::get_module("__k_keyboard").unwrap(), &mut kernel_mmi, false).unwrap();
        let _five     = mod_mgmt::load_kernel_crate(memory::get_module("__k_console").unwrap(), &mut kernel_mmi, false).unwrap();
        let _sched    = mod_mgmt::load_kernel_crate(memory::get_module("__k_scheduler").unwrap(), &mut kernel_mmi, true).unwrap();
        // debug!("========================== Symbol map after __k_log {}, __k_keycodes_ascii {}, __k_console_types {}, __k_keyboard {}, __k_console {}: ========================\n{}", 
        //         _one, _two, _three, _four, _five, mod_mgmt::metadata::dump_symbol_map());
    }



    // now we initialize ACPI/APIC barebones stuff.
    // madt_iter must stay in scope until after all AP booting is finished so it's not prematurely dropped
    let madt_iter = {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        driver_init::early_init(&mut kernel_mmi).expect("Failed to get MADT (APIC) table iterator!")
    };


    // initialize the rest of the BSP's interrupt stuff, including TSS & GDT
    let (double_fault_stack, privilege_stack, syscall_stack) = { 
        let mut kernel_mmi = kernel_mmi_ref.lock();
        (
            kernel_mmi.alloc_stack(1).expect("could not allocate double fault stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("could not allocate privilege stack"),
            kernel_mmi.alloc_stack(KERNEL_STACK_SIZE_IN_PAGES).expect("could not allocate syscall stack")
        )
    };
    // the three stacks we allocated above are never dropped because they stay in scope in this function,
    // but IMO that's not a great design, and they should probably be stored by the interrupt module and the syscall module instead.
    interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable())
                .expect("failed to initialize interrupts!");


    // init other featureful (non-exception) interrupt handlers
    // interrupts::init_handlers_pic();
    interrupts::init_handlers_apic();

    syscall::init(syscall_stack.top_usable());

  
    // create the initial `Task`, i.e., task_zero
    let bsp_apic_id = apic::get_bsp_id().expect("nano_core_main(): Coudln't get BSP's apic_id!");
    spawn::init(kernel_mmi_ref.clone(), bsp_apic_id, bsp_stack_bottom, bsp_stack_top).unwrap();

    // initialize the kernel console
    let console_queue_producer = {
        let section = ::mod_mgmt::metadata::get_symbol("console::init").upgrade().expect("failed to get console::init() symbol!");
        let init_func: fn() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> = unsafe { ::core::mem::transmute(section.virt_addr()) };
        let console_producer = init_func().expect("console::init() failed!");
        console_producer
    };



    // initialize the rest of our drivers
    driver_init::init(console_queue_producer).unwrap();
    

    // boot up the other cores (APs)
    {
        let ap_count = acpi::madt::handle_ap_cores(madt_iter, kernel_mmi_ref.clone(), ap_start_realmode_begin, ap_start_realmode_end)
                       .expect("Error handling AP cores");
        
        info!("Finished handling and booting up all {} AP cores.", ap_count);
        assert!(apic::get_lapics().iter().count() == ap_count + 1, "SANITY CHECK FAILED: too many LocalApics in the list!");
    }


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


    println!("initialization done! Enabling interrupts to schedule away from Task 0 ...");
    enable_interrupts();

    if false {
        spawn::spawn_kthread(test_nic_driver, None, String::from("test_nic_driver")).unwrap();
    }  

    // create some extra tasks to test context switching
    if false {
        spawn::spawn_kthread(test_loop_1, None, String::from("test_loop_1")).unwrap();
        spawn::spawn_kthread(test_loop_2, None, String::from("test_loop_2")).unwrap(); 
        spawn::spawn_kthread(test_loop_3, None, String::from("test_loop_3")).unwrap(); 
    }

	
    
    // create and jump to the first userspace thread
    if true
    {
        debug!("trying to jump to userspace");
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        spawn::spawn_userspace(module, Some(String::from("test_program_1"))).unwrap();
    }

    if true
    {
        debug!("trying to jump to userspace 2nd time");
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        spawn::spawn_userspace(module, Some(String::from("test_program_2"))).unwrap();
    }

    // create and jump to a userspace thread that tests syscalls
    if false
    {
        debug!("trying out a system call module");
        let module = memory::get_module("syscall_send").expect("Error: no module named 'syscall_send' found!");
        spawn::spawn_userspace(module, None).unwrap();
    }

    // a second duplicate syscall test user task
    if false
    {
        debug!("trying out a receive system call module");
        let module = memory::get_module("syscall_receive").expect("Error: no module named 'syscall_receive' found!");
        spawn::spawn_userspace(module, None).unwrap();
    }

    enable_interrupts();
    debug!("nano_core_main(): entering Task 0's idle loop: interrupts enabled: {}", interrupts_enabled());

    assert!(interrupts_enabled(), "logical error: interrupts were disabled when entering the idle loop in nano_core_main()");
    loop { 
        // TODO: exit this loop cleanly upon a shutdown signal
        
        let section = ::mod_mgmt::metadata::get_symbol("scheduler::schedule").upgrade().expect("failed to get scheduler::schedule symbol!");
        let schedule_func: fn() -> bool = unsafe { ::core::mem::transmute(section.virt_addr()) };
        schedule_func();
        
        arch::pause();
    }


    // cleanup here
    // logger::shutdown().expect("WTF: failed to shutdown logger... oh well.");
}
