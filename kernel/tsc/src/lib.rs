#![no_std]

use log::info;
use time::{Instant, Period};

pub struct Tsc;

impl time::ClockSource for Tsc {
    type ClockType = time::Monotonic;

    fn now() -> Instant {
        Instant::new(tsc_value())
    }
}

/// Returns the frequency of the TSC for the system, currently measured using
/// the PIT clock for calibration.
pub fn get_tsc_period() -> Option<Period> {
    const PIT_WAIT_MICROSECONDS: u32 = 10_000;
    const PIT_WAIT_FEMTOSECONDS: u64 = PIT_WAIT_MICROSECONDS as u64 * 1_000_000_000;

    let start = tsc_value();
    pit_clock_basic::pit_wait(PIT_WAIT_MICROSECONDS).ok()?;
    let end = tsc_value();

    let increments = end.checked_sub(start)?;
    let tsc_period = Period::new(PIT_WAIT_FEMTOSECONDS / increments);

    info!("TSC period calculated by PIT is: {tsc_period}");

    Some(tsc_period)
}

#[doc(hidden)]
pub fn tsc_value() -> u64 {
    let mut _aux = 0;
    // SAFETY: Reading the TSC value is a platform-specific intrinsic that has no
    // side effects or dangerous behavior, and is supported on all modern x86_64
    // hardware.
    unsafe { core::arch::x86_64::__rdtscp(&mut _aux) }
}
