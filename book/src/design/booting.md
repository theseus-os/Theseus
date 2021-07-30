# Booting Process and Flow of Execution

## Initial assembly code 
The Theseus kernel takes over from the bootloader and first executes code in *32-bit protected mode*, which corresponds to the `start` function in `kernel/nano_core/src/boot/arch_x86_64/boot.asm`.
Currently we use GRUB configured as a legacy bootloader (non-UEFI) and Theseus expects to be booted by a *Multiboot2*-compliant bootloader.
In the future, we intend to add support for booting via the UEFI standard, especially on other architectures without a legacy BIOS.

After initializing a very simple page table and other miscellaneous hardware features, the assembly file `boot.asm` jumps to `long_mode_start`, which now runs *64-bit code* in long mode.
Then, it jumps to `start_high`, such that we're not running the base kernel image in the higher half (see more about [higher-half kernels here](https://wiki.osdev.org/Higher_Half_Kernel)).
We then set up a new Global Descriptor Table (GDT), segmentation registers, and finally call the Rust code entry point [`nano_core_start()`](https://theseus-os.github.io/Theseus/doc/nano_core/index.html) with the proper arguments. 
After calling `nano_core_start`, the assembly files are no longer used, and `nano_core_start` should never return.


## Initial Rust code: the `nano_core`
The `nano_core`, specifically `nano_core_start()`, is the first Rust code to run in Theseus. 
It performs a very minimal bootstrap/setup procedure, in which it performs the following duties:

* Initializes logging and a basic VGA text display, for the purpose of debugging.
* Sets up simple CPU exception handlers, for the purpose of catching early errors. 
* Sets up a basic virtual memory environment.
    * This creates the first and only virtual address space and remaps all of the bootloader-loaded sections into that new single address space. 
    * Importantly, Theseus doesn't depend on anything else from the bootloader after this point.
* Initializes the `mod_mgmt` subsystem, which creates the first `CrateNamespace` and allows other crates to be dynamically loaded. 
* Loads the invokes the `captain`, which handles the rest of the OS initialization procedures. 

The `nano_core` is quite general and minimalistic; it rarely needs to change. The majority of the OS-specific configuration and initialization happens in the `captain`, so changes should likely be made there.


## Main Initialization routine: the `captain`
The `captain` "steers the ship" of Theseus, meaning that it contains basic logic for initializing all of the other subsystems in the proper order and with the proper flow of data between them.

Currently, there is a single `captain` implementation in Theseus (for a standard x86_64 machine), which does the following:
* Initializes ACPI and APIC to discover multicore and other hardware configuration,
* Sets up interrupt and exception handlers,
* Sets up basic device drivers,
* Spawns event handling threads,
* Initializes the window manager and graphics subsystem,
* Starts the first user application, which is currently a single terminal window.

At the end, the `captain` must enable interrupts to allow the system to schedule other tasks. 
It then falls into an idle loop that does nothing and will never be run again by the scheduler.

> Note: in the future, Theseus will add additional architecture-specific `captain`s for different platforms.
