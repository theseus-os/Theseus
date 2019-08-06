//! Basic interrupt handling structures and simple handler routines.

#![no_std]
#![feature(abi_x86_interrupt)]

#![allow(dead_code)]


// #[macro_use] extern crate log;
// #[macro_use] extern crate vga_buffer;
// #[cfg(target_arch = "x86_64")]
// extern crate x86_64;
// #[cfg(any(target_arch = "aarch64"))]
// extern crate aarch64;
// extern crate spin;
// extern crate port_io;
// extern crate kernel_config;
extern crate memory;
// #[cfg(target_arch = "x86_64")]
// extern crate apic;
// extern crate pit_clock;
// #[cfg(target_arch = "x86_64")]
// extern crate tss;
// #[cfg(target_arch = "x86_64")]
// extern crate gdt;
// #[cfg(target_arch = "x86_64")]
// extern crate exceptions_early;
// extern crate pic;
// extern crate scheduler;
// extern crate keyboard;
// extern crate mouse;
// extern crate ps2;
// extern crate tlb_shootdown;



// use ps2::handle_mouse_packet;
// #[cfg(target_arch = "x86_64")]
// use x86_64::structures::idt::{Idt, LockedIdt, ExceptionStackFrame, HandlerFunc};
// #[cfg(any(target_arch = "aarch64"))]
// use aarch64::structures::idt::{Idt, LockedIdt, HandlerFunc};
// use spin::Once;
// use kernel_config::time::{CONFIG_PIT_FREQUENCY_HZ}; //, CONFIG_RTC_FREQUENCY_HZ};
// // use rtc;
// use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use memory::VirtualAddress;
// #[cfg(target_arch = "x86_64")]
// use apic::{INTERRUPT_CHIP, InterruptChip};
// use pic::PIC_MASTER_OFFSET;


#[cfg(target_arch = "aarch64")]
pub fn init(double_fault_stack_top_unusable: VirtualAddress, privilege_stack_top_unusable: VirtualAddress) 
    -> Result<(), &'static str> 
{
    // TODO
    Ok(())
}

#[cfg(target_arch = "aarch64")]
pub fn init_ap(/*parameters*/) -> Result<(), &'static str> {
    // TODO
    Ok(())
}

/// Establishes the default interrupt handlers that are statically known.
fn set_handlers() {
    // TODO
}