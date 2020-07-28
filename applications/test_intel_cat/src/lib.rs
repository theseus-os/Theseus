#![no_std]

extern crate alloc;

extern crate intel_cat;

extern crate closid_settings;

extern crate task;

#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;
#[cfg(use_intel_cat)]
use intel_cat::{allocate_clos, validate_clos_on_single_core, reset_cache_allocations, set_closid_on_current_task};

pub fn main(args: Vec<String>) -> isize{

	// boolean value to check error status
	let mut success = true;

	#[cfg(not(use_intel_cat))]
	{
		println!("Intel cat is not enabled. No tests will be run. Compile with use_intel_cat to run these tests.");
		return 0;
	}

	#[cfg(use_intel_cat)]
	{
		// testing the maximum closid value
		debug!("Maximum supported closid is: {}", closid_settings::get_max_closid());

		debug!("Testing valid and invalid cache allocation requests.");
		// some invalid cache requests
		// too much space exclusive/nonexclusive
		match allocate_clos(11, true){
			Ok(_) => {
				error!("test_intel_cat : ERROR: Expected cache allocation of too much exclusive space to fail.");
				success = false;
			}
			Err(_) => { },
		};

		match allocate_clos(12, false){
			Ok(_) => {
				error!("test_intel_cat : ERROR: Expected cache allocation of too much shared space to fail.");
				success = false;
			}
			Err(_) => { },
		};

		// insufficient space requested
		match allocate_clos(0, true){
			Ok(_) => {
				error!("test_intel_cat : ERROR: Expected cache allocation of too little exclusive space to fail.");
				success = false;
			}
			Err(_) => { },
		};

		match allocate_clos(0, false){
			Ok(_) => {
				error!("test_intel_cat : ERROR: Expected cache allocation of too little shared space to fail.");
				success = false;
			}
			Err(_) => { },
		};

		// trying some valid allocations
		match allocate_clos(3, true){
			Ok(closid_settings::ClosId(1)) => { },
			Ok(i) => {
				error!("test_intel_cat : ERROR: Expected a return value of 1. Instead returned: {}.", i.0);
				success = false;
			}
			Err(e) => {
				error!("test_intel_cat : ERROR: Allocation of three megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
				success = false;
			}
		}

		match allocate_clos(5, false){
			Ok(closid_settings::ClosId(2)) => { },
			Ok(i) => {
				error!("test_intel_cat : ERROR: Expected a return value of 2. Instead returned: {}.", i.0);
				success = false;
			}
			Err(e) => {
				error!("test_intel_cat : ERROR: Allocation of five megabytes of shared space expected to succeed, but it failed with error: {}", e);
				success = false;
			}
		}

		match allocate_clos(4, true){
			Ok(closid_settings::ClosId(3)) => { },
			Ok(i) => {
				error!("test_intel_cat : ERROR: Expected a return value of 3. Instead returned: {}.", i.0);
				success = false;
			}
			Err(e) => {
				error!("test_intel_cat : ERROR: Allocation of four megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
				success = false;
			}
		}

		// this test should fail because now there are only 4 megabytes of non-exclusive cache space left
		match allocate_clos(4, true){
			Ok(_) => {
				// this allocation is expected to fail because at this point we have allocated 7 megabytes exclusively in the previous section
				// thus, only 4 megabytes of shared space remain, only 3 of which can be converted into an exclusive region, as 1 megabyte is always reserved as shared space
				error!("test_intel_cat : ERROR: Expected cache allocation of 4 megabytes with only 4 megabytes of LLC left to fail.");
				success = false;
			}
			Err(_) => { },
		};

		debug!("Attempting to set the closid of the task.");
		// attempting to set the closid of our current task
		// getting our task structure
		// invalid closid
		match set_closid_on_current_task(256){
			Ok(_) => {
			error!("test_intel_cat : ERROR: Expected closid setting to fail.");
			success = false;
			},
			Err(_) => {
			debug!("Setting closid failed as expected.");
			},
		};

		// valid closid
		match set_closid_on_current_task(1) {
			Ok(_) => {
			debug!("Setting closid succeeded.");
			},
			Err(_) => {
			error!("test_intel_cat : ERROR: Expected closid setting to succeed.");
			success = false;
			},
		};

		/*
		// verifying the results of our cache allocations
		let current_list = get_current_cache_allocation();

		debug!("Current Cache Allocation: {:?}", current_list);
		*/

		debug!("Verifying that MSRs were set properly.");

		match validate_clos_on_single_core() {
			Ok(_) => { },
			Err((expected, found)) => {
				error!("test_intel_cat : ERROR: MSR read failed.\nExpected: {}\nFound: {}", expected, found);
				success = false;
			},
		};

		// attempting a reset of the cache allocations
		debug!("Resetting cache allocations.");
		match reset_cache_allocations() {
			Ok(_) => { },
			Err(e) => {
				error!("test_intel_cat : ERROR: Expected cache reset to succeed but return with error: {}", e);
				success = false;
			},
		};

		/*
		//checking that the reset succeeded in resetting all the MSRs
		let mut reset_vec : Vec<ClosDescriptor> = Vec::new();

		for i in 0..4 {
		reset_vec.push(
			ClosDescriptor{
			closid: i as u16,
			bitmask: 0x7ff,
			exclusive: false,
			}
		);
		}

		let reset_list = ClosList::new(reset_vec);
		*/

		match validate_clos_on_single_core() {
			Ok(_) => { },
			Err((expected, found)) => {
				error!("test_intel_cat : ERROR: MSR read failed.\nExpected: {}\nFound: {}", expected, found);
				success = false;
			},
		};

		// checking if all tests have succeeded
		if success {println!("All tests passed!");}
	}

	if !success { return 1; }

	0
}
