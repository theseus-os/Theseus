//! Implements functions for accessing CPU-specific information on aarch64.

use cortex_a::registers::MPIDR_EL1;
use tock_registers::interfaces::Readable;
use derive_more::{Display, Binary, Octal, LowerHex, UpperHex};
use sync_irq::IrqSafeRwLock;
use core::fmt;
use alloc::vec::Vec;
use arm_boards::{mpidr::DefinedMpidrValue, BOARD_CONFIG};

use super::CpuId;

// The vector of CpuIds for known and online CPU cores
static ONLINE_CPUS: IrqSafeRwLock<Vec<CpuId>> = IrqSafeRwLock::new(Vec::new());

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
    MpidrValue(MPIDR_EL1.get()).into()
}

/// A unique identifier for a CPU, read from the `MPIDR_EL1` register on aarch64.
#[derive(
    Clone, Copy, Debug, Display, PartialEq, Eq, PartialOrd, Ord,
    Hash, Binary, Octal, LowerHex, UpperHex,
)]
#[repr(transparent)]
pub struct MpidrValue(u64);

/// Affinity Levels and corresponding bit ranges
///
/// The associated integers are the locations (N..(N+8))
/// of the corresponding bits in an [`MpidrValue`].
#[derive(Copy, Clone, Debug)]
#[repr(u64)]
pub enum AffinityShift {
    LevelZero  = 0,
    LevelOne   = 8,
    LevelTwo   = 16,
    LevelThree = 32,
}

impl MpidrValue {
    /// Returns the inner raw value read from the `MPIDR_EL1` register.
    pub fn value(self) -> u64 {
        self.0
    }

    /// Reads an affinity `level` from this `MpidrValue`.
    pub fn affinity(self, level: AffinityShift) -> u64 {
        (self.0 >> (level as u64)) & (u8::MAX as u64)
    }
}

impl From<DefinedMpidrValue> for MpidrValue {
    fn from(def_mpidr: DefinedMpidrValue) -> Self {
        Self(def_mpidr.value())
    }
}

impl From<DefinedMpidrValue> for CpuId {
    fn from(def_mpidr: DefinedMpidrValue) -> Self {
        Self::from(MpidrValue::from(def_mpidr))
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

impl TryFrom<u64> for MpidrValue {
    type Error = &'static str;

    /// Tries to find this MPIDR value in those defined by
    /// `arm_boards::cpu_ids`. Fails if No CPU has this MPIDR value.
    fn try_from(mpidr_value: u64) -> Result<Self, Self::Error> {
        for def_mpidr in BOARD_CONFIG.cpu_ids {
            if def_mpidr.value() == mpidr_value {
                return Ok(def_mpidr.into())
            }
        }

        Err("No CPU has this MPIDR value")
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
