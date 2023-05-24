//! Provides CPU-local storage (per-CPU variables) and preemption management.
//!
//! CPU-local variables cannot be used until after a given CPU has been initialized,
//! i.e., its Local APIC (on x86_64) has been discovered and properly configured.
//! Currently, the [`init()`] routine in this crate should be invoked by
//! another init routine from the `per_cpu` crate.
//!
//! Note that Rust offers the `#[thread_local]` attribute for thread-local storage (TLS),
//! but there is no equivalent for CPU-local storage.
//! On x86_64, TLS areas use the `fs` segment register for the TLS base,
//! and this crate uses the `gs` segment register for the CPU-local base.

#![no_std]
#![feature(negative_impls, thread_local)]

extern crate alloc;

use core::sync::atomic::{AtomicU8, Ordering};
use cpu::CpuId;

#[cls::cpu_local]
static PREEMPTION_COUNT: AtomicU8 = AtomicU8::new(0);

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
    // FIXME: Is a relaxed ordering sufficient?
    let prev_val = PREEMPTION_COUNT.fetch_add(1, Ordering::Relaxed);
    // If the previous counter value was 0, that indicates we are transitioning
    // from preemption being enabled to disabled on this CPU.
    let preemption_was_enabled = prev_val == 0;
    // Create a real guard here immediately after incrementing the counter,
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
/// More specifically, it is invalid to move a `PreemptionGuard` across
/// CPUs; this error condition is checked for when dropping it.
pub struct PreemptionGuard {
    /// The ID of the CPU on which preemption was held.
    ///
    /// This is mostly used for strict sanity checks to ensure that
    /// a guard isn't created on one CPU and then dropped on a different CPU.
    cpu_id: CpuId,
    /// Whether preemption was enabled when this guard was created.
    preemption_was_enabled: bool,
}
impl !Send for PreemptionGuard { }

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

        // FIXME: Is a relaxed ordering sufficient?
        let prev_val = PREEMPTION_COUNT.fetch_sub(1, Ordering::Relaxed);
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
    PREEMPTION_COUNT.load(Ordering::Relaxed) == 0
}
