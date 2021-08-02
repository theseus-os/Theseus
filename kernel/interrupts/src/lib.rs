#![no_std]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate cfg_if;

cfg_if!{

if #[cfg(target_arch="x86_64")] {

#[macro_use] extern crate log;
#[macro_use] extern crate vga_buffer;
extern crate x86_64;
extern crate spin;
extern crate port_io;
extern crate serial_port;
extern crate kernel_config;
extern crate memory;
extern crate apic;
extern crate pit_clock;
extern crate tss;
extern crate gdt;
extern crate exceptions_early;
extern crate pic;
extern crate scheduler;
extern crate keyboard;
extern crate mouse;
extern crate ps2;
extern crate tlb_shootdown;

mod arch_x86_64;
pub use arch_x86_64::*;

} else if #[cfg(target_arch="arm")] {

mod arch_armv7em;
pub use arch_armv7em::*;

}

}
