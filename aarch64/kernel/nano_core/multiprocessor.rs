use cortex_a::registers::*;

use tock_registers::interfaces::Readable;

pub fn is_multicore() -> bool {
    (MPIDR_EL1.get() & 0x40000000) == 0
}

pub fn get_core_num() -> u32 {
    let reg = MPIDR_EL1.get();
    let aff0_1_2 = (reg & 0xffffff) as u32;
    let aff3 = ((reg >> 32) & 0xff) as u32;
    aff0_1_2 | (aff3 << 24)
}