#![no_std]
#![feature(asm)]
#![feature(no_more_cas)]

extern crate alloc;
extern crate task;
extern crate memory;
extern crate apic;
extern crate hpet;
extern crate runqueue;
extern crate pmu_x86;
extern crate libm;

use hpet::get_hpet;
use pmu_x86::{Counter, EventType};
use alloc::vec::Vec;

const NANO_TO_FEMTO: u64 = 1_000_000;

/// Helper function to convert ticks to nano seconds
pub fn hpet_2_ns(hpet: u64) -> u64 {
	let hpet_period = get_hpet().as_ref().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / NANO_TO_FEMTO
}

#[macro_export]
macro_rules! CPU_ID {
	() => (apic::get_my_apic_id())
}

/// Helper function return the tasks in a given core's runqueue
pub fn nr_tasks_in_rq(core: u8) -> Option<usize> {
	match runqueue::get_runqueue(core).map(|rq| rq.read()) {
		Some(rq) => { Some(rq.iter().count()) }
		_ => { None }
	}
}

/// True if only two tasks are running in the current runqueue.
/// Used to verify if there are any other tasks than the current task and idle task in the runqueue
pub fn check_myrq() -> bool {
	match nr_tasks_in_rq(CPU_ID!()) {
		Some(2) => { true }
		_ => { false }
	}
}

#[inline(always)]
/// Starts the PMU counter to measure reference cycles.
/// The PMU should be initialized before calling this function.
/// The PMU initialization, start count and stop count should all be called on the same core.
pub fn start_counting_reference_cycles() -> Result<Counter, &'static str> {
	let mut counter = Counter::new(EventType::UnhaltedReferenceCycles)?;
	counter.start()?;
	Ok(counter)
}

#[inline(always)]
/// Stops the PMU counter and stores the reference cycles since the start.
/// The PMU should be initialized before calling this function.
/// The PMU initialization, start count and stop count should all be called on the same core.
pub fn stop_counting_reference_cycles(counter: Counter) -> Result<u64, &'static str> {
	let count = counter.diff();
	let _ = counter.end()?;
	Ok(count)
}

/// Measures the overhead of using the PMU reference cycles counter.
/// Calls `timing_overhead_inner` multiple times and averages the value. 
/// Overhead is in reference cycles.
pub fn cycle_count_overhead() -> Result<u64, &'static str>  {
	const TRIES: u64 = 10;

	let mut overhead_sum: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;

	for _ in 0..TRIES {
		let overhead = cycle_count_overhead_inner()?;
		overhead_sum += overhead;
		if overhead > max {max = overhead;}
		if overhead < min {min = overhead;}
	}

	Ok(overhead_sum/ TRIES as u64)
}

/// Internal function that actually calculates cycle count overhead. 
/// Calls the counter read instruction multiple times and average the value. 
/// Overhead is in reference cycles. 
fn cycle_count_overhead_inner() -> Result<u64, &'static str> {
	const ITERATIONS: usize = 10_000;
	let mut _tmp: u64;
	let delta: u64;

	let counter = start_counting_reference_cycles()?;

	for _ in 0..ITERATIONS {
		_tmp = counter.diff();
	}
	delta = stop_counting_reference_cycles(counter)?;

	Ok(delta / ITERATIONS as u64)
}

/// Helper function to calculate statistics of a provided dataset
pub fn calculate_stats(vec: &Vec<u64>) -> Option<Stats>{
	let mean;
  	let median;
  	let p_75;
	let p_25;
	let min;
	let max;
	let var;
    let std_dev;

	if vec.is_empty() {
		return None;
	}

	let len = vec.len();

  	{ // calculate average
		let mut sum: u64 = 0;
		for &x in vec {
			sum = sum + x;
		}

		mean = sum as u64 / len as u64;
  	}

	{ // calculate median
		let mut vec2 = vec.clone();
		vec2.sort();
		let mid = len / 2;
		let i_75 = len * 3 / 4;
		let i_25 = len * 1 / 4;

		median = vec2[mid];
		p_25 = vec2[i_25];
		p_75 = vec2[i_75];
		min = vec2[0];
		max = vec2[len - 1];
  	}

	{ // calculate sample variance
		let mut diff_sum: u64 = 0;
      	for &x in vec {
			if x > mean {
				diff_sum = diff_sum + ((x-mean)*(x-mean));
			}
			else {
				diff_sum = diff_sum + ((mean - x)*(mean - x));
			}
      	}

    	var = (diff_sum) / (len as u64);
        std_dev = libm::sqrt(var as f64);
	}
	Some(Stats{ min, p_25, median, p_75, max, mean, std_dev })
}

pub struct Stats {
	pub min: u64,
	pub p_25: u64,
	pub median: u64,
	pub p_75: u64,
	pub max: u64, 
	pub mean: u64,
	pub std_dev: f64,
}
