//! The aptly-named tiny crate containing the first OS code to run.
//! 
//! The `nano_core` is very simple, and only does the following things:
//! 
//! 1. Bootstraps the OS after the bootloader is finished, and initializes simple things like logging.
//! 2. Establishes a simple virtual memory subsystem so that other modules can be loaded.
//! 3. Loads the core library module, the `captain` module, and then calls [`captain::init()`](../captain/fn.init.html) as a final step.
//! 4. That's it! Once `nano_core` gives complete control to the `captain`, it takes no other actions.
//!
//! In general, you shouldn't ever need to change `nano_core`. 
//! That's because `nano_core` doesn't contain any specific program logic, 
//! it just sets up an initial environment so that other subsystems can run.
//! 
//! If you want to change how the OS starts up and which systems it initializes, 
//! you should change the code in the [`captain`](../captain/index.html) crate instead.
//! 

#![no_std]
#![feature(compiler_builtins_lib)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate rlibc; // basic memset/memcpy libc functions
extern crate spin;
extern crate multiboot2;
extern crate x86_64;
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate logger;
extern crate state_store;
extern crate memory; // the virtual memory subsystem
extern crate mod_mgmt;
extern crate exceptions_early;
#[macro_use] extern crate vga_buffer;
extern crate panic_entry; // contains required panic-related lang items
#[cfg(not(loadable))] extern crate captain;


/// This module is a hack to get around the lack of the 
/// `__truncdfsf2` function in the `compiler_builtins` crate.
/// See this: <https://github.com/rust-lang-nursery/compiler-builtins/pull/262>
mod truncate;


#[link_section = ".pre_init_array"] // "pre_init_array" is a section never removed by --gc-sections
#[doc(hidden)]
pub fn nano_core_public_func(val: u8) {
    error!("NANO_CORE_PUBLIC_FUNC: got val {}", val);
}


use core::ops::DerefMut;
use x86_64::structures::idt::LockedIdt;
use memory::VirtualAddress;
use kernel_config::memory::KERNEL_OFFSET;

/// An initial interrupt descriptor table for catching very simple exceptions only.
/// This is no longer used after interrupts are set up properly, it's just a failsafe.
static EARLY_IDT: LockedIdt = LockedIdt::new();


/// Just like Rust's `try!()` macro, but instead of performing an early return upon an error,
/// it invokes the `shutdown()` function upon an error in order to cleanly exit Theseus OS.
macro_rules! try_exit {
    ($expr:expr) => (match $expr {
        Ok(val) => val,
        Err(err_msg) => {
            $crate::shutdown(format_args!("{}", err_msg));
        }
    });
    // ($expr:expr,) => (try!($expr));
}


/// Shuts down Theseus and prints the given formatted arguuments.
fn shutdown(msg: core::fmt::Arguments) -> ! {
    warn!("Theseus is shutting down, msg: {}", msg);

    // TODO: handle shutdowns properly with ACPI commands
    panic!("{}", msg);
}



/// The main entry point into Theseus, that is, the first Rust code that the Theseus kernel runs. 
///
/// This is called from assembly code entry point for Theseus, found in `nano_core/src/boot/arch_x86_64/boot.asm`.
///
/// This function does the following things: 
///
/// * Bootstraps the OS, including [logging](../logger/index.html) 
///   and basic early [exception handlers](../exceptions_early/fn.init.html)
/// * Sets up basic [virtual memory](../memory/fn.init.html)
/// * Initializes the [state_store](../state_store/index.html) module
/// * Finally, calls the Captain module, which initializes and configures the rest of Theseus.
///
/// If a failure occurs and is propagated back up to this function, the OS is shut down.
/// 
/// # Note
/// In general, you never need to modify the `nano_core` to change Theseus's behavior,
/// because the `nano_core` is essentially logic-agnostic boilerplate code and set up routines. 
/// If you want to customize or change how the OS components are initialized, 
/// then change the [`captain::init`](../captain/fn.init.html) routine.
/// 
#[no_mangle]
pub extern "C" fn nano_core_start(multiboot_information_virtual_address: usize) {
    println_raw!("Entered nano_core_start()."); 
	
	// start the kernel with interrupts disabled
	irq_safety::disable_interrupts();

    // first, bring up the logger so we can debug
    try_exit!(logger::init().map_err(|_| "couldn't init logger!"));
    trace!("Logger initialized.");
    println_raw!("nano_core_start(): initialized logger."); 

    // initialize basic exception handlers
    exceptions_early::init(&EARLY_IDT);
    println_raw!("nano_core_start(): initialized early IDT with exception handlers."); 

    // safety-wise, we have to trust the multiboot address we get from the boot-up asm code, but we can check its validity
    if VirtualAddress::new(multiboot_information_virtual_address).is_err() {
        try_exit!(Err("multiboot info virtual address was invalid! Ensure that nano_core_start() is being invoked properly from boot.asm!"));
    }
    let boot_info = unsafe { multiboot2::load(multiboot_information_virtual_address) };
    println_raw!("nano_core_start(): booted via multiboot2."); 

    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    let (kernel_mmi_ref, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) = try_exit!(memory::init(&boot_info));
    println_raw!("nano_core_start(): initialized memory subsystem."); 
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped upon a returned error, causing all execution to stop. 
    // (at least until we transfer ownership of them to the `parse_nano_core` function below.)

    state_store::init();
    trace!("state_store initialized.");
    println_raw!("nano_core_start(): initialized state store.");     

    // initialize the module management subsystem, so we can create the default crate namespace
    let default_namespace = match mod_mgmt::init(&boot_info, kernel_mmi_ref.lock().deref_mut()) {
        Ok(namespace) => namespace,
        Err(err) => { 
            core::mem::forget(text_mapped_pages);
            core::mem::forget(rodata_mapped_pages);
            core::mem::forget(data_mapped_pages);
            core::mem::forget(identity_mapped_pages);
            shutdown(format_args!("{}", err));
        }
    };
    println_raw!("nano_core_start(): initialized crate namespace subsystem."); 

    // Parse the nano_core crate (the code we're already running) since we need it to load and run applications.
    println_raw!("nano_core_start(): parsing nano_core crate, please wait ..."); 
    let (nano_core_crate_ref, bsp_stack_top, bsp_stack_bottom, ap_realmode_begin, ap_realmode_end) = {
        match mod_mgmt::parse_nano_core::parse_nano_core(default_namespace, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, false) {
            Ok((nano_core_crate_ref, init_symbols, _num_new_syms)) => {
                // Get symbols from the boot assembly code that defines where the BSP stack and the ap_start code are.
                // The symbols in the ".init" section will be in init_symbols, all others will be in the regular namespace symbol tree.
                let bsp_stack_top     = try_exit!(default_namespace.get_symbol("initial_bsp_stack_top").upgrade().ok_or("Missing expected symbol from assembly code \"initial_bsp_stack_top\"")).start_address();
                let bsp_stack_bottom  = try_exit!(default_namespace.get_symbol("initial_bsp_stack_bottom").upgrade().ok_or("Missing expected symbol from assembly code \"initial_bsp_stack_bottom\"")).start_address();
                let ap_realmode_begin = try_exit!(init_symbols.get("ap_start_realmode").ok_or("Missing expected symbol from assembly code \"ap_start_realmode\"").and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET)));
                let ap_realmode_end   = try_exit!(init_symbols.get("ap_start_realmode_end").ok_or("Missing expected symbol from assembly code \"ap_start_realmode_end\"").and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET)));
                // debug!("bsp_stack_top: {:#X}, bsp_stack_bottom: {:#X}, ap_realmode_begin: {:#X}, ap_realmode_end: {:#X}", bsp_stack_top, bsp_stack_bottom, ap_realmode_begin, ap_realmode_end);
                (nano_core_crate_ref, bsp_stack_top, bsp_stack_bottom, ap_realmode_begin, ap_realmode_end)
            }
            Err((msg, mapped_pages_array)) => {
                // Because this function takes ownership of the text/rodata/data mapped_pages that cover the currently-running code,
                // we have to make sure these mapped_pages aren't dropped.
                core::mem::forget(mapped_pages_array);
                shutdown(format_args!("parse_nano_core() failed! error: {}", msg));
            }
        }
    };
    println_raw!("nano_core_start(): finished parsing the nano_core crate."); 

    // If in loadable mode, load each of the nano_core's constituent crates such that other crates loaded in the future
    // can depend on those dynamically-loaded instances rather than on the statically-linked sections in the nano_core's base kernel image.
    #[cfg(loadable)] {
        // TODO FIXME: this does not yet work correctly
        // try_exit!(mod_mgmt::replace_nano_core_crates::replace_nano_core_crates(&default_namespace, nano_core_crate_ref, &kernel_mmi_ref));
    }
    #[cfg(not(loadable))] {
        drop(nano_core_crate_ref);
    }
    
    // if in loadable mode, parse the crates we always need: the core library (Rust no_std lib), the panic handlers, and the captain
    #[cfg(loadable)] 
    {
        use mod_mgmt::CrateNamespace;
        println_raw!("nano_core_start(): loading the \"captain\" crate...");     
        let (captain_file, _ns) = try_exit!(CrateNamespace::get_crate_object_file_starting_with(default_namespace, "captain-").ok_or("couldn't find the singular \"captain\" crate object file"));
        let (_captain_crate, _num_captain_syms) = try_exit!(default_namespace.load_crate(&captain_file, None, &kernel_mmi_ref, false));
        println_raw!("nano_core_start(): loading the panic handling crate(s)...");     
        let (panic_wrapper_file, _ns) = try_exit!(CrateNamespace::get_crate_object_file_starting_with(default_namespace, "panic_wrapper-").ok_or("couldn't find the singular \"panic_wrapper\" crate object file"));
        let (_pw_crate, _num_pw_syms) = try_exit!(default_namespace.load_crate(&panic_wrapper_file, None, &kernel_mmi_ref, false));
    }


    // at this point, we load and jump directly to the Captain, which will take it from here. 
    // That's it, the nano_core is done! That's really all it does! 
    println_raw!("nano_core_start(): invoking the captain...");     
    #[cfg(not(loadable))]
    {
        try_exit!(
            captain::init(kernel_mmi_ref, identity_mapped_pages, 
                bsp_stack_bottom, bsp_stack_top,
                ap_realmode_begin, ap_realmode_end
            )
        );
    }
    #[cfg(loadable)]
    {
        use alloc::vec::Vec;
        use memory::{MmiRef, MappedPages};

        let section = try_exit!(
            default_namespace.get_symbol_starting_with("captain::init::")
            .upgrade()
            .ok_or("no single symbol matching \"captain::init\"")
        );
        info!("The nano_core (in loadable mode) is invoking the captain init function: {:?}", section.name);

        type CaptainInitFunc = fn(MmiRef, Vec<MappedPages>, VirtualAddress, VirtualAddress, VirtualAddress, VirtualAddress) -> Result<(), &'static str>;
        let mut space = 0;
        let func: &CaptainInitFunc = {
            try_exit!(section.mapped_pages.lock().as_func(section.mapped_pages_offset, &mut space))
        };

        try_exit!(
            func(kernel_mmi_ref, identity_mapped_pages, 
                bsp_stack_bottom, bsp_stack_top,
                ap_realmode_begin, ap_realmode_end
            )
        );
    }


    // the captain shouldn't return ...
    try_exit!(Err("captain::init returned unexpectedly... it should be an infinite loop (diverging function)"));
}



// These extern definitions are here just to ensure that these symbols are defined in the assembly files. 
// Defining them here produces a linker error if they are absent, which is better than a runtime error (early detection!).
// We don't actually use them, and they should not be accessed or dereferenced, because they are merely values, not addresses. 
#[allow(dead_code)]
extern {
    static initial_bsp_stack_top: usize;
    static initial_bsp_stack_bottom: usize;
    static ap_start_realmode: usize;
    static ap_start_realmode_end: usize;
}
