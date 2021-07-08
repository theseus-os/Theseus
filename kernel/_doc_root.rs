//! # Overview of Theseus
//! 
//! Theseus is a new OS written from scratch in Rust, with primary goals of runtime composability and state spill freedom.
//! 
//! The Theseus kernel is composed of many small entities, each contained within a single Rust crate, and built all together as a cargo virtual workspace. 
//! All crates in Theseus are listed in the sidebar to the left. Click on a crate name to read more about it, and the functions and types it provides.
//! Each crate is its own project with its own `Cargo.toml` manifest file that specifies that crate's dependencies and features.
//! 
//! ## The Theseus Book
//! 
//! You are currently reading the documentation auto-generated from inline source code comments,
//! which is useful for obtaining specific, developer-oriented implementation details about types and functions in each crate. 
//! 
//! For a more general description of Theseus's concepts, organization, build process, contributing guidelines, and more,
//! please see the [Theseus Book](https://theseus-os.github.io/Theseus/book/).
//! 
//! ## Basic Overview of All Crates
//! 
//! One-line summaries of what each crate includes (may be incomplete/out-of-date):
//! 
//! * `acpi`: ACPI (Advanced Configuration and Power Interface) support for Theseus, including multicore discovery.
//! * `apic`: APIC (Advanced Programmable Interrupt Controller) support for Theseus (x86 only), including apic/xapic and x2apic.
//! * `ap_start`: High-level initialization code that runs on each AP (core) after it has booted up
//! * `ata_pio`: Support for ATA hard disks (IDE/PATA) using PIO (not DMA), and not SATA.
//! * `captain`: The main driver of Theseus. Controls the loading and initialization of all subsystems and other crates.
//! * `compositor`: The trait of a compositor. It composites a list of buffers to a final buffer.
//! * `event_types`: The types used for passing input and output events across the system.
//! * `device_manager`: Code for handling the sequence required to initialize each driver.
//! * `displayable`: Defines a displayable trait. A displayable can display itself in a framebuffer.
//! * `text_display`: A text display is a displayable. It contains a block of text and can display in a framebuffer.
//! * `e1000`: Support for the e1000 NIC and driver.
//! * `exceptions_early`: Early exception handlers that do nothing but print an error and hang.
//! * `exceptions_full`: Exception handlers that are more fully-featured, i.e., kills tasks on an exception.
//! * `font`: Defines font for an array of ASCII code.
//! * `framebuffer_compositor`: Composites a list of framebuffers to a final framebuffer which is mapped to the screen.
//! * `framebuffer_drawer`: Basic draw functions.
//! * `framebuffer_printer`: Prints a string in a framebuffer.
//! * `framebuffer`: Defines a Framebuffer structure. It is a buffer of pixels in which an application can display.
//! * `fs_node`: defines the traits for File and Directory. These files and directories mimic that of a standard unix virtual filesystem
//! * `gdt`: GDT (Global Descriptor Table) support (x86 only) for Theseus.
//! * `interrupts`: Interrupt configuration and handlers for Theseus. 
//! * `ioapic`: IOAPIC (I/O Advanced Programmable Interrupt Controller) support (x86 only) for Theseus.
//! * `keyboard`: simple PS2 keyboard driver.
//! * `memory`: The virtual memory subsystem.
//! * `mod_mgmt`: Module management, including parsing, loading, linking, unloading, and metadata management.
//! * `mouse`: simple PS2 mouse driver.
//! * `nano-core`: a tiny module that is responsible for bootstrapping the OS at startup.
//! * `panic_entry`: Default entry point for panics and unwinding, as required by the Rust compiler.
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
//! * `spawn`: Functions and wrappers for spawning new Tasks.
//! * `task`: Task types and structure definitions, a Task is a thread of execution.
//! * `tsc`: TSC (TimeStamp Counter) support for performance counters on x86. Basically a wrapper around rdtsc.
//! * `tss`: TSS (Task State Segment support (x86 only) for Theseus.
//! * `vfs_node`: contains the structs VFSDirectory and VFSFile, which are the most basic, generic implementers of the traits Directory and File
//! * `vga_buffer`: Simple routines for printing to the screen using the x86 VGA buffer text mode.
//! * `window`: Defines window structure which wraps a window inner object.
//! * `window_inner`: Defines a `WindowInner` structure which contains the information required by the window manager.
//! * `window_manager`: A window manager maintains a list of existing windows.
