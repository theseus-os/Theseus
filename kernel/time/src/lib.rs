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

static MONOTONIC_CLOCK_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static REALTIME_CLOCK_FUNCTION: AtomicUsize = AtomicUsize::new(0);

/// Discover and initialise best available montonic and realtime clocks.
///
/// This function should be called after parsing ACPI tables.
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
            if tsc::calibrate().is_ok() {
                tsc::now as usize
            } else {
                warn!("tsc calibration failed");
                0
            }
        } else {
            0
        };
    }

    #[cfg(feature = "hpet")]
    {
        if addr == 0 && hpet::exists() {
            addr = if hpet::init().is_ok() {
                hpet::now as usize
            } else {
                warn!("hpet initialisation failed");
                0
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
        MONOTONIC_CLOCK_FUNCTION.store(addr, Ordering::SeqCst);
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
        REALTIME_CLOCK_FUNCTION.store(addr, Ordering::SeqCst);
        Ok(())
    } else {
        Err(())
    }
}

pub fn now<T>() -> Duration
where
    T: ClockType,
{
    let addr = <T as ClockType>::func_addr();
    let f = unsafe { transmute::<_, fn() -> Duration>(addr) };
    f()
}

/// Either a [`Monotonic`] or [`Realtime`] clock.
///
/// This trait is sealed and so cannot be implemented outside of this crate.
pub trait ClockType: private::Sealed {
    #[doc(hidden)]
    fn func_addr() -> usize;
}

pub struct Monotonic;

impl private::Sealed for Monotonic {}

impl ClockType for Monotonic {
    fn func_addr() -> usize {
        MONOTONIC_CLOCK_FUNCTION.load(Ordering::SeqCst)
    }
}

pub struct Realtime;

impl private::Sealed for Realtime {}

impl ClockType for Realtime {
    fn func_addr() -> usize {
        REALTIME_CLOCK_FUNCTION.load(Ordering::SeqCst)
    }
}

mod private {
    pub trait Sealed {}
}
