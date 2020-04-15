#![no_std]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate hpet;
extern crate hashbrown;
extern crate qp_trie;
extern crate apic;
extern crate runqueue;

use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::{BTreeSet,BTreeMap}
};
use hpet::get_hpet;
use hashbrown::HashMap;
use qp_trie::{Trie, wrapper::BString};

const NANO_TO_FEMTO: u64 = 1_000_000;
const ITERATIONS: u64 = 10_000;
const TRIES: u64 = 10;
const CAPACITY: [usize; 10] = [8,16,32,64,128,256,512,1024,2048,4096];
const VERBOSE: bool = false;

/// Helper function to convert ticks to nano seconds
fn hpet_2_ns(hpet: u64) -> u64 {
	let hpet_period = get_hpet().as_ref().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / NANO_TO_FEMTO
}

macro_rules! CPU_ID {
	() => (apic::get_my_apic_id())
}

/// Helper function return the tasks in a given core's runqueue
fn nr_tasks_in_rq(core: u8) -> Option<usize> {
	match runqueue::get_runqueue(core).map(|rq| rq.read()) {
		Some(rq) => { Some(rq.iter().count()) }
		_ => { None }
	}
}

/// True if only two tasks are running in the current runqueue
/// Used to verify if there are any other tasks than the current task and idle task in the runqueue
fn check_myrq() -> bool {
	match nr_tasks_in_rq(CPU_ID!()) {
		Some(2) => { true }
		_ => { false }
	}
}


pub fn main(args: Vec<String>) -> isize {
    if args.len() != 1 {
		print_usage();
		return 0;
	}

	if !check_myrq() {
		println!("cannot run on a busy core (#{}). Pin me on an idle core.", CPU_ID!());
		return 0;
	}

    match args[0].as_str() {
		"vec" => {
			do_vec();
		}
		"hashmap" => {
			do_hashmap();
		}
		"btreemap" => {
			do_btreemap();
		}
		"btreeset" => {
			do_btreeset();
		}
		"qptrie" => {
			do_qptrie();
		}

		_arg => {
			println!("Unknown command: {}", args[0]);
			print_usage();
			return 0;
		}
	}

    0
}




fn do_vec() {
    let tries = TRIES;
    let mut results: Vec<u64> = Vec::with_capacity(tries as usize);

    // // run tests for creating a vector with a certain capacity
    // for cap in &CAPACITY {
    //     for _ in 0..tries {
    //         results.push(create_vec_with_capacity(*cap));
    //     }

    //     println!("Create and drop a vector of capacity: {}", *cap);
    //     println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
    //     println!("");

    //     results.clear();
    // }

    // run tests for creating an empty vector and adding items till it reaches a certain length
    for cap in &CAPACITY {
        for _ in 0..tries {
            results.push(create_vec_with_length(*cap));
        }

        println!("Create a vector, add elements till it reaches length = {}, and then drop", *cap);
        println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
        println!("");

        results.clear();
    }
}

#[allow(dead_code)]
fn create_vec_with_capacity(cap: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let mut vec: Vec<u8> = Vec::with_capacity(cap);
        vec.push(255);
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }

    time
}

fn create_vec_with_length(length: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let mut vec: Vec<u8> = Vec::new();
        for i in 0..length {
            vec.push(i as u8);
        }
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }

    time
}




fn do_hashmap() {
    let tries = TRIES;

    let mut results: Vec<u64> = Vec::with_capacity(tries as usize);

    // // run tests to create a hashmap of a certain capacity 
    // for cap in &CAPACITY {
    //     for _ in 0..tries {
    //         results.push(create_hashmap_with_capacity(*cap));
    //     }

    //     println!("Create and drop a HashMap of capacity: {}", *cap);
    //     println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
    //     println!("");

    //     results.clear();
    // }

    // run tests for creating an empty hashmap and adding items till it reaches a certain length
    for cap in &CAPACITY {
        for _ in 0..tries {
            results.push(create_hashmap_with_length(*cap));
        }

        println!("Create a hashmap, add elements till it reaches length = {}, and then drop", *cap);
        println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
        println!("");

        results.clear();
    }

}

#[allow(dead_code)]
fn create_hashmap_with_capacity(cap: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let _vec: HashMap<usize, String> = HashMap::with_capacity(cap);
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }    

    time
}

fn create_hashmap_with_length(length: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let mut vec: HashMap<usize, String> = HashMap::new();        
        for i in 0..length {
            vec.insert(i, String::from("hello"));
        }
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }    

    time
}



fn do_btreemap() {
    let tries = TRIES;

    let mut results: Vec<u64> = Vec::with_capacity(tries as usize);

    // run tests for creating an empty btree map and adding items till it reaches a certain length
    for cap in &CAPACITY {
        for _ in 0..tries {
            results.push(create_btreemap_with_length(*cap));
        }

        println!("Create a btree map, add elements till it reaches length = {}, and then drop", *cap);
        println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
        println!("");

        results.clear();
    }
}

fn create_btreemap_with_length(length: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let mut vec: BTreeMap<usize, String> = BTreeMap::new();        
        for i in 0..length {
            vec.insert(i, String::from("hello"));
        }
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }    

    time
}



fn do_btreeset() {
    let tries = TRIES;

    let mut results: Vec<u64> = Vec::with_capacity(tries as usize);

    // run tests for creating an empty btree set and adding items till it reaches a certain length
    for cap in &CAPACITY {
        for _ in 0..tries {
            results.push(create_btreeset_with_length(*cap));
        }

        println!("Create a btree set, add elements till it reaches length = {}, and then drop", *cap);
        println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
        println!("");

        results.clear();
    }
}

fn create_btreeset_with_length(length: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let mut vec: BTreeSet<usize> = BTreeSet::new();        
        for i in 0..length {
            vec.insert(i);
        }
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }    

    time
}




fn do_qptrie() {
    let tries = TRIES;

    let mut results: Vec<u64> = Vec::with_capacity(tries as usize);

    // run tests for creating an empty qp trie and adding items till it reaches a certain length
    for cap in &CAPACITY {
        for _ in 0..tries {
            results.push(create_qptrie_with_length(*cap));
        }

        println!("Create a QP trie set, add elements till it reaches length = {}, and then drop", *cap);
        println!("Mean time: {} ns", results.iter().sum::<u64>() / results.len() as u64);
        println!("");

        results.clear();
    }
}

fn create_qptrie_with_length(length: usize) -> u64 {
    let iterations = ITERATIONS;

    let start_hpet = get_hpet().as_ref().unwrap().get_counter();
    for _ in 0..iterations {
        let mut vec: Trie<BString, usize> = Trie::new();        
        for i in 0..length {
            vec.insert_str(&i.to_string(), i);
        }
    }
    let end_hpet = get_hpet().as_ref().unwrap().get_counter();
    let time = hpet_2_ns(end_hpet - start_hpet) / iterations;

    if VERBOSE{ println!("mean time: {} ns", time); }    

    time
}

/// Print help
fn print_usage() {
	println!("\n Usage");
	println!("\n  available cmds:");
	println!("\n    vec");
	println!("\n    hashmap");
	println!("\n    btreemap");
	println!("\n    btreeset");
	println!("\n    qptrie");
}