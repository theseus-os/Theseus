//! Utility functions for the most common UEFI patterns.
//!
//! This crate simply exports some extension traits
//! which add utility functions to various UEFI objects.

#![no_std]


 #![feature(extern_crate_item_prelude)]
 extern crate alloc;

mod boot;
pub use self::boot::BootServicesExt;
