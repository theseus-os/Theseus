use core::sync::atomic::{AtomicUsize, Ordering};

pub use core::time::Duration;

static MONOTONIC_CLOCK_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static REALTIME_CLOCK_FUNCTION: AtomicUsize = AtomicUsize::new(0);

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
    
    /// Sleep for the given `duration`.
    ///
    /// This function must only be called after [`init`](Clock::init). Furthermore, it should only
    /// be used if the `sleep` crate is unavailable e.g. during boot.
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
