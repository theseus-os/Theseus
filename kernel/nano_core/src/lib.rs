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
#![no_std]


#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 


// #![feature(compiler_builtins_lib)]  // this is needed for our odd approach of including the nano_core as a library for other kernel crates
// extern crate compiler_builtins; // this is needed for our odd approach of including the nano_core as a library for other kernel crates


// ------------------------------------
// ----- EXTERNAL CRATES BELOW --------
// ------------------------------------
extern crate rlibc; // basic memset/memcpy libc functions
extern crate volatile;
extern crate spin; // core spinlocks 
extern crate multiboot2;
#[macro_use] extern crate bitflags;
extern crate x86;
extern crate x86_64;
#[macro_use] extern crate once; // for assert_has_not_been_called!()
extern crate bit_field;
#[macro_use] extern crate lazy_static; // for lazy static initialization
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate atomic;
extern crate xmas_elf;
extern crate rustc_demangle;
extern crate goblin;
extern crate zero;


// ------------------------------------
// ------ OUR OWN CRATES BELOW --------
// ----------  LIBRARIES   ------------
// ------------------------------------
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate keycodes_ascii; // for keyboard 
extern crate port_io; // for port_io, replaces external crate "cpu_io"
extern crate heap_irq_safe; // our wrapper around the linked_list_allocator crate
extern crate dfqueue; // decoupled, fault-tolerant queue
extern crate atomic_linked_list;

// ------------------------------------
// -------  THESEUS MODULES   ---------
// ------------------------------------
extern crate serial_port;
extern crate logger;
extern crate state_store;
#[macro_use] extern crate vga_buffer; 
extern crate test_lib;
extern crate rtc;

#[macro_use] mod console;  // I think this mod declaration MUST COME FIRST because it includes the macro for println!
#[macro_use] pub mod util; // must come first because it contains just macros
#[macro_use] mod drivers;  
mod arch;
#[macro_use] mod task;
#[macro_use] mod dbus;
mod memory;
mod interrupts;
mod syscall;
mod mod_mgmt;
mod start;

// TODO FIXME: add pub use statements for any function or data that we want to export from the nano_core
// and make visible/accessible to other modules that depend on nano_core functions.
// Or, just make the modules public above. Basically, they need to be exported from the nano_core like a regular library would.


use task::{spawn_kthread, spawn_userspace};
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
// use drivers::{ata_pio, pci};
use dbus::{BusConnection, BusMessage, BusConnectionTable, get_connection_table};
use kernel_config::memory::KERNEL_STACK_SIZE_IN_PAGES;


fn test_loop_1(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_1!");
    loop {
        let mut i = 10000000; // usize::max_value();
        while i > 0 {
            i -= 1;
        }
        print!("1");
    }
}


fn test_loop_2(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_2!");
    loop {
        let mut i = 10000000; // usize::max_value();
        while i > 0 {
            i -= 1;
        }
        print!("2");
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
        let mut i = 10000000; // usize::max_value();
        while i > 0 {
            i -= 1;
            // if i % 3 == 0 {
            //     debug!("GOT FRAME: {:?}",  memory::allocate_frame()); // TODO REMOVE
            //     debug!("GOT FRAMES: {:?}", memory::allocate_frames(20)); // TODO REMOVE
            // }
        }
        print!("3");
    }
}




fn first_thread_main(arg: Option<u64>) -> u64  {
    println!("Hello from first thread, arg: {:?}!!", arg);
    
    unsafe{
        let mut table = get_connection_table().write();
        let mut connection = table.get_connection(String::from("bus.connection.first"))
            .expect("Fail to create the first bus connection").write();
        println!("Create the first connection.");

  //      loop {
            let obj = connection.receive();
            if(obj.is_some()){
                println!("{}", obj.unwrap().data);
            } else {
                println!("No message!");
            }
 //
  //      }
        print!("3");
    }
    1
}

fn second_thread_main(arg: u64) -> u64  {
    println!("Hello from second thread, arg: {}!!", arg);
    unsafe {
        let mut table = get_connection_table().write();
        {
            let mut connection = table.get_connection(String::from("bus.connection.second"))
                .expect("Fail to create the second bus connection").write();
            println!("Create the second connection.");
            let message = BusMessage::new(String::from("bus.connection.first"), String::from("This is a message from 2 to 1."));       
            connection.send(&message);
        }

        table.match_msg(&String::from("bus.connection.second"));

        {
            let mut connection = table.get_connection(String::from("bus.connection.first"))
                .expect("Fail to create the first bus connection").write();
            println!("Get the first connection.");
            let obj = connection.receive();
            if(obj.is_some()){
                println!("{}", obj.unwrap().data);
            } else {
                println!("No message!");
            }

        }
    }
    2
}


fn third_thread_main(arg: String) -> String {
    println!("Hello from third thread, arg: {}!!", arg);
    String::from("3")
}


fn fourth_thread_main(arg: u64) -> Option<String> {
    println!("Hello from fourth thread, arg: {:?}!!", arg);
    None
}



/// An atomic flag used for synchronizing progress between the BSP and all APs.
/// False means the BSP hasn't finished with initialization yet.
pub static BSP_READY_FLAG: AtomicBool = AtomicBool::new(false);


#[no_mangle]
pub extern "C" fn rust_main(multiboot_information_virtual_address: usize) {
	
	// start the kernel with interrupts disabled
	unsafe { ::x86_64::instructions::interrupts::disable(); }
	
    unsafe {
        // print RUST
        asm!("  mov dword ptr [0xFFFFFFFF800b802c], 0x4f554f52; \
	            mov dword ptr [0xFFFFFFFF800b8030], 0x4f544f53;"
                : : : : "intel"
        );
    }
    

    // first, bring up the logger so we can debug
    logger::init().expect("WTF: couldn't init logger.");
    trace!("Logger initialized.");
    

    // initialize basic exception handlers
    interrupts::init_early_exceptions();

    // safety-wise, we just have to trust the multiboot address we get from the boot-up asm code
    let boot_info = unsafe { multiboot2::load(multiboot_information_virtual_address) };
    // debug!("multiboot2 boot_info: {:?}", boot_info);
    // debug!("end of multiboot2 info");
    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    // this returns a MMI struct with the page table, stack allocator, and VMA list for the kernel's address space (idle_task_ap0)
    let (kernel_mmi_ref, identity_mapped_pages) = memory::init(boot_info).unwrap(); // consumes boot_info


    // unsafe {
    //     // print MEMORY
    //     asm!("  mov dword ptr [0xFFFFFFFF800b80f0], 0x4f454f4d; \
    //             mov dword ptr [0xFFFFFFFF800b80f4], 0x4f4f4f4d; \
	//             mov dword ptr [0xFFFFFFFF800b80f8], 0x4f594f4f;"
    //             : : : : "intel"
    //     );
    // }


    // now that we have a heap, we can create basic things like state_store
    state_store::init();
    if cfg!(feature = "no_serial") {
         // enables mirroring of serial port logging outputs to VGA buffer (for real hardware)
        unsafe{  logger::enable_vga(); }
    }
    trace!("state_store initialized.");

    // parse the nano_core ELF object to load its symbols into our metadata
    {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let _num_new_syms = memory::load_kernel_crate(memory::get_module("__k_nano_core").unwrap(), &mut kernel_mmi).unwrap();
        // debug!("Symbol map after __k_nano_core: {}", mod_mgmt::metadata::dump_symbol_map());
    }


    // now we initialize ACPI/APIC barebones stuff
    let madt_iter = {
        trace!("calling drivers::early_init()");
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
    let bsp_apic_id = interrupts::apic::get_bsp_id().expect("rust_main(): Coudln't get BSP's apic_id!");
    task::init(kernel_mmi_ref.clone(), bsp_apic_id, get_bsp_stack_bottom(), get_bsp_stack_top()).unwrap();

    // initialize the kernel console
    let console_queue_producer = console::init().unwrap();

    // initialize the rest of our drivers
    drivers::init(console_queue_producer);


    // boot up the other cores (APs)
    {
        let mut kernel_mmi = kernel_mmi_ref.lock();
        let ap_count = drivers::acpi::madt::handle_ap_cores(madt_iter, &mut kernel_mmi)
                        .expect("Error handling AP cores");
        info!("Finished handling and booting up all {} AP cores.", ap_count);
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


    println_unsafe!("initialization done! Enabling interrupts to schedule away from Task 0 ...");
    interrupts::enable_interrupts();
    BSP_READY_FLAG.store(true, Ordering::SeqCst); // BSP is finished initializing

    {
        // just wait a while for the APs to boot up -- not necessary,
        // but waiting for their runqueues to be ready helps distribute tasks among them evenly
        let mut i = 10000000; // usize::max_value();
        while i > 0 {
            i -= 1;
        }
    }

    // create some extra tasks to test context switching
    if true {
        spawn_kthread(first_thread_main, Some(6),  "first_thread").unwrap();
        spawn_kthread(second_thread_main, 6, "second_thread").unwrap();
        spawn_kthread(third_thread_main, String::from("hello"), "third_thread").unwrap();
        spawn_kthread(fourth_thread_main, 12345u64, "fourth_thread").unwrap();

        spawn_kthread(test_loop_1, None, "test_loop_1").unwrap();
        spawn_kthread(test_loop_2, None, "test_loop_2").unwrap(); 
        spawn_kthread(test_loop_3, None, "test_loop_3").unwrap(); 
    }

	
    // attempt to parse a test kernel module
    if false {
        let kernel_mmi_ref = memory::get_kernel_mmi_ref().unwrap(); // stupid lexical lifetimes...
        let mut kernel_mmi_locked = kernel_mmi_ref.lock();
        memory::load_kernel_crate(memory::get_module("__k_test_server").unwrap(), &mut *kernel_mmi_locked).unwrap();
        // debug!("Symbol map after __k_test_server: {}", mod_mgmt::metadata::dump_symbol_map());
        memory::load_kernel_crate(memory::get_module("__k_test_client").unwrap(), &mut *kernel_mmi_locked).unwrap();
        // debug!("Symbol map after __k_test_client: {}", mod_mgmt::metadata::dump_symbol_map());
        memory::load_kernel_crate(memory::get_module("__k_test_lib").unwrap(), &mut *kernel_mmi_locked).unwrap();
        // debug!("Symbol map after __k_test_lib: {}", mod_mgmt::metadata::dump_symbol_map());

        // now let's try to invoke the test_server function we just loaded
        let func_sec = ::mod_mgmt::metadata::get_symbol("test_server::server_func1").upgrade().unwrap();
        debug!("server_func_vaddr: {:#x}", func_sec.virt_addr());
        let server_func: fn(u8, u64) -> (u8, u64) = unsafe { ::core::mem::transmute(func_sec.virt_addr()) };
        debug!("Called server_func(10, 20) = {:?}", server_func(10, 20));

        // now let's try to invoke the test_client function we just loaded
        let client_func_sec = ::mod_mgmt::metadata::get_symbol("test_client::client_func").upgrade().unwrap();
        debug!("client_func_vaddr: {:#x}", client_func_sec.virt_addr());
        let client_func: fn() -> (u8, u64) = unsafe { ::core::mem::transmute(client_func_sec.virt_addr()) };
        debug!("Called client_func() = {:?}", client_func());

        // now let's try to invoke the test_lib function we just loaded
        let test_lib_public_sec = ::mod_mgmt::metadata::get_symbol("test_lib::test_lib_public").upgrade().unwrap();
        debug!("test_lib_public_vaddr: {:#x}", client_func_sec.virt_addr());
        let test_lib_public_func: fn(u8) -> (u8, &'static str, u64) = unsafe { ::core::mem::transmute(test_lib_public_sec.virt_addr()) };
        debug!("Called test_lib_public() = {:?}", test_lib_public_func(10));
    }

    // create and jump to the first userspace thread
    if true
    {
        debug!("trying to jump to userspace");
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        spawn_userspace(module, Some("test_program_1")).unwrap();
    }

    if true
    {
        debug!("trying to jump to userspace 2nd time");
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        spawn_userspace(module, Some("test_program_2")).unwrap();
    }

    // create and jump to a userspace thread that tests syscalls
    if true
    {
        debug!("trying out a system call module");
        let module = memory::get_module("syscall_send").expect("Error: no module named 'syscall_send' found!");
        spawn_userspace(module, None).unwrap();
    }

    // a second duplicate syscall test user task
    if true
    {
        debug!("trying out a receive system call module");
        let module = memory::get_module("syscall_receive").expect("Error: no module named 'syscall_receive' found!");
        spawn_userspace(module, None).unwrap();
    }

    interrupts::enable_interrupts();
    debug!("rust_main(): entering Task 0's idle loop: interrupts enabled: {}", interrupts::interrupts_enabled());

    assert!(interrupts::interrupts_enabled(), "logical error: interrupts were disabled when entering the idle loop in rust_main()");
    loop { 
        // TODO: exit this loop cleanly upon a shutdown signal
        schedule!();
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
    let apic_id = interrupts::apic::get_my_apic_id().unwrap_or(0xFF);

    error!("\n\nPANIC (AP {:?}) in {} at line {}:", apic_id, file, line);
    error!("    {}", fmt);
    unsafe { memory::stack_trace(); }

    println_unsafe!("\n\nPANIC (AP {:?}) in {} at line {}:", apic_id, file, line);
    println_unsafe!("    {}", fmt);


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
    println_unsafe!("\n\nin rust_eh_unwind_resume, unimplemented!");
    loop {}
}


#[allow(non_snake_case)]
#[no_mangle]
#[cfg(not(target_os = "windows"))]
pub extern "C" fn _Unwind_Resume() -> ! {
    error!("\n\nin _Unwind_Resume, unimplemented!");
    println_unsafe!("\n\nin _Unwind_Resume, unimplemented!");
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
