#![no_std]

#[macro_use] extern crate log;
extern crate pit_clock;

use core::sync::atomic::{AtomicUsize, Ordering};


#[derive(Debug)]
pub struct TscTicks(u64);

impl TscTicks {
    /// Converts ticks to nanoseconds. 
    /// Returns None if the TSC tick frequency is unavailable.
    pub fn to_ns(&self) -> Option<u64> {
         get_tsc_frequency().ok().map(|freq| {
            (self.0 * 1000000000) / freq 
         })
    }

    /// Checked subtraction. Computes `self - other`, 
    /// returning `None` if underflow occurred.
    pub fn sub(&self, other: &TscTicks) -> Option<TscTicks> {
        let checked_sub = self.0.checked_sub(other.0);
        checked_sub.map( |tt| TscTicks(tt) )
    }
    
    /// Checked addition. Computes `self + other`, 
    /// returning `None` if overflow occurred.
    pub fn add(&self, other: &TscTicks) -> Option<TscTicks> {
        let checked_add = self.0.checked_add(other.0);
        checked_add.map( |tt| TscTicks(tt) )
    }

    /// Get the inner value, the number of ticks.
    pub fn into(self) -> u64 {
        self.0
    }
}



/// Returns the current number of ticks from the TSC, i.e., `rdtscp`. 
pub fn tsc_ticks() -> TscTicks {
    let mut val = 0;
    // SAFE: just reading TSC value
    #[cfg(any(target_arch="x86", target_arch="x86_64"))]
    let ticks = unsafe { core::arch::x86_64::__rdtscp(&mut val) };
    #[cfg(any(target_arch="aarch64"))]
    let ticks = 0; 
    TscTicks(ticks)
}

/// Returns the frequency of the TSC for the system, 
/// currently measured using the PIT clock for calibration.
pub fn get_tsc_frequency() -> Result<u64, &'static str> {
    // this is a soft state, so it's not a form of state spill
    static TSC_FREQUENCY: AtomicUsize = AtomicUsize::new(0);

    let freq = TSC_FREQUENCY.load(Ordering::SeqCst) as u64;
    
    if freq != 0 {
        Ok(freq)
    }
    else {
        // a freq of zero means it hasn't yet been initialized.
        let start = tsc_ticks();
        // wait 10000 us (10 ms)
        try!(pit_clock::pit_wait(10000));
        let end = tsc_ticks(); 

        let diff = try!(end.sub(&start).ok_or("couldn't subtract end-start TSC tick values"));
        let tsc_freq = diff.into() * 100; // multiplied by 100 because we measured a 10ms interval
        info!("TSC frequency calculated by PIT is: {}", tsc_freq);
        TSC_FREQUENCY.store(tsc_freq as usize, Ordering::Release);
        Ok(tsc_freq)
    }
}
