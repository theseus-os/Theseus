//! Contains [`PerCpuData`], the data stored on a per-CPU basis in Theseus.
//!
//! Each CPU has its own instance of `PerCpuData`, and each CPU's instance
//! can only be accessed by itself.
//!
//! ## Relationship to `cpu_local`
//! * This crate `per_cpu` directly depends on many other kernel crates,
//!   specifically the ones that define the types needed for each field of [`PerCpuData`].
//! * The `cpu_local` crate is the "top-level" crate that is depended upon
//!   by each of the crates that needs to access per-CPU data.
//!   * This crate also depends on `cpu_local` in order to initialize itself
//!     for each CPU right after that CPU has booted.
//!   

#![no_std]

extern crate alloc; // TODO temp remove this

use cpu::CpuId;
use preemption::{PreemptionCount, PreemptionGuard};
use task::TaskRef;


/// The data stored on a per-CPU basis in Theseus.
///
/// Currently, we do not support additional arbitrary per-CPU states,
/// e.g., dynamically adding or removing them.
///
/// This is not directly accessible, it must be accessed by other crates
/// via the functions in the [`cpu_local`] crate.
#[allow(dead_code)] // These fields are accessed via `cpu_local` functions.
#[repr(C)]
pub struct PerCpuData {
    /// A pointer to the start of this struct in memory, similar to a TLS self pointer.
    /// This has a different initial value for each CPU's data image, of course.
    ///
    /// We use this to allow writes to this entire structure (for initialization),
    /// and also to allow faster access to large fields herein () accelerate accesses to large items
    self_ptr: usize,
    // NOTE: These fields must be kept in sync with `cpu_local::FixedCpuLocal`.
    /*
    cpu_id: CpuId,
    preemption_count: PreemptionCount,
    task_switch_preemption_guard: Option<PreemptionGuard>,
    drop_after_task_switch: Option<TaskRef>,
    */
    test_value: u64,
    test_string: alloc::string::String,
}
impl PerCpuData {
    pub fn new(self_ptr: usize, cpu_id: CpuId) -> Self {
        Self {
            self_ptr,
            /*
            cpu_id,
            preemption_count: PreemptionCount::new(),
            task_switch_preemption_guard: None,
            drop_after_task_switch: None,
            */
            test_value: 0xDEADBEEF,
            test_string: alloc::string::String::from("test_string hello"),
        }
    }
}


/// Initializes the current CPU's `PerCpuData`.
///
/// This must be invoked from (run on) the actual CPU with the given `cpu_id`;
/// the main bootstrap CPU cannot run this for all CPUs itself.
pub fn init(cpu_id: CpuId) -> Result<(), &'static str> {
    cpu_local::init(
        cpu_id.value(),
        |self_ptr| PerCpuData::new(self_ptr, cpu_id),
    )
}
