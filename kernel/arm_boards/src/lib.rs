//! Per-board definitions for AArch64 builds
//!
//! | Feature | CPU Cores | Interrupt Controller | Secondary Cores Startup Method |
//! | --- | --- | --- | --- |
//! | qemu_virt | 4 | GICv3 | PSCI |
//!

#![no_std]
#![feature(const_trait_impl)]

use memory_structs::PhysicalAddress;

#[derive(Debug, Copy, Clone)]
pub struct GicV3InterruptControllerConfig {
    pub distributor_base_address: PhysicalAddress,
    pub redistributor_base_addresses: [PhysicalAddress; board::NUM_CPUS],
}

#[derive(Debug, Copy, Clone)]
pub enum InterruptControllerConfig {
    GicV3(GicV3InterruptControllerConfig),
}

/*
TODO: multicore_bringup: wake secondary cores based on this:
pub enum SecondaryCoresStartup {
    Psci,
}
*/

#[derive(Debug, Copy, Clone)]
pub struct BoardConfig {
    pub cpu_ids: [mpidr::DefinedMpidrValue; board::NUM_CPUS],
    pub interrupt_controller: InterruptControllerConfig,
}

// by default & on x86_64, the default.rs file is used
#[cfg_attr(all(target_arch = "aarch64", feature = "qemu_virt"), path = "boards/qemu_virt.rs")]
#[cfg_attr(not(any(
    all(target_arch = "aarch64", feature = "qemu_virt"),
)), path = "boards/unselected.rs")]
mod board;

pub mod mpidr;

pub use board::NUM_CPUS;
pub use board::BOARD_CONFIG;
