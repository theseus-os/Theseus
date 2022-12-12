//! This crate contains no definition; it
//! just re-exports types and functions from `apic` on x86_64

#![no_std]

#[cfg(target_arch = "x86_64")]
pub use apic::{
    CoreId,
    cores_count,
    get_bootstrap_core_id,
    is_bootstrap_core,
    get_my_core_id,
};
