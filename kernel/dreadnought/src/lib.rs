#![deny(unsafe_op_in_unsafe_fn)]
#![feature(sync_unsafe_cell)]
#![no_std]

extern crate alloc;

mod join;
mod task;

use crate::task::Task;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    cell::SyncUnsafeCell,
    future::Future,
    pin::Pin,
    task::{Context, Waker},
};
use futures::FutureExt;
use join::JoinHandle;
use spin::Mutex;
use crossbeam_queue::SegQueue;

pub struct Executor {
    queue: SegQueue,
}

impl Executor {
    pub fn new() -> Self {
        // let (sender, receiver) = async_channel::new_channel(64);
        // Self { sender, receiver }
        todo!();
    }

    pub fn spawn<F>(&self, future: F) -> ()
    where
        F: Future<Output = ()> + 'static + Send,
    {
        // let boxed = SyncUnsafeCell::new(Box::pin(future));
        // let task = Task {
        //     future: boxed,
        //     run_queue: self.sender.clone(),
        // };

        // let waker = Arc::new(task);
        // // let context = Context::from_waker(&Waker::from(waker));
        // todo!();
        // boxed.poll(&mut context);
    }

    pub fn block_on<F>(&self, _future: F)
    where
        F: Future,
    {
        todo!();
    }
}

fn temp() {
    Executor::new().spawn(async {
        println!("hello world");
    });
}
