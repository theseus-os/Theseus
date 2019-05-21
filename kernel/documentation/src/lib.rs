//! Theseus is a new OS written from scratch in Rust, with the goals of runtime composability and state spill freedom.    
//! 
//! # Overview of Theseus
//! The Theseus kernel is composed of many small module entities, each contained within a single Rust crate, and built all together as a cargo virtual workspace. 
//! All crates in Theseus are listed in the sidebar to the left. Click on a crate name to read more about it, and the functions and types it provides.
//! Each crate is its own project with its own "Cargo.toml" manifest file that specifies that crate's dependencies and features. 
//! 
//! Theseus is essentially a loose "bag of modules" without any source-level hierarchy or submodules, as you can see by flatly listing the contents of the `kernel` directory.
//! All crate entities (modules) are on equal footing, except for the [`nano_core`](../nano_core/index.html), which is a tiny module containing the first code that runs after the bootloader to bootstrap the OS.
//! 
//! 
//! # Table of Contents
//! * [Advice and Principles for Contributing to Theseus](contributing/index.html)
//! * [Git-based Development](git/index.html)
//! * [The PHIS Principle: Performance in Hardware, Isolation in Software](phis/index.html)
//! * [How the Build Process Works](build_process/index.html)
//! * [Loadable Mode: Runtime Loading and Linking of Crates](build_process/index.html#loadable-mode-runtime-loading-and-linking-of-crates)
//! * [How Theseus Boots](booting/index.html)
//! 
//! # Basic Overview of All Crates 
//! One-line summaries of what each crate includes (may be incomplete):
//! 
//! * `acpi`: ACPI (Advanced Configuration and Power Interface) support for Theseus, including multicore discovery.
//! * `apic`: APIC (Advanced Programmable Interrupt Controller) support for Theseus (x86 only), including apic/xapic and x2apic.
//! * `ap_start`: High-level initialization code that runs on each AP (core) after it has booted up
//! * `ata_pio`: Support for ATA hard disks (IDE/PATA) using PIO (not DMA), and not SATA.
//! * `captain`: The main driver of Theseus. Controls the loading and initialization of all subsystems and other crates.
//! * `input_event_manager`: Handles input events from the keyboard and routes them to the correct application. ** Being phased out by window manager
//! * `event_types`: A temporary way to move the input_event_manager typedefs out of the input_event_manager crate.
//! * `dbus`: Simple dbus-like IPC support for Theseus (incomplete).
//! * `device_manager`: Code for handling the sequence required to initialize each driver.
//! * `display`:Display graphs in a virual frame buffer.
//! * `display`:Display graphs in a virual frame buffer.
//! * `display_text`:Print text in a virual frame buffer.
//! * `e1000`: Support for the e1000 NIC and driver.
//! * `exceptions_early`: Early exception handlers that do nothing but print an error and hang.
//! * `exceptions_full`: Exception handlers that are more fully-featured, i.e., kills tasks on an exception.
//! * `frame_buffer`:Create virtual frame buffer and map them to the final frame buffer. Display the contents of a virtual frame buffer
//! * `fs_node`: defines the traits for File and Directory. These files and directories mimic that of a standard unix virtual filesystem
//! * `gdt`: GDT (Global Descriptor Table) support (x86 only) for Theseus.
//! * `interrupts`: Interrupt configuration and handlers for Theseus. 
//! * `ioapic`: IOAPIC (I/O Advanced Programmable Interrupt Controller) support (x86 only) for Theseus.
//! * `keyboard`: simple PS2 keyboard driver.
//! * `memory`: The virtual memory subsystem.
//! * `mod_mgmt`: Module management, including parsing, loading, linking, unloading, and metadata management.
//! * `mouse`: simple PS2 mouse driver.
//! * `nano-core`: a tiny module that is responsible for bootstrapping the OS at startup.
//! * `panic_unwind`: Default entry point for panics and unwinding, as required by the Rust compiler.
//! * `panic_wrapper`: Wrapper functions for handling and propagating panics.
//! * `path`: contains functions for navigating the filesystem / getting pointers to specific directories via the Path struct 
//! * `pci`: Basic PCI support for Theseus, x86 only.
//! * `pic`: PIC (Programmable Interrupt Controller), support for a legacy interrupt controller that isn't used much.
//! * `pit_clock`: PIT (Programmable Interval Timer) support for Theseus, x86 only.
//! * `ps2`: general driver for interfacing with PS2 devices and issuing PS2 commands (for mouse/keyboard).
//! * `root`: special implementation of the root directory; initializes the root of the filesystem
//! * `rtc`: simple driver for handling the Real Time Clock chip.
//! * `scheduler`: The scheduler and runqueue management.
//! * `serial_port`: simple driver for writing to the serial_port, used mostly for debugging.
//! * `spawn`: Functions and wrappers for spawning new Tasks, both kernel threads and userspace processes.
//! * `syscall`: Initializes the system call support, and provides basic handling and dispatching of syscalls in Theseus.
//! * `task`: Task types and structure definitions, a Task is a thread of execution.
//! * `text_display` : Defines a trait for anything that can display text to the screen
//! * `tsc`: TSC (TimeStamp Counter) support for performance counters on x86. Basically a wrapper around rdtsc.
//! * `tss`: TSS (Task State Segment support (x86 only) for Theseus.
//! * `vfs_node`: contains the structs VFSDirectory and VFSFile, which are the most basic, generic implementers of the traits Directory and File
//! * `vga_buffer`: Simple routines for printing to the screen using the x86 VGA buffer text mode.
//!



#![no_std]
pub mod contributing;
pub mod build_process;
pub mod booting;
pub mod phis;
pub mod git;
