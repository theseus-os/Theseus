//! This crate contains abstractions to interact with hardware clocks.

#![no_std]

use core::ops;
use crossbeam_utils::atomic::AtomicCell;

mod dummy;

pub use core::time::Duration;

const FEMTOS_TO_NANOS: u128 = 1_000_000;

static EARLY_SLEEP_FUNCTION: AtomicCell<fn(Duration)> = AtomicCell::new(dummy::early_sleep);
static EARLY_SLEEPER_PERIOD: AtomicCell<Period> = AtomicCell::new(Period::MAX);

static MONOTONIC_NOW_FUNCTION: AtomicCell<fn() -> Instant> = AtomicCell::new(dummy::monotonic_now);
static MONOTONIC_PERIOD: AtomicCell<Period> = AtomicCell::new(Period::MAX);

static WALL_TIME_NOW_FUNCTION: AtomicCell<fn() -> Duration> = AtomicCell::new(dummy::wall_time_now);
static WALL_TIME_PERIOD: AtomicCell<Period> = AtomicCell::new(Period::MAX);

/// A measurement of a monotonically nondecreasing clock.
///
/// The inner value usually represents the internal counter value but the type
/// is intentionally opaque and only useful with [`Duration`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Instant {
    counter: u64,
}

impl Instant {
    pub const ZERO: Self = Self { counter: 0 };

    pub const MAX: Self = Self { counter: u64::MAX };

    pub fn new(counter: u64) -> Self {
        Self { counter }
    }

    /// Returns the amount of time elapsed from another instant to this one, or
    /// zero duration if that instant is later than this one.
    pub fn duration_since(&self, earlier: Self) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }

    pub fn checked_duration_since(&self, earlier: Self) -> Option<Duration> {
        let instant = Instant {
            counter: self.counter.checked_sub(earlier.counter)?,
        };
        let femtos = u128::from(instant.counter) * u128::from(MONOTONIC_PERIOD.load());
        Some(Duration::from_nanos((femtos / FEMTOS_TO_NANOS) as u64))
    }
}

impl Default for Instant {
    fn default() -> Self {
        Self::ZERO
    }
}

impl ops::Add<Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        let femtos = rhs.as_nanos() * FEMTOS_TO_NANOS;
        let ticks = (femtos / u128::from(MONOTONIC_PERIOD.load())) as u64;
        Self {
            counter: self
                .counter
                .checked_add(ticks)
                .expect("overflow when adding duration to instant"),
        }
    }
}

impl ops::AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl ops::Sub<Duration> for Instant {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        let femtos = rhs.as_nanos() * FEMTOS_TO_NANOS;
        let ticks = (femtos / u128::from(MONOTONIC_PERIOD.load())) as u64;
        Self {
            counter: self
                .counter
                .checked_sub(ticks)
                .expect("overflow when subtracting duration from instant"),
        }
    }
}

impl ops::Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.duration_since(rhs)
    }
}

impl ops::SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}

/// A clock period, measured in femtoseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Period(u64);

impl Period {
    const MAX: Self = Self(u64::MAX);

    /// Creates a new period with the specified femtoseconds.
    pub fn new(period: u64) -> Self {
        Self(period)
    }
}

impl From<Period> for u64 {
    /// Returns the period in femtoseconds.
    fn from(f: Period) -> Self {
        f.0
    }
}

impl From<Period> for u128 {
    /// Returns the period in femtoseconds.
    fn from(f: Period) -> Self {
        f.0.into()
    }
}

impl From<u64> for Period {
    /// Creates a new period with the specified femtoseconds.
    fn from(period: u64) -> Self {
        Self(period)
    }
}

/// Register the clock source that can be used to sleep when interrupts are
/// disabled.
///
/// The provided early sleeper will overwrite the current early sleeper only if
/// `period` is smaller than that of the current early sleeper.
///
/// Returns whether the early sleeper was overwritten.
pub fn register_early_sleeper<T>(period: Period) -> bool
where
    T: EarlySleeper,
{
    if EARLY_SLEEPER_PERIOD
        .fetch_update(|old_period| {
            if period < old_period {
                Some(period)
            } else {
                None
            }
        })
        .is_ok()
    {
        EARLY_SLEEP_FUNCTION.store(T::sleep);
        true
    } else {
        false
    }
}

/// Wait for the given `duration`.
///
/// This function spins the current task rather than sleeping it and so, when
/// possible, the `sleep` crate should be used. However, unlike the `sleep`
/// crate, this function doesn't rely on interrupts.
///
/// This function must not be called prior to registering an early sleeper using
/// [`register_early_sleeper`].
pub fn early_sleep(duration: Duration) {
    let f = EARLY_SLEEP_FUNCTION.load();
    f(duration)
}

/// Register a clock source.
///
/// The provided clock source will overwrite the current clock source only if
/// `period` is smaller than that of the current clock source.
///
/// Returns whether the clock source was overwritten.
pub fn register_clock_source<T>(period: Period) -> bool
where
    T: ClockSource,
{
    let period_atomic = T::ClockType::period_atomic();
    if period_atomic
        .fetch_update(|old_period| {
            if period < old_period {
                Some(period)
            } else {
                None
            }
        })
        .is_ok()
    {
        let now_fn = T::ClockType::now_fn();
        now_fn.store(T::now);

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
/// This function must not be called prior to registering a clock source of the
/// specified type using [`register_clock_source`].
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
    fn period_atomic() -> &'static AtomicCell<Period>;
}

pub struct Monotonic;

impl private::Sealed for Monotonic {}

impl ClockType for Monotonic {
    type Unit = Instant;

    fn now_fn() -> &'static AtomicCell<fn() -> Self::Unit> {
        &MONOTONIC_NOW_FUNCTION
    }

    fn period_atomic() -> &'static AtomicCell<Period> {
        &MONOTONIC_PERIOD
    }
}

pub struct WallTime;

impl private::Sealed for WallTime {}

impl ClockType for WallTime {
    type Unit = Duration;

    fn now_fn() -> &'static AtomicCell<fn() -> Self::Unit> {
        &WALL_TIME_NOW_FUNCTION
    }

    fn period_atomic() -> &'static AtomicCell<Period> {
        &WALL_TIME_PERIOD
    }
}

mod private {
    /// This trait is a supertrait of [`Clocktype`](super::ClockType).
    ///
    /// Since it's in a private module, it can't be implemented by types outside
    /// this crate and thus neither can [`Clocktype`](super::ClockType).
    pub trait Sealed {}
}
