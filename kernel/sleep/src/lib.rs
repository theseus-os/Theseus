//! Provides APIs for tasks to sleep for specified time durations.
//!
//! Key functions:
//! * The [`sleep`] function delays the current task for a given number of ticks.
//! * The [`sleep_until`] function delays the current task until a specific moment in the future.
//! * The [`sleep_periodic`] function allows for tasks to be delayed for periodic intervals
//!  of time and can be used to implement a period task.
//!
//! TODO: use regular time-keeping abstractions like Duration and Instant.

#![no_std]
#![feature(let_else)]

extern crate task;
extern crate irq_safety;
extern crate alloc;
#[macro_use] extern crate lazy_static;
extern crate scheduler;

use alloc::collections::binary_heap::BinaryHeap;
use alloc::sync::Arc;
use core::cmp;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{Ordering, AtomicUsize};
use core::task::{Context, Poll, Waker};

use irq_safety::MutexIrqSafe;

/// A duration given in ticks of the [APIC timer].
/// 
/// [APIC timer]: interrupts::lapic_timer_handler
pub type Ticks = usize;

pub type AtomicTicks = AtomicUsize;

/// A sleeper is the `Future` returned by the asynchronous sleep functions.
#[derive(Debug, Clone)]
struct Sleeper {
    /// The absolute time in ticks at which the sleeper will wake up.
    wake_time: Ticks,
    /// Mutable inner state.
    inner: Arc<MutexIrqSafe<SleeperInner>>,
}

#[derive(Debug)]
struct SleeperInner {
    /// Is the sleeper still asleep or have they woken up?
    sleep_state: SleepState,
    /// Waker to notify when the sleeper wakes up.
    waker: Option<Waker>,
}

#[derive(Clone, Debug)]
enum SleepState {
    Asleep,
    Awake,
}

impl Sleeper {
    /// Create a new sleeper that will wake up at the specified time.
    /// If `wake_time` is in the past the sleeper starts out already awake.
    fn new(wake_time: Ticks) -> Self  {
        let sleep_state = if wake_time > tick_count() {
                SleepState::Asleep
            } else {
                SleepState::Awake
            };
        Self {
            wake_time,
            inner: Arc::new(MutexIrqSafe::new(SleeperInner {
                sleep_state,
                waker: None,
            })),
        }
    }

    /// Awaken the sleeper, i.e. mark the future as complete as notify the `Waker` (if there is one).
    fn awaken(&mut self) {
        let mut inner = self.inner.lock();
        inner.sleep_state = SleepState::Awake;
        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
    }
}

impl Future for Sleeper {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.lock();
        match inner.sleep_state {
            SleepState::Asleep => {
                inner.waker = Some(cx.waker().clone());
                Poll::Pending
            }

            SleepState::Awake => {
                Poll::Ready(())
            }
        }
    }
}

// The priority queue depends on `Ord`.
impl Ord for Sleeper {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // Reverse the ordering so the queue becomes a min-heap instead of a max-heap.
        self.wake_time.cmp(&other.wake_time).reverse()
    }
}
impl PartialOrd for Sleeper {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Sleeper {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == cmp::Ordering::Equal
    }
}
impl Eq for Sleeper {}


/// The number of timer ticks elapsed since system startup.
static TICK_COUNT: AtomicTicks = AtomicTicks::new(0);

/// Returns the number of timer ticks elapsed since system startup.
fn tick_count() -> Ticks {
    TICK_COUNT.load(Ordering::SeqCst)
}

lazy_static! {
    /// A list of all sleepers in the system sorted in increasing order of `wake_time`.
    static ref SLEEPERS: MutexIrqSafe<BinaryHeap<Sleeper>>
        = MutexIrqSafe::new(BinaryHeap::new());
}

/// Keeps track of when the next sleeper will awake. By default, it is the maximum time.
static NEXT_WAKE_TIME: AtomicTicks = AtomicTicks::new(Ticks::MAX);

/// Called by the [APIC timer] each tick of the clock.
/// Increments the global tick count and wakes up sleepers who've hit their wake time.
/// 
/// Keep in mind that interrupts are disabled when this is called.
/// 
/// [APIC timer]: interrupts::lapic_timer_handler
pub fn tick() {
    TICK_COUNT.fetch_add(1, Ordering::SeqCst);
    awaken_sleepers();
}

/// Awaken all sleepers who have hit their wake time.
pub fn awaken_sleepers() {
    let time = tick_count();

    // Check the next wake time. If it's in the future we don't need to get the `SLEEPERS` lock.
    let mut next_wake_time = NEXT_WAKE_TIME.load(Ordering::SeqCst);
    if next_wake_time > time {
        return;
    } 

    // Wake up all sleepers who've hit their wake time.
    let mut sleepers = SLEEPERS.lock();
    while next_wake_time <= time {
        let Some(mut sleeper) = sleepers.pop() else {
            return;
        };

        sleeper.awaken();

        next_wake_time = match sleepers.peek() {
            Some(next_sleeper) => next_sleeper.wake_time,
            None => Ticks::MAX,
        };
    }

    // Store the earliest sleeper's wake time for next time.
    NEXT_WAKE_TIME.store(next_wake_time, Ordering::SeqCst);
}



/// Blocks asynchronously for `duration` ticks.
pub async fn sleep(duration: Ticks) {
    sleep_until(tick_count() + duration).await;
}

/// Blocks asynchronously until a specific absolute time.
/// If the time has already passed there is no delay.
pub fn sleep_until(wake_time: Ticks) -> impl Future<Output = ()> {
    let sleeper = Sleeper::new(wake_time);

    let mut sleepers = SLEEPERS.lock();
    sleepers.push(sleeper.clone());

    let next_wake_time = NEXT_WAKE_TIME.load(Ordering::SeqCst);
    if sleeper.wake_time < next_wake_time {
        NEXT_WAKE_TIME.store(sleeper.wake_time, Ordering::SeqCst);
    }

    sleeper
}

/// Returns a timer that can be used to sleep for predictable durations correcting for drift.
/// For example, if you wanted to sleep for precise 100ms intervals
/// but there could be anywhere from 0-5ms of latency between calls to `sleep`,
/// this would compensate for the variable delay and sleep up to 5ms less as needed.
pub fn regular_timer() -> RegularTimer {
    RegularTimer {
        last_wake_time: tick_count(),
    }
}

pub struct RegularTimer {
    last_wake_time: Ticks,
}

impl RegularTimer {
    /// Blocks asynchronously for `duration` ticks, correcting for any delay since the last
    /// `sleep` call.
    pub async fn sleep(&mut self, duration: Ticks) {
        sleep_until(self.last_wake_time + duration).await;
        self.last_wake_time += duration;
    }
}
