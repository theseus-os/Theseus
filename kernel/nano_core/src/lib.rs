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

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate multiboot2;
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate logger;
extern crate state_store;
extern crate memory; // the virtual memory subsystem
extern crate stack;
extern crate serial_port_basic;
extern crate mod_mgmt;
extern crate exceptions_early;
#[macro_use] extern crate vga_buffer;
extern crate panic_entry; // contains required panic-related lang items
#[cfg(not(loadable))] extern crate captain;
extern crate memory_initialization;


use core::ops::DerefMut;
use memory::VirtualAddress;
use kernel_config::memory::KERNEL_OFFSET;
use serial_port_basic::{take_serial_port, SerialPortAddress};

/// Used to obtain information about this build of Theseus.
mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

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
    println_raw!("Theseus is shutting down, msg: {}", msg); 
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
pub extern "C" fn nano_core_start(
    multiboot_information_virtual_address: usize,
    early_double_fault_stack_top: usize,
) {
    // start the kernel with interrupts disabled
	irq_safety::disable_interrupts();
    println_raw!("Entered nano_core_start(). Interrupts disabled.");

    // Initialize the logger up front so we can see early log messages for debugging.
    let logger_ports = [take_serial_port(SerialPortAddress::COM1)]; // some servers use COM2 instead. 
    try_exit!(logger::early_init(None, IntoIterator::into_iter(logger_ports).flatten()).map_err(|_a| "logger::early_init() failed."));
    info!("Logger initialized.");
    println_raw!("nano_core_start(): initialized logger."); 

    // Dump basic information about this build of Theseus.
    info!("\n    \
        ===================== Theseus build info: =====================\n    \
        CUSTOM CFGs: {} \n    \
        ===============================================================",
        build_info::CUSTOM_CFG_STR,
    );

    // initialize basic exception handlers
    exceptions_early::init(Some(VirtualAddress::new_canonical(early_double_fault_stack_top)));
    println_raw!("nano_core_start(): initialized early IDT with exception handlers."); 

    // safety-wise, we have to trust the multiboot address we get from the boot-up asm code, but we can check its validity
    if VirtualAddress::new(multiboot_information_virtual_address).is_none() {
        try_exit!(Err("multiboot info virtual address was invalid! Ensure that nano_core_start() is being invoked properly from boot.asm!"));
    }
    let boot_info = try_exit!(
        unsafe { multiboot2::load(multiboot_information_virtual_address) }.map_err(|e| {
            error!("Error loading multiboot2 info: {:?}", e);
            "Error loading multiboot2 info"
        })
    );
    println_raw!("nano_core_start(): booted via multiboot2 with boot info at {:#X}.", multiboot_information_virtual_address); 

    // init memory management: set up stack with guard page, heap, kernel text/data mappings, etc
    let (
        kernel_mmi_ref,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack,
        bootloader_modules,
        identity_mapped_pages
    ) = try_exit!(memory_initialization::init_memory_management(boot_info));
    println_raw!("nano_core_start(): initialized memory subsystem."); 
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped upon a returned error, causing all execution to stop. 
    // (at least until we transfer ownership of them to the `parse_nano_core` function below.)


    state_store::init();
    trace!("state_store initialized.");
    println_raw!("nano_core_start(): initialized state store.");     

    // initialize the module management subsystem, so we can create the default crate namespace
    let default_namespace = match mod_mgmt::init(bootloader_modules, kernel_mmi_ref.lock().deref_mut()) {
        Ok(namespace) => namespace,
        Err(err) => { 
            core::mem::forget(text_mapped_pages);
            core::mem::forget(rodata_mapped_pages);
            core::mem::forget(data_mapped_pages);
            core::mem::forget(stack);
            core::mem::forget(identity_mapped_pages);
            shutdown(format_args!("{}", err));
        }
    };
    println_raw!("nano_core_start(): initialized crate namespace subsystem."); 

    // Parse the nano_core crate (the code we're already running) since we need it to load and run applications.
    println_raw!("nano_core_start(): parsing nano_core crate, please wait ..."); 
    let (nano_core_crate_ref, ap_realmode_begin, ap_realmode_end) = match mod_mgmt::parse_nano_core::parse_nano_core(
        default_namespace, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, false
    ) {
        Ok((nano_core_crate_ref, init_symbols, _num_new_syms)) => {
            // Get symbols from the boot assembly code that defines where the ap_start code are.
            // They will be present in the ".init" sections, i.e., in the `init_symbols` list. 
            let ap_realmode_begin = try_exit!(
                init_symbols.get("ap_start_realmode")
                    .and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET))
                    .ok_or("Missing/invalid symbol expected from assembly code \"ap_start_realmode\"")
            );
            let ap_realmode_end   = try_exit!(
                init_symbols.get("ap_start_realmode_end")
                    .and_then(|v| VirtualAddress::new(*v + KERNEL_OFFSET))
                    .ok_or("Missing/invalid symbol expected from assembly code \"ap_start_realmode_end\"")
            );
            // debug!("ap_realmode_begin: {:#X}, ap_realmode_end: {:#X}", ap_realmode_begin, ap_realmode_end);
            (nano_core_crate_ref, ap_realmode_begin, ap_realmode_end)
        }
        Err((msg, mapped_pages_array)) => {
            // Because this function takes ownership of the text/rodata/data mapped_pages that cover the currently-running code,
            // we have to make sure these mapped_pages aren't dropped.
            core::mem::forget(mapped_pages_array);
            shutdown(format_args!("parse_nano_core() failed! error: {}", msg));
        }
    };
    println_raw!("nano_core_start(): finished parsing the nano_core crate."); 

    #[cfg(loadable)] {
        // This isn't currently necessary; we can always add it in back later if/when needed.
        // // If in loadable mode, load each of the nano_core's constituent crates such that other crates loaded in the future
        // // can depend on those dynamically-loaded instances rather than on the statically-linked sections in the nano_core's base kernel image.
        // try_exit!(mod_mgmt::replace_nano_core_crates::replace_nano_core_crates(&default_namespace, nano_core_crate_ref, &kernel_mmi_ref));
        drop(nano_core_crate_ref);
    }
    #[cfg(not(loadable))] {
        drop(nano_core_crate_ref);
    }
    
    // if in loadable mode, parse the crates we always need: the core library (Rust no_std lib), the panic handlers, and the captain
    #[cfg(loadable)] {
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
    #[cfg(not(loadable))] {
        try_exit!(
            captain::init(kernel_mmi_ref, identity_mapped_pages, stack, ap_realmode_begin, ap_realmode_end)
        );
    }
    #[cfg(loadable)] {
        use alloc::vec::Vec;
        use memory::{MmiRef, MappedPages};

        let section = try_exit!(
            default_namespace.get_symbol_starting_with("captain::init::")
            .upgrade()
            .ok_or("no single symbol matching \"captain::init\"")
        );
        info!("The nano_core (in loadable mode) is invoking the captain init function: {:?}", section.name);

        type CaptainInitFunc = fn(MmiRef, Vec<MappedPages>, stack::Stack, VirtualAddress, VirtualAddress) -> Result<(), &'static str>;
        let func: &CaptainInitFunc = try_exit!(unsafe { section.as_func() });

        try_exit!(
            func(kernel_mmi_ref, identity_mapped_pages, stack, ap_realmode_begin, ap_realmode_end)
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
    static initial_bsp_stack_guard_page: usize;
    static initial_bsp_stack_bottom: usize;
    static initial_bsp_stack_top: usize;
    static ap_start_realmode: usize;
    static ap_start_realmode_end: usize;
}


/// This module is a hack to get around the issue of no_mangle symbols
/// not being exported properly from the `libm` crate in no_std environments.
mod libm;

/// Implements OS support for GCC's stack smashing protection.
/// This isn't used at the moment, but we make it available in case 
/// any foreign code (e.g., C code) wishes to use it.
/// 
/// You can disable the need for this via the `-fno-stack-protection` GCC option.
mod stack_smash_protection;
