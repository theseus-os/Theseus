#![no_std]

extern crate alloc;

extern crate intel_cat;

extern crate task;

#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;
#[cfg(use_intel_cat)]
use intel_cat::{allocate_clos, get_current_cache_allocation, validate_clos_on_single_core, reset_cache_allocations, ClosDescriptor, ClosList};

pub fn main(args: Vec<String>) -> isize{

	// return value to check error status
	// a retun value of 0 indicates that all tests passed successfully
	let mut ret = 0;
	
	#[cfg(not(use_intel_cat))]
	{
    	println!("Intel cat is not enabled. No tests will be run. Compile with use_intel_cat to run these tests.");
		return 0;
	}

    #[cfg(use_intel_cat)]
    {
		// testing the maximum closid value
		debug!("Maximum supported closid is: {}", task::get_max_closid());

		debug!("Testing valid and invalid cache allocation requests.");
		// some invalid cache requests
		// too much space exclusive/nonexclusive
		match allocate_clos(11, true){
		Ok(_) => {
			error!("test_intel_cat : ERROR: Expected cache allocation of too much exclusive space to fail.");
			ret = 1;
		}
		Err(_) => { },
		};

		match allocate_clos(12, false){
		Ok(_) => {
			error!("test_intel_cat : ERROR: Expected cache allocation of too much shared space to fail.");
			ret = 2;
		}
		Err(_) => { },
		};

		// insufficient space requested
		match allocate_clos(0, true){
		Ok(_) => {
			error!("test_intel_cat : ERROR: Expected cache allocation of too little exclusive space to fail.");
			ret = 3;
		}
		Err(_) => { },
		};

		match allocate_clos(0, false){
		Ok(_) => {
			error!("test_intel_cat : ERROR: Expected cache allocation of too little shared space to fail.");
			ret = 4;
		}
		Err(_) => { },
		};

		// trying some valid allocations
		match allocate_clos(3, true){
		Ok(1) => { },
		Ok(i) => {
			error!("test_intel_cat : ERROR: Expected a return value of 1. Instead returned: {}.", i);
			ret = 5;
		}
		Err(e) => {
			error!("test_intel_cat : ERROR: Allocation of three megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
			ret = 6;
		}
		}

		match allocate_clos(5, false){
		Ok(2) => { },
		Ok(i) => {
			error!("test_intel_cat : ERROR: Expected a return value of 2. Instead returned: {}.", i);
			ret = 7;
		}
		Err(e) => {
			error!("test_intel_cat : ERROR: Allocation of five megabytes of shared space expected to succeed, but it failed with error: {}", e);
			ret = 8;
		}
		}

		match allocate_clos(4, true){
		Ok(3) => { },
		Ok(i) => {
			error!("test_intel_cat : ERROR: Expected a return value of 3. Instead returned: {}.", i);
			ret = 9;
		}
		Err(e) => {
			error!("test_intel_cat : ERROR: Allocation of four megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
			ret = 10;
		}
		}

		// this test should fail because now there are only 4 megabytes of non-exclusive cache space left
		match allocate_clos(4, true){
		Ok(_) => {
			// this allocation is expected to fail because at this point we have allocated 7 megabytes exclusively in the previous section
			// thus, only 4 megabytes of shared space remain, only 3 of which can be converted into an exclusive region, as 1 megabyte is always reserved as shared space
			error!("test_intel_cat : ERROR: Expected cache allocation of 4 megabytes with only 4 megabytes of LLC left to fail.");
			ret = 11;
		}
		Err(_) => { },
		};

		debug!("Attempting to set the closid of the task.");
		// attempting to set the closid of our current task
		// getting our task structure
		if let Some(taskref) = task::get_my_current_task() {
			debug!("Getting taskref succeeded.");

			// invalid closid
			match taskref.set_closid(256){
				Ok(_) => {
				error!("test_intel_cat : ERROR: Expected closid setting to fail.");
				ret = 12;
				},
				Err(_) => {
				debug!("Setting closid failed as expected.");
				},
			};

			// valid closid
			match taskref.set_closid(1) {
				Ok(_) => {
				debug!("Setting closid succeeded.");
				},
				Err(_) => {
				error!("test_intel_cat : ERROR: Expected closid setting to succeed.");
				ret = 13;
				},
			};
		}
		
		else{
			error!("test_intel_cat : ERROR: Failed to get task ref.");
			ret = 14;
		}

		// verifying the results of our cache allocations
		let current_list = get_current_cache_allocation();

		debug!("Current Cache Allocation: {:?}", current_list);

		debug!("Verifying that MSRs were set properly.");

		match validate_clos_on_single_core(current_list) {
		Ok(_) => { },
		Err((expected, found)) => {
			error!("test_intel_cat : ERROR: MSR read failed.\nExpected: {}\nFound: {}", expected, found);
			ret = 15;
		},
		};

		// attempting a reset of the cache allocations
		debug!("Resetting cache allocations.");
		match reset_cache_allocations() {
		Ok(_) => { },
		Err(e) => {
			error!("test_intel_cat : ERROR: Expected cache reset to succeed but return with error: {}", e);
			ret = 16;
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
			ret = 17;
		},
		};
		
		// checking if all tests have succeeded
		if ret == 0 {println!("All tests passed!");}
    }

    ret
}
