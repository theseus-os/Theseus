use cortex_a::registers::MPIDR_EL1;
use tock_registers::interfaces::Readable;

use core::fmt;

/// A unique identifier for a CPU core.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct CpuId(u32);

/// An equivalent to Option<CpuId>, which internally encodes None as
/// `u32::MAX`, which is an invalid CpuId (bits [4:7] of affinity level
/// 0 must always be cleared). This guarantees that it compiles down to
/// lock-free native atomic instructions when using it inside of an atomic
/// type like [`AtomicCell`], as u32 is atomic when running on ARMv8.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct OptionalCpuId(u32);

/// A unique identifier for a CPU core.
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct MpidrValue(u64);

/// Returns the number of CPUs (SMP cores) that exist and
/// are currently initialized on this system.
pub fn cpu_count() -> u32 {
    // The ARM port doesn't start secondary cores for the moment.
    1
}

/// Returns the ID of the bootstrap CPU (if known), which
/// is the first CPU to run after system power-on.
pub fn bootstrap_cpu() -> CpuId {
    // The ARM port doesn't start secondary cores for the moment,
    // so the current CPU can only be the "bootstrap" CPU.
    current_cpu()
}

/// Returns true if the currently executing CPU is the bootstrap
/// CPU, i.e., the first procesor to run after system power-on.
pub fn is_bootstrap_cpu() -> bool {
    // The ARM port doesn't start secondary cores for the moment,
    // so the current CPU can only be the "bootstrap" CPU.
    true
}

/// Returns the ID of the currently executing CPU.
pub fn current_cpu() -> CpuId {
    MpidrValue(MPIDR_EL1.get() as u64).into()
}

impl CpuId {
    /// Reads an affinity level from this CpuId
    ///
    /// Valid affinity levels are 0, 1, 2, 3
    pub fn affinity(self, level: u8) -> u8 {
        assert!(level < 4, "Valid affinity levels are 0, 1, 2, 3");

        (self.0 >> (level * 8)) as u8
    }
}

impl MpidrValue {
    /// Obtain the inner raw u64 that was read from the MPIDR_EL1 register
    pub fn get(self) -> u64 {
        self.0
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
