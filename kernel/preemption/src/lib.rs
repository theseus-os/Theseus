//! Manages preemption on a per-CPU core basis.
//! 
//! Supports enabling and disabling preemption for the purpose of 
//! safe task state management, e.g., through preemption-safe locks.

#![no_std]
#![feature(negative_impls)]

extern crate alloc;

use core::sync::atomic::{AtomicU8, Ordering};

/// Theseus uses a `u8` to hold each CPU core's ID (apic ID),
/// so the maximum number of cores it supports is `u8::MAX` (256).
const MAX_CPU_CORES: usize = u8::MAX as usize;

#[allow(clippy::declare_interior_mutable_const)]
const ATOMIC_U8_ZERO: AtomicU8 = AtomicU8::new(0);

/// The per-core preemption count, indexed by a CPU core's APIC ID.
/// 
/// If a CPU's count is `0`, preemption is enabled.
/// If a CPU's count is greater than `0`, preemption is disabled.
static PREEMPTION_COUNT: [AtomicU8; MAX_CPU_CORES] = [ATOMIC_U8_ZERO; MAX_CPU_CORES];


/// Prevents preemption (preemptive task switching) from occurring
/// until the returned guard object is dropped.
pub fn hold_preemption() -> PreemptionGuard {
    let cpu_id = apic::current_cpu();
    let prev_val = PREEMPTION_COUNT[cpu_id as usize].fetch_add(1, Ordering::Relaxed);
    // If the previous counter value was 0, that indicates we are transitioning
    // from preemption being enabled to disabled on this CPU.
    let preemption_was_enabled = prev_val == 0;
    // Create a guard here immediately after incrementing the counter,
    // in order to guarantee that a failure below will drop it and decrement the counter.
    let guard = PreemptionGuard {
        cpu_id,
        preemption_was_enabled,
    };

    if preemption_was_enabled {
        // log::trace!(" CPU {}:   disabling preemption", cpu_id);
        
        // When transitioning from preemption being enabled to disabled,
        // we must disable the local APIC timer used for preemptive task switching.
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
    cpu_id: u8,
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
    pub fn cpu_id(&self) -> u8 {
        self.cpu_id
    }
}

impl Drop for PreemptionGuard {
    fn drop(&mut self) {
        let cpu_id = apic::current_cpu();
        assert!(
            self.cpu_id == cpu_id,
            "PreemptionGuard::drop(): BUG: CPU IDs did not match! \
            This indicates an unexpected task migration across CPUs."
        );

        let prev_val = PREEMPTION_COUNT[cpu_id as usize].fetch_sub(1, Ordering::Relaxed);
        if prev_val == 1 {
            // log::trace!("CPU {}: re-enabling preemption", cpu_id);

            // If the previous counter value was 1, that means the current value is 1,
            // which indicates we are transitioning from preemption disabled to enabled on this CPU.
            // Thus, we re-enable the local APIC timer used for preemptive task switching.
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
    let cpu_id = apic::current_cpu();
    let val = PREEMPTION_COUNT[cpu_id as usize].load(Ordering::Relaxed);
    val == 0
}
