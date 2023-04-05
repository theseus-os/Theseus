use derive_more::{Display, Binary, Octal, LowerHex, UpperHex};

/// A unique identifier for a CPU, hardcoded in `arm_boards`.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, PartialOrd, Ord,
    Hash, Binary, Octal, LowerHex, UpperHex,
)]
#[repr(transparent)]
pub struct DefinedMpidrValue(u64);

impl DefinedMpidrValue {
    /// Returns the contained value
    pub fn value(self) -> u64 {
        self.0
    }

    /// Create an `MpidrValue` from its four affinity numbers
    pub(crate) const fn new(aff3: u8, aff2: u8, aff1: u8, aff0: u8) -> Self {
        let aff3 = (aff3 as u64) << 32;
        let aff2 = (aff2 as u64) << 16;
        let aff1 = (aff1 as u64) <<  8;
        let aff0 = (aff0 as u64) <<  0;
        Self(aff3 | aff2 | aff1 | aff0)
    }
}

/// Tries to find this MPIDR value among those known for this board.
///
/// Returns None if no CPU has this MPIDR value.
pub fn find_mpidr(mpidr_value: u64) -> Option<DefinedMpidrValue> {
    for def_mpidr in crate::BOARD_CONFIG.cpu_ids {
        if def_mpidr.value() == mpidr_value {
            return Some(def_mpidr)
        }
    }

    None
}
