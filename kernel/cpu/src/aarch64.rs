//! Implements functions for accessing CPU-specific information on aarch64.

use cortex_a::registers::MPIDR_EL1;
use tock_registers::interfaces::Readable;
use derive_more::{Display, Binary, Octal, LowerHex, UpperHex};
use irq_safety::RwLockIrqSafe;
use core::fmt;
use alloc::vec::Vec;

use super::CpuId;

// The vector of CpuIds for known and online CPU cores
static ONLINE_CPUS: RwLockIrqSafe<Vec<CpuId>> = RwLockIrqSafe::new(Vec::new());

/// This must be called once for every CPU core in the system.
///
/// The first CPU to register itself is called the BSP (bootstrap processor).
/// When it does so (from captain), it must set the `bootstrap` parameter
/// to `true`. Other cores must set it to `false`.
pub fn register_cpu(bootstrap: bool) -> Result<(), &'static str> {
    let mut locked = ONLINE_CPUS.write();

    // the vector must be empty when the bootstrap
    // processor registers itself.
    if bootstrap == locked.is_empty() {
        let cpu_id = current_cpu();

        if !locked.contains(&cpu_id) {
            locked.push(cpu_id);
            Ok(())
        } else {
            Err("Tried to register the same CpuId twice")
        }
    } else {
        match bootstrap {
            true  => Err("Tried to register the BSP after another core: invalid"),
            false => Err("Tried to register a secondary CPU before the BSP: invalid"),
        }
    }
}

/// Returns the number of CPUs (SMP cores) that exist and
/// are currently initialized on this system.
pub fn cpu_count() -> u32 {
    ONLINE_CPUS.read().len() as u32
}

/// Returns the ID of the bootstrap CPU (if known), which
/// is the first CPU to run after system power-on.
pub fn bootstrap_cpu() -> Option<CpuId> {
    ONLINE_CPUS.read().first().copied()
}

/// Returns true if the currently executing CPU is the bootstrap
/// CPU, i.e., the first CPU to run after system power-on.
pub fn is_bootstrap_cpu() -> bool {
    Some(current_cpu()) == bootstrap_cpu()
}

/// Returns the ID of the currently executing CPU.
pub fn current_cpu() -> CpuId {
    MpidrValue(MPIDR_EL1.get() as u64).into()
}

/// A unique identifier for a CPU, read from the `MPIDR_EL1` register on aarch64.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, PartialOrd, Ord,
    Hash, Binary, Octal, LowerHex, UpperHex,
)]
#[repr(transparent)]
pub struct MpidrValue(u64);

impl MpidrValue {
    /// Returns the inner raw value read from the `MPIDR_EL1` register.
    pub fn value(self) -> u64 {
        self.0
    }

    /// Reads an affinity `level` from this `MpidrValue`.
    ///
    /// Panics if the given affinity level is not 0, 1, 2, or 3.
    pub fn affinity(self, level: u8) -> u8 {
        let shift = match level {
            0 => 0,
            1 => 8,
            2 => 16,
            3 => 32,
            _ => panic!("Valid affinity levels are 0, 1, 2, 3"),
        };
        (self.0 >> shift) as u8
    }

    /// Create an `MpidrValue` from its four affinity numbers
    pub fn new(aff3: u8, aff2: u8, aff1: u8, aff0: u8) -> Self {
        let aff3 = (aff3 as u64) << 32;
        let aff2 = (aff2 as u64) << 16;
        let aff1 = (aff1 as u64) <<  8;
        let aff0 = (aff0 as u64) <<  0;
        Self(aff3 | aff2 | aff1 | aff0)
    }
}

impl From<CpuId> for MpidrValue {
    fn from(cpu_id: CpuId) -> Self {
        // move aff3 from bits [24:31] to [32:39]
        let aff_3     = ((cpu_id.0 & 0xff000000) as u64) << 8;
        let aff_0_1_2 =  (cpu_id.0 & 0x00ffffff) as u64;

        Self(aff_3 | aff_0_1_2)
    }
}

impl From<MpidrValue> for CpuId {
    fn from(mpidr: MpidrValue) -> Self {
        // move aff3 from bits [32:39] to [24:31]
        let aff_3     = ((mpidr.0 & 0xff00000000) >> 8) as u32;
        let aff_0_1_2 =  (mpidr.0 & 0x0000ffffff) as u32;

        Self(aff_3 | aff_0_1_2)
    }
}

/// An equivalent to Option<CpuId>, which internally encodes None as
/// `u32::MAX`, which is an invalid CpuId (bits [4:7] of affinity level
/// 0 must always be cleared). This guarantees that it compiles down to
/// lock-free native atomic instructions when using it inside of an atomic
/// type like [`AtomicCell`], as u32 is atomic when running on ARMv8.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct OptionalCpuId(u32);

impl From<Option<CpuId>> for OptionalCpuId {
    fn from(opt: Option<CpuId>) -> Self {
        match opt.map(|v| v.0) {
            Some(u32::MAX) => panic!("CpuId is too big!"),
            Some(cpu_id) => OptionalCpuId(cpu_id),
            None => OptionalCpuId(u32::MAX),
        }
    }
}

impl From<OptionalCpuId> for Option<CpuId> {
    fn from(val: OptionalCpuId) -> Self {
        match val.0 {
            u32::MAX => None,
            v => Some(CpuId(v)),
        }
    }
}

impl fmt::Debug for OptionalCpuId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", Option::<CpuId>::from(*self))
    }
}
