//! This crate contains abstractions to interact with hardware timers.

#![no_std]

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub use core::time::Duration;

static EARLY_SLEEP_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static EARLY_SLEEPER_FREQUENCY: AtomicU64 = AtomicU64::new(0);

static MONOTONIC_NOW_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static INSTANT_TO_DURATION_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static DURATION_TO_INSTANT_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static MONOTONIC_FREQUENCY: AtomicU64 = AtomicU64::new(0);

static REALTIME_NOW_FUNCTION: AtomicUsize = AtomicUsize::new(0);
static REALTIME_FREQUENCY: AtomicU64 = AtomicU64::new(0);

fn duration_to_instant(duration: Duration) -> Instant {
    let func_addr = DURATION_TO_INSTANT_FUNCTION.load(Ordering::SeqCst);
    let f: fn(Duration) -> Instant = unsafe { core::mem::transmute(func_addr) };
    f(duration)
}

fn instant_to_duration(instant: Instant) -> Duration {
    let func_addr = INSTANT_TO_DURATION_FUNCTION.load(Ordering::SeqCst);
    let f: fn(Instant) -> Duration = unsafe { core::mem::transmute(func_addr) };
    f(instant)
}

/// A measurement of a monotonically nondecreasing clock.
///
/// The inner value usually represents the internal counter value but the type
/// is intentionally opaque and only useful with [`Duration`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Instant {
    counter: u64,
}

impl Instant {
    /// Returns the amout of time elapsed from another instant to this one, or
    /// zero duration if that instant is later than this one.
    pub fn duration_since(&self, earlier: Self) -> Duration {
        let instant = Instant {
            counter: match self.counter.checked_sub(earlier.counter) {
                Some(value) => value,
                None => return Duration::ZERO,
            },
        };
        instant_to_duration(instant)
    }
}

impl core::ops::Add<Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        let instant = duration_to_instant(rhs);
        Self {
            counter: self.counter + instant.counter,
        }
    }
}

impl core::ops::Sub<Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        let instant = duration_to_instant(rhs);
        Self {
            counter: self.counter - instant.counter,
        }
    }
}

impl core::ops::Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.duration_since(rhs)
    }
}

/// Register the clock source that can be used to sleep when interrupts are
/// disabled.
///
/// The current early sleeper will only be overwritten by `T` if `frequency` is
/// larger than the frequency of the current early sleeper.
///
/// Returns whether the early sleeper was overwritten.
///
/// # Errors
///
/// Returns an error if [`INIT_REQUIRED`](EarlySleeper::INIT_REQUIRED) is `true`
/// and [`ClockSource::init`] returned an error.
pub fn register_early_sleeper<T>(frequency: u64) -> Result<bool, &'static str>
where
    T: EarlySleeper,
{
    let old_frequency = EARLY_SLEEPER_FREQUENCY.load(Ordering::SeqCst);
    if frequency > old_frequency {
        if T::INIT_REQUIRED {
            // FIXME: The source may be double initialised: once here and once in
            // register_clock_source. This doesn't currently cause issues but needs to be
            // fixed. This will probably be fixed if we separate interrupts from clocks in a
            // future PR.
            T::init()?;
        }

        EARLY_SLEEP_FUNCTION.store(T::sleep as usize, Ordering::SeqCst);
        EARLY_SLEEPER_FREQUENCY.store(frequency, Ordering::SeqCst);

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Wait for the given `duration`.
///
/// This function can be used even when interrupts are disabled.
///
/// # Panics
///
/// This function will panic if called prior to [`register_early_sleeper`].
pub fn early_sleep(duration: Duration) {
    let addr = EARLY_SLEEP_FUNCTION.load(Ordering::SeqCst);
    if addr == 0 {
        panic!("early sleep function not set");
    } else {
        let f: fn(Duration) = unsafe { core::mem::transmute(addr) };
        f(duration)
    }
}

/// Register a clock source.
///
/// The provided source will overwrite the previous source only if `frequency`
/// is larger than that of the previous source.
///
/// Returns whether the previous source was overwritten.
///
/// # Errors
///
/// Returns an error if the source doesn't exist or if
/// [`init`](ClockSource::init) returns an error.
pub fn register_clock_source<T>(frequency: u64) -> Result<bool, &'static str>
where
    T: ClockSource,
{
    let old_frequency = T::ClockType::frequency_atomic().load(Ordering::SeqCst);
    if frequency > old_frequency {
        T::init()?;

        let now_addr = T::ClockType::now_addr();
        now_addr.store(T::now as usize, Ordering::SeqCst);

        T::ClockType::store_instant_to_duration_func(T::unit_to_duration as usize);
        T::ClockType::store_duration_to_instant_func(T::duration_to_unit as usize);

        T::ClockType::frequency_atomic().store(frequency, Ordering::SeqCst);

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Returns the current time.
///
/// Monotonic clocks return an [`Instant`] whereas realtime clocks return a
/// [`Duration`] signifying the time since 12:00am January 1st 1970 (i.e. Unix
/// time).
///
/// # Panics
///
/// This function will panic if called prior to registering a clock using
/// [`register_clock_source`]. [`register_clock_source`] must return [`Ok`] and
/// the [`ClockType`] of the registered clock must be the same as `T`.
pub fn now<T>() -> T::Unit
where
    T: ClockType,
{
    let addr = T::now_addr().load(Ordering::SeqCst);
    if addr == 0 {
        panic!("time function not set");
    } else {
        let f: fn() -> T::Unit = unsafe { core::mem::transmute(addr) };
        f()
    }
}

/// A clock source.
pub trait ClockSource {
    /// The type of clock (either [`Monotonic`] or [`Realtime`]).
    type ClockType: ClockType;

    /// Whether the clock source exists on the system.
    fn exists() -> bool;

    /// Initialise the clock source.
    fn init() -> Result<(), &'static str>;

    /// The current time according to the clock.
    ///
    /// For monotonic clocks this is usually the time since boot, and for
    /// realtime clocks it's the time since 12:00am January 1st 1970 (i.e.
    /// Unix time).
    ///
    /// This function must only be called after [`init`](ClockSource::init).
    fn now() -> <Self::ClockType as ClockType>::Unit;

    /// Converts a [`ClockType::Unit`] into a [`Duration`].
    ///
    /// Monotonic clocks should just return the [`ClockType::Unit`].
    fn unit_to_duration(unit: <Self::ClockType as ClockType>::Unit) -> Duration;

    /// Converts a [`Duration`] into a [`ClockType::Unit`].
    ///
    /// Monotonic clocks should just return the [`Duration`].
    fn duration_to_unit(duration: Duration) -> <Self::ClockType as ClockType>::Unit;
}

/// A hardware clock that can sleep without relying on interrupts.
pub trait EarlySleeper: ClockSource<ClockType = Monotonic> {
    /// Whether the clock must be initialised using [`ClockSource::init`] prior
    /// to [`sleep`](EarlySleeper::sleep) being called.
    const INIT_REQUIRED: bool;

    /// Sleep for the specified duration.
    ///
    /// # Note to Implementors
    ///
    /// The default implementation of this function uses [`ClockSource::now`] -
    /// it can only be used if [`ClockSource::now`] doesn't rely on
    /// interrupts.
    fn sleep(duration: Duration) {
        let start = Self::now();
        while Self::now() < start + duration {}
    }
}

/// Either a [`Monotonic`] or [`Realtime`] clock.
///
/// This trait is sealed and so cannot be implemented outside of this crate.
pub trait ClockType: private::Sealed {
    /// The type returned by the [`now`] function.
    type Unit;

    #[doc(hidden)]
    fn now_addr() -> &'static AtomicUsize;
    #[doc(hidden)]
    fn frequency_atomic() -> &'static AtomicU64;

    #[doc(hidden)]
    fn store_instant_to_duration_func(addr: usize);
    #[doc(hidden)]
    fn store_duration_to_instant_func(addr: usize);
}

pub struct Monotonic;

impl private::Sealed for Monotonic {}

impl ClockType for Monotonic {
    type Unit = Instant;

    fn now_addr() -> &'static AtomicUsize {
        &MONOTONIC_NOW_FUNCTION
    }

    fn frequency_atomic() -> &'static AtomicU64 {
        &MONOTONIC_FREQUENCY
    }

    fn store_instant_to_duration_func(addr: usize) {
        INSTANT_TO_DURATION_FUNCTION.store(addr, Ordering::SeqCst);
    }

    fn store_duration_to_instant_func(addr: usize) {
        DURATION_TO_INSTANT_FUNCTION.store(addr, Ordering::SeqCst);
    }
}

pub struct Realtime;

impl private::Sealed for Realtime {}

impl ClockType for Realtime {
    type Unit = Duration;

    fn now_addr() -> &'static AtomicUsize {
        &REALTIME_NOW_FUNCTION
    }

    fn frequency_atomic() -> &'static AtomicU64 {
        &REALTIME_FREQUENCY
    }

    fn store_instant_to_duration_func(_: usize) {
        // We intentionally don't use instant_to_duration for realtime clocks.
    }

    fn store_duration_to_instant_func(_: usize) {
        // We intentionally don't use duration_to_instant for realtime clocks.
    }
}

mod private {
    /// This trait is a supertrait of [`Clocktype`](super::ClockType).
    ///
    /// Since it's in a private module, it can't be implemented by types outside
    /// this crate and thus neither can [`Clocktype`](super::ClockType).
    pub trait Sealed {}
}
