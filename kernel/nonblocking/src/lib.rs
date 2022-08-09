
#![no_std]

extern crate alloc;
#[cfg(trace_channel)] #[macro_use] extern crate log;
extern crate irq_safety;

#[cfg(downtime_eval)]
extern crate task;
extern crate futures_core;
extern crate crossbeam_queue;

pub mod channel;
pub mod el;

pub use el::{Event, EventListener};
