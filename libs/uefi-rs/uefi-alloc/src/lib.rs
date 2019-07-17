//! `uefi-alloc` implements Rust's global allocator interface using UEFI's memory allocation functions.
//!
//! Linking this crate in your app will allow you to use Rust's higher-level data structures,
//! like boxes, vectors, hash maps, linked lists and so on.
//!
//! # Usage
//!
//! Call the `init` function with a reference to the boot services table.
//! Failure to do so before calling a memory allocating function will panic.
//!
//! Call the `exit_boot_services` function before exiting UEFI boot services.
//! Failure to do so will turn subsequent allocation into undefined behaviour.

// Enable additional lints.
#![warn(missing_docs)]
#![deny(clippy::all)]
#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

use uefi::prelude::*;
use uefi::table::boot::{BootServices, MemoryType};

/// Reference to the boot services table, used to call the pool memory allocation functions.
///
/// The inner pointer is only safe to dereference if UEFI boot services have not been
/// exited by the host application yet.
static mut BOOT_SERVICES: Option<NonNull<BootServices>> = None;

/// Initializes the allocator.
///
/// This function is unsafe because you _must_ make sure that exit_boot_services
/// will be called when UEFI boot services will be exited.
pub unsafe fn init(boot_services: &BootServices) {
    BOOT_SERVICES = NonNull::new(boot_services as *const _ as *mut _);
}

/// Access the boot services
fn boot_services() -> NonNull<BootServices> {
    unsafe { BOOT_SERVICES.expect("Boot services are unavailable or have been exited") }
}

/// Notify the allocator library that boot services are not safe to call anymore
///
/// You must arrange for this function to be called on exit from UEFI boot services
pub fn exit_boot_services() {
    unsafe {
        BOOT_SERVICES = None;
    }
}

/// Allocator which uses the UEFI pool allocation functions.
///
/// Only valid for as long as the UEFI runtime services are available.
pub struct Allocator;

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mem_ty = MemoryType::LOADER_DATA;
        let size = layout.size();
        let align = layout.align();

        // TODO: add support for other alignments.
        if align > 8 {
            // Unsupported alignment for allocation, UEFI can only allocate 8-byte aligned addresses
            ptr::null_mut()
        } else {
            boot_services()
                .as_ref()
                .allocate_pool(mem_ty, size)
                .warning_as_error()
                .unwrap_or(ptr::null_mut())
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        boot_services()
            .as_ref()
            .free_pool(ptr)
            .warning_as_error()
            .unwrap();
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
