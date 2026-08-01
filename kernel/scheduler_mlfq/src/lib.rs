//! A multi-level feedback queue (MLFQ) scheduler.
//!
//! Tasks start out in the highest-priority queue (level `0`) and are
//! demoted a level whenever they consume a full quantum's worth of CPU time
//! without blocking. A task that blocks or yields before its quantum is
//! exhausted keeps its current level and is granted a fresh quantum next
//! time it runs; this is what makes MLFQ favour interactive/I/O-bound tasks
//! over CPU-bound ones without needing any a-priori classification of
//! tasks. A periodic priority boost moves every task back to level `0`,
//! which bounds the worst-case wait time of any runnable task and prevents
//! starvation of tasks that were previously demoted.
//!
//! Quanta grow linearly with level (level `n` gets `(n + 1) * BASE_QUANTUM`)
//! so that long-running, CPU-bound tasks are scheduled less often but for
//! longer stretches, reducing context-switch overhead for exactly the
//! workloads that don't need low latency.

#![no_std]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};

use task::TaskRef;
use time::{Duration, Instant};

/// Number of priority levels, from `0` (highest priority, shortest quantum)
/// to `NUM_LEVELS - 1` (lowest priority, longest quantum).
const NUM_LEVELS: usize = 8;

/// The quantum granted to tasks at level `0`.
const BASE_QUANTUM: Duration = Duration::from_millis(5);

/// How often every task is boosted back to level `0`. This bounds the
/// worst-case wait time of any runnable task.
const BOOST_INTERVAL: Duration = Duration::from_millis(300);

/// Returns the quantum granted to tasks at the given level.
fn quantum_for_level(level: usize) -> Duration {
    BASE_QUANTUM * (level as u32 + 1)
}

/// Priority levels are exposed through [`task::scheduler::PriorityScheduler`]
/// as `u8`s, with a *higher* value meaning *higher* priority (matching the
/// convention used by `scheduler_priority` and relied on by
/// `scheduler::inherit_priority`). This is the inverse of our internal
/// level numbering, so we convert between the two at the API boundary.
fn priority_for_level(level: usize) -> u8 {
    (NUM_LEVELS - 1 - level) as u8
}

/// Priorities above `NUM_LEVELS - 1` all clamp to level `0`: MLFQ only has
/// as many distinct priorities as it has levels.
fn level_for_priority(priority: u8) -> usize {
    let priority = priority.min((NUM_LEVELS - 1) as u8) as usize;
    NUM_LEVELS - 1 - priority
}

/// A task's entry within one of the MLFQ's level queues.
#[derive(Clone)]
struct Entry {
    task: TaskRef,
    /// CPU time accumulated at the current level since the task last
    /// entered it (via being added, demoted, promoted, or boosted). Reset
    /// to zero whenever the task changes levels.
    runtime_in_level: Duration,
}

impl Entry {
    const fn new(task: TaskRef) -> Self {
        Self {
            task,
            runtime_in_level: Duration::ZERO,
        }
    }
}

/// Bookkeeping for whichever task was most recently returned by
/// [`Scheduler::next`], so that the next call can charge it for the CPU
/// time it consumed in the interim.
struct Dispatched {
    task: TaskRef,
    level: usize,
    started_at: Instant,
}

pub struct Scheduler {
    idle_task: TaskRef,
    levels: Vec<VecDeque<Entry>>,
    dispatched: Option<Dispatched>,
    last_boost: Instant,
}

impl Scheduler {
    pub fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            levels: (0..NUM_LEVELS).map(|_| VecDeque::new()).collect(),
            dispatched: None,
            last_boost: Instant::now(),
        }
    }

    /// Charges the previously-dispatched task (if any) for the CPU time it
    /// just consumed, demoting it a level if that pushed it over its
    /// quantum.
    ///
    /// A task that is no longer runnable (it blocked or exited) is not
    /// charged and is not demoted: blocking before a quantum is exhausted
    /// is exactly the behaviour MLFQ is meant to reward, since it's the
    /// hallmark of an interactive or I/O-bound task rather than a
    /// CPU-bound one.
    fn charge_dispatched(&mut self, now: Instant) {
        let Some(dispatched) = self.dispatched.take() else {
            return;
        };

        if !dispatched.task.is_runnable() {
            return;
        }

        let level = dispatched.level;
        let Some(entry_index) = self.levels[level]
            .iter()
            .position(|entry| entry.task == dispatched.task)
        else {
            // It was removed from the run queue (e.g. it exited) while it
            // was running; nothing left to charge.
            return;
        };

        let ran_for = now.duration_since(dispatched.started_at);
        let runtime_in_level = self.levels[level][entry_index].runtime_in_level + ran_for;
        let quantum = quantum_for_level(level);

        if runtime_in_level >= quantum && level + 1 < self.levels.len() {
            let mut entry = self.levels[level].remove(entry_index).unwrap();
            entry.runtime_in_level = Duration::ZERO;
            self.levels[level + 1].push_back(entry);
        } else {
            self.levels[level][entry_index].runtime_in_level = runtime_in_level;
        }
    }

    /// If enough time has passed since the last boost, moves every task
    /// back to level `0` and resets its accumulated runtime, including that
    /// of the currently-dispatched task.
    fn maybe_boost(&mut self, now: Instant) {
        if now.duration_since(self.last_boost) < BOOST_INTERVAL {
            return;
        }

        for level in 1..self.levels.len() {
            while let Some(mut entry) = self.levels[level].pop_front() {
                entry.runtime_in_level = Duration::ZERO;
                self.levels[0].push_back(entry);
            }
        }
        for entry in self.levels[0].iter_mut() {
            entry.runtime_in_level = Duration::ZERO;
        }

        if let Some(dispatched) = &mut self.dispatched {
            dispatched.level = 0;
            dispatched.started_at = now;
        }

        self.last_boost = now;
    }

    /// Finds and removes `task` from whichever level queue currently holds
    /// it, returning the level it was found at and its entry.
    fn take_entry(&mut self, task: &TaskRef) -> Option<(usize, Entry)> {
        for (level, queue) in self.levels.iter_mut().enumerate() {
            if let Some(index) = queue.iter().position(|entry| &entry.task == task) {
                return Some((level, queue.remove(index).unwrap()));
            }
        }
        None
    }
}

impl task::scheduler::Scheduler for Scheduler {
    fn next(&mut self) -> TaskRef {
        let now = Instant::now();

        self.maybe_boost(now);
        self.charge_dispatched(now);

        for (level, queue) in self.levels.iter_mut().enumerate() {
            if let Some(index) = queue.iter().position(|entry| entry.task.is_runnable()) {
                let entry = queue.remove(index).unwrap();
                let task = entry.task.clone();
                queue.push_back(entry);
                self.dispatched = Some(Dispatched {
                    task: task.clone(),
                    level,
                    started_at: now,
                });
                return task;
            }
        }

        self.dispatched = None;
        self.idle_task.clone()
    }

    fn add(&mut self, task: TaskRef) {
        // New tasks start at the highest priority level so that short-lived
        // or interactive tasks run promptly; only tasks that turn out to be
        // CPU-bound get demoted over time.
        self.levels[0].push_back(Entry::new(task));
    }

    fn busyness(&self) -> usize {
        self.levels.iter().map(VecDeque::len).sum()
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        if self.dispatched.as_ref().is_some_and(|d| &d.task == task) {
            self.dispatched = None;
        }
        self.take_entry(task).is_some()
    }

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn task::scheduler::PriorityScheduler> {
        Some(self)
    }

    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_> {
        self.dispatched = None;
        Box::new(
            self.levels
                .iter_mut()
                .flat_map(|queue| queue.drain(..))
                .map(|entry| entry.task)
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }

    fn tasks(&self) -> Vec<TaskRef> {
        self.levels
            .iter()
            .flat_map(|queue| queue.iter())
            .map(|entry| entry.task.clone())
            .collect()
    }
}

/// Implementing this trait means MLFQ correctly participates in priority
/// inheritance (see `sync_block::MutexGuard`, which calls
/// `scheduler::inherit_priority` on a lock holder to prevent unbounded
/// priority inversion when a higher-priority task blocks on that lock).
/// Without this, a task holding a lock could sit at a low MLFQ level while
/// higher-priority waiters starve behind it.
impl task::scheduler::PriorityScheduler for Scheduler {
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        let Some((_old_level, mut entry)) = self.take_entry(task) else {
            return false;
        };

        let level = level_for_priority(priority);
        entry.runtime_in_level = Duration::ZERO;
        self.levels[level].push_back(entry);

        if let Some(dispatched) = &mut self.dispatched {
            if &dispatched.task == task {
                dispatched.level = level;
            }
        }

        true
    }

    fn priority(&mut self, task: &TaskRef) -> Option<u8> {
        self.levels.iter().enumerate().find_map(|(level, queue)| {
            queue
                .iter()
                .any(|entry| &entry.task == task)
                .then(|| priority_for_level(level))
        })
    }
}
