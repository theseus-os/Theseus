//! Manages preemption on a per-CPU core basis.
//! 
//! Supports enabling and disabling preemption for the purpose of 
//! safe task state management, e.g., through preemption-safe locks.

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

use core::sync::atomic::{AtomicU8, Ordering};
use cpu::CpuId;

/// Theseus previously used a `u8` to hold each CPU core's ID
/// so the maximum number of cores it supports is 256.
///
/// In the future, we will use CPU-local storage for each CPU's preemption count,
/// so we won't need to use a system-wide primitive array here.
const MAX_CPU_CORES: usize = 256;

#[allow(clippy::declare_interior_mutable_const)]
const ATOMIC_U8_ZERO: AtomicU8 = AtomicU8::new(0);

/// The per-core preemption count, indexed by a CPU core's APIC ID.
///
/// If a CPU's preemption count is `0`, preemption is enabled.
/// If a CPU's preemption count is greater than `0`, preemption is disabled.
static PREEMPTION_COUNT: [AtomicU8; MAX_CPU_CORES] = [ATOMIC_U8_ZERO; MAX_CPU_CORES];

pub struct PreemptionCount(AtomicU8);
impl PreemptionCount {
    pub const fn new() -> Self {
        PreemptionCount(ATOMIC_U8_ZERO)
    }
}

/// Prevents preemption (preemptive task switching) from occurring
/// until the returned guard object is dropped.
///
/// If this results in a transition from preemption being enabled to being disabled
/// on this CPU, the local timer interrupt used for preemptive task switches
/// will also be disabled until preemption is re-enabled.
pub fn hold_preemption() -> PreemptionGuard {
    hold_preemption_internal::<true>()
}

/// Prevents preemption (preemptive task switching) from occurring
/// until the returned guard object is dropped.
///
/// ## Usage notes
/// Callers should use [`hold_preemption()`] instead of this function.
/// This is a "lightweight" version of that function that does not
/// disable this CPU's local timer interrupt used for preemptive task switches.
/// Thus, it is only for select contexts where we are very briefly
/// disabling preemption.
#[doc(hidden)]
pub fn hold_preemption_no_timer_disable() -> PreemptionGuard {
    hold_preemption_internal::<false>()
}

/// The internal routine for disabling preemption.
///
/// If the const argument `DISABLE_TIMER` is `true`, the local timer interrupt
/// will be disabled upon a transition from preemption being enabled to being disabled.
fn hold_preemption_internal<const DISABLE_TIMER: bool>() -> PreemptionGuard {
    let cpu_id = cpu::current_cpu();
    let prev_val = PREEMPTION_COUNT[cpu_id.value() as usize].fetch_add(1, Ordering::Relaxed);
    // If the previous counter value was 0, that indicates we are transitioning
    // from preemption being enabled to disabled on this CPU.
    let preemption_was_enabled = prev_val == 0;
    // Create a guard here immediately after incrementing the counter,
    // in order to guarantee that a failure below will drop it and decrement the counter.
    let guard = PreemptionGuard {
        cpu_id,
        preemption_was_enabled,
    };

    if DISABLE_TIMER && preemption_was_enabled {
        // log::trace!(" CPU {}:   disabling local timer interrupt", cpu_id);
        
        // When transitioning from preemption being enabled to disabled,
        // we must disable the local APIC timer used for preemptive task switching.
        #[cfg(target_arch = "x86_64")]
        apic::get_my_apic()
            .expect("BUG: hold_preemption() couldn't get local APIC")
            .write()
            .enable_lvt_timer(false);
    } else if prev_val == u8::MAX {
        // Overflow occurred and the counter value wrapped around, which is a bug.
        panic!("BUG: Overflow occurred in the preemption counter for CPU {}", cpu_id);
    }
    guard
}


/// A guard type that ensures preemption is disabled as long as it is held.
///
/// Call [`hold_preemption()`] to obtain a `PreemptionGuard`.
///
/// Preemption *may* be re-enabled when this guard is dropped,
/// but not necessarily so, because other previous functions 
/// in the call stack may have already acquired a `PreemptionGuard`.
///
/// This type does not implement `Send` because it is invalid
/// to move it across a "thread" boundary (into a different task).
pub struct PreemptionGuard {
    /// The ID of the CPU on which preemption was held.
    ///
    /// This is mostly used for strict sanity checks to ensure that
    /// a guard isn't created on one CPU and then dropped on a different CPU.
    cpu_id: CpuId,
    /// Whether preemption was enabled when this guard was created.
    preemption_was_enabled: bool,
}

// TODO FIXME: currently we transfer a `PreemptionGuard` between tasks
//             during a task switch (right before and right after the context switch).
//             Thus, this type needs to impl `Send`, even though it doesn't make sense
//             to move it across a thread boundary in the general case.
// // Similar guard types in Rust `std` are not `Send`.
// impl !Send for PreemptionGuard { }

impl PreemptionGuard {
    /// Returns whether preemption was originally enabled when this guard was created.
    ///
    /// # Return
    /// * `true`: indicates that the caller function/task holding this guard
    ///    was the one that caused the transition from preemption
    ///    being enabled on this CPU to being disabled.
    /// * `false`: indicates that preemption was already disabled
    ///    and that no transition occurred when the caller function/task
    ///    obtained this guard.
    pub fn preemption_was_enabled(&self) -> bool {
        self.preemption_was_enabled
    }

    /// Returns the ID of the CPU on which this guard was created.
    pub fn cpu_id(&self) -> CpuId {
        self.cpu_id
    }
}

impl Drop for PreemptionGuard {
    fn drop(&mut self) {
        let cpu_id = cpu::current_cpu();
        assert!(
            self.cpu_id == cpu_id,
            "PreemptionGuard::drop(): BUG: CPU IDs did not match! \
            This indicates an unexpected task migration across CPUs."
        );

        let prev_val = PREEMPTION_COUNT[cpu_id.value() as usize].fetch_sub(1, Ordering::Relaxed);
        if prev_val == 1 {
            // log::trace!("CPU {}: re-enabling local timer interrupt", cpu_id);

            // If the previous counter value was 1, that means the current value is 1,
            // which indicates we are transitioning from preemption disabled to enabled on this CPU.
            // Thus, we re-enable the local APIC timer used for preemptive task switching.
            #[cfg(target_arch = "x86_64")]
            apic::get_my_apic()
                .expect("BUG: PreemptionGuard::drop() couldn't get local APIC")
                .write()
                .enable_lvt_timer(true);
        } else if prev_val == 0 {
            // Underflow occurred and the counter value wrapped around, which is a bug.
            panic!("BUG: Underflow occurred in the preemption counter for CPU {}", cpu_id);
        }
    }
}


/// Returns `true` if preemption is currently enabled on this CPU.
///
/// Note that this value can't be used as a lock indicator or property,
/// as it is just a snapshot that offers no guarantee that preemption
/// will continue to be enabled or disabled immediately after returning.
pub fn preemption_enabled() -> bool {
    let cpu_id = cpu::current_cpu();
    let val = PREEMPTION_COUNT[cpu_id.value() as usize].load(Ordering::Relaxed);
    val == 0
}
