//! Configuration and definitions for specific boards on aarch64 systems.
//!
//! | Board Name | Num CPUs  | Interrupt Controller | Secondary CPU Startup Method |
//! | ---------- | --------- | -------------------- | ---------------------------- |
//! | qemu_virt  | 4         | GICv3                | PSCI                         |
//!

#![no_std]
#![feature(const_trait_impl)]

cfg_if::cfg_if! {
if #[cfg(target_arch = "aarch64")] {

use memory_structs::PhysicalAddress;

#[derive(Debug, Copy, Clone)]
pub struct GicV3InterruptControllerConfig {
    pub distributor_base_address: PhysicalAddress,
    pub redistributor_base_addresses: [PhysicalAddress; NUM_CPUS],
}

#[derive(Debug, Copy, Clone)]
pub struct PciEcamConfig {
    pub base_address: PhysicalAddress,
    pub size_bytes: usize,
}

/// This excludes GICv2 because there's no way to send IPIs
/// with GICv2 via the current driver API.
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

#[derive(Debug, Clone)]
pub struct BoardConfig {
    pub cpu_ids: [mpidr::DefinedMpidrValue; NUM_CPUS],
    pub interrupt_controller: InterruptControllerConfig,
    pub pl011_base_addresses: [PhysicalAddress; NUM_PL011_UARTS],

    /// The IRQ number reserved for the PL011 Single-Serial-Port Controller
    /// which Theseus currently uses for logging and UART console.
    pub pl011_rx_spi: u8,

    /// The IRQ number reserved for CPU-local timer interrupts,
    /// which Theseus currently uses for preemptive task switching.
    //
    // aarch64 manuals define the default timer IRQ number to be 30.
    pub cpu_local_timer_ppi: u8,

    pub pci_ecam: PciEcamConfig,
}

// by default & on x86_64, the default.rs file is used
#[cfg_attr(feature = "qemu_virt", path = "boards/qemu_virt.rs")]
#[cfg_attr(not(any(
    feature = "qemu_virt",
)), path = "boards/unselected.rs")]
mod board;

pub mod mpidr;

pub use board::{NUM_CPUS, NUM_PL011_UARTS, BOARD_CONFIG};


} // end of cfg(target_arch = "aarch64")
} // end of cfg_if
