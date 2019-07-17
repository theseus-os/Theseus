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

#![feature(lang_items)]
#![feature(alloc_error_handler)]
#![feature(type_ascription)]
//#[cfg(loadable)] 
//#[macro_use] extern crate alloc;
//#[cfg(not(loadable))] 
#[macro_use] extern crate alloc;

#[macro_use] extern crate log;
extern crate rlibc; // basic memset/memcpy libc functions
extern crate cortex_a;

/*extern crate spin;
extern crate multiboot2;
extern crate x86_64;
extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities


extern crate state_store;
extern crate memory; // the virtual memory subsystem
extern crate mod_mgmt;
extern crate exceptions_early;
extern crate captain;
extern crate panic_unwind; // the panic/unwind lang items
*/
extern crate logger;
extern crate uefi;
extern crate uefi_services;
extern crate uefi_exts;
extern crate uefi_alloc;

use uefi::prelude::*;
use uefi_exts::BootServicesExt;
use uefi::proto::console::text::Output;
use uefi::table::boot::{MemoryDescriptor};

use core::fmt::Write;
use core::mem;

use crate::alloc::vec::Vec;
use alloc::string::String;


/// Just like Rust's `try!()` macro, but instead of performing an early return upon an error,
/// it invokes the `shutdown()` function upon an error in order to cleanly exit Theseus OS.
macro_rules! try_exit {
    ($expr:expr) => (match $expr {
        Ok(val) => val,
        Err(err_msg) => {
            $crate::shutdown(format_args!("{}", err_msg));
        }
    });
    ($expr:expr,) => (try!($expr));
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
fn warp (int:u64) -> u64 {
    return int;
}

#[cfg(any(windows, target_arch="aarch64", target_env = "msvc"))]
#[no_mangle]
pub extern "win64" fn efi_main(image: uefi::Handle, st: SystemTable<Boot>){
    /*use cortex_a::{asm, regs::*};

    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;
    const EL2: u32 = CurrentEL::EL::EL2.value;

    if (CORE_0 == MPIDR_EL1.get() & CORE_MASK) && (EL2 == CurrentEL.get()) {
        setup_and_enter_el1_from_el2(image, st)
    }*/
    nano_core_start(image, st);


    // loop {
    //     asm::wfe();
    // }
}

/// Prepare and execute transition from EL2 to EL1.
#[cfg(any(target_arch="aarch64"))]
#[inline]
fn setup_and_enter_el1_from_el2(image: uefi::Handle, st: SystemTable<Boot>) -> ! {
    use cortex_a::{asm, regs::*};

    const STACK_START: u64 = 0x80_000;

    // Enable timer counter registers for EL1
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No offset for reading the counters
    CNTVOFF_EL2.set(0);

    // Set EL1 execution state to AArch64
    // TODO: Explain the SWIO bit
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);

    // Set up a simulated exception return.
    //
    // First, fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1h,
    );

    // Second, let the link register point to reset().
    ELR_EL2.set(reset as *const () as u64);

    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    SP_EL1.set(STACK_START);

    nano_core_start(image: uefi::Handle, st: SystemTable<Boot>);
    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.

    asm::eret()
}

unsafe fn reset(image: uefi::Handle, st: SystemTable<Boot>) {
    extern "C" {
        // Boundaries of the .bss section, provided by the linker script
        static mut __bss_start: u64;
        static mut __bss_end: u64;
    }

    // Zeroes the .bss section
    //r0::zero_bss(&mut __bss_start, &mut __bss_end);
}

#[no_mangle]
#[cfg(any(target_arch="aarch64"))]
pub extern "win64" fn nano_core_start(image: uefi::Handle, st: SystemTable<Boot>) -> !{
    uefi_services::init(&st);
    let bt = st.boot_services();
    let stdout = st.stdout();
    
    let _ = stdout.clear();
    let (kernel_mmi_ref, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) =  try_exit!(memory::init(&bt, stdout, image));

    debug!("nano_core_start(): initialized memory subsystem.");

    match stdout.write_str(WELCOME_STRING){
        Ok(_) => {},
        Err(err) => {},
    };

    // Get the root directory
    type SearchedProtocol<'boot> = uefi::proto::media::fs::SimpleFileSystem;

    let protocol = bt.find_protocol::<SearchedProtocol>().expect_success("Failed to init the SimpleFileSystem protocol").get();

    unsafe{ 
        let dir =(*protocol).open_volume().expect("Fail to get access to the file system");
    }

    // TODO: captain::init()
    loop {
        let stdin = st.stdin();
        let key_opt = stdin.read_key().expect_success("Fail to read the input");
        match key_opt {
            Some(key) => {
                let string = format!("{}", key);
                match stdout.write_str(&string) {
                        Ok(_) => {},
                        Err(err) => {},
                };
            },
            None => { }
        };          
    }
}


//EFI application entry point related
fn main() {
    unsafe {
        //*(0x09000000 as *mut u32) = 'd' as u32;
    }
    loop { }
}


#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    loop{

     }
}

#[alloc_error_handler]
fn alloc_error_handler(_layout: core::alloc::Layout) -> ! {
    loop{ 
    }
}

#[lang = "start"]
fn start() { }


const WELCOME_STRING: &'static str = "
 _____ _
|_   _| |__   ___  ___  ___ _   _ ___
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n
To stop Theseus, type Ctrl+A, X\n";
