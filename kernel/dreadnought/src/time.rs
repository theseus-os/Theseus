use alloc::collections::BinaryHeap;
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};
use irq_safety::MutexIrqSafe;

static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
lazy_static::lazy_static! {
    static ref SLEEPING_TASKS: MutexIrqSafe<BinaryHeap<Node>> = MutexIrqSafe::new(BinaryHeap::new());
}

#[derive(Debug)]
struct Node {
    wakeup: usize,
    waker: Waker,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.wakeup == other.wakeup
    }
}

impl Eq for Node {}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other.wakeup.cmp(&self.wakeup)
    }
}

pub fn sleep(ticks: usize) -> Sleep {
    let current_ticks = TICK_COUNT.load(Ordering::SeqCst);
    let wakeup = current_ticks + ticks;
    Sleep { wakeup }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Sleep {
    wakeup: usize,
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if TICK_COUNT.load(Ordering::SeqCst) >= self.wakeup {
            Poll::Ready(())
        } else {
            SLEEPING_TASKS.lock().push(Node {
                wakeup: self.wakeup,
                waker: cx.waker().clone(),
            });
            Poll::Pending
        }
    }
}

#[doc(hidden)]
pub fn increment_tick_count() {
    let ticks = TICK_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
    let mut sleeping_tasks = SLEEPING_TASKS.lock();

    while let Some(next_node) = sleeping_tasks.peek() {
        if next_node.wakeup <= ticks {
            let next_node = sleeping_tasks.pop().unwrap();
            next_node.waker.wake();
        } else {
            break;
        }
    }
}
