#![no_std]

extern crate alloc;
extern crate intel_cat;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;
use intel_cat::*;

pub fn main(args: Vec<String>) -> isize{
    // testing the maximum closid value
    unsafe {
	println!("Maximum supported closid is: {}", get_max_closid());
    }
    
    // testing the three types of invalid bitmask
    // bitmask is 0
    let zero_bitmask = ClosDescriptor {
	closid: 1,
	bitmask: 0,
    };

    // bitmask is greater than 0x7ff
    let too_large_bitmask = ClosDescriptor {
	closid: 1,
	bitmask: 0x8ff,
    };

    // bitmask contains nonconsecutive ones
    let nonconsecutive_bitmask = ClosDescriptor {
	closid: 1,
	bitmask: 0b11100111000,
    };

    // testing that the closid is less than 128
    let too_large_closid = ClosDescriptor {
	closid: 128,
	bitmask: 1,
    };

    // creating an array to hold the bad closdescriptors
    let mut bad_clos : Vec<ClosDescriptor> = Vec::new();
    bad_clos.push(zero_bitmask);
    bad_clos.push(too_large_bitmask);
    bad_clos.push(nonconsecutive_bitmask);
    bad_clos.push(too_large_closid);

    //testing that all of these return an error from update closid
    for clos in bad_clos{
	match update_clos(clos){
	    Ok(_) => {
		println!("Bad ClosDescriptor {:?} should have failed but succeeded on update_clos. Exiting.", clos);
		return -1;
	    }
	    Err(_) => {
	    },
	}
    }

    println!("ClosDescriptor error checking worked!\nTesting valid bitmask on update_clos.");

    //testing that update_clos
    let first_valid_clos_descriptor = ClosDescriptor{
	closid: 1,
	bitmask: 0x7fe,
    };

    match update_clos(first_valid_clos_descriptor){
	Ok(_) => println!("update_clos succeeded on write!"),

	Err(s) => {
	    println!("update_clos should have succeeded but returned error: {}.\nExiting.", s);
	    return -1;
	}
    }

    // now testing the set_clos_on_single_core function with a series of eleven valid clos_descriptors
    println!("Testing set_clos_on_single_core.");

    let mut good_clos_list : Vec<ClosDescriptor> = Vec::new();
    // adding eleven clos_descriptors
    for i in 0..10{
	good_clos_list.push(
	    ClosDescriptor{
		closid: i,
		bitmask: (1 << i),
	    }
	);
    }

    let good_clos = ClosList{
	descriptor_list: good_clos_list,
    };

    match set_clos_on_single_core(good_clos.clone()){
	Ok(_) => println!("set_clos_on_single_core returned successfully!"),
	Err(s) => {
	    println!("set_clos_on_single_core return with error: {}\nExiting", s);
	    return -1;
	}
    }

    // checking that the msrs were properly written to
    println!("Checking that MSRs were properly written to.");

    if let Err((expected, value)) = validate_clos_on_single_core(good_clos.clone()){
	println!("MSRs were not properly written.\nExpected: {}\nRead: {}\nExiting.", expected, value);
	return -1;
    }

    println!("MSRs were written properly!\nTesting writing to MSRs on all cores.");

    // testing set_clos function
    match set_clos(good_clos.clone()){
	Ok(_) => println!("set_clos returned successfully!"),
	Err(s) => {
	    println!("set_clos returned with error: {}.\nExiting.", s);
	    return -1;
	}
    }


    println!("All tests passed!");
    0
}
