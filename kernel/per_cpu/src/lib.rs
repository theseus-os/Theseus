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
#![feature(const_refs_to_cell)]

use core::ops::Deref;
use cpu::CpuId;
use cpu_local::PerCpuField;
use task::{DropAfterTaskSwitch, TaskSwitchPreemptionGuard};

/// The data stored on a per-CPU basis in Theseus.
///
/// Currently, we do not support additional arbitrary per-CPU states, e.g.,
/// dynamically adding or removing states, or defining per-CPU states
/// outside this struct.
///
/// This struct is not directly accessible; per-CPU states are accessible
/// by other crates using the functions in the [`cpu_local`] crate.
///
/// ## Required traits
/// Each field in this struct must implement [`cpu_local::PerCpuField`],
/// which in turn mandates that each field have a unique type distinct from the type
/// of every other field.
/// Currently we achieve this with newtype wrappers
#[allow(dead_code)] // These fields are accessed via `cpu_local` functions.
#[repr(C)]
//
// IMPORTANT NOTE:
// * These fields must be kept in sync with `cpu_local::PerCpuField`.
// * The same applies for the `const_assertions` module at the end of this file.
//
pub struct PerCpuData {
    /// A pointer to the start of this struct in memory, similar to a TLS self pointer.
    /// This has a different initial value for each CPU's data image, of course.
    ///
    /// We use this to allow writes to this entire structure (for initialization),
    /// and also to allow faster access to large fields herein, as they don't need to be
    /// loaded in full before accessing a single sub-field. See this for more:
    /// <https://github.com/rust-osdev/x86_64/pull/257#issuecomment-849514649>.
    self_ptr: usize,
    /// The unique ID of this CPU. This is immutable.
    cpu_id: CpuLocalCpuId,
    /// The current preemption count of this CPU, which is used to determine
    /// whether task switching can occur or not.
    preemption_count: u8,
    /// A preemption guard used during task switching to ensure that one task switch
    /// cannot interrupt (preempt) another task switch already in progress.
    task_switch_preemption_guard: TaskSwitchPreemptionGuard,
    /// Data that should be dropped after switching away from a task that has exited.
    /// Currently, this contains the previous task's `TaskRef` that was removed
    /// from its TLS area during the last task switch away from it.
    drop_after_task_switch: DropAfterTaskSwitch,
}

impl PerCpuData {
    /// Defines the initial values of each per-CPU state.
    fn new(self_ptr: usize, cpu_id: cpu::CpuId) -> Self {
        Self {
            self_ptr,
            cpu_id: CpuLocalCpuId(cpu_id),
            preemption_count: 0,
            task_switch_preemption_guard: TaskSwitchPreemptionGuard::new(),
            drop_after_task_switch: DropAfterTaskSwitch::new(),
        }
    }
}


/// An immutable type wrapper for this CPU's unique ID, kept in CPU-local storage.
///
/// Derefs into a [`CpuId`].
pub struct CpuLocalCpuId(CpuId);
impl Deref for CpuLocalCpuId {
    type Target = CpuId;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
// SAFETY: The `CpuLocalCpuId` type corresponds to a field in `PerCpuData`
//         with the offset specified by `PerCpuField::CpuId`.
unsafe impl cpu_local::CpuLocalField for CpuLocalCpuId {
    const FIELD: PerCpuField = PerCpuField::CpuId;
}


/// Initializes the current CPU's `PerCpuData`.
///
/// This must be invoked from (run on) the actual CPU with the given `cpu_id`;
/// the main bootstrap CPU cannot run this for all CPUs itself.
pub fn init(cpu_id: cpu::CpuId) -> Result<(), &'static str> {
    cpu_local::init(
        cpu_id.value(),
        core::mem::size_of::<PerCpuData>(),
        |self_ptr| PerCpuData::new(self_ptr, cpu_id),
    )
}

mod const_assertions {
    use memoffset::offset_of;
    use super::*;

    const _: () = assert!(0 == offset_of!(PerCpuData, self_ptr));
    const _: () = assert!(PerCpuField::CpuId.offset() == offset_of!(PerCpuData, cpu_id));
    const _: () = assert!(PerCpuField::PreemptionCount.offset() == offset_of!(PerCpuData, preemption_count));
    const _: () = assert!(PerCpuField::TaskSwitchPreemptionGuard.offset() == offset_of!(PerCpuData, task_switch_preemption_guard));
    const _: () = assert!(PerCpuField::DropAfterTaskSwitch.offset() == offset_of!(PerCpuData, drop_after_task_switch));
} 
