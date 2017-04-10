// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed

// except according to those terms.
#![feature(lang_items)]
#![feature(const_fn, unique)]
#![feature(alloc, collections)]
#![feature(asm)]
#![feature(naked_functions)]
#![feature(abi_x86_interrupt)]
#![no_std]

extern crate rlibc;
extern crate volatile;
extern crate spin; // core spinlocks 
extern crate multiboot2;
#[macro_use] extern crate bitflags;
extern crate x86;
#[macro_use] extern crate x86_64;
#[macro_use] extern crate once; // Once type (Singletons)
extern crate bit_field;
#[macro_use] extern crate lazy_static; // for lazy static initialization
extern crate hole_list_allocator; // our own allocator
extern crate alloc;
#[macro_use] extern crate collections;
extern crate cpuio; 
extern crate keycodes_ascii; // our own crate for keyboard 


#[macro_use] mod vga_buffer;
mod memory;
mod interrupts;
mod drivers; 

#[no_mangle]
pub extern "C" fn rust_main(multiboot_information_address: usize) {
	
	// start the kernel with interrupts disabled
	unsafe { ::x86_64::instructions::interrupts::disable(); }
	
    // ATTENTION: we have a very small stack and no guard page
    vga_buffer::clear_screen();
    println!("Hello World{}", "!");
    drivers::serial_port::serial_out("Hello serial port!");

    let boot_info = unsafe { multiboot2::load(multiboot_information_address) };
    enable_nxe_bit();
    enable_write_protect_bit();

    // set up guard page and map the heap pages
    let mut memory_controller = memory::init(boot_info);

    // initialize our IDT
    interrupts::init(&mut memory_controller);

    println!("It did not crash!");
	// unsafe{	int!(0x80); }
	// println!("called int 0x80");
	
	// unsafe{	int!(0x81); }
	// println!("called int 0x81");
	
	unsafe { x86::shared::irq::enable();  }
	println!("enabled interrupts!");

	loop { 
        let keyevent = drivers::keyboard::pop_key_event();
        match keyevent {
            Some(keyevent) => { 
                print!("{:?}  ", keyevent.keycode.to_ascii(&keyevent.modifiers));
            }
            _ => { } 
        }
     }
//    loop { unsafe  {x86_64::instructions::halt(); } }
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
