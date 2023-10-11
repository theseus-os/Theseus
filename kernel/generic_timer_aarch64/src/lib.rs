//! Support for aarch64's generic timer and system counter.
//!
//! Docs: <https://developer.arm.com/documentation/102379/latest/>

#![no_std]
#![feature(negative_impls)]

// This crate is only relevant on aarch64 systems,
// but we use a cfg gate here to allow it to be included in x86 builds
// because the build system currently builds _all_ crates for x86.
#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

#[cfg(target_arch = "aarch64")]
mod aarch64 {

use cortex_a::registers::*;
use log::*;
use time::{Monotonic, ClockSource, Instant, Period, register_clock_source};
use tock_registers::interfaces::Writeable;
use tock_registers::interfaces::Readable;


/// Initializes the aarch64 generic system timer
/// and registers it as a monotonic [`ClockSource`].
///
/// This only needs to be invoked once, system-wide.
/// However, each CPU will need to enable their own timer interrupt separately,
/// as this function itself does not enable the timer interrupt.
pub fn init() {
    let period = Period::new(timer_period_femtoseconds());
    register_clock_source::<PhysicalSystemCounter>(period);
}

/// A ClockSource for the time crate, implemented using
/// the System Counter of the Generic Arm Timer. The
/// period of this timer is computed in `init` above.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PhysicalSystemCounter;
impl !Send for PhysicalSystemCounter { }
impl PhysicalSystemCounter {
    /// An instant (no-op) function that returns the current CPU's system counter. 
    pub fn get() -> Self {
        Self
    }
}
impl ClockSource for PhysicalSystemCounter {
    type ClockType = Monotonic;

    fn now() -> Instant {
        Instant::new(CNTPCT_EL0.get())
    }
}

/// Returns the period in femtoseconds of the generic system timer.
///
/// This reads the `CNTFRQ_EL0` system register.
pub fn timer_period_femtoseconds() -> u64 {
    let counter_freq_hz = CNTFRQ_EL0.get();
    let fs_in_one_sec = 1_000_000_000_000_000;
    fs_in_one_sec / counter_freq_hz
}

/// Sets the current CPU's system timer interrupt to fire after `ticks_to_elapse` from now.
pub fn set_next_timer_interrupt(ticks_to_elapse: u64) {
    enable_timer_interrupt(false);
    CNTP_TVAL_EL0.set(ticks_to_elapse);
    enable_timer_interrupt(true);
}

/// Enables/disables the generic system timer interrupt on the current CPU.
///
/// This writes the `CNTP_CTL_EL0` system register.
pub fn enable_timer_interrupt(enable: bool) {
    // Unmask the interrupt (to enable it), and enable the timer.
    CNTP_CTL_EL0.write(
          CNTP_CTL_EL0::IMASK.val(0)
        + CNTP_CTL_EL0::ENABLE.val(match enable {
            true => 1,
            false => 0,
        })
    );

    if false {
        info!("timer enabled: {:?}", CNTP_CTL_EL0.read(CNTP_CTL_EL0::ENABLE));
        info!("timer IMASK: {:?}",   CNTP_CTL_EL0.read(CNTP_CTL_EL0::IMASK));
        info!("timer status: {:?}",  CNTP_CTL_EL0.read(CNTP_CTL_EL0::ISTATUS));
    }
}

}
