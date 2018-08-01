//! Theseus is a new OS written from scratch in Rust, with the goals of runtime composability and state spill freedom.    
//! 
//! # Structure of Theseus
//! The Theseus kernel is composed of many small modules, each contained within a single Rust crate, and built all together as a cargo workspace. 
//! All crates in Theseus are listed in the sidebar to the left, click on a crate name to read more about what that module does and the functions and types it provides.
//! Each module is a separate project that lives in its own crate, with its own "Cargo.toml" manifest file that specifies that module's dependencies and features. 
//! 
//! Theseus is essentially a "bag of modules" without any source-level hierarchy, as you can see every crate by flatly listing the contents of the `kernel` directory. 
//! However, there are two special "metamodules" that warrant further explanation: the `nano_core` and the `captain`.
//!
//! ## Key Modules
//! #### `nano_core`
//! `nano_core` is the aptly-named tiny module that contains the first code to run.
//! `nano_core` is very simple, and only does the following things:
//! 
//! 1. Bootstraps the OS after the bootloader is finished, and initializes simple things like logging.
//! 2. Establishes a simple virtual memory subsystem so that other modules can be loaded.
//! 3. Loads the core library module, the `captain` module, and then calls [`captain::init()`](../captain/fn.init.html) as a final step.
//! 4. That's it! Once `nano_core` gives complete control to the `captain`, it takes no other actions.
//!
//! In general, you shouldn't ever need to change `nano_core` ... **ever**. That's because `nano_core` doesn't contain any specific program logic, it just sets up an initial environment so that other things can run.
//! If you want to change how the OS starts up and initializes, you should change the code in the `captain` instead.
//!
//! #### `captain`
//! LZ: module has its own meaning in Rust. Here you are using module not as Rust module but it can be confusing.
//! LZ: maybe we need a new term 
//! KAB:  yeah, agreed. What about something ship-themed? I really like "plank"? Or maybe there's a more modern word for "ship part"?
//! 
//! The `captain` steers the ship of Theseus, meaning that it contains the logic that initializes and connects all the other module crates in the proper order and with the proper flow of data between modules. 
//! Currently, the default `captain` in Theseus loads a bunch of crates, then initializes ACPI and APIC to discover multicore configurations, sets up interrupt handlers, spawns a input_event_manager thread and createsa queue to send keyboard presses to the input_event_manager, boots up other cores (APs), unmaps the initial identity-mapped pages, and then finally spawns some test Tasks (liable to change).     
//! At the end, the `captain` must enable interrupts to allow the system to schedule other Tasks. It then falls into an idle loop that does nothing except yields the processor to another Task.    
//!


//! # Basic Overview of Each Crate
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
//! * `driver_init`: Code for handling the sequence required to initialize each driver.
//! * `e1000`: Support for the e1000 NIC and driver.
//! * `exceptions_early`: Early exception handlers that do nothing but print an error and hang.
//! * `exceptions_full`: Exception handlers that are more fully-featured, i.e., kills tasks on an exception.
//! * `gdt`: GDT (Global Descriptor Table) support (x86 only) for Theseus.
//! * `interrupts`: Interrupt configuration and handlers for Theseus. 
//! * `ioapic`: IOAPIC (I/O Advanced Programmable Interrupt Controller) support (x86 only) for Theseus.
//! * `keyboard`: simple PS2 keyboard driver..
//! * `memory`: The virtual memory subsystem.
//! * `mod_mgmt`: Module management, including parsing, loading, linking, unloading, and metadata management.
//! * `mouse`: simple PS2 mouse driver.
//! * `panic_handling`: Wrapper functions for handling and propagating panics.
//! * `panic_info`: Struct definitions containing panic information and such.
//! * `pci`: Basic PCI support for Theseus, x86 only.
//! * `pic`: PIC (Programmable Interrupt Controller), support for a legacy interrupt controller that isn't used much.
//! * `pit_clock`: PIT (Programmable Interval Timer) support for Theseus, x86 only.
//! * `ps2`: general driver for interfacing with PS2 devices and issuing PS2 commands (for mouse/keyboard).
//! * `rtc`: simple driver for handling the Real Time Clock chip.
//! * `scheduler`: The scheduler and runqueue management.
//! * `serial_port`: simple driver for writing to the serial_port, used mostly for debugging.
//! * `spawn`: Functions and wrappers for spawning new Tasks, both kernel threads and userspace processes.
//! * `syscall`: Initializes the system call support, and provides basic handling and dispatching of syscalls in Theseus.
//! * `task`: Task types and structure definitions, a Task is a thread of execution.
//! * `text_display` : Defines a trait for anything that can display text to the screen
//! * `tsc`: TSC (TimeStamp Counter) support for performance counters on x86. Basically a wrapper around rdtsc.
//! * `tss`: TSS (Task State Segment support (x86 only) for Theseus.
//! * `vga_buffer`: Simple routines for printing to the screen using the x86 VGA buffer text mode.
//! * `window_manager` : Support for running and managing multiple windows
//! 



//! # Theseus's Build Process
//! Theseus uses the [cargo virtual workspace](https://doc.rust-lang.org/cargo/reference/manifest.html#the-workspace-section) feature to group all of the crates together into a single meta project, which significantly speeds up build times.     
//! 
//! The top-level Makefile basically just calls the kernel Makefile, copies the kernel build files into a top-level build directory, and then calls `grub-mkrescue` to generate a bootable .iso image.     
//!
//! The kernel Makefile (`kernel/Makefile`) actually builds all of the Rust code using [`xargo`](https://github.com/japaric/xargo), a cross-compiler toolchain that wraps the default Rust `cargo`.
//! The only special action it takes is to build the `nano_core` separately and fully link it against the architecture-specific assembly code in `nano_core/boot` into a static binary.    
//! 
//! ### Debug vs. Release mode
//! Theseus can be built in a variety of modes, but offers two presets: **debug** and **release** build modes.
//! By default, Theseus is built in debug mode (cargo's "dev" profile) for easy development. To build in release mode, set the `BUILD_MODE` environment variable when running `make`, like so:    
//! `make run BUILD_MODE=release`    
//! 
//! There is a special file `kernel/Config.mk` that contains the build mode options as well as other configuration options used in the kernel Makefile. 
//! As with most languages, release mode in Rust is *way* faster, but it does take longer to compile and can be difficult to attach a debugger.
//! 


//! ## Proper Runtime Module Loading
//! By default, Theseus is built into a single kernel binary just like a regular OS, in which all crates are linked into a single static library and then zipped up into a bootable .iso file. 
//! However, the actual research into runtime composability dictates that all modules (except the `nano_core`) are loaded at runtime, and not linked into a single static kernel binary. 
//!
//! To enable this, use the `make loadable` command to enable the `loadable` feature, which does the following:
//!
//! * Builds each crate into its own separate object file, which are not all linked together like in other OSes.
//! * Enables release mode in order to make each module file smaller and faster to load, i.e., sets `BUILD_MODE=release`.
//! * Copies each crate's object file into the top-level build directory's module subdirectory (`build/grub-isofiles/modules`) such that each module is a separate object file in the final .iso image. 
//!   That allows the running instance of Theseus to see all the modules currently available just by asking the bootloader (without needing a filesystem), and to load them individually.
//! * Sets the `loadable` config option, which as seen in the `nano_core`, will enable the `#![cfg(loadable)]` code blocks that loads other crates (e.g., the `captain`) dynamically rather than include them as static dependencies.
//! 
//! 


//! # Booting and Flow of Execution
//! The kernel takes over from the bootloader (GRUB, or another multiboot2-compatible bootloader) in `nano_core/src/boot/arch_x86_64/boot.asm:start` and is running in *32-bit protected mode*. 
//! After initializing paging and other things, the assembly file `boot.asm` jumps to `long_mode_start`, which runs 64-bit code in long mode. 
//! Then, it jumps to `start_high`, so now we're running in the higher half because Theseus is a [higher-half kernel](https://wiki.osdev.org/Higher_Half_Kernel). 
//! We then set up a new Global Descriptor Table (GDT), segmentation registers, and finally call the Rust code entry point [`nano_core_start()`](../nano_core/fn.nano_core_start.html) with the address of the multiboot2 boot information structure as the first argument (in register RDI).
//!
//! After calling `nano_core_start`, the assembly files are no longer used, and `nano_core_start` should never return. 



//! # Adding New Functionality to Theseus
//! The easiest way to add new functionality is just to create a new crate by duplicating an existing crate and changing the details in its new `Cargo.toml` file.
//! At the very least, you'll need to change the `name` entry under the `[package]` heading at the top of the `Cargo.toml` file, and you'll most likely need to change the dependencies for your new crate.     
//!
//! If your new crate needs to be initialized, you can invoke it from the [`captain::init()`](../captain/fn.init.html) function, although there may be more appropriate places to do so, such as the [`driver_init::init()`](../driver_init/fn.init.html) function for drivers.
//! 


#![no_std]
pub mod phis;
pub mod principles;

