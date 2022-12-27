//! An asynchronous executor.
//!
//! This crate is very experimental. We currently spawn an OS thread (task) per
//! future, and use run states to wake the executor. Calling it an executor is
//! generous. It's merely a wrapper around a future that communicates between
//! the waker and the task system.
//!
//! The executor polls the future, passing in a waker which will unblock the
//! current task when awoken. If the future returns pending, the executor will
//! block the current task. When the future uses the waker, it unblocks the
//! task, and the executor loops around, polling the future again. It will
//! continue doing so until the future returns ready.
//!
//! The crate is named after the [Executor-class Start
//! Dreadnought][dreadnought] (`super_star_destroyer` was a bit too on the
//! nose).
//!
//! [dreadnought]: https://starwars.fandom.com/wiki/Executor-class_Star_Dreadnought

#![no_std]

extern crate alloc;

use core::{
    future::Future,
    task::{Context, Poll},
};

pub use futures::{future, pin_mut, select_biased, FutureExt};

pub mod task;
pub mod time;

/// Executes a future to completion.
///
/// This runs the given future on the current thread, blocking until it is
/// complete, and yielding its result.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    // Pin the future onto the stack. This works because we don't send it anywhere.
    pin_mut!(future);
    let (waker, blocker) = waker::waker();
    let mut context = Context::from_waker(&waker);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => blocker.block(),
        }
    }
}
