//! This crate provides x86_64 specific functions and data structures,
//! and access to various system registers.

#![cfg(any(target_arch="aarch64", target_arch="x86_64"))]

#![warn(missing_docs)]

#![feature(const_fn)]
#![feature(asm)]
// #![feature(associated_consts)]
#![feature(abi_x86_interrupt)]
#![cfg_attr(test, allow(unused_features))]

#![no_std]

pub use address::{VirtualAddress, PhysicalAddress};

extern crate bit_field;
extern crate irq_safety;

#[macro_use]
mod bitflags;

pub mod instructions;
pub mod registers;
pub mod structures;

mod address;

/// Represents a protection ring level.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum PrivilegeLevel {
    /// Privilege-level 0 (most privilege): This level is used by critical system-software
    /// components that require direct access to, and control over, all processor and system
    /// resources. This can include BIOS, memory-management functions, and interrupt handlers.
    Ring0 = 0,

    /// Privilege-level 1 (moderate privilege): This level is used by less-critical system-
    /// software services that can access and control a limited scope of processor and system
    /// resources. Software running at these privilege levels might include some device drivers
    /// and library routines. The actual privileges of this level are defined by the
    /// operating system.
    Ring1 = 1,

    /// Privilege-level 2 (moderate privilege): Like level 1, this level is used by
    /// less-critical system-software services that can access and control a limited scope of
    /// processor and system resources. The actual privileges of this level are defined by the
    /// operating system.
    Ring2 = 2,

    /// Privilege-level 3 (least privilege): This level is used by application software.
    /// Software running at privilege-level 3 is normally prevented from directly accessing
    /// most processor and system resources. Instead, applications request access to the
    /// protected processor and system resources by calling more-privileged service routines
    /// to perform the accesses.
    Ring3 = 3,
}

impl PrivilegeLevel {
    /// Creates a `PrivilegeLevel` from a numeric value. The value must be in the range 0..4.
    ///
    /// This function panics if the passed value is >3.
    pub fn from_u16(value: u16) -> PrivilegeLevel {
        match value {
            0 => PrivilegeLevel::Ring0,
            1 => PrivilegeLevel::Ring1,
            2 => PrivilegeLevel::Ring2,
            3 => PrivilegeLevel::Ring3,
            i => panic!("{} is not a valid privilege level", i),
        }
    }
}
