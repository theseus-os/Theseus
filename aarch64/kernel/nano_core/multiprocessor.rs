use cortex_a::registers::*;

use tock_registers::interfaces::Readable;

// Reads the "multicore" bit in MPIDR_EL1
pub fn is_multicore() -> bool {
    // if bit 30 is set, system is single-core
    // if it's clear, system is multi-core
    (MPIDR_EL1.get() & (1 << 30)) == 0
}

/// Obtain the core "affinity" numbers
/// as a u32 (one byte per affinity level)
///
/// - Affinity level 0 in bits [0:7]
/// - Affinity level 1 in bits [8:15]
/// - Affinity level 2 in bits [16:23]
/// - Affinity level 3 in bits [24:31]
pub fn get_core_num() -> u32 {
    let reg: u64 = MPIDR_EL1.get();

    let aff_0_1_2 =    reg & 0x00_ff_ff_ff;
    let aff_3 = (reg >> 8) & 0xff_00_00_00;

    (aff_0_1_2 | aff_3) as u32
}