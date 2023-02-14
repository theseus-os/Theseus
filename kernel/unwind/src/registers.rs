//! Struct definitions for various sets of register values that are useful in unwinding.

use gimli::{self, X86_64};
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::ops::{Index, IndexMut};

/// The set of register values that existed during a single point in time,
/// i.e., at one point in a given stack frame.
/// 
/// These are used for iterating through frames in a call stack
/// and calculating the caller frame's register values.
/// 
/// The register values herein can be indexed by using DWARF-specific register IDs,
/// which are constant values that are defined in the ELF x86_86 ABI.
/// [Here is a brief link](https://docs.rs/gimli/0.19.0/gimli/struct.X86_64.html)
/// that defines these constants in a practical, useful manner.
/// 
/// # Important Note
/// The number of registers defined here must be one greater than 
/// the number of registers defined in the `LandingRegisters` struct,
/// because this one includes the return address too.
/// 
/// Currently, this structure has room for `17` optional registers.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Registers {
    registers: [Option<u64>; 17],
}

impl Registers {
    /// Returns the value of the stack pointer register.
    pub fn stack_pointer(&self) -> Option<u64> {
        self[X86_64::RSP]
    }

    /// Returns the value of the return address for this register set.
    pub fn return_address(&self) -> Option<u64> {
        self[X86_64::RA]
    }
}

impl Debug for Registers {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        for (i, reg) in self.registers.iter().enumerate() {
            match *reg {
                None => { } // write!(fmt, "[{}]: None, ", i)?,
                Some(r) => write!(fmt, "[{i}]: {r:#X}, ")?,
            }
        }
        Ok(())
    }
}

impl Index<gimli::Register> for Registers {
    type Output = Option<u64>;

    fn index(&self, reg: gimli::Register) -> &Option<u64> {
        &self.registers[reg.0 as usize]
    }
}

impl IndexMut<gimli::Register> for Registers {
    fn index_mut(&mut self, reg: gimli::Register) -> &mut Option<u64> {
        &mut self.registers[reg.0 as usize]
    }
}


/// Contains the register values that will be restored to the actual CPU registers
/// right before jumping to a landing pad function.
/// 
/// # Important Note
/// This should be kept in sync with the number of elements 
/// in the `Registers` struct; this must have one less element.
#[derive(Debug)]
#[repr(C)]
pub struct LandingRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub r8:  u64,
    pub r9:  u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    // Not sure if we need to include other registers here, like rflags or segment registers. 
    // We probably do for SIMD at least.
}


/// Contains the registers that are callee-saved.
/// This is intended to be used at the beginning of stack unwinding for two purposes:
/// 1. The unwinding tables need an initial value for these registers in order to 
///    calculate the register values for the previous stack frame based on register transformation rules,
/// 2. To know which register values to restore after unwinding is complete.
/// 
/// This is currently x86_64-specific.
#[derive(Debug)]
#[repr(C)]
pub struct SavedRegs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
}
