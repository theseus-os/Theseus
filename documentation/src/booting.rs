//! Documentation about the execution flow during boot up.
//! 
//! # Booting Process and Flow of Execution
//! The kernel takes over from the bootloader (GRUB, or another multiboot2-compatible bootloader) in `nano_core/src/boot/arch_x86_64/boot.asm:start` and is running in *32-bit protected mode*. 
//! After initializing paging and other things, the assembly file `boot.asm` jumps to `long_mode_start`, which runs 64-bit code in long mode. 
//! Then, it jumps to `start_high`, so now we're running in the higher half because Theseus is a [higher-half kernel](https://wiki.osdev.org/Higher_Half_Kernel). 
//! We then set up a new Global Descriptor Table (GDT), segmentation registers, and finally call the Rust code entry point [`nano_core_start()`](../nano_core/fn.nano_core_start.html) with the address of the multiboot2 boot information structure as the first argument (in register RDI).
//!
//! After calling `nano_core_start`, the assembly files are no longer used, and `nano_core_start` should never return. 
//! 

