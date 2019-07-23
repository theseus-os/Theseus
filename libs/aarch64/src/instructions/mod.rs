//! Low level functions for special x86 instructions.

pub mod interrupts;
pub mod tlb;

/// For compatibility
/// Write 64 bits to msr register in x86.
/// In arm, use cortex-m::register to get the value of registers
pub unsafe fn wrmsr(_msr: u32, _value: u64) {
    //TODO
}

/// For compatibility
/// Read 64 bits msr register in x86.
/// In arm, use cortex-m::register to get the value of registers
pub fn rdmsr(_msr: u32) -> u64 {
    ///
    0
}