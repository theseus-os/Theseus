#![no_std]

extern crate alloc;
extern crate task;
extern crate memory;
extern crate apic;
extern crate hpet;
extern crate runqueue;
extern crate pmu_x86;
extern crate libm;
#[macro_use] extern crate log;
extern crate hashbrown;

use hpet::get_hpet;
use pmu_x86::{Counter, EventType};
use alloc::vec::Vec;
use hashbrown::HashMap;
use core::fmt;
use apic::get_lapics;

const MICRO_TO_FEMTO: u64 = 1_000_000_000;
const NANO_TO_FEMTO: u64 = 1_000_000;

/// Helper function to convert ticks to nano seconds
pub fn hpet_2_ns(hpet: u64) -> u64 {
	let hpet_period = get_hpet().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / NANO_TO_FEMTO
}

/// Helper function to convert ticks to micro seconds
pub fn hpet_2_us(hpet: u64) -> u64 {
	let hpet_period = get_hpet().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / MICRO_TO_FEMTO
}

#[macro_export]
macro_rules! CPU_ID {
	() => (apic::current_cpu())
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


/// Helper function to pick a free child core if possible
pub fn pick_free_core() -> Result<u8, &'static str> {
	// a free core will only have 1 task, the idle task, running on it.
	const NUM_TASKS_ON_FREE_CORE: usize = 1;

	// try with current core -1
	let child_core: u8 = CPU_ID!() as u8 - 1;
	if nr_tasks_in_rq(child_core) == Some(NUM_TASKS_ON_FREE_CORE) {return Ok(child_core);}

	// if failed, iterate through all cores
	for lapic in get_lapics().iter() {
		let child_core = lapic.0;
		if nr_tasks_in_rq(*child_core) == Some(1) {return Ok(*child_core);}
	}

	warn!("Cannot pick a free core because cores are busy");
	Err("Cannot pick a free core because cores are busy")
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
	counter.end()?;
	Ok(count)
}

pub const THRESHOLD_ERROR_RATIO: u64 = 1;

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

	let overhead = overhead_sum / TRIES as u64;
	let err = (overhead * 10 + overhead * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - overhead > err || overhead - min > err {
		warn!("cycle_count_overhead diff is too big: {} ({} - {}) ctr", max-min, max, min);
	}
	info!("cycle counts timing overhead is {} cycles", overhead);
	Ok(overhead)
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


/// Measures the overhead of using the hpet timer. 
/// Calls `timing_overhead_inner` multiple times and averages the value. 
/// Overhead is a count value. It is not time. 
pub fn hpet_timing_overhead() -> Result<u64, &'static str> {
	const TRIES: u64 = 10;
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;

	for _ in 0..TRIES {
		let overhead = hpet_timing_overhead_inner()?;
		tries += overhead;
		if overhead > max {max = overhead;}
		if overhead < min {min = overhead;}
	}

	let overhead = tries / TRIES as u64;
	let err = (overhead * 10 + overhead * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - overhead > err || overhead - min > err {
		warn!("hpet_timing_overhead diff is too big: {} ({} - {}) ctr", max-min, max, min);
	}
	info!("HPET timing overhead is {} ticks", overhead);
	Ok(overhead)
}

/// Internal function that actually calculates timer overhead. 
/// Calls the timing instruction multiple times and averages the value. 
/// Overhead is a count value. It is not time. 
fn hpet_timing_overhead_inner() -> Result<u64, &'static str> {
	const ITERATIONS: usize = 10_000;
	let mut _start_hpet_tmp: u64;
	let start_hpet: u64;
	let end_hpet: u64;
    let hpet = get_hpet().ok_or("couldn't get HPET timer")?;

	// to warm cache and remove error
	_start_hpet_tmp = hpet.get_counter();

	start_hpet = hpet.get_counter();
	for _ in 0..ITERATIONS {
		_start_hpet_tmp = hpet.get_counter();
	}
	end_hpet = hpet.get_counter();

	let delta_hpet = end_hpet - start_hpet;
	let delta_hpet_avg = delta_hpet / ITERATIONS as u64;

	Ok(delta_hpet_avg)
}

/// Helper function to calculate statistics of a provided dataset
pub fn calculate_stats(vec: &Vec<u64>) -> Option<Stats>{
	let mean;
  	let median;
	let mode;
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
		let sum: u64 = vec.iter().sum();
		mean = sum as f64 / len as f64;
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
		let mut diff_sum: f64 = 0.0;
      	for &val in vec {
			let x = val as f64; 
			if x > mean {
				diff_sum = diff_sum + ((x - mean)*(x - mean));
			}
			else {
				diff_sum = diff_sum + ((mean - x)*(mean - x));
			}
      	}

    	var = (diff_sum) / (len as f64);
        std_dev = libm::sqrt(var);
	}

	{ // calculate mode
		let mut values: HashMap<u64,usize> = HashMap::with_capacity(len);
		for val in vec {
			values.entry(*val).and_modify(|v| {*v += 1}).or_insert(1);
		}
		mode = *values.iter().max_by(|(_k1,v1), (_k2,v2)| v1.cmp(v2)).unwrap().0; // safe to call unwrap since we've already checked if the vector is empty
	}
	
	Some(Stats{ min, p_25, median, p_75, max, mode, mean, std_dev })
}

pub struct Stats {
	pub min: 	u64,
	pub p_25: 	u64,
	pub median: u64,
	pub p_75:	u64,
	pub max: 	u64, 
	pub mode: 	u64,
	pub mean: 	f64,
	pub std_dev: f64,
}

impl fmt::Debug for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stats \n 
        min:     {} \n 
        p_25:    {} \n 
        median:  {} \n 
        p_75:    {} \n 
        max:     {} \n 
        mode:    {} \n 
        mean:    {} \n 
        std_dev: {} \n", 
        self.min, self.p_25, self.median, self.p_75, self.max, self.mode, self.mean, self.std_dev)
    }
}
