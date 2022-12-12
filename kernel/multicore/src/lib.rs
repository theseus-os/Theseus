#![no_std]

#[cfg(target_arch = "x86_64")]
pub use apic::{
    CoreId,
    cores_count,
    get_bootstrap_core_id,
    is_bootstrap_core,
    get_my_core_id,
};
