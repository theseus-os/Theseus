// Copyright 2017 Kevin Boos. 
// Licensed under the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// This file may not be copied, modified, or distributed
// except according to those terms.



#![feature(lang_items)]
#![feature(const_fn, unique)]
#![feature(alloc)]
#![feature(asm)]
#![feature(naked_functions)]
#![feature(abi_x86_interrupt)]
#![feature(compiler_fences)]
#![feature(iterator_step_by)]
#![feature(core_intrinsics)]
#![feature(conservative_impl_trait)]
#![feature(used)]
#![feature(i128_type)]
#![no_std]


#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 


// this is needed for our odd approach of loading the libcore ELF file at runtime
#![feature(compiler_builtins_lib)]
extern crate compiler_builtins;



// ------------------------------------
// ----- EXTERNAL CRATES BELOW --------
// ------------------------------------
extern crate rlibc; // basic memset/memcpy libc functions
// extern crate volatile;
extern crate spin; // core spinlocks 
extern crate multiboot2;
extern crate x86;
extern crate x86_64;
#[macro_use] extern crate once; // for assert_has_not_been_called!()
#[macro_use] extern crate lazy_static; // for lazy static initialization
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate atomic;


// ------------------------------------
// ------ OUR OWN CRATES BELOW --------
// ----------  LIBRARIES   ------------
// ------------------------------------
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate keycodes_ascii; // for keyboard scancode translation
#[macro_use] extern crate util;
extern crate port_io; // for port_io, replaces external crate "cpu_io"
extern crate heap_irq_safe; // our wrapper around the linked_list_allocator crate
extern crate dfqueue; // decoupled, fault-tolerant queue
extern crate atomic_linked_list;


// ------------------------------------
// -------  THESEUS MODULES   ---------
// ------------------------------------
pub mod reexports;
extern crate console_types; // a temporary way to use console types 
extern crate serial_port;
extern crate logger;
extern crate state_store;
#[macro_use] extern crate vga_buffer; 
extern crate rtc;
extern crate memory; // the virtual memory subsystem 
extern crate task;
extern crate pit_clock;
extern crate apic; 
extern crate mod_mgmt;
extern crate tss;
extern crate gdt;
extern crate arch; 
extern crate spawn;



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






#[macro_use] mod drivers;  
mod dbus;
pub mod interrupts;
mod syscall;
mod start;

// Here, we add pub use statements for any function or data that we want to export from the nano_core
// and make visible/accessible to other modules that depend on nano_core functions.
// Or, just make the modules public above. Basically, they need to be exported from the nano_core like a regular library would.


use alloc::String;
use core::fmt;
use drivers::{pci, test_nic_driver};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;
use dfqueue::DFQueueProducer;
use console_types::ConsoleEvent;
use irq_safety::{enable_interrupts, interrupts_enabled};


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

pub fn test_driver(_: Option<u64>) {
    println!("TESTING DRIVER!!");
    test_nic_driver::dhcp_request_packet();

}

// see this: https://doc.rust-lang.org/1.22.1/unstable-book/print.html#used
#[link_section = ".pre_init_array"] // "pre_init_array" is a section never removed by --gc-sections
#[used]
pub fn nano_core_public_func(val: u8) {
    error!("NANO_CORE_PUBLIC_FUNC: got val {}", val);
}


/// the callback use in the logger crate for mirroring log functions to the console
fn mirror_to_vga_cb(_color: logger::LogColor, prefix: &'static str, args: fmt::Arguments) {
    println!("{} {}", prefix, args);
}


#[no_mangle]
pub extern "C" fn rust_main(multiboot_information_virtual_address: usize) {
	
	// start the kernel with interrupts disabled
	irq_safety::disable_interrupts();
	
    // first, bring up the logger so we can debug
    logger::init().expect("WTF: couldn't init logger.");
    trace!("Logger initialized.");
    
    // initialize basic exception handlers
    interrupts::init_early_exceptions();

    // calculate TSC period and initialize it
    interrupts::tsc::init().expect("couldn't init TSC timer");


    // safety-wise, we just have to trust the multiboot address we get from the boot-up asm code
    let boot_info = unsafe { multiboot2::load(multiboot_information_virtual_address) };
    // debug!("multiboot2 boot_info: {:?}", boot_info);
    // debug!("end of multiboot2 info");
    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    // this returns a MMI struct with the page table, stack allocator, and VMA list for the kernel's address space (idle_task_ap0)
    let (kernel_mmi_ref, identity_mapped_pages) = memory::init(boot_info, apic::broadcast_tlb_shootdown).unwrap(); // consumes boot_info


    // now that we have a heap, we can create basic things like state_store
    state_store::init();
    if cfg!(feature = "mirror_serial") {
         // enables mirroring of serial port logging outputs to VGA buffer (for real hardware)
        logger::mirror_to_vga(mirror_to_vga_cb);
    }
    trace!("state_store initialized.");

    // parse our two main crates, the nano_core (the code we're already running), and the libcore (Rust no_std lib),
    // both which satisfy dependencies that many other crates have. 
    if true {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let _num_nano_core_syms = mod_mgmt::load_kernel_crate(memory::get_module("__k_nano_core").unwrap(), &mut kernel_mmi, false).unwrap();
        // debug!("========================== Symbol map after __k_nano_core {}: ========================\n{}", _num_nano_core_syms, mod_mgmt::metadata::dump_symbol_map());
        let _num_libcore_syms = mod_mgmt::load_kernel_crate(memory::get_module("__k_libcore").unwrap(), &mut kernel_mmi, false).unwrap();
        // debug!("========================== Symbol map after nano_core {} and libcore {}: ========================\n{}", _num_nano_core_syms, _num_libcore_syms, mod_mgmt::metadata::dump_symbol_map());
    }


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
        drivers::early_init(&mut kernel_mmi).expect("Failed to get MADT (APIC) table iterator!")
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
    let bsp_apic_id = apic::get_bsp_id().expect("rust_main(): Coudln't get BSP's apic_id!");
    spawn::init(kernel_mmi_ref.clone(), bsp_apic_id, get_bsp_stack_bottom(), get_bsp_stack_top()).unwrap();

    // initialize the kernel console
    let console_queue_producer = {
        let section = ::mod_mgmt::metadata::get_symbol("console::init").upgrade().expect("failed to get console::init() symbol!");
        let init_func: fn() -> Result<DFQueueProducer<ConsoleEvent>, &'static str> = unsafe { ::core::mem::transmute(section.virt_addr()) };
        let console_producer = init_func().expect("console::init() failed!");
        console_producer
    };



    // initialize the rest of our drivers
    drivers::init(console_queue_producer).unwrap();
    

    // boot up the other cores (APs)
    {
        let ap_count = drivers::acpi::madt::handle_ap_cores(madt_iter, kernel_mmi_ref.clone())
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
        spawn::spawn_kthread(test_driver, None, String::from("driver_test_thread")).unwrap();
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
    debug!("rust_main(): entering Task 0's idle loop: interrupts enabled: {}", interrupts_enabled());

    assert!(interrupts_enabled(), "logical error: interrupts were disabled when entering the idle loop in rust_main()");
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


#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
pub extern "C" fn eh_personality() {}


#[cfg(not(test))]
#[lang = "panic_fmt"]
#[no_mangle]
pub extern "C" fn panic_fmt(fmt: core::fmt::Arguments, file: &'static str, line: u32) -> ! {
    let apic_id = apic::get_my_apic_id().unwrap_or(0xFF);

    error!("\n\nPANIC (AP {:?}) in {} at line {}:", apic_id, file, line);
    error!("    {}", fmt);
    unsafe { memory::stack_trace(); }

    println_early!("\n\nPANIC (AP {:?}) in {} at line {}:", apic_id, file, line);
    println_early!("    {}", fmt);


    loop {}
}

/// This function isn't used since our Theseus target.json file
/// chooses panic=abort (as does our build process), 
/// but building on Windows (for an IDE) with the pc-windows-gnu toolchain requires it.
#[allow(non_snake_case)]
#[lang = "eh_unwind_resume"]
#[no_mangle]
#[cfg(all(target_os = "windows", target_env = "gnu"))]
pub extern "C" fn rust_eh_unwind_resume(_arg: *const i8) -> ! {
    error!("\n\nin rust_eh_unwind_resume, unimplemented!");
    println_early!("\n\nin rust_eh_unwind_resume, unimplemented!");
    loop {}
}


#[allow(non_snake_case)]
#[no_mangle]
#[cfg(not(target_os = "windows"))]
pub extern "C" fn _Unwind_Resume() -> ! {
    error!("\n\nin _Unwind_Resume, unimplemented!");
    println_early!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}


// symbols exposed in the initial assembly boot files
// DO NOT DEREFERENCE THESE DIRECTLY!! THEY ARE SIMPLY ADDRESSES USED FOR SIZE CALCULATIONS.
// A DIRECT ACCESS WILL CAUSE A PAGE FAULT
extern {
    static initial_bsp_stack_top: usize;
    static initial_bsp_stack_bottom: usize;
    static ap_start_realmode: usize;
    static ap_start_realmode_end: usize;
}

use kernel_config::memory::KERNEL_OFFSET;
/// Returns the starting virtual address of where the ap_start realmode code is.
fn get_ap_start_realmode() -> usize {
    let addr = unsafe { &ap_start_realmode as *const _ as usize };
    debug!("ap_start_realmode addr: {:#x}", addr);
    addr + KERNEL_OFFSET
}

/// Returns the ending virtual address of where the ap_start realmode code is.
fn get_ap_start_realmode_end() -> usize {
    let addr = unsafe { &ap_start_realmode_end as *const _ as usize };
    debug!("ap_start_realmode_end addr: {:#x}", addr);
    addr + KERNEL_OFFSET
} 

fn get_bsp_stack_bottom() -> usize {
    unsafe {
        &initial_bsp_stack_bottom as *const _ as usize
    }
}

fn get_bsp_stack_top() -> usize {
    unsafe {
        &initial_bsp_stack_top as *const _ as usize
    }
}


// /// TODO FIXME:  simple temporary wrapper for spawn_kthread in the task crate.
// ///              should be removed once the Captain crate is up and running
// #[inline(never)]
// pub fn spawn_kthread<A: fmt::Debug, R: fmt::Debug>(func: fn(arg: A) -> R, arg: A, thread_name: String)
//         -> Result<Arc<RwLockIrqSafe<Task>>, &'static str> {




// CODE FOR TESTING ATA DMA
/*
let bus_array = pci::PCI_BUSES.try().expect("PCI_BUSES not initialized");
    
    let ref bus_zero = bus_array[0];
    let ref slot_zero = bus_zero.connected_devices[0]; 
    println!("pci config data for bus 0, slot 0: dev id - {:#x}, class - {:#x}, subclass - {:#x}", slot_zero.device_id, slot_zero.class, slot_zero.subclass);
    println!("{:?}", bus_zero);
    // pci::allocate_mem();
    let data = ata_pio::pio_read(0xE0,0).unwrap();
    
    println!("ATA PIO read data: ==========================");
    for sh in data.iter() {
        print!("{:#x} ", sh);
    }
    println!("=============================================");
    
    let paddr = pci::read_from_disk(0xE0,0).unwrap() as usize;

    // TO CHECK PHYSICAL MEMORY:
    //  In QEMU, press Ctrl + Alt + 2
    //  xp/x 0x2b5000   
    //        ^^ substitute the frame_start value
    // xp means "print physical memory",   /x means format as hex



    let vaddr: usize = {
        let mut curr_task = get_my_current_task().unwrap().write();
        let curr_mmi = curr_task.mmi.as_ref().unwrap();
        let mut curr_mmi_locked = curr_mmi.lock();
        use memory::*;
        let vaddr = curr_mmi_locked.map_dma_memory(paddr, 512, PRESENT | WRITABLE);
        println!("\n========== VMAs after DMA ============");
        for vma in curr_mmi_locked.vmas.iter() {
            println!("    vma: {:?}", vma);
        }
        println!("=====================================");
        vaddr
    };
    let dataptr = vaddr as *const u16;
    let dma_data = unsafe { collections::slice::from_raw_parts(dataptr, 256) };
    println!("======================DMA read data phys_addr: {:#x}: ==========================", paddr);
    for i in 0..256 {
        print!("{:#x} ", dma_data[i]);
    }
    println!("\n========================================================");
*/
