//! Basic interrupt handling structures and simple handler routines.

#![no_std]

#![allow(dead_code)]

extern crate memory;

use memory::VirtualAddress;

/// TODO: init the interrupts handlers
pub fn init(double_fault_stack_top_unusable: VirtualAddress, privilege_stack_top_unusable: VirtualAddress) 
    -> Result<(), &'static str> 
{
    // TODO
    Ok(())
}

// Establishes the default interrupt handlers that are statically known.
fn set_handlers() {
    // TODO
}