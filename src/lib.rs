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
#![feature(drop_types_in_const)] // unsure about this, prompted to add by rust compiler for Once<>
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
extern crate cpuio; 
#[macro_use] extern crate log;
extern crate keycodes_ascii; // our own crate for keyboard 




#[macro_use] mod drivers;  // I think this mod declaration MUST COME FIRST because it includes the macro for println!
#[macro_use] mod util;
mod arch;
mod logger;
#[macro_use] mod task;
mod memory;
mod interrupts;


use spin::RwLockWriteGuard;
use task::TaskList;
use collections::string::String;





fn test_loop_1(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_1!");
    loop {
        println!("1");
    }
}


fn test_loop_2(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_2!");
    loop {
        println!("2");
    }
}


fn test_loop_3(_: Option<u64>) -> Option<u64> {
    debug!("Entered test_loop_3!");
    loop {
        println!("3");
    }
}



fn second_thr(a: u64) -> u64 {
    return a * 2;
}





fn second_thread_main(arg: Option<u64>) -> u64  {
    println!("Hello from second thread!!");
    let res = second_thr(arg.unwrap());
    println!("calling second_thr({}) = {}", arg.unwrap(), res);
    res
}

fn second_thread_u64_main(arg: u64) -> u64  {
    println!("Hello from second thread!!");
    let res = second_thr(arg);
    println!("calling second_thr({}) = {}", arg, res);
    res
}


fn second_thread_str_main(arg: String) -> String {
    println!("Hello from second thread str version!!");
    let res = arg.to_uppercase();
    println!("arg: {:?}, res:{:?}", arg, res);
    res
}


fn second_thread_none_main(_: u64) -> Option<String> {
    println!("Hello from second thread None version!!");
    // String::from("returned None")
    None
}



#[no_mangle]
pub extern "C" fn rust_main(multiboot_information_address: usize) {
	
	// start the kernel with interrupts disabled
	unsafe { ::x86_64::instructions::interrupts::disable(); }
	
    // early initialization of things like vga console and logging
    logger::init_logger().expect("WTF: couldn't init logger.");
    drivers::early_init();
    

    let boot_info = unsafe { multiboot2::load(multiboot_information_address) };
    enable_nxe_bit();
    enable_write_protect_bit();

    // set up stack guard page and map the heap pages
    let mut memory_controller = memory::init(boot_info);

    // initialize our interrupts and IDT
    interrupts::init(&mut memory_controller);

    // initialize the rest of our drivers
    drivers::init();


    // create the initial `Task`, called task_zero
    // this is scoped in order to automatically release the tasklist RwLock
    {
        let mut tasklist_mut: RwLockWriteGuard<TaskList> = task::get_tasklist().write();
        let task_zero = tasklist_mut.init_first_task();
    }


    println!("initialization done!");

	
	unsafe { x86_64::instructions::interrupts::enable();  }
	println!("enabled interrupts!");

    // create a second task to test context switching
    {
        let ref mut tasklist_mut: RwLockWriteGuard<TaskList> = task::get_tasklist().write();    
        // let second_task = tasklist_mut.spawn(second_thread_main, Some(6));
        // let second_task = tasklist_mut.spawn(second_thread_u64_main, 6);

        // let second_task = tasklist_mut.spawn(second_thread_str_main, String::from("hello"));
        {
        let second_task = tasklist_mut.spawn(second_thread_none_main, 12345u64);
        match second_task {
            Ok(_) => {
                println!("successfully spawned and queued second task!");
            }
            Err(err) => { 
                println!("Failed to spawn second task: {}", err); 
            }
        }
        }

        { tasklist_mut.spawn(test_loop_1, None); }
        { tasklist_mut.spawn(test_loop_2, None); } 
        { tasklist_mut.spawn(test_loop_3, None); } 
    }

    // try to schedule in the second task
    println!("attempting to schedule second task");
    schedule!();






	'outer: loop { 
        let keyevent = drivers::input::keyboard::pop_key_event();
        match keyevent {
            Some(keyevent) => { 
                use drivers::input::keyboard::KeyAction;
                use keycodes_ascii::Keycode;

                // Ctrl+D or Ctrl+Alt+Del kills the OS
                if keyevent.modifiers.control && keyevent.keycode == Keycode::D || 
                        keyevent.modifiers.control && keyevent.modifiers.alt && keyevent.keycode == Keycode::Delete {
                    break 'outer;
                }

                // only print ascii values on a key press down
                if keyevent.action != KeyAction::Pressed {
                    continue 'outer; // aren't Rust's loop labels cool? 
                }

                if keyevent.modifiers.control && keyevent.keycode == Keycode::T {
                    unsafe { debug!("TICKS = {}", interrupts::pit_clock::TICKS); }
                    continue 'outer;
                }


                // PUT ADDITIONAL KEYBOARD-TRIGGERED BEHAVIORS HERE


                let ascii = keyevent.keycode.to_ascii(keyevent.modifiers);
                match ascii {
                    Some(c) => { print!("{}", c); }
                    // _ => { println!("Couldn't get ascii for keyevent {:?}", keyevent); } 
                    _ => { } 
                }
            }
            _ => { }
        }

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
    println!("\n\nPANIC in {} at line {}:", file, line);
    println!("    {}", fmt);
    loop {}
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn _Unwind_Resume() -> ! {
    println!("\n\nin _Unwind_Resume, unimplemented!");
    loop {}
}
