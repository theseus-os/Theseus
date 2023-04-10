//! A growable, first-in first-out, multi-producer, multi-consumer, queue.
//!
//! The implementation is **heavily** inspired by the [Tokio inject queue].
//!
//! [Tokio inject queue]: https://github.com/tokio-rs/tokio/blob/master/tokio/src/runtime/task/inject.rs

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use core::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};
use sync::{Flavour, Mutex, MutexGuard};

/// A growable, first-in first-out, multi-producer, multi-consumer, queue.
pub struct Queue<F, T>
where
    F: Flavour,
{
    pointers: Mutex<F, Pointers<T>>,
    /// Prevents unnecessary locking in the fast path.
    len: AtomicUsize,
}

struct Pointers<T> {
    head: Option<NonNull<Node<T>>>,
    tail: Option<NonNull<Node<T>>>,
}

unsafe impl<T> Send for Pointers<T> {}

struct Node<T> {
    item: T,
    next: Option<NonNull<Node<T>>>,
}

impl<T> Node<T> {
    fn new(item: T) -> Self {
        Self { item, next: None }
    }
}

impl<F, T> Queue<F, T>
where
    F: Flavour,
{
    pub const fn new() -> Self {
        Self {
            pointers: Mutex::new(Pointers {
                head: None,
                tail: None,
            }),
            len: AtomicUsize::new(0),
        }
    }

    /// Returns `true` if the queue contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of elements in the queue.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Appends an item to the queue.
    pub fn push(&self, item: T) {
        let node = box_pointer(Node::new(item));
        let pointers = self.pointers.lock();
        unsafe { self.push_inner(pointers, node, node, 1) };
    }

    /// Appends an item to the queue if a condition fails.
    ///
    /// The condition will be tested while the internal lock is held.
    pub fn push_if_fail<A, B, C>(&self, item: T, condition: A) -> Result<B, C>
    where
        A: FnOnce() -> Result<B, C>,
    {
        let pointers = self.pointers.lock();
        match condition() {
            Ok(value) => Ok(value),
            Err(e) => {
                let node = box_pointer(Node::new(item));
                unsafe { self.push_inner(pointers, node, node, 1) };
                Err(e)
            }
        }
    }

    /// Appends several items to the queue.
    pub fn push_batch<I>(&self, mut iterator: I)
    where
        I: Iterator<Item = T>,
    {
        let first = match iterator.next() {
            Some(item) => box_pointer(Node::new(item)),
            None => return,
        };

        let mut tail = first;
        let mut len = 1;

        for next in iterator {
            let next = box_pointer(Node::new(next));
            // SAFETY: The only other pointer to the tail is stored in the second-to-last
            // node. We own first, hence we own all the nodes, hence that reference is not
            // being used.
            unsafe { tail.as_mut() }.next = Some(next);
            tail = next;
            len += 1;
        }

        let pointers = self.pointers.lock();
        unsafe { self.push_inner(pointers, first, tail, len) };
    }

    /// Appends a batch of nodes to the queue.
    ///
    /// # Safety
    ///
    /// `head` must be the start of the batch, and `tail` must point to the end
    /// of the batch. The batch must be `len` nodes long.
    unsafe fn push_inner(
        &self,
        mut pointers: MutexGuard<'_, F, Pointers<T>>,
        head: NonNull<Node<T>>,
        tail: NonNull<Node<T>>,
        len: usize,
    ) {
        if let Some(mut tail_pointer) = pointers.tail {
            // SAFETY: The only other pointer to the tail is stored in the second-to-last
            // node. We hold the lock to pointers, hence we own all the nodes, hence that
            // reference is not being used.
            let tail = unsafe { tail_pointer.as_mut() };
            tail.next = Some(head);
        } else {
            debug_assert!(pointers.head.is_none());
            pointers.head = Some(head);
        }
        pointers.tail = Some(tail);

        self.len.fetch_add(len, Ordering::Release);
    }

    /// Pops a node from the front of the queue.
    pub fn pop(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let mut pointers = self.pointers.lock();

        // This will return none if another thread popped the last task between us
        // checking the fast path and now.
        let current_head = pointers.head.take()?;
        // SAFETY: current_head is a valid pointer and it was created from a box which
        // ensures the correct layout.
        pointers.head = unsafe { current_head.as_ref() }.next;

        // If we are the last node in the list:
        if pointers.head.is_none() {
            pointers.tail = None;
        }

        self.len.fetch_sub(1, Ordering::Release);
        // SAFETY: current_head is a valid pointer and it was created from a box which
        // ensures the correct layout.
        Some(unsafe { Box::from_raw(current_head.as_ptr()) }.item)
    }
}

/// Boxes the item and returns a non-null pointer to the box.
fn box_pointer<T>(item: T) -> NonNull<T> {
    NonNull::from(Box::leak(Box::new(item)))
}

#[cfg(test)]
mod tests {
    use super::Queue;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };
    use sync_spin::Spin;

    #[test]
    fn test_spsc() {
        static QUEUE: Queue<Spin, i32> = Queue::new();

        for i in 0..100 {
            QUEUE.push(i);
        }

        for i in 0..100 {
            assert_eq!(QUEUE.pop().unwrap(), i);
        }

        assert!(QUEUE.is_empty());
    }

    /// Adapted from the standard library.
    #[test]
    fn test_mpmc_stress() {
        const AMOUNT: usize = 10_000;
        const NUM_THREADS: usize = 8;
        #[allow(clippy::declare_interior_mutable_const)]
        const FALSE: AtomicBool = AtomicBool::new(false);

        static RECEIVED: [AtomicBool; AMOUNT * NUM_THREADS] = [FALSE; AMOUNT * NUM_THREADS];
        static QUEUE: Queue<Spin, usize> = Queue::new();

        let mut receivers = Vec::with_capacity(NUM_THREADS);
        for _ in 0..NUM_THREADS {
            let thread = thread::spawn(move || {
                let mut counter = 0;
                while counter < AMOUNT {
                    if let Some(i) = QUEUE.pop() {
                        RECEIVED[i].store(true, Ordering::Relaxed);
                        counter += 1;
                    }
                }
            });
            receivers.push(thread);
        }

        let mut senders = Vec::with_capacity(NUM_THREADS);
        let half = NUM_THREADS / 2;
        for i in 0..half {
            let thread = thread::spawn(move || {
                let start = i * AMOUNT;
                let end = start + AMOUNT;
                QUEUE.push_batch(start..end);
            });
            senders.push(thread);
        }
        for i in half..NUM_THREADS {
            let thread = thread::spawn(move || {
                let start = i * AMOUNT;
                let end = start + AMOUNT;
                for i in start..end {
                    QUEUE.push(i);
                }
            });
            senders.push(thread);
        }

        for thread in receivers {
            thread.join().unwrap();
        }
        for thread in senders {
            thread.join().unwrap();
        }

        for received in RECEIVED.iter() {
            assert!(received.load(Ordering::Relaxed));
        }
        assert!(QUEUE.is_empty());
    }
}
