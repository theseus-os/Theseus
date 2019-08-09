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

extern crate panic_unwind; // the panic/unwind lang items