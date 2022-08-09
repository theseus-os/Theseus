use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::noop_waker;

use super::Task;
use alloc::collections::VecDeque;

pub struct Executor {
    task_queue: VecDeque<Task>,
}

impl Executor {
    pub fn new() -> Executor {
        Executor {
            task_queue: VecDeque::new(),
        }
    }

    pub fn spawn(&mut self, task: Task) {
        self.task_queue.push_back(task)
    }

    pub fn run(&mut self) {
        while let Some(mut task) = self.task_queue.pop_front() {
            let waker = noop_waker::waker();
            let mut context = Context::from_waker(&waker);
            let pinned = Pin::new(&mut task);
            match pinned.poll(&mut context) {
                Poll::Ready(()) => {} // task done
                Poll::Pending => self.task_queue.push_back(task),
            }
        }
    }
}
