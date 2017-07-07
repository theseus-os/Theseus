// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


// Copyright 2017 Kevin Boos. 
// Licensed under the  MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// This file may not be copied, modified, or distributed
// except according to those terms.



#![feature(lang_items)]
#![feature(const_fn, unique)]
#![feature(alloc, collections)]
#![feature(asm)]
#![feature(naked_functions)]
#![feature(abi_x86_interrupt)]
#![feature(drop_types_in_const)] 
#![no_std]


extern crate rlibc;
extern crate volatile;
extern crate spin; // core spinlocks 
extern crate multiboot2;
#[macro_use] extern crate bitflags;
extern crate x86;
#[macro_use] extern crate x86_64;
#[macro_use] extern crate once; // for assert_has_not_been_called!()
extern crate bit_field;
#[macro_use] extern crate lazy_static; // for lazy static initialization
extern crate hole_list_allocator; // our own allocator
extern crate alloc;
#[macro_use] extern crate collections;
extern crate port_io; // our own crate for port_io, replaces exising "cpu_io"
extern crate irq_safety; // our own crate for irq-safe locking and interrupt utilities
#[macro_use] extern crate log;
extern crate keycodes_ascii; // our own crate for keyboard 
//extern crate atomic;



pub mod CONFIG; // TODO: need a better way to separate this out
#[macro_use] mod console;  // I think this mod declaration MUST COME FIRST because it includes the macro for println!
#[macro_use] mod drivers;  
#[macro_use] mod util;
mod arch;
mod logger;
#[macro_use] mod task;
mod memory;
mod interrupts;


use spin::RwLockWriteGuard;
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeReadGuard, RwLockIrqSafeWriteGuard};
use task::TaskList;
use collections::string::String;
use core::sync::atomic::{AtomicUsize, Ordering};
use interrupts::tsc;
use drivers::pci;



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



fn second_thr(a: u64) -> u64 {
    return a * 2;
}





fn first_thread_main(arg: Option<u64>) -> u64  {
    println!("Hello from first thread!!");
    1
}

fn second_thread_main(arg: u64) -> u64  {
    println!("Hello from second thread!!");
    2
}


fn third_thread_main(arg: String) -> String {
    println!("Hello from third thread!!");
    String::from("3")
}


fn fourth_thread_main(_: u64) -> Option<String> {
    println!("Hello from fourth thread!!");
    // String::from("returned None")
    None
}



#[no_mangle]
pub extern "C" fn rust_main(multiboot_information_physical_address: usize) {
	
	// start the kernel with interrupts disabled
	unsafe { ::x86_64::instructions::interrupts::disable(); }
	
    // early initialization of things like vga console and logging
    logger::init_logger().expect("WTF: couldn't init logger.");
    println_unsafe!("Logger initialized.");
    
    drivers::early_init();
    

    let boot_info = unsafe { multiboot2::load(multiboot_information_physical_address + memory::KERNEL_OFFSET) };
    enable_nxe_bit();
    enable_write_protect_bit();

    // set up stack guard page and map the heap pages
    let mut memory_controller = memory::init(boot_info);

    
    // initialize our interrupts and IDT
    interrupts::init(&mut memory_controller);

    // create the initial `Task`, called task_zero
    // this is scoped in order to automatically release the tasklist RwLockIrqSafe
    {
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();
        tasklist_mut.init_first_task();
    }

    // initialize the kernel console
    console::console_init(task::get_tasklist().write());

    // initialize the rest of our drivers
    drivers::init();



    println!("initialization done!");

	
	//interrupts::enable_interrupts(); //apparently this line is unecessary
	println!("enabled interrupts!");

    //println!("PCI configuration value: {}",pci::pciConfigRead(0,0,0,0));
    println!("Value from ATA identification function: {}", pci::ATADriveExists(0xA0));
    println!("{} is value", pci::pio_read(0));
    /*
    // create a second task to test context switching
    {
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();    
        { let second_task = tasklist_mut.spawn_kthread(first_thread_main, Some(6),  "first_thread"); }
        { let second_task = tasklist_mut.spawn_kthread(second_thread_main, 6, "second_thread"); }
        { let second_task = tasklist_mut.spawn_kthread(third_thread_main, String::from("hello"), "third_thread"); } 
        { let second_task = tasklist_mut.spawn_kthread(fourth_thread_main, 12345u64, "fourth_thread"); }

        // must be lexically scoped like this to avoid the "multiple mutable borrows" error
        { tasklist_mut.spawn_kthread(test_loop_1, None, "test_loop_1"); }
        { tasklist_mut.spawn_kthread(test_loop_2, None, "test_loop_2"); } 
        { tasklist_mut.spawn_kthread(test_loop_3, None, "test_loop_3"); } 
    }
    */
    // try to schedule in the second task
    info!("attempting to schedule away from zeroth init task");
    schedule!();





    // the idle thread's (Task 0) busy loop
    trace!("Entering Task0's idle loop");
	

    // create and jump to the first userspace thread
    {
        interrupts::disable_interrupts();
        debug!("disabled interrupts, trying to jump to userspace");
        let mut tasklist_mut: RwLockIrqSafeWriteGuard<TaskList> = task::get_tasklist().write();   
        let module = memory::get_module(0).expect("Error: no userspace modules found!");
        tasklist_mut.spawn_userspace(module, None);
    }


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
extern "C" fn eh_personality() {}

#[cfg(not(test))]
#[lang = "panic_fmt"]
#[no_mangle]
pub extern "C" fn panic_fmt(fmt: core::fmt::Arguments, file: &'static str, line: u32) -> ! {
    println_unsafe!("\n\nPANIC in {} at line {}:", file, line);
    println_unsafe!("    {}", fmt);
    loop {}
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn _Unwind_Resume() -> ! {
    println_unsafe!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}
