//! Contains [`PerCpuData`], the data stored on a per-CPU basis in Theseus.
//!
//! Each CPU has its own instance of `PerCpuData`, and each CPU's instance
//! can only be accessed by itself.
//!
//! ## This `per_cpu` crate vs. the `cpu_local` crate
//! These two crates exist to solve a circular dependency problem:
//! the crate that defines the per-CPU data structure (this `per_cpu` crate)
//! must depend on all the foreign crates that define the types used for
//! each field in the per-CPU data structure.
//! However, those foreign crates also want to *access* these per-CPU states,
//! which would require depending on this `per_cpu` crate.
//! This would create a cyclic dependency, so we break it into two crates.
//!
//! 1. This crate `per_cpu` directly depends on many other kernel crates,
//!    specifically the ones that define the types needed for each field of [`PerCpuData`].
//!    * If you want to add another piece of per-CPU data, you can do that here
//!      by modifying the [`PerCpuData`] struct, and then updating the const definitions
//!      of offsets and other metadata in `cpu_local::FixedCpuLocal`.
//!    * To actually access per-CPU data, do not use this crate,
//!      use `cpu_local::CpuLocal` instead.
//!
//! 2. The `cpu_local` crate is the "top-level" crate that is depended upon
//!    by each of the crates that needs to access per-CPU data.
//!    * `cpu_local` is a mostly standalone crate that does not depend
//!      on any of the specific types from other Theseus crates,
//!      which allows other Theseus crates to depend upon it.
//!    * `cpu_local` effectively decouples the definitions
//!    * This `per_cpu` crate also depends on `cpu_local` in order to initialize itself
//!      for each CPU right after that CPU has booted.
//!

#![no_std]

extern crate alloc; // TODO temp remove this 

use cpu::CpuId;
use preemption::{PreemptionCount, PreemptionGuard};
use task::TaskRef;


struct TestU32(u32);
impl Drop for TestU32 {
    fn drop(&mut self) {
        panic!("Dropping TestU32({})", self.0);
    }
}

/// The data stored on a per-CPU basis in Theseus.
///
/// Currently, we do not support additional arbitrary per-CPU states, e.g.,
/// dynamically adding or removing states, or defining per-CPU states
/// outside this struct.
///
/// This struct is not directly accessible; per-CPU states are accessible
/// by other crates using the functions in the [`cpu_local`] crate.
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
    /// The unique ID of this CPU.
    cpu_id: CpuId,
    /// The current preemption count of this CPU, which is used to determine
    /// whether task switching can occur or not.
    preemption_count: PreemptionCount,
    /// A preemption guard used during task switching to ensure that one task switch
    /// cannot interrupt (preempt) another task switch already in progress.
    // task_switch_preemption_guard: Option<TestU32>, // TODO temp remove this
    task_switch_preemption_guard: Option<PreemptionGuard>,
    /// Data that should be dropped after switching away from a task that has exited.
    /// Currently, this contains the previous task's `TaskRef` that was removed
    /// from its TLS area during the last task switch away from it.
    drop_after_task_switch: Option<TaskRef>,
    test_value: u64,
    test_string: alloc::string::String,
}
impl PerCpuData {
    /// Defines the initial values of each per-CPU state.
    fn new(self_ptr: usize, cpu_id: CpuId) -> Self {
        Self {
            self_ptr,
            cpu_id,
            preemption_count: PreemptionCount::new(),
            task_switch_preemption_guard: None,
            drop_after_task_switch: None,
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
