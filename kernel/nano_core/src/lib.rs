// Copyright 2017 Kevin Boos. 
// Licensed under the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// This file may not be copied, modified, or distributed
// except according to those terms.


#![no_std]

#![feature(lang_items)]
#![feature(i128_type)]
#![feature(used)]


// this is needed for our odd approach of loading the libcore ELF file at runtime
#![feature(compiler_builtins_lib)]
extern crate compiler_builtins;


#[macro_use] extern crate log;
extern crate rlibc; // basic memset/memcpy libc functions
extern crate multiboot2;
extern crate x86_64;
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities


#[macro_use] extern crate vga_buffer; 
extern crate logger;
extern crate state_store;
extern crate memory; // the virtual memory subsystem 
extern crate mod_mgmt;
extern crate apic;
extern crate exceptions;
extern crate captain;


pub mod reexports;



// see this: https://doc.rust-lang.org/1.22.1/unstable-book/print.html#used
#[link_section = ".pre_init_array"] // "pre_init_array" is a section never removed by --gc-sections
#[used]
pub fn nano_core_public_func(val: u8) {
    error!("NANO_CORE_PUBLIC_FUNC: got val {}", val);
}



use x86_64::structures::idt::LockedIdt;

static EARLY_IDT: LockedIdt = LockedIdt::new();


/// The Rust entry point for Theseus, which does the following:
/// * Bootstraps the OS, including basic exception handlers and logging
/// * Sets up basic virtual memory
/// * Initializes the state_store module
/// * Finally, calls the Captain module, which initializes and configures the rest of Theseus.
#[no_mangle]
pub extern "C" fn nano_core_main(multiboot_information_virtual_address: usize) {
	
	// start the kernel with interrupts disabled
	irq_safety::disable_interrupts();
	
    // first, bring up the logger so we can debug
    logger::init().expect("WTF: couldn't init logger.");
    trace!("Logger initialized.");

    // initialize basic exception handlers
    exceptions::init_early_exceptions(&EARLY_IDT);


    // safety-wise, we just have to trust the multiboot address we get from the boot-up asm code
    let boot_info = unsafe { multiboot2::load(multiboot_information_virtual_address) };

    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    let (kernel_mmi_ref, identity_mapped_pages) = memory::init(boot_info, apic::broadcast_tlb_shootdown).unwrap(); // consumes boot_info


    // now that we have a heap, we can create basic things like state_store
    state_store::init();
    if cfg!(feature = "mirror_serial") {
         // enables mirroring of serial port logging outputs to VGA buffer (for real hardware)
        logger::mirror_to_vga(captain::mirror_to_vga_cb);
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


    // at this point, we load and jump directly to the Captain, which will take it from here. 
    // That's it, the nano_core is done! That's really all it does! 
    captain::init(kernel_mmi_ref, identity_mapped_pages, 
                  get_bsp_stack_bottom(), get_bsp_stack_top(),
                  get_ap_start_realmode_begin(), get_ap_start_realmode_end()
    );
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

    println_raw!("\n\nPANIC (AP {:?}) in {} at line {}:", apic_id, file, line);
    println_raw!("    {}", fmt);

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
    println_raw!("\n\nin rust_eh_unwind_resume, unimplemented!");
    loop {}
}


#[allow(non_snake_case)]
#[no_mangle]
#[cfg(not(target_os = "windows"))]
pub extern "C" fn _Unwind_Resume() -> ! {
    error!("\n\nin _Unwind_Resume, unimplemented!");
    println_raw!("\n\nin _Unwind_Resume, unimplemented!");
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
fn get_ap_start_realmode_begin() -> usize {
    let addr = unsafe { &ap_start_realmode as *const _ as usize };
    // debug!("ap_start_realmode addr: {:#x}", addr);
    addr + KERNEL_OFFSET
}

/// Returns the ending virtual address of where the ap_start realmode code is.
fn get_ap_start_realmode_end() -> usize {
    let addr = unsafe { &ap_start_realmode_end as *const _ as usize };
    // debug!("ap_start_realmode_end addr: {:#x}", addr);
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
