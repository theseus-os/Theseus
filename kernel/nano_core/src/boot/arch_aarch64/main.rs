//! This crate is the entry point of the generated .efi file.
//!
//! The rust compiler compiles the crate which contains the main.rs file as ahe lib, link it and generate a .efi file.
//!
//! The aarch64-theseus.json file specifies the entry point of the uefi file.
//!
//! Currently the nano_core crate does the following:
//! 1. Initialize the UEFI console services for early log.
//! 2. Initialize the memory and create a new page table.
//! 3. Initialize the early exception handler.
//!
//! To be compatible with x86, the `make arm` command will copy this file to nano_core/src, does the compiling and remove it. If the file is in nano_core/src, for x86 architecture the compiler will try to compile the crate as an application.
//!
//! In generating the kernel.efi file, first, the rust compiler compiles all crates and `nano_core` as libraries. It then uses rust-lld to wrap them and generate a kernel.efi file. The entry point of the file is specified in aarch64-theseus.json
//!  
//! To create the image, `grub-mkrescue` wraps kernel.efi together with modules and a grub.cfg file and generate an image. The command will put a grub.efi file in this image.
//!
//! To run the system, QEMU loads a firmware and the image. The firmware searches for grub.efi automatically and starts it. Grub then uses a UEFI chainloader to load the kernel.efi. Finally, the UEFI bootloader initializes the environment and jumps to the entrypoint of Theseus.

#![no_std]
#![feature(lang_items)]
#![feature(alloc_error_handler)]
#![feature(type_ascription)]

#[macro_use]
extern crate alloc;

#[macro_use]
extern crate log;
extern crate cortex_a;
extern crate rlibc; // basic memset/memcpy libc functions

extern crate exceptions_arm;
extern crate logger;
extern crate uefi;
extern crate uefi_exts;
extern crate uefi_services;

use uefi::prelude::*;
use uefi_exts::BootServicesExt;

use core::fmt::Write;

/// Just like Rust's `try!()` macro, but instead of performing an early return upon an error,
/// it invokes the `shutdown()` function upon an error in order to cleanly exit Theseus OS.
macro_rules! try_exit {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(err_msg) => {
                $crate::shutdown(format_args!("{}", err_msg));
            }
        }
    };
    ($expr:expr,) => {
        try!($expr)
    };
}

/// Shuts down Theseus and prints the given formatted arguuments.
fn shutdown(msg: core::fmt::Arguments) -> ! {
    warn!("Theseus is shutting down, msg: {}", msg);

    // TODO: handle shutdowns properly with ACPI commands
    panic!("{}", msg);
}

/// The main entry point of the UEFI application. It enters the Theseus OS
#[cfg(any(windows, target_arch = "aarch64", target_env = "msvc"))]
#[no_mangle]
pub extern "win64" fn efi_main(image: uefi::Handle, st: SystemTable<Boot>) {
    nano_core_start(image, st);
}

// Prepare and execute transition from EL2 to EL1.
// Not in use now
// #[cfg(any(target_arch = "aarch64"))]
// #[inline]
// fn _setup_and_enter_el1_from_el2(image: uefi::Handle, st: SystemTable<Boot>) -> ! {
//     use cortex_a::{asm, regs::*};

//     const STACK_START: u64 = 0x80_000;

//     // Enable timer counter registers for EL1
//     CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

//     // No offset for reading the counters
//     CNTVOFF_EL2.set(0);

//     // Set EL1 execution state to AArch64
//     // TODO: Explain the SWIO bit
//     HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);

//     // Set up a simulated exception return.
//     //
//     // First, fake a saved program status, where all interrupts were
//     // masked and SP_EL1 was used as a stack pointer.
//     SPSR_EL2.write(
//         SPSR_EL2::D::Masked
//             + SPSR_EL2::A::Masked
//             + SPSR_EL2::I::Masked
//             + SPSR_EL2::F::Masked
//             + SPSR_EL2::M::EL1h,
//     );

//     // Second, let the link register point to reset().
//     ELR_EL2.set(reset as *const () as u64);

//     // Set up SP_EL1 (stack pointer), which will be used by EL1 once
//     // we "return" to it.
//     SP_EL1.set(STACK_START);

//     nano_core_start(image: uefi::Handle, st: SystemTable<Boot>);
//     // Use `eret` to "return" to EL1. This will result in execution of
//     // `reset()` in EL1.

//     asm::eret()
// }

#[no_mangle]
#[cfg(any(target_arch = "aarch64"))]
/// The entrypoint of Theseus.
/// `image` is the handler of the image file. Currently it is of no use.
/// `st` is the systemtable of UEFI. It contains all the services provides by UEFI.
pub extern "win64" fn nano_core_start(_image: uefi::Handle, st: SystemTable<Boot>) -> ! {
    // init useful UEFI services
    uefi_services::init(&st);
    let bt = st.boot_services();
    let stdout = st.stdout();
    let _ = stdout.clear();

    // init memory manager
    let (
        _kernel_mmi_ref,
        _text_mapped_pages,
        _rodata_mapped_pages,
        _data_mapped_pages,
        _identity_mapped_pages,
    ) = try_exit!(memory::init(&bt));
    debug!("nano_core_start(): initialized memory subsystem.");

    match stdout.write_str(WELCOME_STRING) {
        Ok(_) => {}
        Err(err) => error!("Fail to write the welcome string: {}", err),
    };

    // Test: Get the root directory of the filesystem so that we can iterate all modules. 
    let protocol = bt
        .find_protocol::<uefi::proto::media::fs::SimpleFileSystem>()
        .expect_success("Failed to init the SimpleFileSystem protocol")
        .get();

    unsafe {
        let _dir = (*protocol)
            .open_volume()
            .expect("Fail to get access to the file system");
    }

    // Test: Get the load file protocol
    let protocol = bt
        .find_protocol::<uefi::proto::media::load::LoadFile>()
        .expect_success("Failed to init the SimpleFileSystem protocol")
        .get();

    // Disable temporarily because the interrupt handler is to be implemented. Currently we should rely on UEFI handler for keyboard support
    // exceptions_arm::init();

    // TODO: captain::init()

    loop {
        let stdin = st.stdin();
        let key_opt = stdin.read_key().expect_success("Fail to read the input");
        match key_opt {
            Some(key) => {
                let string = format!("{}", key);
                match stdout.write_str(&string) {
                    Ok(_) => {}
                    Err(err) => debug!("Fail to display the input:{}", err),
                };
            }
            None => {}
        };
    }
}

/// No use since the linker specifies efi_main as th entrypoint
pub fn main() {
    loop {}
}

#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[alloc_error_handler]
fn alloc_error_handler(_layout: core::alloc::Layout) -> ! {
    loop {}
}

#[lang = "start"]
fn start() {}

const WELCOME_STRING: &'static str = "
 _____ _
|_   _| |__   ___  ___  ___ _   _ ___
  | | | '_ \\ / _ \\/ __|/ _ \\ | | / __|
  | | | | | |  __/\\__ \\  __/ |_| \\__ \\
  |_| |_| |_|\\___||___/\\___|\\__,_|___/ \n
Type any letter to test Theseus\n
To stop Theseus, type Ctrl+A, X\n";
