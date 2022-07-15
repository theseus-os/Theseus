#![cfg_attr(feature = "hpet", feature(abi_x86_interrupt))]
#![feature(stmt_expr_attributes)]
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
use log::{info, warn};

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
    let mut addr = None;

    // TODO: Check if TSC reliable?
    #[cfg(feature = "tsc")]
    addr = addr.or_else(|| {
        if tsc::exists() {
            match tsc::init() {
                Ok(_) => {
                    info!("using tsc as monotonic time source");
                    Some(tsc::now as usize)
                }
                Err(e) => {
                    warn!("tsc initialisation failed: {e}");
                    None
                }
            }
        } else {
            None
        }
    });

    #[cfg(feature = "hpet")]
    addr = addr.or_else(|| {
        if hpet::exists() {
            match hpet::init() {
                Ok(_) => {
                    info!("using hpet as monotonic time source");
                    Some(hpet::now as usize)
                }
                Err(e) => {
                    warn!("hpet initialisation failed: {e}");
                    None
                }
            }
        } else {
            None
        }
    });

    #[cfg(feature = "apic")]
    addr = addr.or_else(|| {
        if apic::exists() {
            info!("using apic as monotonic time source");
            Some(apic::now as usize)
        } else {
            None
        }
    });

    #[cfg(feature = "pit")]
    addr = addr.or_else(|| {
        if pit::exists() {
            match pit::init() {
                Ok(_) => {
                    info!("using pit as monotonic time source");
                    Some(pit::now as usize)
                }
                Err(e) => {
                    warn!("pit initialisation failed: {e}");
                    None
                }
            }
        } else {
            None
        }
    });

    if let Some(addr) = addr {
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
            info!("using rtc as realtime time source");
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
