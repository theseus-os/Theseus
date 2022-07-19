//! This crate provides abstractions for the x86 TSC.

#![no_std]

use core::{
    arch::x86_64::{CpuidResult, __cpuid},
    sync::atomic::{AtomicUsize, Ordering},
};
use log::{info, trace, warn};
use time::Duration;

static TSC_FREQUENCY: AtomicUsize = AtomicUsize::new(0);

pub struct TscClock;

impl time::Clock for TscClock {
    type ClockType = time::Monotonic;

    fn exists() -> bool {
        // TODO: Where should we check whether the TSC is reliable and how should we
        // encode that in the trait?
        // FIXME
        true
    }

    fn init() -> Result<(), &'static str> {
        let tsc_freq = match native_calibrate() {
            Ok(f) => f,
            Err(e) => {
                warn!("native tsc calibration failed: {e}");
                backup_calibrate()?
            }
        };
        TSC_FREQUENCY.store(tsc_freq as usize, Ordering::SeqCst);
        Ok(())
    }

    fn now() -> Duration {
        let nanos = TscTicks::now().to_ns().expect("TSC not calibrated");
        Duration::from_nanos(nanos)
    }
}

/// Attempt to calibrate the TSC using information from the CPUID instruction.
///
/// Returns the frequency of the TSC in hertz.
fn native_calibrate() -> Result<u64, &'static str> {
    // FIXME: This does not work on QEMU. It returns a numerator, denominator, and
    // hertz but they are wrong.
    Err("not implemented")
    // let CpuidResult {
    //     eax: denominator,
    //     ebx: numerator,
    //     ecx: hertz,
    //     ..
    // } = unsafe { __cpuid(0x15) };
    // let (denominator, numerator, mut hertz) = (denominator as u64, numerator
    // as u64, hertz as u64);

    // if numerator == 0 || denominator == 0 {
    //     return Err("numerator and/or denominator is not enumerated");
    // }

    // let CpuidResult { eax: max_level, .. } = unsafe { __cpuid(0x0) };
    // trace!("cpuid max level: {max_level}");

    // // TODO: I think this should only be done for Intel CPUs
    // if hertz == 0 && max_level >= 0x16 {
    //     let CpuidResult { eax: base_mhz, .. } = unsafe { __cpuid(0x16) };
    //     if base_mhz == 0 {
    //         return Err("CPU base clock not enumerated");
    //     } else {
    //         hertz = base_mhz as u64 * 1_000_000;
    //         info!("TSC frequency set to cpu base clock of {base_mhz} MHz");
    //         return Ok(hertz);
    //     }
    // }

    // if hertz == 0 {
    //     return Err("hertz in not enumerated");
    // }

    // let tsc_freq = hertz * (numerator / denominator);

    // Ok(tsc_freq)
}

/// Calibrate the TSC using the PIT timer.
///
/// Returns the frequency of the TSC in hertz.
fn backup_calibrate() -> Result<u64, &'static str> {
    let start = TscTicks::now();
    time::early_sleep(Duration::from_millis(50));
    let end = TscTicks::now();

    let diff = end
        .checked_sub(&start)
        .ok_or("TSC ticks did not act monotonically during calibration")?;
    // Multiplied by 20 because we measured a 50ms interval i.e. 1/20th of a second.
    let tsc_freq = u64::from(diff) * 20;

    info!("TSC frequency calculated by wait: {tsc_freq}");
    Ok(tsc_freq)
}

#[derive(Debug)]
struct TscTicks(u64);

impl TscTicks {
    /// Returns the current number of ticks from the TSC, i.e. `rdtscp`.
    fn now() -> Self {
        let mut val = 0;
        // SAFE: just reading TSC value
        let ticks = unsafe { core::arch::x86_64::__rdtscp(&mut val) };
        TscTicks(ticks)
    }

    /// Converts ticks to nanoseconds.
    ///
    /// Returns an error if the TSC hasn't been calibrated.
    fn to_ns(&self) -> Result<u64, ()> {
        const NANOS_IN_SEC: u32 = 1_000_000_000;

        match TSC_FREQUENCY.load(Ordering::SeqCst) {
            0 => Err(()),
            freq => {
                // NOTE: This is guaranteed to not overflow as u64::max * NANOS_IN_SEC <
                // u128::max.
                let nanos = (self.0 as u128) * NANOS_IN_SEC as u128;
                Ok((nanos / freq as u128) as u64)
            }
        }
    }

    /// Checked subtraction. Computes `self - other`, returning `None` if
    /// underflow occurred.
    fn checked_sub(&self, other: &Self) -> Option<Self> {
        let checked_sub = self.0.checked_sub(other.0);
        checked_sub.map(TscTicks)
    }
}

impl From<TscTicks> for u64 {
    fn from(t: TscTicks) -> Self {
        t.0
    }
}
