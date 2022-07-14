use crate::Duration;
use core::{
    arch::x86_64::{CpuidResult, __cpuid},
    sync::atomic::{AtomicUsize, Ordering},
};
use log::info;

static TSC_FREQUENCY: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn exists() -> bool {
    // FIXME
    true
}

pub(crate) fn calibrate() -> Result<(), ()> {
    let tsc_freq = match native_calibrate() {
        Ok(f) => f,
        Err(_) => {
            info!("native tsc calibration failed");
            backup_calibrate()?
        },
    };
    TSC_FREQUENCY.store(tsc_freq as usize, Ordering::SeqCst);
    Ok(())
}

/// Attempt to calibrate the TSC using information from the CPUID instruction.
///
/// Returns the frequency of the TSC in hertz.
fn native_calibrate() -> Result<u64, ()> {
    let CpuidResult { eax: max_level, .. } = unsafe { __cpuid(0x0) };

    if max_level < 0x15 {
        return Err(());
    }

    let CpuidResult {
        eax: denominator,
        ebx: numerator,
        ecx: hertz,
        ..
    } = unsafe { __cpuid(0x15) };
    let (denominator, numerator, mut hertz) = (denominator as u64, numerator as u64, hertz as u64);

    if numerator == 0 || denominator == 0 {
        return Err(());
    }

    if hertz == 0 && max_level >= 0x16 {
        let CpuidResult { eax: base_mhz, .. } = unsafe { __cpuid(0x16) };
        // Yes, according to the Linux source code it's denominator / numerator.
        hertz = base_mhz as u64 * 1000 * (denominator / numerator)
    }

    if hertz == 0 {
        return Err(());
    }

    Ok(hertz * (numerator / denominator))
}

/// Calibrate the TSC using the PIT timer.
///
/// Returns the frequency of the TSC in hertz.
fn backup_calibrate() -> Result<u64, ()> {
    let start = TscTicks::now();
    // wait 10000 us (10 ms)
    // pit_clock::pit_wait(10000)?;
    // FIXME: Wait on pit_clock
    let end = TscTicks::now();

    let diff = end
        .checked_sub(&start)
        .ok_or(())?;
    let tsc_freq = u64::from(diff) * 100; // multiplied by 100 because we measured a 10ms interval

    info!("TSC frequency calculated by PIT is: {}", tsc_freq);

    Ok(tsc_freq)
}

pub(crate) fn now() -> Duration {
    // TODO: Remove unwrap
    let nanos = TscTicks::now().to_ns().unwrap();
    Duration::from_nanos(nanos)
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
    /// Returns `None` if the TSC hasn't been calibrated or if an overflow occured during
    /// the conversion.
    fn to_ns(&self) -> Option<u64> {
        match TSC_FREQUENCY.load(Ordering::SeqCst) {
            0 => None,
            freq => (self.0 as u128)
                .checked_mul(1_000_000_000)
                .map(|tsc| (tsc / freq as u128) as u64)
        }
    }

    /// Checked subtraction. Computes `self - other`, returning `None` if underflow occurred.
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
