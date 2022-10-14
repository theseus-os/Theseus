//! This crate contains abstractions to interact with hardware clocks.

#![no_std]

mod dummy;

pub use core::time::Duration;

use core::sync::atomic::{AtomicU64, Ordering};
use crossbeam_utils::atomic::AtomicCell;

static EARLY_SLEEP_FUNCTION: AtomicCell<fn(Duration)> = AtomicCell::new(dummy::early_sleep);
static EARLY_SLEEPER_FREQUENCY: AtomicU64 = AtomicU64::new(0);

static MONOTONIC_NOW_FUNCTION: AtomicCell<fn() -> Instant> = AtomicCell::new(dummy::monotonic_now);
static INSTANT_TO_DURATION_FUNCTION: AtomicCell<fn(Instant) -> Duration> =
    AtomicCell::new(dummy::instant_to_duration);
static DURATION_TO_INSTANT_FUNCTION: AtomicCell<fn(Duration) -> Instant> =
    AtomicCell::new(dummy::duration_to_instant);
static MONOTONIC_FREQUENCY: AtomicU64 = AtomicU64::new(0);

static WALL_TIME_NOW_FUNCTION: AtomicCell<fn() -> Duration> = AtomicCell::new(dummy::wall_time_now);
static WALL_TIME_FREQUENCY: AtomicU64 = AtomicU64::new(0);

fn duration_to_instant(duration: Duration) -> Instant {
    let f = DURATION_TO_INSTANT_FUNCTION.load();
    f(duration)
}

fn instant_to_duration(instant: Instant) -> Duration {
    let f = INSTANT_TO_DURATION_FUNCTION.load();
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
    const ZERO: Self = Self { counter: 0 };

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
            counter: self
                .counter
                .checked_add(instant.counter)
                .expect("overflow when adding duration to instant"),
        }
    }
}

impl core::ops::Sub<Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        let instant = duration_to_instant(rhs);
        Self {
            counter: self
                .counter
                .checked_sub(instant.counter)
                .expect("overflow when subtracting duration from instant"),
        }
    }
}

impl core::ops::Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.duration_since(rhs)
    }
}

/// A clock frequency.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Frequency(u64);

impl Frequency {
    /// Creates a new frequency with the specified hertz.
    pub fn new(frequency: u64) -> Self {
        Self(frequency)
    }
}

impl From<Frequency> for u64 {
    /// Returns the frequency in hertz.
    fn from(f: Frequency) -> Self {
        f.0
    }
}

impl From<u64> for Frequency {
    /// Creates a new frequency with the specified hertz.
    fn from(f: u64) -> Self {
        Self(f)
    }
}

/// Register the clock source that can be used to sleep when interrupts are
/// disabled.
///
/// The current early sleeper will only be overwritten by `T` if `frequency` is
/// larger than the frequency of the current early sleeper.
///
/// Returns whether the early sleeper was overwritten.
pub fn register_early_sleeper<T>(frequency: Frequency) -> bool
where
    T: EarlySleeper,
{
    let old_frequency = EARLY_SLEEPER_FREQUENCY.load(Ordering::SeqCst);
    if frequency > Frequency::new(old_frequency) {
        EARLY_SLEEP_FUNCTION.store(T::sleep);
        EARLY_SLEEPER_FREQUENCY.store(frequency.into(), Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// Wait for the given `duration`.
///
/// This function spins the current task rather than sleeping it and so, when
/// possible, the `sleep` crate should be used.
///
/// However, unlike the `sleep` crate, this function doesn't rely on interrupts,
/// and can be used prior to the scheduler being initiated.
pub fn early_sleep(duration: Duration) {
    let f = EARLY_SLEEP_FUNCTION.load();
    f(duration)
}

/// Register a clock source.
///
/// The provided source will overwrite the previous source only if `frequency`
/// is larger than that of the previous source.
///
/// Returns whether the previous source was overwritten.
pub fn register_clock_source<T>(frequency: Frequency) -> bool
where
    T: ClockSource,
{
    let old_frequency = Frequency::new(T::ClockType::frequency_atomic().load(Ordering::SeqCst));
    if frequency > old_frequency {
        let now_fn = T::ClockType::now_fn();
        now_fn.store(T::now);

        T::ClockType::store_unit_to_duration_func(T::unit_to_duration);
        T::ClockType::store_duration_to_unit_func(T::duration_to_unit);

        T::ClockType::frequency_atomic().store(frequency.into(), Ordering::SeqCst);

        true
    } else {
        false
    }
}

/// Returns the current time.
///
/// Monotonic clocks return an [`Instant`] whereas wall time clocks return a
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
    let f = T::now_fn().load();
    f()
}

/// A clock source.
pub trait ClockSource {
    /// The type of clock (either [`Monotonic`] or [`WallTime`]).
    type ClockType: ClockType;

    /// The current time according to the clock.
    ///
    /// Monotonic clocks return an [`Instant`] whereas wall time clocks return a
    /// [`Duration`] signifying the time since 12:00am January 1st 1970 (i.e.
    /// Unix time).
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
    /// Wait for the given `duration`.
    ///
    /// This function spins the current task rather than sleeping it and so,
    /// when possible, the `sleep` crate should be used.
    ///
    /// However, unlike the `sleep` crate, this function doesn't rely on
    /// interrupts, and can be used prior to the scheduler being initiated.
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

/// Either a [`Monotonic`] or [`WallTime`] clock.
///
/// This trait is sealed and so cannot be implemented outside of this crate.
pub trait ClockType: private::Sealed {
    /// The type returned by the [`now`] function.
    type Unit: 'static;

    #[doc(hidden)]
    fn now_fn() -> &'static AtomicCell<fn() -> Self::Unit>;
    #[doc(hidden)]
    fn frequency_atomic() -> &'static AtomicU64;

    #[doc(hidden)]
    fn store_unit_to_duration_func(f: fn(Self::Unit) -> Duration);
    #[doc(hidden)]
    fn store_duration_to_unit_func(f: fn(Duration) -> Self::Unit);
}

pub struct Monotonic;

impl private::Sealed for Monotonic {}

impl ClockType for Monotonic {
    type Unit = Instant;

    fn now_fn() -> &'static AtomicCell<fn() -> Self::Unit> {
        &MONOTONIC_NOW_FUNCTION
    }

    fn frequency_atomic() -> &'static AtomicU64 {
        &MONOTONIC_FREQUENCY
    }

    fn store_unit_to_duration_func(f: fn(Self::Unit) -> Duration) {
        INSTANT_TO_DURATION_FUNCTION.store(f);
    }

    fn store_duration_to_unit_func(f: fn(Duration) -> Self::Unit) {
        DURATION_TO_INSTANT_FUNCTION.store(f);
    }
}

pub struct WallTime;

impl private::Sealed for WallTime {}

impl ClockType for WallTime {
    type Unit = Duration;

    fn now_fn() -> &'static AtomicCell<fn() -> Self::Unit> {
        &WALL_TIME_NOW_FUNCTION
    }

    fn frequency_atomic() -> &'static AtomicU64 {
        &WALL_TIME_FREQUENCY
    }

    fn store_unit_to_duration_func(_: fn(Self::Unit) -> Duration) {
        // We intentionally don't store the function for wall time clocks.
    }

    fn store_duration_to_unit_func(_: fn(Duration) -> Self::Unit) {
        // We intentionally don't store the function for wall time clocks.
    }
}

mod private {
    /// This trait is a supertrait of [`Clocktype`](super::ClockType).
    ///
    /// Since it's in a private module, it can't be implemented by types outside
    /// this crate and thus neither can [`Clocktype`](super::ClockType).
    pub trait Sealed {}
}
