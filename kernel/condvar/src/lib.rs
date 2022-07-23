//! This crate contains a condition variable implementation.

#![feature(result_option_inspect)]
#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use mutex::Mutex;
use task::TaskRef;

// TODO: Do these need to be IRQ safe?

#[derive(Debug)]
pub struct Condvar {
    queue: MutexIrqSafe<VecDeque<&'static TaskRef>>,
    mutex_lock: MutexIrqSafe<()>,
}

impl Condvar {
    pub fn new() -> Self {
        Self {
            queue: MutexIrqSafe::new(VecDeque::new()),
            mutex_lock: MutexIrqSafe::new(()),
        }
    }

    pub fn notify_one(&self) {
        let mut queue = self.queue.lock();
        let _lock = self.mutex_lock.lock();
        queue.pop_front().inspect(|task| task.unblock());
    }

    pub fn notify_all(&self) {
        let mut queue = self.queue.lock();
        let _lock = self.mutex_lock.lock();

        for task in queue.drain(..) {
            task.unblock();
        }
    }

    pub fn wait(&self, mutex: &Mutex) {
        let current_task = task::get_my_current_task().unwrap();

        let mut queue = self.queue.lock();
        queue.push_back(current_task);
        drop(queue);

        let lock = self.mutex_lock.lock();
        mutex.unlock();
        current_task.block();
        drop(lock);

        scheduler::schedule();
        mutex.lock();
    }
}
