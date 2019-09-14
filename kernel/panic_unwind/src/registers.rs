use gimli;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::ops::{Index, IndexMut};

#[derive(Default, Clone, PartialEq, Eq)]
pub struct Registers {
    registers: [Option<u64>; 17],
}

impl Debug for Registers {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        for reg in &self.registers {
            match *reg {
                None => write!(fmt, " XXX")?,
                Some(x) => write!(fmt, " 0x{:x}", x)?,
            }
        }
        Ok(())
    }
}

impl Index<u16> for Registers {
    type Output = Option<u64>;

    fn index(&self, index: u16) -> &Option<u64> {
        &self.registers[index as usize]
    }
}

impl IndexMut<u16> for Registers {
    fn index_mut(&mut self, index: u16) -> &mut Option<u64> {
        &mut self.registers[index as usize]
    }
}

impl Index<gimli::Register> for Registers {
    type Output = Option<u64>;

    fn index(&self, reg: gimli::Register) -> &Option<u64> {
        &self[reg.0]
    }
}

impl IndexMut<gimli::Register> for Registers {
    fn index_mut(&mut self, reg: gimli::Register) -> &mut Option<u64> {
        &mut self[reg.0]
    }
}
