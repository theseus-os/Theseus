//! This crate contains abstractions to interact with hardware timers.

#![no_std]

use core::sync::atomic::{AtomicUsize, Ordering};

pub use core::time::Duration;

static EARLY_SLEEP_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static MONOTONIC_CLOCK_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static REALTIME_CLOCK_FUNCTION: AtomicUsize = AtomicUsize::new(0);

/// Register the clock that can be used to sleep when interrupts are disabled.
///
/// The provided clock will overwrite the previous clock.
///
/// Returns an error if the clock doesn't exist.
pub fn register_early_sleeper<T>() -> Result<(), &'static str>
where
    T: EarlySleeper,
{
    if T::exists() {
        if T::INIT_REQUIRED {
            // FIXME: The clock may be double initialised: once here and once in register_clock.
            T::init()?;
        }
        EARLY_SLEEP_FUNCTION.store(T::sleep as usize, Ordering::SeqCst);
        Ok(())
    } else {
        Err("clock doesn't exist")
    }
}

/// Wait for the given `duration`.
///
/// This function can be used even when interrupts are disabled.
///
/// # Panics
/// This function will panic if called prior to [`register_early_sleeper`].
pub fn early_sleep(duration: Duration) {
    let addr = EARLY_SLEEP_FUNCTION.load(Ordering::SeqCst);
    if addr == 0 {
        panic!("early wait function not set");
    } else {
        let f: fn(Duration) = unsafe { core::mem::transmute(addr) };
        f(duration)
    }
}

/// Register a hardware clock.
///
/// The provided clock will overwrite the previous clock.
///
/// Returns an error if the clock doesn't exist.
pub fn register_clock<T>() -> Result<(), &'static str>
where
    T: Clock,
{
    // TODO: Check if clock is better than current clock?
    if T::exists() {
        T::init()?;
        let func_addr = T::ClockType::func_addr();
        func_addr.store(T::now as usize, Ordering::SeqCst);
        Ok(())
    } else {
        Err("clock doesn't exist")
    }
}

/// Returns the current time.
///
/// For monotonic clocks this is usually the time since boot, and for realtime clocks its the
/// time since 12:00am January 1st 1970 (i.e. Unix time).
///
/// # Panics
/// This function panics if called prior to registering a clock using [`register_clock`].
/// [`register_clock`] must return [`Ok`] and the [`ClockType`] of the registered clock must be the
/// same as `T`.
pub fn now<T>() -> Duration
where
    T: ClockType,
{
    let addr = T::func_addr().load(Ordering::SeqCst);
    if addr == 0 {
        panic!("time function not set");
    } else {
        let f: fn() -> Duration = unsafe { core::mem::transmute(addr) };
        f()
    }
}

/// A hardware clock.
pub trait Clock {
    /// The type of clock (either [`Monotonic`] or [`Realtime`]).
    type ClockType: ClockType;

    /// Whether the clock exists on the system.
    fn exists() -> bool;

    /// Initialise the clock.
    fn init() -> Result<(), &'static str>;

    /// The current time according to the clock.
    ///
    /// For monotonic clocks this is usually the time since boot, and for realtime clocks its the
    /// time since 12:00am January 1st 1970 (i.e. Unix time).
    ///
    /// This function must only be called after [`init`](Clock::init).
    fn now() -> Duration;
}

/// A hardware clock that can sleep without relying on interrupts.
pub trait EarlySleeper: Clock {
    /// Whether the clock must be initialised using [`Clock::init`] prior to
    /// [`sleep`](EarlySleeper::sleep) being called.
    const INIT_REQUIRED: bool;

    /// Sleep for the specified duration.
    fn sleep(duration: Duration) {
        let start = Self::now();
        while Self::now() < start + duration {}
    }
}

/// Either a [`Monotonic`] or [`Realtime`] clock.
///
/// This trait is sealed and so cannot be implemented outside of this crate.
pub trait ClockType: private::Sealed {
    #[doc(hidden)]
    fn func_addr() -> &'static AtomicUsize;
}

pub struct Monotonic;

impl private::Sealed for Monotonic {}

impl ClockType for Monotonic {
    fn func_addr() -> &'static AtomicUsize {
        &MONOTONIC_CLOCK_FUNCTION
    }
}

pub struct Realtime;

impl private::Sealed for Realtime {}

impl ClockType for Realtime {
    fn func_addr() -> &'static AtomicUsize {
        &REALTIME_CLOCK_FUNCTION
    }
}

mod private {
    pub trait Sealed {}
}
