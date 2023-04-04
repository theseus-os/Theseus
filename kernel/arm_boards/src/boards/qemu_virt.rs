//! AArch64 Board Config for the `virt` machine of Qemu

use super::{
    InterruptControllerConfig::GicV3, GicV3InterruptControllerConfig,
    BoardConfig, mpidr::DefinedMpidrValue,
};
use memory_structs::PhysicalAddress;

// local utility function to generate the CPU id from
// the affinity level 0 number of the cpu core
const fn cpu_id(aff0: u8) -> DefinedMpidrValue {
    DefinedMpidrValue::new(0, 0, 0, aff0)
}

// local utility function to generate the redistributor base
// address from the affinity level 0 number of the cpu core
const fn redist(aff0: usize) -> PhysicalAddress {
    PhysicalAddress::new_canonical(0x080A0000 + 0x20000 * aff0)
}

pub const NUM_CPUS: usize = 4;
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
};
