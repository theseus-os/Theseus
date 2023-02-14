//! Registers and stores unwind information from external components.
//! 
//! Typically, Theseus maintains metadata about all crates loaded and linked into system.
//! Unwind info is part of this (the `.eh_frame` section data).
//! 
//! However, for other components compiled outside of Theseus and beyond it's knowledge,
//! we don't necessarily have control over or access to those components' section structure
//! and miscellaneous internal information.
//! Nevertheless, we still want to be able to handle their resource management properly
//! by unwinding through their stack frames and running cleanup handlers as needed.
//! 
//! Hence, this crate allows those external components to register their own 
//! unwind information as needed.
//! Currently, we use this crate to support [`wasmtime-jit`], which needs to 
//! [register unwinding information] for JIT-compiled native code built from WASM binaries
//! and then deregister it when those JIT-ted modules are destroyed.
//!
//! The API here is loosely similar to `libunwind`'s two-part API: 
//! `__register_frame()` and `__deregister_frame()`.
//! Because it must accept raw pointers to sections, all APIs are inherently unsafe.
//! 
//! ## Future TODOs
//! Currently, this crate unsafely assumes that the third party component that registers its unwind info
//! will ensure that unwind info is valid for the full duration of it being registered.
//! 
//! In the future, ideally we would redesign this crate to offer a safer API
//! that obtains shared ownership
//! However, that would of course require modifying the dependent crates and redesigning them
//! to utilize this safer API.
//! 
//! [register unwinding information]: https://github.com/bytecodealliance/wasmtime/blob/7cf5f058303d2ee8c42df658d4ca608771a8561d/crates/jit/src/code_memory.rs#L185
//! [`wasmtime-jit`]: https://docs.rs/wasmtime-jit/latest/wasmtime_jit/

// TODO: add documentation to each unsafe block, laying out all the conditions under which it's safe or unsafe to use it.
#![allow(clippy::missing_safety_doc)]
#![no_std]
#![feature(map_try_insert)]

extern crate alloc;

use core::ops::{Range, Bound::{Included, Unbounded}};
use alloc::collections::BTreeMap;
use memory::VirtualAddress;
use spin::Mutex;
use log::*;

/// The system-wide set of unwind information registered by external components.
/// 
/// The map key is the text section's base `VirtualAddress`.
static EXTERNAL_UNWIND_INFO: Mutex<BTreeMap<VirtualAddress, ExternalUnwindInfo>> = Mutex::new(BTreeMap::new());

/// Unwinding information for an external (non-Theseus) component.
#[derive(Debug, Clone)]
pub struct ExternalUnwindInfo {
    /// The bounds of the text section that this unwinding info pertains to.
    pub text_section: Range<VirtualAddress>,
    /// The bounds of the unwinding information (e.g., `.eh_frame`) for the above text section.
    pub unwind_info: Range<VirtualAddress>,
}

/// Register a new section of external unwinding information.
/// 
/// Returns an error if unwinding information has already been registered 
/// for the given `text_section_base_address`.
pub unsafe fn register_unwind_info(
    text_section_base_address: *mut u8,
    text_section_len: usize,
    unwind_info: *mut u8,
    unwind_len: usize,
) -> Result<(), ExternalUnwindInfoError> {

    let mut uw = EXTERNAL_UNWIND_INFO.lock();
    let text_start = VirtualAddress::new_canonical(text_section_base_address as usize);
    let text_end   = VirtualAddress::new_canonical(text_section_base_address as usize + text_section_len);
    let uw_start   = VirtualAddress::new_canonical(unwind_info as usize);
    let uw_end     = VirtualAddress::new_canonical(unwind_info as usize + unwind_len);

    let uw_info = ExternalUnwindInfo {
        text_section: text_start .. text_end,
        unwind_info:  uw_start   .. uw_end,
    };

    uw.try_insert(text_start, uw_info)
        .map(|_| ())
        .map_err(|_e| {
            error!("External unwind info for {text_start:#X} was already registered");
            ExternalUnwindInfoError::AlreadyRegistered
        })
}


/// Remove a previously-registered section of external unwinding information.
/// 
/// Returns [`ExternalUnwindInfoError::NotRegistered`] if no unwinding information
/// was registered for the given `text_section_base_address`.
pub unsafe fn deregister_unwind_info(
    text_section_base_address: *mut u8
) -> Result<(), ExternalUnwindInfoError> {
    EXTERNAL_UNWIND_INFO.lock()
        .remove(&VirtualAddress::new_canonical(text_section_base_address as usize))
        .map(|_| ())
        .ok_or(ExternalUnwindInfoError::NotRegistered)
}


/// Returns the registered external unwind information that covers
/// the given address in its text section.
pub fn get_unwind_info(
    text_section_address: VirtualAddress
) -> Option<ExternalUnwindInfo> {
    // iterate over the entries in sorted order, up to the given `text_section_address` inclusively.
    for (_text_base_addr, uw_info) in EXTERNAL_UNWIND_INFO.lock().range((Unbounded, Included(text_section_address))) {
        if uw_info.text_section.contains(&text_section_address) {
            return Some(uw_info.clone());
        }
    }

    None
}

/// Errors that may occur when [registering] or [deregistering]
/// external unwind info.
///
/// [registering]: register_unwind_info
/// [deregistering] deregister_unwind_info
pub enum ExternalUnwindInfoError {
    /// The unwinding info trying to be registered was already registered.
    AlreadyRegistered,
    /// The unwinding info trying to be deregistered was not yet registered.
    NotRegistered,
}
