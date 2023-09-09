//! Board configuration for QEMU's basic `virt` machine with 4 CPUs.

use super::{
    InterruptControllerConfig::GicV3, GicV3InterruptControllerConfig,
    BoardConfig, mpidr::DefinedMpidrValue, PciEcamConfig,
};
use memory_structs::PhysicalAddress;

/// Generates an MPIDR value from a CPU's 0th affinity level.
const fn cpu_id(aff0: u8) -> DefinedMpidrValue {
    DefinedMpidrValue::new(0, 0, 0, aff0)
}

/// Generates a Redistributor base address from a CPU's 0th affinity level.
const fn redist(aff0: usize) -> PhysicalAddress {
    PhysicalAddress::new_canonical(0x080A0000 + 0x20000 * aff0)
}

pub const NUM_CPUS: usize = 4;
pub const NUM_PL011_UARTS: usize = 1;

pub const BOARD_CONFIG: BoardConfig = BoardConfig {
    cpu_ids: [
        cpu_id(0),
        cpu_id(1),
        cpu_id(2),
        cpu_id(3),
    ],
    interrupt_controller: GicV3(GicV3InterruptControllerConfig {
        distributor_base_address: PhysicalAddress::new_canonical(0x08000000),
        redistributor_base_addresses: [
            redist(0),
            redist(1),
            redist(2),
            redist(3),
        ],
    }),
    pl011_base_addresses: [ PhysicalAddress::new_canonical(0x09000000) ],
    pl011_rx_spi: 33,
    cpu_local_timer_ppi: 30,

    // obtained via internal qemu debugging
    // todo: will this always be correct?
    pci_ecam: PciEcamConfig {
        base_address: PhysicalAddress::new_canonical(0x4010000000),
        size_bytes: 0x10000000,
    }
};
