#![no_std]

extern crate alloc;
extern crate intel_cat;
extern crate task;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;
use intel_cat::{allocate_clos, get_current_cache_allocation, validate_clos_on_single_core, reset_cache_allocations};

pub fn main(args: Vec<String>) -> isize{

    debug!("Hello from test_intel_cat!");
    println!("Hello from test_intel_cat!");
    /*
    // testing the maximum closid value
    println!("Maximum supported closid is: {}", task::get_max_closid());

    println!("Testing valid and invalid cache allocation requests.");
    // some invalid cache requests
    // too much space exclusive/nonexclusive
    match allocate_clos(11, true){
	Ok(_) => {
	    println!("Expected cache allocation of too much exclusive space to fail. Exiting.");
	    return 1;
	}
	Err(_) => { },
    };

    match allocate_clos(12, false){
	Ok(_) => {
	    println!("Expected cache allocation of too much shared space to fail. Exiting.");
	    return 1;
	}
	Err(_) => { },
    };

    // insufficient space requested
    match allocate_clos(0, true){
	Ok(_) => {
	    println!("Expected cache allocation of too little exclusive space to fail. Exiting.");
	    return 1;
	}
	Err(_) => { },
    };

    match allocate_clos(0, false){
	Ok(_) => {
	    println!("Expected cache allocation of too little shared space to fail. Exiting.");
	    return 1;
	}
	Err(_) => { },
    };

    println!("All tests passed!");

    // trying some valid allocations
    match allocate_clos(3, true){
	Ok(1) => { },
	Ok(i) => {
	    println!("Expected a return value of 1. Instead returned: {}. Exiting.", i);
	    return 1;
	}
	Err(e) => {
	    println!("Allocation of three megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
	    return 1;
	}
    }

    match allocate_clos(5, false){
	Ok(2) => { },
	Ok(i) => {
	    println!("Expected a return value of 2. Instead returned: {}. Exiting.", i);
	    return 1;
	}
	Err(e) => {
	    println!("Allocation of five megabytes of shared space expected to succeed, but it failed with error: {}", e);
	    return 1;
	}
    }

    match allocate_clos(4, true){
	Ok(3) => { },
	Ok(i) => {
	    println!("Expected a return value of 3. Instead returned: {}. Exiting.", i);
	    return 1;
	}
	Err(e) => {
	    println!("Allocation of four megabytes of exclusive space expected to succeed, but it failed with error: {}", e);
	    return 1;
	}
    }

    // this test should fail because now there are only 4 megabytes of non-exclusive cache space left
    match allocate_clos(4, true){
	Ok(_) => {
	    println!("Expected cache allocation of 4 megabytes with only 4 megabytes of LLC left to fail. Exiting.");
	    return 1;
	}
	Err(_) => { },
    };

    println!("Attempting to set the closid of the task.");
    
    // attempting to set the closid of our current task
    // getting our task structure
    if let Some(taskref) = task::get_my_current_task() {
	// invalid closid
	match taskref.set_closid(256){
	    Ok(_) => {
		println!("Expected closid setting to fail. Exiting.");
		return 1;
	    },
	    Err(_) => { },
	};

	// valid closid
	match taskref.set_closid(1) {
	    Ok(_) => { },
	    Err(_) => {
		println!("Expected closid setting to succeed. Exiting.");
		return 1;
	    },
	};
    }
    
    else{
	println!("Failed to get task ref. Exiting");
	return 1;
    }


    // verifying the results of our cache allocations
    let current_list = get_current_cache_allocation();

    println!("Current Cache Allocation: {:?}", current_list);

    println!("Verifying that MSRs were set properly.");

    match validate_clos_on_single_core(current_list) {
	Ok(_) => { },
	Err((expected, found)) => {
	    println!("MSR read failed.\nExpected: {}\nFound: {}", expected, found);
	    return 1;
	},
    };

    // attempting a reset of the cache allocations
    println!("Resetting cache allocations.");
    match reset_cache_allocations() {
	Ok(_) => { },
	Err(e) => {
	    println!("Expected cache reset to succeed but return with error: {}", e);
	    return 1;
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
	    println!("MSR read failed.\nExpected: {}\nFound: {}", expected, found);
	    return 1;
	},
    };

    println!("All tests passed!");
    */
    0
}
