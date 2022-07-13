#![no_std]

use core::sync::atomic::{AtomicUsize, Ordering};
use log::info;
use timer::Duration;

static TSC_FREQUENCY: AtomicUsize = AtomicUsize::new(0);

pub struct TscTimer;

impl timer::Timer for TscTimer {
    fn calibrate() -> Result<(), &'static str> {
        let start = tsc_ticks();
        // wait 10000 us (10 ms)
        pit_clock::pit_wait(10000)?;
        let end = tsc_ticks();

        let diff = end
            .checked_sub(&start)
            .ok_or("couldn't subtract end-start TSC tick values")?;
        let tsc_freq = u64::from(diff) * 100; // multiplied by 100 because we measured a 10ms interval
        info!("TSC frequency calculated by PIT is: {}", tsc_freq);
        TSC_FREQUENCY.store(tsc_freq as usize, Ordering::SeqCst);
        Ok(())
    }

    fn value() -> Duration {
        // TODO: Remove unwrap
        let nanos = tsc_ticks().to_ns().unwrap();
        Duration::from_nanos(nanos)
    }
}

#[derive(Debug)]
struct TscTicks(u64);

impl TscTicks {
    /// Converts ticks to nanoseconds.
    ///
    /// Returns `None` if the TSC hasn't been calibrated or if an overflow occured during
    /// the conversion.
    pub fn to_ns(&self) -> Option<u64> {
        match TSC_FREQUENCY.load(Ordering::SeqCst) {
            0 => None,
            freq => {
                (self.0 as u128).checked_mul(1_000_000_000)
                .map(|tsc| tsc / freq as u128).map(|tsc| tsc as u64)
            }
        }
    }

    /// Checked subtraction. Computes `self - other`, returning `None` if underflow occurred.
    pub fn checked_sub(&self, other: &TscTicks) -> Option<TscTicks> {
        let checked_sub = self.0.checked_sub(other.0);
        checked_sub.map(TscTicks)
    }
}

impl From<TscTicks> for u64 {
    fn from(t: TscTicks) -> Self {
        t.0
    }
}

/// Returns the current number of ticks from the TSC, i.e., `rdtscp`.
fn tsc_ticks() -> TscTicks {
    let mut val = 0;
    // SAFE: just reading TSC value
    let ticks = unsafe { core::arch::x86_64::__rdtscp(&mut val) };
    TscTicks(ticks)
}
