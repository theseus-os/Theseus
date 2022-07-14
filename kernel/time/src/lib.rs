#![cfg_attr(feature = "hpet", feature(abi_x86_interrupt))]
#![no_std]

#[cfg(feature = "apic")]
mod apic;
#[cfg(feature = "hpet")]
mod hpet;
#[cfg(feature = "pit")]
mod pit;
#[cfg(feature = "rtc")]
mod rtc;
#[cfg(feature = "tsc")]
mod tsc;

use core::{
    mem::transmute,
    sync::atomic::{AtomicUsize, Ordering},
};
use log::warn;

pub use core::time::Duration;

static MONOTONIC_TIMER_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static REALTIME_TIMER_FUNCTION: AtomicUsize = AtomicUsize::new(0);

pub fn init() -> Result<(), ()> {
    init_monotonic_timer()?;
    init_realtime_timer()?;
    Ok(())
}

fn init_monotonic_timer() -> Result<(), ()> {
    #[allow(unused_assignments)]
    let mut addr = 0;

    #[cfg(feature = "tsc")]
    {
        // TODO: Check if TSC reliable?
        addr = if tsc::exists() {
            if tsc::calibrate().is_err() {
                warn!("tsc calibration failed");
                0
            } else {
                tsc::now as usize
            }
        } else {
            0
        };
    }

    #[cfg(feature = "hpet")]
    {
        if addr == 0 && hpet::exists() {
            addr = if hpet::init().is_err() {
                warn!("hpet initialisation failed");
                0
            } else {
                hpet::now as usize
            };
        }
    }

    #[cfg(feature = "apic")]
    {
        if addr == 0 && apic::exists() {
            addr = apic::now as usize;
        }
    }

    #[cfg(feature = "pit")]
    {
        if addr == 0 && pit::exists() {
            addr = pit::now as usize;
        }
    }

    if addr != 0 {
        MONOTONIC_TIMER_FUNCTION.store(addr, Ordering::SeqCst);
        Ok(())
    } else {
        Err(())
    }
}

fn init_realtime_timer() -> Result<(), ()> {
    #[allow(unused_assignments)]
    let mut addr = 0;

    #[cfg(feature = "rtc")]
    {
        if rtc::exists() {
            addr = rtc::now as usize;
        }
    }

    if addr != 0 {
        REALTIME_TIMER_FUNCTION.store(addr, Ordering::SeqCst);
        Ok(())
    } else {
        Err(())
    }
}

pub fn monotonic_time() -> Duration {
    let addr = MONOTONIC_TIMER_FUNCTION.load(Ordering::SeqCst);
    // TODO: Check if address == 0?
    unsafe { transmute::<_, fn() -> Duration>(addr)() }
}

pub fn real_time() -> Duration {
    let addr = REALTIME_TIMER_FUNCTION.load(Ordering::SeqCst);
    // TODO: Check if address == 0?
    unsafe { transmute::<_, fn() -> Duration>(addr)() }
}

// TODO: Do we need really need a trait?

// /// A hardware timer.
// pub trait Timer {
//     // fn exists() -> bool;
//     fn calibrate() -> Result<(), &'static str>;
//     fn value() -> Duration;

//     // TODO: configure, period/frequency, and accuracy
// }

// pub trait ToggleableTimer: Timer {
//     fn enable();
//     fn disable();
//     fn is_enabled() -> bool;

//     fn is_disabled() -> bool {
//         !Self::is_enabled()
//     }

//     fn toggle() {
//         if Self::is_enabled() {
//             Self::disable();
//         } else {
//             Self::enable();
//         }
//     }
// }
