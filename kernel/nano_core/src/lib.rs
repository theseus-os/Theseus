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
#![no_std]


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
#[macro_use] extern crate x86_64;
#[macro_use] extern crate once; // for assert_has_not_been_called!()
extern crate bit_field;
#[macro_use] extern crate lazy_static; // for lazy static initialization
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate xmas_elf;
extern crate rustc_demangle;
//extern crate atomic;


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

// ------------------------------------
// -------  THESEUS MODULES   ---------
// ------------------------------------
extern crate serial_port;
#[macro_use] extern crate logger;
extern crate state_store;
#[macro_use] extern crate vga_buffer; 
extern crate test_lib;
extern crate rtc;


#[macro_use] mod console;  // I think this mod declaration MUST COME FIRST because it includes the macro for println!
#[macro_use] mod drivers;  
#[macro_use] mod util;
mod arch;
#[macro_use] mod task;
#[macro_use] mod dbus;
mod memory;
mod interrupts;
mod syscall;
mod mod_mgmt;


use spin::RwLockWriteGuard;
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeReadGuard, RwLockIrqSafeWriteGuard};
use task::TaskList;
use alloc::string::String;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::ops::DerefMut;
use interrupts::tsc;
use drivers::{ata_pio, pci};
use dbus::{BusConnection, BusMessage, BusConnectionTable, get_connection_table};


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
    loop {
        let mut i = 10000000; // usize::max_value();
        while i > 0 {
            i -= 1;
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




#[no_mangle]
pub extern "C" fn rust_main(multiboot_information_physical_address: usize) {
	
	// start the kernel with interrupts disabled
	unsafe { ::x86_64::instructions::interrupts::disable(); }
	
    // first, bring up the logger so we can debug
    logger::init().expect("WTF: couldn't init logger.");
    trace!("Logger initialized.");
    
    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    let boot_info = unsafe { multiboot2::load(multiboot_information_physical_address) };
    enable_nxe_bit();
    enable_write_protect_bit();
    // this returns a MMI struct with the page table, stack allocator, and VMA list for the kernel's address space (task_zero)
    let mut kernel_mmi: memory::MemoryManagementInfo = memory::init(boot_info);
    
    
    // now that we have a heap, we can create basic things like state_store
    state_store::init();
    trace!("state_store initialized.");
    drivers::early_init();


    // unsafe{  logger::enable_vga(); }


    // initialize our interrupts and IDT
    let double_fault_stack = kernel_mmi.alloc_stack(1).expect("could not allocate double fault stack");
    let privilege_stack = kernel_mmi.alloc_stack(4).expect("could not allocate privilege stack");
    let syscall_stack = kernel_mmi.alloc_stack(4).expect("could not allocate syscall stack");
    interrupts::init(double_fault_stack.top_unusable(), privilege_stack.top_unusable());

    syscall::init(syscall_stack.top_usable());

    // debug!("KernelCode: {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::KernelCode).0); 
    // debug!("KernelData: {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::KernelData).0); 
    // debug!("UserCode32: {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::UserCode32).0); 
    // debug!("UserData32: {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::UserData32).0); 
    // debug!("UserCode64: {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::UserCode64).0); 
    // debug!("UserData64: {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::UserData64).0); 
    // debug!("TSS:        {:#x}", interrupts::get_segment_selector(interrupts::AvailableSegmentSelector::Tss).0); 

    // create the initial `Task`, called task_zero
    // this is scoped in order to automatically release the tasklist RwLockIrqSafe
    // TODO: transform this into something more like "task::init(initial_mmi)"
    {
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();
        tasklist_mut.init_task_zero(kernel_mmi);
    }

    // initialize the kernel console
    let console_queue_producer = console::console_init(task::get_tasklist().write());

    // initialize the rest of our drivers
    drivers::init(console_queue_producer);



    println_unsafe!("initialization done! (interrupts enabled?: {})", interrupts::interrupts_enabled());
	


    // create a second task to test context switching
    if true {
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();    
        { let _second_task = tasklist_mut.spawn_kthread(first_thread_main, Some(6),  "first_thread"); }
        { let _second_task = tasklist_mut.spawn_kthread(second_thread_main, 6, "second_thread"); }
        { let _second_task = tasklist_mut.spawn_kthread(third_thread_main, String::from("hello"), "third_thread"); } 
        { let _second_task = tasklist_mut.spawn_kthread(fourth_thread_main, 12345u64, "fourth_thread"); }

        // must be lexically scoped like this to avoid the "multiple mutable borrows" error
        { tasklist_mut.spawn_kthread(test_loop_1, None, "test_loop_1"); }
        { tasklist_mut.spawn_kthread(test_loop_2, None, "test_loop_2"); } 
        { tasklist_mut.spawn_kthread(test_loop_3, None, "test_loop_3"); } 
    }
    
    // try to schedule in the second task
    info!("attempting to schedule away from zeroth init task");
    schedule!(); // this automatically enables interrupts right now


    // the idle thread's (Task 0) busy loop
    trace!("Entering Task0's idle loop");
	
    // attempt to parse a test kernel module
    if true {
        memory::load_kernel_crate("__k_test_lib");
    }

    // create and jump to the first userspace thread
    if true
    {
        debug!("trying to jump to userspace");
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();   
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        tasklist_mut.spawn_userspace(module, Some("test_program_1"));
    }

    if true
    {
        debug!("trying to jump to userspace 2nd time");
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();   
        let module = memory::get_module("test_program").expect("Error: no userspace modules named 'test_program' found!");
        tasklist_mut.spawn_userspace(module, Some("test_program_2"));
    }

    // create and jump to a userspace thread that tests syscalls
    if true
    {
        debug!("trying out a system call module");
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();   
        let module = memory::get_module("syscall_send").expect("Error: no module named 'syscall_send' found!");
        tasklist_mut.spawn_userspace(module, None);
    }

    // a second duplicate syscall test user task
    if true
    {
        debug!("trying out a receive system call module");
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();   
        let module = memory::get_module("syscall_receive").expect("Error: no module named 'syscall_receive' found!");
        tasklist_mut.spawn_userspace(module, None);
    }


    debug!("rust_main(): entering idle loop: interrupts enabled: {}", interrupts::interrupts_enabled());


    loop { 
        // TODO: exit this loop cleanly upon a shutdown signal
    }


    // cleanup here
    logger::shutdown().expect("WTF: failed to shutdown logger... oh well.");
    
    

}

fn enable_nxe_bit() {
    use x86_64::registers::msr::{IA32_EFER, rdmsr, wrmsr};

    let nxe_bit = 1 << 11;
    unsafe {
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | nxe_bit);
    }
}

fn enable_write_protect_bit() {
    use x86_64::registers::control_regs::{cr0, cr0_write, Cr0};

    unsafe { cr0_write(cr0() | Cr0::WRITE_PROTECT) };
}

#[cfg(not(test))]
#[lang = "eh_personality"]
#[no_mangle]
pub extern "C" fn eh_personality() {}

#[cfg(not(test))]
#[lang = "panic_fmt"]
#[no_mangle]
pub extern "C" fn panic_fmt(fmt: core::fmt::Arguments, file: &'static str, line: u32) -> ! {
    error!("\n\nPANIC in {} at line {}:", file, line);
    error!("    {}", fmt);

    println_unsafe!("\n\nPANIC in {} at line {}:", file, line);
    println_unsafe!("    {}", fmt);

    // TODO: check out Redox's unwind implementation: https://github.com/redox-os/kernel/blob/b364d052f20f1aa8bf4c756a0a1ea9caa6a8f381/src/arch/x86_64/interrupt/trace.rs#L9

    loop {}
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn _Unwind_Resume() -> ! {
    error!("\n\nin _Unwind_Resume, unimplemented!");
    println_unsafe!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}
