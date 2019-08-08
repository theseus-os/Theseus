//! This crate is reserved for arm instructions. It is to be implemented and part of the functions will be replaced by `cortex-m` crate

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
extern crate cortex_m;

#[macro_use]
mod bitflags;

pub mod instructions;

mod address;

pub type Instruction = u32;