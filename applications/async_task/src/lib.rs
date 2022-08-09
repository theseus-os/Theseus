#![no_std]

#[macro_use]
extern crate app_io;
#[macro_use]
extern crate alloc;
#[macro_use]
extern crate log;
// #[macro_use] extern crate terminal_print;
extern crate apic;
extern crate core2;
extern crate scheduler;
extern crate spawn;
extern crate spin;
extern crate task as rtask;
extern crate wait_condition;
extern crate nonblocking;
extern crate futures_util;

mod executor;
mod noop_waker;
mod task;

// use core::sync::atomic::{Ordering, AtomicBool};
use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;
use wait_condition::{WaitCondition, WaitConditionFn};
use core::future::ready;
use crate::{executor::Executor, task::Task};
use crate::core2::io::Write;

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use core::sync::atomic::Ordering::Relaxed;
use core::task::Poll;
use core::task::Context;
use core::pin::Pin;
use core::future::Future;
use core::sync::atomic::AtomicBool;
use core::time::Duration;
use nonblocking::Event;
use spin::Once;
use futures_util::task::AtomicWaker;

static ITERATIONS: AtomicUsize = AtomicUsize::new(10);
macro_rules! iterations {
    () => {
        ITERATIONS.load(Ordering::SeqCst)
    };
}

static PINNED: Once<bool> = Once::new();
macro_rules! pin_task {
    ($tb:ident, $cpu:expr) => (
        if PINNED.get() == Some(&true) {
            $tb.pin_on_core($cpu)
        } else {
            $tb
        });
}

pub fn main(_args: Vec<String>) -> isize {
    match async_channel2() {
        Ok(_) => 0,
        Err(e) => {
            error!("Error: {}", e);
            -1
        }
    }
}

fn async_channel() -> Result<(), &'static str> {
    let my_cpu = apic::get_my_apic_id();

    let flag = Arc::new(AtomicBool::new(false));
	let event = Arc::new(Event::new());


    let t1 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_oneshot(): Entered sender task!");
       	sleep::sleep(5_000);

		flag.store(true, Ordering::SeqCst);
		event.notify(usize::MAX);

        Ok(())
    }, ())
        .name(String::from("sender_task_asynchronous_oneshot"))
        .block();
    let t1 = pin_task!(t1, my_cpu).spawn()?;

    let t2 = spawn::new_task_builder(|_: ()| -> Result<(), &'static str> {
        warn!("asynchronous_test_oneshot(): Entered receiver task!");

		// Check the flag.
		if flag.load(Ordering::SeqCst) {
		    warn!("Check the flag.");
		}

		// Start listening for events.
		let listener = event.listen();

		// Check the flag again after creating the listener.
		if flag.load(Ordering::SeqCst) {
		    warn!("Check the flag again after creating the listener.");
		}

		info!("Awating....");
		// Wait for a notification and continue the loop.
		let msg = listener.wait();
		
        warn!("asynchronous_test_oneshot(): Receiver got msg: {:?}", msg);
        Ok(())
    }, ())
        .name(String::from("receiver_task_asynchronous_oneshot"))
        .block();
    let t2 = pin_task!(t2, my_cpu).spawn()?;

    warn!("asynchronous_test_oneshot(): Finished spawning the sender and receiver tasks");
    t2.unblock(); t1.unblock();

    t1.join()?;
    t2.join()?;
    warn!("asynchronous_test_oneshot(): Joined the sender and receiver tasks.");
    
    Ok(())
}

fn event_listener() -> Result<(), &'static str> {
	let flag = Arc::new(AtomicBool::new(false));
	let event = Arc::new(Event::new());


	let t1 = spawn::new_task_builder({
			let flag = flag.clone();
			let event = event.clone();
			move |_: ()| -> Result<(), &'static str> {
				info!("Sleeping zzZZ");
				// Wait for a second.
				sleep::sleep(5_000);
				info!("Awake!");

				// Set the flag.
				flag.store(true, Ordering::SeqCst);

				// Notify all listeners that the flag has been set.
				event.notify(usize::MAX);
				
				Ok(())
			}
		},
        (),
    )
    .name(String::from("async_task"))
    .block()
    .pin_on_core(apic::get_my_apic_id())
    .spawn()?;


    info!("test_multiple(): Finished spawning the executor task");
    t1.unblock();
    
    // Wait until the flag is set.
	loop {
		// Check the flag.
		if flag.load(Ordering::SeqCst) {
		    break;
		}

		// Start listening for events.
		let listener = event.listen();

		// Check the flag again after creating the listener.
		if flag.load(Ordering::SeqCst) {
		    break;
		}

		info!("Awating....");
		// Wait for a notification and continue the loop.
		listener.wait();
	}

    t1.join()?;
    info!("test_multiple(): Joined the executor task.");
    let _t1_exit = t1.take_exit_value();
    
    info!("Event task ended!");

    Ok(())
}

fn async_channel2() -> Result<(), &'static str> {
	let (s, r) = nonblocking::channel::unbounded();

	let t1 = spawn::new_task_builder({
			move |_: ()| -> Result<(), &'static str> {
				info!("Sleeping zzZZ");
				// Wait for a second.
				sleep::sleep(5_000);
				info!("Awake!");

				// Set the flag.
				let mut executor = Executor::new();
				executor.spawn(Task::new(async move {
					info!("Future running!");
					info!("Future {:?}!", s.send("Hello").await);
				}));

				executor.run();

				
				Ok(())
			}
		},
        (),
    )
    .name(String::from("async_task"))
    .block()
    .pin_on_core(apic::get_my_apic_id())
    .spawn()?;


    info!("test_multiple(): Finished spawning the executor task");
    t1.unblock();
    
				// Set the flag.
				let mut executor = Executor::new();
				executor.spawn(Task::new(async move {
					info!("Future running!");
					info!("Future {:?}!", r.recv().await);
				}));

				executor.run();

    t1.join()?;
    info!("test_multiple(): Joined the executor task.");
    let _t1_exit = t1.take_exit_value();
    
    info!("Event task ended!");

    Ok(())
}



struct Inner {
    waker: AtomicWaker,
    set: AtomicBool,
}

#[derive(Clone)]
struct Flag(Arc<Inner>);

impl Flag {
    pub fn new() -> Self {
        Flag(Arc::new(Inner {
            waker: AtomicWaker::new(),
            set: AtomicBool::new(false),
        }))
    }

    pub fn signal(&self) {
        self.0.set.store(true, Relaxed);
        self.0.waker.wake();
    }
}

impl Flag {
    pub async fn read(&mut self, buf: &mut [u8]) {
		core::future::poll_fn(|cx| {
			// quick check to avoid registration if already done.
		    if self.0.set.load(Relaxed) {
		        return Poll::Ready(());
		    }

		    self.0.waker.register(cx.waker());

		    // Need to check condition **after** `register` to avoid a race
		    // condition that would result in lost notifications.
		    if self.0.set.load(Relaxed) {
		        Poll::Ready(())
		    } else {
		        Poll::Pending
		    }
		}).await
    }
}



fn rmain2() -> Result<(), &'static str> {
	let my_cpu = apic::get_my_apic_id();
	
	let stdout = app_io::stdout()?;
    let mut stdin = app_io::stdin()?;



    let mut stdin_ = stdin.clone();
    let t1 = spawn::new_task_builder(
        |_: ()| -> Result<(), &'static str> {
            info!("async_task: Executor task spawned!");
    
			let mut executor = Executor::new();
			executor.spawn(Task::new(async move {
				info!("Future running!");
				let mut buf = String::new();
				let fut = stdin_._read_line(&mut buf).await;
				info!("Future {:?}!", buf);
			}));

    
    
    		executor.run();
    		
	        info!("async_task: Executor task ended!");
            
            Ok(())
        },
        (),
    )
    .name(String::from("receiver_task"))
    .block()
    .pin_on_core(my_cpu)
    .spawn()?;

    info!("test_multiple(): Finished spawning the executor task");
    t1.unblock();


    t1.join()?;
    info!("test_multiple(): Joined the executor task.");
    let _t1_exit = t1.take_exit_value();

    Ok(())
}


