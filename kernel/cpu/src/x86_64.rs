//! Implements functions for accessing CPU-specific information on x86_64,
//! which is primarily a simple wrapper around [`apic`]-provided features.

use core::fmt;

use crate::CpuId;
use apic::ApicId;

impl From<ApicId> for CpuId {
    fn from(apic_id: ApicId) -> Self {
        CpuId(apic_id.value())
    }
}

impl From<CpuId> for ApicId {
    fn from(cpu_id: CpuId) -> Self {
        ApicId::try_from(cpu_id.value()).expect("An invalid CpuId was encountered")
    }
}

impl TryFrom<u32> for CpuId {
    type Error = u32;
    fn try_from(raw_cpu_id: u32) -> Result<Self, Self::Error> {
        ApicId::try_from(raw_cpu_id)
            .map(Into::into)
    }
}


/// Returns the number of CPUs (SMP cores) that exist and
/// are currently initialized on this system.
pub fn cpu_count() -> u32 {
    apic::cpu_count()
}

/// Returns the ID of the bootstrap CPU (if known), which
/// is the first CPU to run after system power-on.
pub fn bootstrap_cpu() -> Option<CpuId> {
    apic::bootstrap_cpu().map(Into::into)
}

/// Returns true if the currently executing CPU is the bootstrap
/// CPU, i.e., the first CPU to run after system power-on.
pub fn is_bootstrap_cpu() -> bool {
    apic::is_bootstrap_cpu()
}

/// Returns the ID of the currently executing CPU.
pub fn current_cpu() -> CpuId {
    apic::current_cpu().into()
}

/// A wrapper around `Option<CpuId>` with a forced type alignment of 8 bytes,
/// which guarantees that it compiles down to lock-free native atomic instructions
/// when using it inside of an atomic type like [`AtomicCell`].
#[derive(Copy, Clone)]
#[repr(align(8))]
pub struct OptionalCpuId(Option<CpuId>);
impl From<Option<CpuId>> for OptionalCpuId {
    fn from(opt: Option<CpuId>) -> Self {
        Self(opt)
    }
}

impl From<OptionalCpuId> for Option<CpuId> {
    fn from(val: OptionalCpuId) -> Self {
        val.0
    }
}

impl fmt::Debug for OptionalCpuId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
