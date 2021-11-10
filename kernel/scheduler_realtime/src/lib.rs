#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate runqueue;
extern crate runqueue_realtime;

use task::TaskRef;
use runqueue_realtime::RunQueue;

pub fn select_next_task(apic_id: u8) -> Option<TaskRef> {
	None
}