use core::fmt;

pub use apic::{
    CpuId,
    cpu_count,
    bootstrap_cpu,
    is_bootstrap_cpu,
    current_cpu,
};

/// A wrapper around `Option<CpuId>` with a forced type alignment of 2 bytes,
/// which guarantees that it compiles down to lock-free native atomic instructions
/// when using it inside of an atomic type like [`AtomicCell`].
#[derive(Copy, Clone)]
#[repr(align(2))]
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
