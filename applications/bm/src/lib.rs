//! A collection of micro-benchmarks for Theseus. 
//! They include null syscall, context switching, process creation, memory mapping, IPC and file system benchmarks.
//! 
//! To run the memory mapping benchmark, Theseus should be compiled with the "bm_map" configuration option.
//! To run the IPC benchmarks, Theseus should be compiled with the "bm_ipc" configuration option.
//! For IPC measurements in cycles, a PMU should be available on the test machine.
//! When running on QEMU, the PMU can be made available by enabling KVM.

#![no_std]

#[macro_use] extern crate alloc;
extern crate task;
extern crate hpet;
#[macro_use] extern crate app_io;
// #[macro_use] extern crate log;
extern crate fs_node;
extern crate apic;
extern crate spawn;
extern crate path;
extern crate runqueue;
extern crate heapfile;
extern crate scheduler;
extern crate libtest;
extern crate memory;
extern crate rendezvous;
extern crate async_channel;
extern crate simple_ipc;
extern crate getopts;
extern crate pmu_x86;
extern crate mod_mgmt;

use core::str;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use hpet::get_hpet;
use heapfile::HeapFile;
use path::Path;
use fs_node::{DirRef, FileOrDir, FileRef};
use libtest::*;
use memory::{create_mapping, PteFlags};
use getopts::Options;
use mod_mgmt::crate_name_from_path;

const SEC_TO_NANO: u64 = 1_000_000_000;
const SEC_TO_MICRO: u64 = 1_000_000;
const MB: u64 = 1024 * 1024;
const KB: u64 = 1024;

const ITERATIONS: usize = 10_000;
const TRIES: usize = 10;

const READ_BUF_SIZE: usize = 64*1024;
const WRITE_BUF_SIZE: usize = 1024*1024;
const WRITE_BUF: [u8; WRITE_BUF_SIZE] = [65; WRITE_BUF_SIZE];

#[cfg(bm_in_us)]
const T_UNIT: &str = "micro sec";

#[cfg(not(bm_in_us))]
const T_UNIT: &str = "nano sec";


macro_rules! printlninfo {
	($fmt:expr) => (println!(concat!("BM-INFO: ", $fmt)));
	($fmt:expr, $($arg:tt)*) => (println!(concat!("BM-INFO: ", $fmt), $($arg)*));
}

macro_rules! printlnwarn {
	($fmt:expr) => (println!(concat!("BM-WARN: ", $fmt)));
	($fmt:expr, $($arg:tt)*) => (println!(concat!("BM-WARN: ", $fmt), $($arg)*));
}


pub fn main(args: Vec<String>) -> isize {
	let prog = get_prog_name();

	let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    opts.optflag("", "null", "null syscall");
    opts.optflag("", "ctx", "inter-thread context switching overhead");
    opts.optflag("", "spawn", "process creation");
    opts.optflag("", "memory_map", "create and destroy a memory mapping");
    opts.optflag("", "ipc", "1-byte IPC round trip time. Need to specify channel type ('a' or 'r')");
    opts.optflag("", "simple_ipc", "1-byte IPC round trip time with the simple ipc implementation");
    opts.optflag("", "fs_read_with_open", "file read including open");
    opts.optflag("", "fs_read_only", "file read");
    opts.optflag("", "fs_create", "file create");
    opts.optflag("", "fs_delete", "file delete");
    opts.optflag("", "fs", "test code for checking FS' ability");

    opts.optflag("a", "async", "Run IPC bm for the async channel");
    opts.optflag("r", "rendezvous", "Run IPC bm for the rendezvous channel");
    opts.optflag("p", "pinned", "Sender and Receiver should be pinned to the same core in the IPC bm");
    opts.optflag("b", "blocking", "Sender and Receiver should use blocking versions in the async IPC bm");
    opts.optflag("c", "cycles", "Measure the IPC times in reference cycles (need to have a PMU for this option)");


    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

	if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

	if !check_myrq() {
		printlninfo!("{} cannot run on a busy core (#{}). Pin me on an idle core.", prog, CPU_ID!());
		return 0;
	}

	// store flags for ipc
	let pinned = if matches.opt_present("p") {
		print!("PINNED ");
		true
	} else {
		false
	};

	let blocking = if matches.opt_present("b") {
		print!("BLOCKING ");
		true
	} else {
		false
	};

	let cycles = if matches.opt_present("c") {
		print!("(cycles) ");
		true
	} else {
		false
	};

	let res = if matches.opt_present("null") {
			do_null()
		} else if matches.opt_present("spawn") {
			do_spawn()
		} else if matches.opt_present("ctx") {
			do_ctx()
		} else if matches.opt_present("memory_map") {
			if cfg!(bm_map) {
				do_memory_map()
			} else {
				Err("Need to enable bm_map config option to run the memory_map benchmark")
			}
		} else if matches.opt_present("ipc") {
			if cfg!(not(bm_ipc)) {
				Err("Need to enable bm_ipc config option to run the IPC benchmark")
			} else {
				if matches.opt_present("r") {
					println!("RENDEZVOUS IPC");
					do_ipc_rendezvous(pinned, cycles)
				} else if matches.opt_present("a") {
					println!("ASYNC IPC");
					do_ipc_async(pinned, blocking, cycles)
				} else {
					Err("Specify channel type to use")
				}
			}
		} else if matches.opt_present("simple_ipc") {
			if cfg!(not(bm_ipc)) {
				Err("Need to enable bm_ipc config option to run the IPC benchmark")
			} else {
				println!("SIMPLE IPC");
				do_ipc_simple(pinned, cycles)
			}
		} else if matches.opt_present("fs_read_with_open") {
			do_fs_read(true /*with_open*/)
		} else if matches.opt_present("fs_read_only") {
			do_fs_read(false /*with_open*/)
		} else if matches.opt_present("fs_create") {
			do_fs_create_del()
		} else if matches.opt_present("fs_delete") {
			do_fs_delete()
		} else if matches.opt_present("fs") {
			do_fs_cap_check()
		} else {
			printlnwarn!("Unknown command: {}", args[0]);
			print_usage(opts);
        	Err("Unknown command")
		};

	match res {
		Ok(()) => return 0,
		Err(e) => {
			println!("Error in completing benchmark: {:?}", e);
			return -1;
		}
	}
}


/// Measures the time for null syscall. 
/// Calls `do_null_inner` multiple times and averages the value. 
fn do_null() -> Result<(), &'static str> {
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);

	let overhead_ct = hpet_timing_overhead()?;
	print_header(TRIES, ITERATIONS*1000);

	for i in 0..TRIES {
		let lat = do_null_inner(overhead_ct, i+1, TRIES)?;

		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}
	
	let lat = tries / TRIES as u64;
	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("null_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;
	
	printlninfo!("NULL result: ({})", T_UNIT);
	printlninfo!("{:?}", stats);
	printlninfo!("This test is equivalent to `lat_syscall null` in LMBench");
	Ok(())
}

/// Internal function that actually calculates the time for null syscall.
/// Measures this by calling `get_my_current_task_id` of the current task. 
fn do_null_inner(overhead_ct: u64, th: usize, nr: usize) -> Result<u64, &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let mut mypid = core::usize::MAX;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	// Since this test takes very little time we multiply the default iterations by 1000
	let tmp_iterations = ITERATIONS *1000;

	start_hpet = hpet.get_counter();
	for _ in 0..tmp_iterations {
		mypid = task::get_my_current_task_id();
	}
	end_hpet = hpet.get_counter();

	let mut delta_hpet: u64 = end_hpet - start_hpet;
	if delta_hpet < overhead_ct { // Erroneous case
		printlnwarn!("Ignore overhead for null because overhead({}) > diff({})", overhead_ct, delta_hpet);
	} else {
		delta_hpet -= overhead_ct;
	}
	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / (tmp_iterations as u64);

	printlninfo!("null_test_inner ({}/{}): hpet {} , overhead {}, {} total_time -> {} {} (ignore: {})",
		th, nr, delta_hpet, overhead_ct, delta_time, delta_time_avg, T_UNIT, mypid);

	Ok(delta_time_avg)
}

/// Measures the time to spawn an application. 
/// Calls `do_spawn_inner` multiple times and averages the value. 
fn do_spawn() -> Result<(), &'static str>{
	let child_core = match pick_free_core() {
		Ok(child_core) => { 
			printlninfo!("core_{} is idle, so my children will play on it.", child_core); 
			child_core
		}
		_ => {
			printlnwarn!("Cannot conduct spawn test because cores are busy");
			return Err("Cannot conduct spawn test because cores are busy");
		}
	};

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);

	let overhead_ct = hpet_timing_overhead()?;
	print_header(TRIES, ITERATIONS);
	
	for i in 0..TRIES {
		let lat = do_spawn_inner(overhead_ct, i+1, TRIES, child_core)?;

		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;

	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("spawn_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;

	printlninfo!("SPAWN result: ({})", T_UNIT);
	printlninfo!("{:?}", stats);
	printlninfo!("This test is equivalent to `lat_proc exec` in LMBench");

	Ok(())
}

/// Internal function that actually calculates the time to spawn an application.
/// Measures this by using `TaskBuilder` to spawn a application task.
fn do_spawn_inner(overhead_ct: u64, th: usize, nr: usize, _child_core: u8) -> Result<u64, &'static str> {
    let mut start_hpet: u64;
	let mut end_hpet: u64;
	let mut delta_hpet = 0;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	// Get path to application "hello" that we're going to spawn
	let namespace = task::with_current_task(|t| t.get_namespace().clone())
		.map_err(|_| "could not find the application namespace")?;
	let namespace_dir = namespace.dir();
	let app_path = namespace_dir.get_file_starting_with("hello-")
		.map(|f| Path::new(f.lock().get_absolute_path()))
		.ok_or("Could not find the application 'hello'")?;
	let crate_name = crate_name_from_path(&app_path).to_string();

	// here we are taking the time at every iteration. 
	// otherwise the crate is not fully unloaded from the namespace before the next iteration starts 
	// so it cannot be loaded again and we are returned an error.
	let iterations = 100;
	for _ in 0..iterations{
		while namespace.get_crate(&crate_name).is_some() {  }

		start_hpet = hpet.get_counter();
		let child = spawn::new_application_task_builder(app_path.clone(), None)?
	        .spawn()?;

	    child.join()?;
	    end_hpet = hpet.get_counter();
		delta_hpet += end_hpet - start_hpet - overhead_ct;		
	}

    let delta_time = hpet_2_time("", delta_hpet);
    let delta_time_avg = delta_time / iterations as u64;
	printlninfo!("spawn_test_inner ({}/{}): hpet {} , overhead {}, {} total_time -> {} {}",
		th, nr, delta_hpet, overhead_ct, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}


/// Measures the time to switch between two kernel threads. 
/// Calls `do_ctx_inner` multiple times to perform the actual operation
fn do_ctx() -> Result<(), &'static str> {
	let child_core = match pick_free_core() {
		Ok(child_core) => { 
			printlninfo!("core_{} is idle, so my children will play on it.", child_core); 
			child_core
		}
		_ => {
			printlnwarn!("Cannot conduct ctx test because cores are busy");
			return Err("Cannot conduct ctx test because cores are busy");
		}
	};

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);
	
	print_header(TRIES, ITERATIONS*1000*2);

	for i in 0..TRIES {
		let lat = do_ctx_inner(i+1, TRIES, child_core)?;
	
		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;

	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("ctx_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;

	printlninfo!("Context switch result: ({})", T_UNIT);
	printlninfo!("{:?}", stats);
	printlninfo!("This test does not have an equivalent test in LMBench");

	Ok(())
}

/// Internal function that actually calculates the time to context switch between two threads.
/// This is measured by creating two tasks and pinning them to the same core.
/// The tasks yield to each other repetitively.
/// Overhead is measured by doing the above operation with two tasks that just return.
fn do_ctx_inner(th: usize, nr: usize, child_core: u8) -> Result<u64, &'static str> {
    let start_hpet: u64;
	let end_hpet: u64;
	let overhead_end_hpet: u64;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	// we first spawn two tasks to get the overhead of creating and joining 2 tasks
	// we will subtract this time from the total time so that we are left with the actual time to context switch
	start_hpet = hpet.get_counter();

		let taskref3 = spawn::new_task_builder(overhead_task ,1)
			.name(String::from("overhead_task_1"))
			.pin_on_core(child_core)
			.spawn()?;

		let taskref4 = spawn::new_task_builder(overhead_task ,2)
			.name(String::from("overhead_task_2"))
			.pin_on_core(child_core)
			.spawn()?;

		taskref3.join()?;
		taskref4.join()?;

	overhead_end_hpet = hpet.get_counter();

	// we then spawn them with yielding enabled

		let taskref1 = spawn::new_task_builder(yield_task ,1)
			.name(String::from("yield_task_1"))
			.pin_on_core(child_core)
			.spawn()?;

		let taskref2 = spawn::new_task_builder(yield_task ,2)
			.name(String::from("yield_task_2"))
			.pin_on_core(child_core)
			.spawn()?;

		taskref1.join()?;
		taskref2.join()?;

    end_hpet = hpet.get_counter();

    let delta_overhead = overhead_end_hpet - start_hpet;
	let delta_hpet = end_hpet - overhead_end_hpet - delta_overhead;
    let delta_time = hpet_2_time("", delta_hpet);
	let overhead_time = hpet_2_time("", delta_overhead);
    let delta_time_avg = delta_time / (ITERATIONS*1000*2) as u64; //*2 because each thread yields ITERATION number of times
	printlninfo!("ctx_switch_test_inner ({}/{}): total_overhead -> {} {} , {} total_time -> {} {}",
		th, nr, overhead_time, T_UNIT, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

/// Measures the time to create and destroy a mapping. 
/// Calls `do_memory_map_inner` multiple times to perform the actual operation
fn do_memory_map() -> Result<(), &'static str> {
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);

	let overhead_ct = hpet_timing_overhead()?;
	print_header(TRIES, ITERATIONS);

	for i in 0..TRIES {
		let lat = do_memory_map_inner(overhead_ct, i+1, TRIES)?;

		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}
	
	let lat = tries / TRIES as u64;
	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("memory_map_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;
	
	printlninfo!("MEMORY MAP result: ({})", T_UNIT);
	printlninfo!("{:?}", stats);
	printlninfo!("This test is equivalent to `lat_mmap` in LMBench");
	Ok(())
}

/// Internal function that actually calculates the time to create and destroy a memory mapping.
/// Measures this by continually allocating and dropping `MappedPages`.
fn do_memory_map_inner(overhead_ct: u64, th: usize, nr: usize) -> Result<u64, &'static str> {
	const MAPPING_SIZE: usize = 4096;

    let start_hpet: u64;
	let end_hpet: u64;
	let delta_hpet: u64;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	start_hpet = hpet.get_counter();

	for _ in 0..ITERATIONS{
		let mapping = create_mapping(MAPPING_SIZE, PteFlags::new().writable(true))?;
		// write 0xFF to the first byte as lmbench does
		unsafe{ *(mapping.start_address().value() as *mut u8)  = 0xFF; }
		drop(mapping);
	}

	end_hpet = hpet.get_counter();

	delta_hpet = end_hpet - start_hpet - overhead_ct;
    let delta_time = hpet_2_time("", delta_hpet);
    let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("memory_map_test_inner ({}/{}): hpet {} , overhead {}, {} total_time -> {} {}",
		th, nr, delta_hpet, overhead_ct, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

/// Measures the round trip time to send a 1-byte message on a rendezvous channel. 
/// Calls `do_ipc_rendezvous_inner` multiple times to perform the actual operation
fn do_ipc_rendezvous(pinned: bool, cycles: bool) -> Result<(), &'static str> {
	let child_core = if pinned {
		Some(CPU_ID!())
	} else {
		None
	};

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);

	print_header(TRIES, ITERATIONS);

	for i in 0..TRIES {
		let lat = if cycles {
			do_ipc_rendezvous_inner_cycles(i+1, TRIES, child_core)?	
		} else {
			do_ipc_rendezvous_inner(i+1, TRIES, child_core)?	
		};
		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;

	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("ipc_rendezvous_test diff is too big: {} ({} - {})", max-min, max, min);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;

	if cycles {
		printlninfo!("IPC RENDEZVOUS result: Round Trip Time: (cycles)",);
	} else {
		printlninfo!("IPC RENDEZVOUS result: Round Trip Time: ({})", T_UNIT);
	}

	printlninfo!("{:?}", stats);
	printlninfo!("This test does not have an equivalent test in LMBench");

	Ok(())
}

/// Internal function that actually calculates the round trip time to send a message between two threads.
/// This is measured by creating a child task, and sending messages between the parent and the child.
/// Overhead is measured by creating a task that just returns.
fn do_ipc_rendezvous_inner(th: usize, nr: usize, child_core: Option<u8>) -> Result<u64, &'static str> {
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	// we first spawn one task to get the overhead of creating and joining the task
	// we will subtract this time from the total time so that we are left with the actual time for IPC

	let start = hpet.get_counter();

		let taskref3;

		if let Some(core) = child_core {		
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.spawn()?;
		}
		
		taskref3.join()?;

	let overhead = hpet.get_counter();

		// we then create the sender and receiver endpoints for the 2 tasks
		let (sender1, receiver1) = rendezvous::new_channel();
		let (sender2, receiver2) = rendezvous::new_channel();
		
		let taskref1;

		//then we spawn the child task
		if let Some(core) = child_core {		
			taskref1 = spawn::new_task_builder(rendezvous_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref1 = spawn::new_task_builder(rendezvous_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.spawn()?;
		}

		// then we initiate IPC betweeen the parent and child tasks
		rendezvous_task_receiver((sender2, receiver1));

		taskref1.join()?;

	let end = hpet.get_counter();

	let delta_overhead = overhead - start;
	let delta_hpet = end - overhead - delta_overhead;
	let delta_time = hpet_2_time("", delta_hpet);
	let overhead_time = hpet_2_time("", delta_overhead);
	let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("ipc_rendezvous_inner ({}/{}): total_overhead -> {} {} , {} total_time -> {} {}",
		th, nr, overhead_time, T_UNIT, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

/// Internal function that actually calculates the round trip time to send a message between two threads.
/// This is measured by creating a child task, and sending messages between the parent and the child.
/// Overhead is measured by creating a task that just returns.
fn do_ipc_rendezvous_inner_cycles(th: usize, nr: usize, child_core: Option<u8>) -> Result<u64, &'static str> {
	pmu_x86::init()?;
	let mut counter = start_counting_reference_cycles()?;

	// we first spawn one task to get the overhead of creating and joining the task
	// we will subtract this time from the total time so that we are left with the actual time for IPC

	counter.start()?;

		let taskref3;

		if let Some(core) = child_core {		
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.spawn()?;
		}
		
		taskref3.join()?;

	let overhead = counter.diff();
	counter.start()?;

		// we then create the sender and receiver endpoints for the 2 tasks
		let (sender1, receiver1) = rendezvous::new_channel();
		let (sender2, receiver2) = rendezvous::new_channel();
		
		let taskref1;

		//then we spawn the child task
		if let Some(core) = child_core {		
			taskref1 = spawn::new_task_builder(rendezvous_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref1 = spawn::new_task_builder(rendezvous_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.spawn()?;
		}

		// then we initiate IPC betweeen the parent and child tasks
		rendezvous_task_receiver((sender2, receiver1));

		taskref1.join()?;

	let end = counter.end()?;


	let delta_overhead = overhead;
	let delta_cycles = end - delta_overhead;
	let delta_cycles_avg = delta_cycles / ITERATIONS as u64;
	printlninfo!("ipc_rendezvous_inner ({}/{}): total_overhead -> {} cycles , {} total_time -> {} cycles",
		th, nr, delta_overhead, delta_cycles, delta_cycles_avg);

	Ok(delta_cycles_avg)
}

/// A task which sends and then receives a message for a number of iterations
fn rendezvous_task_sender((sender, receiver): (rendezvous::Sender<u8>, rendezvous::Receiver<u8>)) {
	let mut msg = 0;
    for _ in 0..ITERATIONS{
		sender.send(msg).expect("Rendezvous task: could not send message!");
        msg = receiver.receive().expect("Rendezvous task: could not receive message");
    }
}

/// A task which receives and then sends a message for a number of iterations
fn rendezvous_task_receiver((sender, receiver): (rendezvous::Sender<u8>, rendezvous::Receiver<u8>)) {
	let mut msg;
    for _ in 0..ITERATIONS{
		msg = receiver.receive().expect("Rendezvous task: could not receive message");
		sender.send(msg).expect("Rendezvous task: could not send message!");
    }
}

/// Measures the round trip time to send a 1-byte message on an async channel. 
/// Calls `do_ipc_async_inner` multiple times to perform the actual operation
fn do_ipc_async(pinned: bool, blocking: bool, cycles: bool) -> Result<(), &'static str> {
	let child_core = if pinned {
		Some(CPU_ID!())
	} else {
		None
	};

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);

	print_header(TRIES, ITERATIONS);

	for i in 0..TRIES {
		let lat = if cycles {
			do_ipc_async_inner_cycles(i+1, TRIES, child_core, blocking)?	
		} else {
			do_ipc_async_inner(i+1, TRIES, child_core, blocking)?
		};
		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;

	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("ipc_async_test diff is too big: {} ({} - {})", max-min, max, min);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;

	if cycles {
		printlninfo!("IPC ASYNC result: Round Trip Time: (cycles)",);
	} else {
		printlninfo!("IPC ASYNC result: Round Trip Time: ({})", T_UNIT);
	}
	printlninfo!("{:?}", stats);
	printlninfo!("This test is equivalent to `lat_pipe` in LMBench when run with the pinned flag enabled");

	Ok(())
}

/// Internal function that actually calculates the round trip time to send a message between two threads.
/// This is measured by creating a child task, and sending messages between the parent and child.
/// Overhead is measured by creating a task that just returns.
fn do_ipc_async_inner(th: usize, nr: usize, child_core: Option<u8>, blocking: bool) -> Result<u64, &'static str> {
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	let (sender_task, receiver_task): (fn((async_channel::Sender<u8>, async_channel::Receiver<u8>)), fn((async_channel::Sender<u8>, async_channel::Receiver<u8>))) = if blocking {
		(async_task_sender, async_task_receiver)
	} else {
		(async_task_sender_nonblocking, async_task_receiver_nonblocking)
	};

	// we first spawn one task to get the overhead of creating and joining the task
	// we will subtract this time from the total time so that we are left with the actual time for IPC

	let start = hpet.get_counter();

		let taskref3;

		if let Some(core) = child_core {		
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.spawn()?;
		}
		
		taskref3.join()?;

	let overhead = hpet.get_counter();

		// We then create the sender and receiver endpoints for the 2 tasks.
		// The capacity of the channels is set to match the capacity of pipes in Linux
		// which is 16 4 KiB-pages, or 65,536 bytes.
		const CAPACITY: usize = 65536;

		let (sender1, receiver1) = async_channel::new_channel(CAPACITY);
		let (sender2, receiver2) = async_channel::new_channel(CAPACITY);
		
		let taskref1;

		if let Some(core) = child_core {		
			taskref1 = spawn::new_task_builder(sender_task, (sender1, receiver2))
				.name(String::from("sender"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref1 = spawn::new_task_builder(sender_task, (sender1, receiver2))
				.name(String::from("sender"))
				.spawn()?;
		}

		// then we initiate IPC betweeen the parent and child tasks
		receiver_task((sender2, receiver1));

		taskref1.join()?;

	let end = hpet.get_counter();

	let delta_overhead = overhead - start;
	let delta_hpet = end - overhead - delta_overhead;
	let delta_time = hpet_2_time("", delta_hpet);
	let overhead_time = hpet_2_time("", delta_overhead);
	let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("ipc_rendezvous_inner ({}/{}): total_overhead -> {} {} , {} total_time -> {} {}",
		th, nr, overhead_time, T_UNIT, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

/// Internal function that actually calculates the round trip time to send a message between two threads.
/// This is measured by creating a child task, and sending messages between the parent and child.
/// Overhead is measured by creating a task that just returns.
fn do_ipc_async_inner_cycles(th: usize, nr: usize, child_core: Option<u8>, blocking: bool) -> Result<u64, &'static str> {
	pmu_x86::init()?;
	let mut counter = start_counting_reference_cycles()?;

	let (sender_task, receiver_task): (fn((async_channel::Sender<u8>, async_channel::Receiver<u8>)), fn((async_channel::Sender<u8>, async_channel::Receiver<u8>))) = if blocking {
		(async_task_sender, async_task_receiver)
	} else {
		(async_task_sender_nonblocking, async_task_receiver_nonblocking)
	};

	// we first spawn one task to get the overhead of creating and joining the task
	// we will subtract this time from the total time so that we are left with the actual time for IPC

	counter.start()?;

		let taskref3;

		if let Some(core) = child_core {		
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.spawn()?;
		}
		
		taskref3.join()?;

	let overhead = counter.diff();
	counter.start()?;

		// We then create the sender and receiver endpoints for the 2 tasks.
		// The capacity of the channels is set to match the capacity of pipes in Linux
		// which is 16 4 KiB-pages, or 65,536 bytes.
		const CAPACITY: usize = 65536;

		let (sender1, receiver1) = async_channel::new_channel(CAPACITY);
		let (sender2, receiver2) = async_channel::new_channel(CAPACITY);
		
		let taskref1;

		if let Some(core) = child_core {		
			taskref1 = spawn::new_task_builder(sender_task, (sender1, receiver2))
				.name(String::from("sender"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref1 = spawn::new_task_builder(sender_task, (sender1, receiver2))
				.name(String::from("sender"))
				.spawn()?;
		}

		// then we initiate IPC betweeen the parent and child tasks
		receiver_task((sender2, receiver1));

		taskref1.join()?;

	let end = counter.end()?;

	let delta_overhead = overhead;
	let delta_cycles = end - delta_overhead;
	let delta_cycles_avg = delta_cycles / ITERATIONS as u64;
	printlninfo!("ipc_rendezvous_inner ({}/{}): total_overhead -> {} cycles , {} total_time -> {} cycles",
		th, nr, delta_overhead, delta_cycles, delta_cycles_avg);

	Ok(delta_cycles_avg)
}

/// A task which sends and then receives a message for a number of iterations
fn async_task_sender((sender, receiver): (async_channel::Sender<u8>, async_channel::Receiver<u8>)) {
	let mut msg = 0;
    for _ in 0..ITERATIONS{
		sender.send(msg).expect("async channel task: could not send message!");
        msg = receiver.receive().expect("async channel task: could not receive message");
    }
}

/// A task which receives and then sends a message for a number of iterations
fn async_task_receiver((sender, receiver): (async_channel::Sender<u8>, async_channel::Receiver<u8>)) {
	let mut msg;
    for _ in 0..ITERATIONS{
		msg = receiver.receive().expect("async channel task: could not receive message");
		sender.send(msg).expect("async channel task: could not send message!");
    }
}

/// A task which sends and then receives a message for a number of iterations
fn async_task_sender_nonblocking((sender, receiver): (async_channel::Sender<u8>, async_channel::Receiver<u8>)) {
	let mut msg = Ok(0);
    for _ in 0..ITERATIONS{
		while sender.try_send(*msg.as_ref().unwrap()).is_err() {}
        msg = receiver.try_receive();
		while msg.is_err() {
        	msg = receiver.try_receive();
		}
    }
}

/// A task which receives and then sends a message for a number of iterations
fn async_task_receiver_nonblocking((sender, receiver): (async_channel::Sender<u8>, async_channel::Receiver<u8>)) {
	let mut msg;
    for _ in 0..ITERATIONS{
		msg = receiver.try_receive();
		while msg.is_err() {
        	msg = receiver.try_receive();
		}
		while sender.try_send(*msg.as_ref().unwrap()).is_err() {}
    }
}

/// Measures the round trip time to send a 1-byte message on a simple channel. 
/// Calls `do_ipc_simple_inner` multiple times to perform the actual operation
fn do_ipc_simple(pinned: bool, cycles: bool) -> Result<(), &'static str> {
	let child_core = if pinned {
		Some(CPU_ID!())
	} else {
		None
	};

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::with_capacity(TRIES);
	
	print_header(TRIES, ITERATIONS);

	for i in 0..TRIES {
		let lat = if cycles {
			do_ipc_simple_inner_cycles(i+1, TRIES, child_core)?
		} else {
			do_ipc_simple_inner(i+1, TRIES, child_core)?
		};

		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;

	// We expect the maximum and minimum to be within 10*THRESHOLD_ERROR_RATIO % of the mean value
	let err = (lat * 10 * THRESHOLD_ERROR_RATIO) / 100;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("ipc_simple_test diff is too big: {} ({} - {})", max-min, max, min);
	}
	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;

	if cycles {
		printlninfo!("IPC SIMPLE result: Round Trip Time: (cycles)");
	} else {
		printlninfo!("IPC SIMPLE result: Round Trip Time: ({})", T_UNIT);
	}

	printlninfo!("{:?}", stats);
	printlninfo!("This test does not have an equivalent test in LMBench");

	Ok(())
}

/// Internal function that actually calculates the round trip time to send a message between two threads.
/// This is measured by creating a child task, and sending messages between the parent and child.
/// Overhead is measured by creating a tasks that just returns.
fn do_ipc_simple_inner(th: usize, nr: usize, child_core: Option<u8>) -> Result<u64, &'static str> {
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	// we first spawn one task to get the overhead of creating and joining the task
	// we will subtract this time from the total time so that we are left with the actual time for IPC

	let start = hpet.get_counter();

		let taskref3;

		if let Some(core) = child_core {		
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.spawn()?;
		}
		
		taskref3.join()?;

	let overhead = hpet.get_counter();

		// we then create the sender and receiver endpoints for the 2 tasks
		let (sender1, receiver1) = simple_ipc::new_channel();
		let (sender2, receiver2) = simple_ipc::new_channel();
		
		let taskref1;

		if let Some(core) = child_core {		
			taskref1 = spawn::new_task_builder(simple_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref1 = spawn::new_task_builder(simple_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.spawn()?;
		}


		// then we initiate IPC betweeen the parent and child tasks
		simple_task_receiver((sender2, receiver1));

		taskref1.join()?;

	let end = hpet.get_counter();

	let delta_overhead = overhead - start;
	let delta_hpet = end - overhead - delta_overhead;
	let delta_time = hpet_2_time("", delta_hpet);
	let overhead_time = hpet_2_time("", delta_overhead);
	let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("ipc_rendezvous_inner ({}/{}): total_overhead -> {} {} , {} total_time -> {} {}",
		th, nr, overhead_time, T_UNIT, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

/// Internal function that actually calculates the round trip time to send a message between two threads.
/// This is measured by creating a child task, and sending messages between the parent and child.
/// Overhead is measured by creating a tasks that just returns.
fn do_ipc_simple_inner_cycles(th: usize, nr: usize, child_core: Option<u8>) -> Result<u64, &'static str> {
	pmu_x86::init()?;
	let mut counter = start_counting_reference_cycles()?;

	// we first spawn one task to get the overhead of creating and joining the task
	// we will subtract this time from the total time so that we are left with the actual time for IPC

		counter.start()?;

		let taskref3;

		if let Some(core) = child_core {		
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref3 = spawn::new_task_builder(overhead_task ,1)
				.name(String::from("overhead_task_1"))
				.spawn()?;
		}
		
		taskref3.join()?;

	let overhead = counter.diff();
	counter.start()?;

		// we then create the sender and receiver endpoints for the 2 tasks
		let (sender1, receiver1) = simple_ipc::new_channel();
		let (sender2, receiver2) = simple_ipc::new_channel();
		
		let taskref1;

		if let Some(core) = child_core {		
			taskref1 = spawn::new_task_builder(simple_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.pin_on_core(core)
				.spawn()?;
		} else {
			taskref1 = spawn::new_task_builder(simple_task_sender, (sender1, receiver2))
				.name(String::from("sender"))
				.spawn()?;
		}


		// then we initiate IPC betweeen the parent and child tasks
		simple_task_receiver((sender2, receiver1));

		taskref1.join()?;

	let end = counter.end()?;
		
	let delta_overhead = overhead;
	let delta_cycles = end - delta_overhead;
	let delta_cycles_avg = delta_cycles / ITERATIONS as u64;
	printlninfo!("ipc_rendezvous_inner ({}/{}): total_overhead -> {} cycles , {} total_time -> {} cycles",
		th, nr, delta_overhead, delta_cycles, delta_cycles_avg);

	Ok(delta_cycles_avg)
}

/// A task which sends and then receives a message for a number of iterations
fn simple_task_sender((sender, receiver): (simple_ipc::Sender, simple_ipc::Receiver)) {
	let mut msg = 0;
    for _ in 0..ITERATIONS{
		sender.send(msg);
        msg = receiver.receive();
    }
}

/// A task which receives and then sends a message for a number of iterations
fn simple_task_receiver((sender, receiver): (simple_ipc::Sender, simple_ipc::Receiver)) {
	let mut msg;
    for _ in 0..ITERATIONS{
		msg = receiver.receive();
		sender.send(msg);
    }
}


/// Wrapper function used to measure file read and file read with open. 
/// Accepts a bool argument. If true includes the latency to open a file
/// If false only measure the time to read from file.
/// Actual measuring is deferred to `do_fs_read_with_size` function
fn do_fs_read(with_open: bool) -> Result<(), &'static str>{
	let fsize_kb = 1024;
	printlninfo!("File size     : {} KB", fsize_kb);
	printlninfo!("Read buf size : {} KB", READ_BUF_SIZE / 1024);
	printlninfo!("========================================");

	let overhead_ct = hpet_timing_overhead()?;

	do_fs_read_with_size(overhead_ct, fsize_kb, with_open)?;
	if with_open {
		printlninfo!("This test is equivalent to `bw_file_rd open2close` in LMBench");
	} else {
		printlninfo!("This test is equivalent to `bw_file_rd io_only` in LMBench");
	}
	Ok(())
}

/// Internal function measure file read and read with open time.
/// Accepts `timing overhead`, `file size` and `with_open` bool parameter.
/// If `with_open` is true calls `do_fs_read_with_open_inner` to measure time to open and read.
/// If `with_open` is false calls `do_fs_read_only_inner` to measure time to read only.
fn do_fs_read_with_size(overhead_ct: u64, fsize_kb: usize, with_open: bool) -> Result<(), &'static str> {
	let mut tries: u64 = 0;
	let mut tries_mb: u64 = 0;
	let mut tries_kb: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let fsize_b = fsize_kb * KB as usize;
	let mut vec = Vec::new();

	let filename = format!("tmp_{}k.txt", fsize_kb);

	// we can use `mk_tmp_file()` because it is outside of the loop
	mk_tmp_file(&filename, fsize_b).expect("Cannot create a file");

	for i in 0..TRIES {
		let (lat, tput_mb, tput_kb) = if with_open {
			do_fs_read_with_open_inner(&filename, overhead_ct, i+1, TRIES).expect("Error in read_open inner()")
		} else {
			do_fs_read_only_inner(&filename, overhead_ct, i+1, TRIES).expect("Error in read_only inner()")
		};

		tries += lat;
		tries_mb += tput_mb;
		tries_kb += tput_kb;
		vec.push(tput_kb);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let stats = calculate_stats(&vec).ok_or("couldn't calculate stats")?;

	let lat = tries / TRIES as u64;
	let tput_mb = tries_mb / TRIES as u64;
	let tput_kb = tries_kb / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	print_header(TRIES, ITERATIONS);
	printlninfo!("{} for {} KB: {} {} {} MB/sec {} KB/sec", 
		if with_open {"READ WITH OPEN"} else {"READ ONLY"}, 
		fsize_kb, lat, T_UNIT, tput_mb, tput_kb);
	printlninfo!("{:?}", stats);
	Ok(())
}

/// Internal function that actually calculates the time for open and read a file.
/// This function opens a file and read the file and sums up the read charachters in each chunk.
/// This is performed to be compatible with `LMBench`
fn do_fs_read_with_open_inner(filename: &str, overhead_ct: u64, th: usize, nr: usize) -> Result<(u64, u64, u64), &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;


	let path = Path::new(filename.to_string());
	let mut _dummy_sum: u64 = 0;
	let mut buf = vec![0; READ_BUF_SIZE];
	let size = match get_file(filename) {
		Some(fileref) => {fileref.lock().len()}
		_ => {
			return Err("Cannot get the size");
		}
	} as i64;
	let mut unread_size = size;

	if unread_size % READ_BUF_SIZE as i64 != 0 {
		return Err("File size is not alligned");
	}

	start_hpet = hpet.get_counter();
	for _ in 0..ITERATIONS 	{
		let file_dir_enum = path.get(&get_cwd().unwrap()).expect("Cannot find file");
		match file_dir_enum {
            FileOrDir::File(fileref) => { 
            	let mut file = fileref.lock();	// so far, open()

            	unread_size = size;
            	while unread_size > 0 {	// now read()
                	// XXX: With the Current API, we cannot specify an offset. 
                	// But the API is coming soon. for now, pretend we have it
                	let nr_read = file.read_at(&mut buf,0).expect("Cannot read");
					unread_size -= nr_read as i64;

					// LMbench based on C does the magic to cast a type from char to int
					// But, we dont' have the luxury with type-safe Rust, so we do...
					_dummy_sum += buf.iter().fold(0 as u64, |acc, &x| acc + x as u64);
            	}

            }
            _ => {
				return Err("dir or does not exist");
			}
        }
	}
	end_hpet = hpet.get_counter();

	let delta_hpet = end_hpet - start_hpet - overhead_ct;
	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / ITERATIONS as u64;

	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let mb_per_sec = (size as u64 * to_sec) / (MB * delta_time_avg);	// prefer this
	let kb_per_sec = (size as u64 * to_sec) / (KB * delta_time_avg);

	printlninfo!("read_with_open_inner ({}/{}): {} total_time -> {} {} {} MB/sec {} KB/sec (ignore: {})",
		th, nr, delta_time, delta_time_avg, T_UNIT, mb_per_sec, kb_per_sec, _dummy_sum);

	Ok((delta_time_avg, mb_per_sec, kb_per_sec))
}

/// Internal function that actually calculates the time to read a file.
/// This function read the file and sums up the read charachters in each chunk.
/// This is performed to be compatible with `LMBench`
fn do_fs_read_only_inner(filename: &str, overhead_ct: u64, th: usize, nr: usize) -> Result<(u64, u64, u64), &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;


	let path = Path::new(filename.to_string());
	let _dummy_sum: u64 = 0;
	let mut buf = vec![0; READ_BUF_SIZE];
	let size = match get_file(filename) {
		Some(fileref) => {fileref.lock().len()}
		_ => {
			return Err("Cannot get the size");
		}
	} as i64;
	let mut unread_size = size;

	if unread_size % READ_BUF_SIZE as i64 != 0 {
		return Err("File size is not alligned");
	}

	let file_dir_enum = path.get(&get_cwd().unwrap()).expect("Cannot find file");
	match file_dir_enum {
        FileOrDir::File(fileref) => { 
        	let mut file = fileref.lock();	// so far, open()

			start_hpet = hpet.get_counter();
			for _ in 0..ITERATIONS 	{
				unread_size = size;
            	while unread_size > 0 {	// now read()
                	// XXX: With the Current API, we cannot specify an offset. 
                	// But the API is coming soon. for now, pretend we have it
                	let nr_read = file.read_at(&mut buf, 0).expect("Cannot read");
					unread_size -= nr_read as i64;

					// LMbench based on C does the magic to cast a type from char to int
					// But, we dont' have the luxury with type-safe Rust, so we do...
					// _dummy_sum += buf.iter().fold(0 as u64, |acc, &x| acc + x as u64);
            	}
			}	// for
			end_hpet = hpet.get_counter();

        }
        _ => {
			return Err("dir or does not exist");
		}
    }

	let delta_hpet = end_hpet - start_hpet - overhead_ct;
	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / ITERATIONS as u64;

	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let mb_per_sec = (size as u64 * to_sec) / (MB * delta_time_avg);	// prefer this
	let kb_per_sec = (size as u64 * to_sec) / (KB * delta_time_avg);

	printlninfo!("read_only_inner ({}/{}): {} total_time -> {} {} {} MB/sec {} KB/sec (ignore: {})",
		th, nr, delta_time, delta_time_avg, T_UNIT, mb_per_sec, kb_per_sec, _dummy_sum);

	Ok((delta_time_avg, mb_per_sec, kb_per_sec))
}

/// Measures the time to create and write to a file. 
/// Calls `do_fs_create_del_inner` multiple times to perform the actual operation
/// File sizes of 1K, 4K and 10K are measured in this function
fn do_fs_create_del() -> Result<(), &'static str> {
	// let	fsizes_b = [0 as usize, 1024, 4096, 10*1024];	// Theseus thinks creating an empty file is stupid (for memfs)
	let	fsizes_b = [1024_usize, 4096, 10*1024];
	// let	fsizes_b = [1024*1024];

	let overhead_ct = hpet_timing_overhead()?;

	print_header(TRIES, ITERATIONS);
	printlninfo!("SIZE(KB)    Iteration    created(files/s)     time(ns/file)");
	for fsize_b in fsizes_b.iter() {
		do_fs_create_del_inner(*fsize_b, overhead_ct)?;
	}
	printlninfo!("This test is equivalent to file create in `lat_fs` in LMBench");

	Ok(())
}

/// Internal function that actually calculates the time to create and write to a file.
/// Within the measurin section it creates a heap file and writes to it.
fn do_fs_create_del_inner(fsize_b: usize, overhead_ct: u64) -> Result<(), &'static str> {
	let mut filenames = vec!["".to_string(); ITERATIONS];
	let pid = getpid();
	let start_hpet_create: u64;
	let end_hpet_create: u64;
	let _start_hpet_del: u64;
	let _end_hpet_del: u64;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;


	// don't put these (populating files, checks, etc) into the loop to be timed
	// The loop must be doing minimal operations to exclude unnecessary overhead
	// populate filenames
	for i in 0..ITERATIONS {
		filenames[i] = format!("tmp_{}_{}_{}.txt", pid, fsize_b, i);
	}

	// check if we have enough data to write. We use just const data to avoid unnecessary overhead
	if fsize_b > WRITE_BUF_SIZE {
		return Err("Cannot test because the file size is too big");
	}

	// delete existing files. To make sure that the file creation below succeeds.
	for filename in &filenames {
		del_or_err(filename).expect("Cannot continue the test. We need 'delete()'.");
	}

	let cwd = match get_cwd() {
		Some(dirref) => {dirref}
		_ => {return Err("Cannot get CWD");}
	};

	let wbuf = &WRITE_BUF[0..fsize_b];

	// Measuring loop - create
	start_hpet_create = hpet.get_counter();
	for filename in filenames {
		// We first create a file and then write to resemble LMBench.
		let file = HeapFile::create(filename, &cwd).expect("File cannot be created.");
		file.lock().write_at(wbuf, 0)?;
	}
	end_hpet_create = hpet.get_counter();

	let delta_hpet_create = end_hpet_create - start_hpet_create - overhead_ct;
	let delta_time_create = hpet_2_time("", delta_hpet_create);
	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let files_per_time = (ITERATIONS) as u64 * to_sec / delta_time_create;

	printlninfo!("{:8}    {:9}    {:16}    {:16}", fsize_b/KB as usize, ITERATIONS, files_per_time,delta_time_create / (ITERATIONS) as u64);
	Ok(())
}

/// Measures the time to delete to a file. 
/// Calls `do_fs_delete_inner` multiple times to perform the actual operation
/// File sizes of 1K, 4K and 10K are measured in this function
/// Note : In `LMBench` creating and deleting is done in the same operation.
/// Here we use two functions to avoid time to searach a file.
fn do_fs_delete() -> Result<(), &'static str> {
	// let	fsizes_b = [0 as usize, 1024, 4096, 10*1024];	// Theseus thinks creating an empty file is stupid (for memfs)
	let	fsizes_b = [1024_usize, 4096, 10*1024];

	let overhead_ct = hpet_timing_overhead()?;

	// printlninfo!("SIZE(KB)    Iteration    created(files/s)    deleted(files/s)");
	print_header(TRIES, ITERATIONS);
	printlninfo!("SIZE(KB)    Iteration    deleted(files/s)    time(ns/file)");
	for fsize_b in fsizes_b.iter() {
		do_fs_delete_inner(*fsize_b, overhead_ct)?;
	}
	printlninfo!("This test is equivalent to file delete in `lat_fs` in LMBench");
	Ok(())
}

/// Internal function that actually calculates the time to delete to a file.
/// Within the measurin section it remove the given file reference from current working directory
/// Prior to measuring files are created and their referecnes are added to a vector
fn do_fs_delete_inner(fsize_b: usize, overhead_ct: u64) -> Result<(), &'static str> {
	let mut filenames = vec!["".to_string(); ITERATIONS];
	let pid = getpid();
	let start_hpet_create: u64;
	let end_hpet_create: u64;
	let _start_hpet_del: u64;
	let _end_hpet_del: u64;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

	let mut file_list = Vec::new();

	// don't put these (populating files, checks, etc) into the loop to be timed
	// The loop must be doing minimal operations to exclude unnecessary overhead
	// populate filenames
	for i in 0..ITERATIONS {
		filenames[i] = format!("tmp_{}_{}_{}.txt", pid, fsize_b, i);
	}

	// check if we have enough data to write. We use just const data to avoid unnecessary overhead
	if fsize_b > WRITE_BUF_SIZE {
		return Err("Cannot test because the file size is too big");
	}

	// delete existing files. To make sure that the file creation below succeeds.
	for filename in &filenames {
		del_or_err(filename).expect("Cannot continue the test. We need 'delete()'.");
	}

	let cwd = match get_cwd() {
		Some(dirref) => {dirref}
		_ => {return Err("Cannot get CWD");}
	};

	let wbuf = &WRITE_BUF[0..fsize_b];

	// Non measuring loop for file create
	for filename in &filenames {

		let file = HeapFile::create(filename.to_string(), &cwd).expect("File cannot be created.");
		file.lock().write_at(wbuf, 0)?;
		file_list.push(file);
	}
	

	let mut cwd_locked = cwd.lock();

	start_hpet_create = hpet.get_counter();

	// Measuring loop file delete
	for fileref in file_list{
		cwd_locked.remove(&FileOrDir::File(fileref)).expect("Cannot remove File in Create & Del inner");
	}

	end_hpet_create = hpet.get_counter();

	let delta_hpet_delete = end_hpet_create - start_hpet_create - overhead_ct;
	let delta_time_delete = hpet_2_time("", delta_hpet_delete);
	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let files_per_time = (ITERATIONS) as u64 * to_sec / delta_time_delete;

	printlninfo!("{:8}    {:9}    {:16}    {:16}", fsize_b/KB as usize, ITERATIONS, files_per_time, delta_time_delete /(ITERATIONS) as u64);
	Ok(())
}



/// Helper function to get the name of current task
fn get_prog_name() -> String {
	task::with_current_task(|t| t.name.clone())
		.unwrap_or_else(|_| {
            printlninfo!("failed to get current task");
            "Unknown".to_string()
		})
}

/// Helper function to get the PID of current task
fn getpid() -> usize {
	task::get_my_current_task_id()
}


/// Helper function to convert ticks to time
fn hpet_2_time(msg_header: &str, hpet: u64) -> u64 {
	let t = if cfg!(bm_in_us) {hpet_2_us(hpet)} else {hpet_2_ns(hpet)};
	if msg_header != "" {
		let mut msg = format!("{} {} in ", msg_header, t);
		msg += if cfg!(bm_in_us) {"us"} else {"ns"};
		printlninfo!("{}", msg);
	}

	t
}


/// Helper function to get current working directory
fn get_cwd() -> Option<DirRef> {
	task::with_current_task(|t| 
		Arc::clone(&t.get_env().lock().working_dir)
	).ok()
}

/// Helper function to make a temporary file to be used to measure read open latencies
/// DON'T call this function inside of a measuring loop.
fn mk_tmp_file(filename: &str, sz: usize) -> Result<(), &'static str> {
	if sz > WRITE_BUF_SIZE {
		return Err("Cannot test because the file size is too big");
	}

	if let Some(fileref) = get_file(filename) {
		if fileref.lock().len() == sz {
			return Ok(());
		}
	}

	let file = HeapFile::create(filename.to_string(), &get_cwd().unwrap()).expect("File cannot be created.");
	file.lock().write_at(&WRITE_BUF[0..sz], 0)?;

	Ok(())
}

/// Helper function to delete an existing file
fn del_or_err(filename: &str) -> Result<(), &'static str> {
	if let Some(_fileref) = get_file(filename) {
		return Err("Need to delete a file, but delete() is not implemented yet :(");
	}
	Ok(())
}

/// Wrapper function for file read.
/// Only used to check file system
fn cat(fileref: &FileRef, sz: usize, msg: &str) {
	printlninfo!("{}", msg);
	let mut file = fileref.lock();
	let mut buf = vec![0 as u8; sz];

	match file.read_at(&mut buf,0) {
		Ok(nr_read) => {
			printlninfo!("tries to read {} bytes, and {} bytes are read", sz, nr_read);
			printlninfo!("read: '{}'", str::from_utf8(&buf).unwrap());
		}
		Err(_) => {printlninfo!("Cannot read");}
	}
}

/// Wrapper function for file write.
/// Only used to check file system.
fn write(fileref: &FileRef, sz: usize, msg: &str) {
	printlninfo!("{}", msg);
	let mut buf = vec![0 as u8; sz];

	for i in 0..sz {
		buf[i] = i as u8 % 10 + 48;
	}

	let mut file = fileref.lock();
	match file.write_at(&buf,0) {
		Ok(nr_write) => {
			printlninfo!("tries to write {} bytes, and {} bytes are written", sz, nr_write);
			printlninfo!("written: '{}'", str::from_utf8(&buf).unwrap());
		}
		Err(_) => {printlninfo!("Cannot write");}
	}
}

/// Helper function to check file system by reading and writing
fn test_file_inner(fileref: FileRef) {
	let sz = {fileref.lock().len()};
	printlninfo!("File size: {}", sz);

	cat(&fileref, sz, 	"== Do CAT-NORMAL ==");
	cat(&fileref, sz*2,	"== Do CAT-MORE   ==");

	write(&fileref, sz, "== Do WRITE-NORMAL ==");
	cat(&fileref, sz, 	"== Do CAT-NORMAL ==");

	write(&fileref, sz*2, "== Do WRITE-MORE ==");
	let sz = {fileref.lock().len()};
	cat(&fileref, sz, 	"== Do CAT-NORMAL ==");

}

/// Wrapper function to get a file provided a string.
/// Not used in measurements
fn get_file(filename: &str) -> Option<FileRef> {
	let path = Path::new(filename.to_string());
	match path.get(&get_cwd().unwrap()) {
		Some(file_dir_enum) => {
			match file_dir_enum {
                FileOrDir::File(fileref) => { Some(fileref) }
                _ => {None}
            }
		}
		_ => { None }
	}
}

/// Wrapper of helper function to check file system
fn test_file(filename: &str) {
	if let Some(fileref) = get_file(filename) {
		test_file_inner(fileref);
	}
}

/// Wrapper of helper function to check file system
fn do_fs_cap_check() -> Result<(), &'static str> {
	let filename = format!("tmp{}.txt", getpid());
	if mk_tmp_file(&filename, 4).is_ok() {
		printlninfo!("Testing with the file...");
		test_file(&filename);
	}
	Ok(())
}


/// Print help
fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: OPTION ARG";

/// Print header of each test
fn print_header(tries: usize, iterations: usize) {
	printlninfo!("========================================");
	printlninfo!("Time unit : {}", T_UNIT);
	printlninfo!("Iterations: {}", iterations);
	printlninfo!("Tries     : {}", tries);
	printlninfo!("Core      : {}", CPU_ID!());
	printlninfo!("========================================");
}



/// Task generated to measure time of context switching
fn yield_task(_a: u32) -> u32 {
	let times = ITERATIONS*1000;
    for _i in 0..times {
       scheduler::schedule();
    }
    _a
}

/// Task generated to measure overhead of context switching
fn overhead_task(_a: u32) -> u32 {
    _a
}