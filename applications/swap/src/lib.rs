
//! This application is for performing module management, such as swapping.


#![no_std]
#![feature(alloc)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate itertools;

extern crate getopts;
extern crate memory;
extern crate mod_mgmt;
extern crate acpi;

use core::ops::DerefMut;
use alloc::{Vec, String};
use alloc::slice::SliceConcatExt;
use alloc::string::ToString;
use getopts::Options;
use memory::{get_module, get_module_starting_with};
use mod_mgmt::SwapRequest;
use acpi::get_hpet;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose logging of crate swapping actions");


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

    let verbose = matches.opt_present("v");

    let matches = matches.free.join(" ");
    println!("matches: {}", matches);

    let tuples = match parse_input_tuples(&matches) {
        Ok(t)  => t,
        Err(e) => {
            println!("Error: {}", e);
            print_usage(opts);
            return -1;
        }
    };

    println!("tuples: {:?}", tuples);

    match swap_modules(tuples, verbose) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }
    }
}


/// Takes a string of arguments and parses it into a series of tuples, formatted as 
/// `(OLD,NEW[,REEXPORT]) (OLD,NEW[,REEXPORT]) (OLD,NEW[,REEXPORT])...`
fn parse_input_tuples<'a>(args: &'a str) -> Result<Vec<(&'a str, &'a str, bool)>, String> {
    let mut v: Vec<(&str, &str, bool)> = Vec::new();
    let mut open_paren_iter = args.match_indices('(');

    // looking for open parenthesis
    while let Some((paren_start, _)) = open_paren_iter.next() {
        let the_rest = args.get((paren_start + 1) ..).ok_or_else(|| "unmatched open parenthesis.".to_string())?;
        // find close parenthesis
        let parsed = the_rest.find(')')
            .and_then(|end_index| the_rest.get(.. end_index))
            .and_then(|inside_paren| {
                let mut token_iter = inside_paren.split(',').map(str::trim);
                token_iter.next().and_then(|first| {
                    token_iter.next().map(|second| {
                        (first, second, token_iter.next())
                    })
                })
            });
        match parsed {
            Some((o, n, reexport)) => {
                println!("found triple: {:?}, {:?}, {:?}", o, n, reexport);
                let reexport_bool = match reexport {
                    Some("true")  => true, 
                    Some("yes")   => true, 
                    Some("y")     => true, 
                    _             => false,
                };
                v.push((o, n, reexport_bool));
            }
            _ => return Err("list of module pairs is formatted incorrectly.".to_string()),
        }
    }

    if v.is_empty() {
        Err("no module pairs specified.".to_string())
    }
    else {
        Ok(v)
    }
}


/// Performs the actual swapping of modules.
fn swap_modules(tuples: Vec<(&str, &str, bool)>, verbose_log: bool) -> Result<(), String> {
    let namespace = mod_mgmt::get_default_namespace();

    let swap_requests = {
        let mut mods: Vec<SwapRequest> = Vec::with_capacity(tuples.len());
        for (o, n, r) in tuples {
            // 1) check that the old crate exists
            let old_crate = namespace.get_crate_starting_with(o)
                .ok_or_else(|| format!("Couldn't find old crate that matched {:?}", o))?;

            // 2) check that the new module file exists
            let new_crate_module = get_module(n)
                .ok_or_else(|| format!("Couldn't find new module file {:?}.", n))
                .or_else(|_| get_module_starting_with(n)
                    .ok_or_else(|| format!("Couldn't find single fuzzy match for new module file {:?}.", n))
                )?;
            mods.push(SwapRequest::new(old_crate.lock_as_ref().crate_name.clone(), new_crate_module, r));
        }
        mods
    };

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;
    let mut kernel_mmi = kernel_mmi_ref.lock();
    
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    let swap_result = namespace.swap_crates(
        swap_requests, 
        kernel_mmi.deref_mut(), 
        verbose_log
    );
    
    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();

    let elapsed_ticks = end - start;
    println!("Swap operation complete. Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
        elapsed_ticks, hpet_period);


    swap_result.map_err(|e| e.to_string())
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: swap (OLD1, NEW1 [, true | false]) [(OLD2, NEW2 [, true | false])]...
Swaps the given list of crate-module tuples, with NEW# replacing OLD# in each tuple.
The OLD value is a crate name (\"my_crate-<hash>\"), whereas the NEW value is a module file name (\"k#my_crate-<hash>\").
Both the old crate name and the new module name can be autocompleted, e.g., \"my_cra\" will find \"my_crate-<hash>\" 
if there is only ONE crate or module file that matched \"my_cra\".
A third element of each tuple is the optional 'reexport_new_symbols_as_old' boolean, which if true, 
will fuzzily match symbols across the old and new crate to equivocate symbols that are equal except for their hashes.";
