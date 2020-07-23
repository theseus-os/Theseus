#![no_std]

extern crate alloc;

#[cfg(use_intel_cat)]
extern crate intel_cat;

#[cfg(use_intel_cat)]
extern crate task;

#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;
#[cfg(use_intel_cat)]
use intel_cat::{allocate_clos, get_current_cache_allocation, validate_clos_on_single_core, reset_cache_allocations, ClosDescriptor, ClosList};

pub fn main(args: Vec<String>) -> isize{

    debug!("Hello from test_intel_cat!");
    println!("Hello from test_intel_cat!");

    // return value to check error status
    let mut ret = 0;

    #[cfg(use_intel_cat)]
    {
    // testing the maximum closid value
    debug!("Maximum supported closid is: {}", task::get_max_closid());

    debug!("Testing valid and invalid cache allocation requests.");
    // some invalid cache requests
    // too much space exclusive/nonexclusive
    match allocate_clos(11, true){
	Ok(_) => {
	    debug!("Expected cache allocation of too much exclusive space to fail. Exiting.");
	    ret = 1;
	}
	Err(_) => { },
    };

    match allocate_clos(12, false){
	Ok(_) => {
	    debug!("Expected cache allocation of too much shared space to fail. Exiting.");
	    ret = 2;
	}
	Err(_) => { },
    };

    // insufficient space requested
    match allocate_clos(0, true){
	Ok(_) => {
	    debug!("Expected cache allocation of too little exclusive space to fail. Exiting.");
	    ret = 3;
	}
	Err(_) => { },
    };

    match allocate_clos(0, false){
	Ok(_) => {
	    debug!("Expected cache allocation of too little shared space to fail. Exiting.");
	    ret = 4;
	}
	Err(_) => { },
    };

    debug!("All tests passed!");

    // trying some valid allocations
    match allocate_clos(3, true){
	Ok(1) => { },
	Ok(i) => {
	    debug!("Expected a return value of 1. Instead returned: {}. Exiting.", i);
	    ret = 5;
	}
	Err(e) => {
	    debug!("Allocation of three megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
	    ret = 6;
	}
    }

    match allocate_clos(5, false){
	Ok(2) => { },
	Ok(i) => {
	    debug!("Expected a return value of 2. Instead returned: {}. Exiting.", i);
	    ret = 7;
	}
	Err(e) => {
	    debug!("Allocation of five megabytes of shared space expected to succeed, but it failed with error: {}", e);
	    ret = 8;
	}
    }

    match allocate_clos(4, true){
	Ok(3) => { },
	Ok(i) => {
	    debug!("Expected a return value of 3. Instead returned: {}. Exiting.", i);
	    ret = 9;
	}
	Err(e) => {
	    debug!("Allocation of four megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
	    ret = 10;
	}
    }

    // this test should fail because now there are only 4 megabytes of non-exclusive cache space left
    match allocate_clos(4, true){
	Ok(_) => {
	    debug!("Expected cache allocation of 4 megabytes with only 4 megabytes of LLC left to fail. Exiting.");
	    ret = 11;
	}
	Err(_) => { },
    };

    debug!("Attempting to set the closid of the task.");
    // attempting to set the closid of our current task
    // getting our task structure
    if let Some(taskref) = task::get_my_current_task() {
	// invalid closid
	debug!("Getting taskref succeeded.");
	match taskref.set_closid(256){
	    Ok(_) => {
		debug!("Expected closid setting to fail. Exiting.");
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
		debug!("Expected closid setting to succeed. Exiting.");
		ret = 13;
	    },
	};
    }
    
    else{
	debug!("Failed to get task ref. Exiting");
	ret = 14;
    }

    // verifying the results of our cache allocations
    let current_list = get_current_cache_allocation();

    debug!("Current Cache Allocation: {:?}", current_list);

    debug!("Verifying that MSRs were set properly.");

    match validate_clos_on_single_core(current_list) {
	Ok(_) => { },
	Err((expected, found)) => {
	    debug!("MSR read failed.\nExpected: {}\nFound: {}", expected, found);
	    ret = 15;
	},
    };

    // attempting a reset of the cache allocations
    debug!("Resetting cache allocations.");
    match reset_cache_allocations() {
	Ok(_) => { },
	Err(e) => {
	    debug!("Expected cache reset to succeed but return with error: {}", e);
	    ret = 16;
	},
    };

    //checking that the reset reset all the MSR registers
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

    match validate_clos_on_single_core(reset_list) {
	Ok(_) => { },
	Err((expected, found)) => {
	    debug!("MSR read failed.\nExpected: {}\nFound: {}", expected, found);
	    ret = 17;
	},
    };

	debug!("All tests passed!");
    }

    ret
}
