#![no_std]

#[macro_use] extern crate cfg_if;
#[macro_use] extern crate log;
extern crate memory; // the virtual memory subsystem 
extern crate stack;
extern crate spawn;
extern crate scheduler;

cfg_if!{

if #[cfg(target_arch="x86_64")] {

extern crate alloc;

extern crate kernel_config; // our configuration options, just a set of const definitions.
extern crate irq_safety; // for irq-safe locking and interrupt utilities
extern crate dfqueue; // decoupled, fault-tolerant queue

extern crate logger;
extern crate apic; 
extern crate mod_mgmt;
extern crate tsc;
extern crate task; 
extern crate interrupts;
extern crate acpi;
extern crate device_manager;
extern crate e1000;
#[cfg(mirror_log_to_vga)] #[macro_use] extern crate print;
extern crate first_application;
extern crate exceptions_full;
extern crate network_manager;
extern crate window_manager;
extern crate multiple_heaps;
#[cfg(simd_personality)] extern crate simd_personality;

mod x86_64;
pub use x86_64::*;

} else if #[cfg(target_arch="arm")] {

#[macro_use] extern crate lazy_static;
extern crate alloc;
extern crate cortex_m;
extern crate memory_structs;

mod armv7em;
pub use armv7em::*;

}

}
