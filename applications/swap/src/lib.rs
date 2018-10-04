
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
use memory::get_module;
use mod_mgmt::SwapRequest;
use mod_mgmt::metadata::CrateType;
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

    let mod_pairs = match parse_module_pairs(&matches) {
        Ok(pairs) => pairs,
        Err(e) => {
            println!("Error: {}", e);
            print_usage(opts);
            return -1;
        }
    };

    println!("mod_pairs: {:?}", mod_pairs);

    match swap_modules(mod_pairs, verbose) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }
    }
}


/// Takes a string of arguments and parses it into a series of triples, formatted as 
/// `(OLD,NEW,OVERRIDE) (OLD,NEW,OVERRIDE) (OLD,NEW,OVERRIDE)...`
fn parse_module_pairs<'a>(args: &'a str) -> Result<Vec<(&'a str, &'a str, Option<String>)>, String> {
    let mut v: Vec<(&str, &str, Option<String>)> = Vec::new();
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
            Some((o, n, override_name)) => {
                println!("found triple: {:?}, {:?}, {:?}", o, n, override_name);
                v.push((o, n, override_name.map(|n| n.to_string())));
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
fn swap_modules(tuples: Vec<(&str, &str, Option<String>)>, verbose_log: bool) -> Result<(), String> {
    let swap_requests = {
        let mut mods: Vec<SwapRequest> = Vec::with_capacity(tuples.len());
        for (o, n, override_name) in tuples {
            println!("   Looking for ({},{})  [override: {:?}]", o, n, override_name);
            let new_crate_module = get_module(n).ok_or_else(|| format!("Couldn't find new module file \"{}\".", n))?;
            let new_crate_name = if let Some(new_name) = override_name {
                new_name
            } else {
                CrateType::from_module_name(new_crate_module.name())?.1.to_string()
            };
            mods.push(SwapRequest::new(String::from(o), new_crate_module, new_crate_name));
        }
        mods
    };

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;
    let mut kernel_mmi = kernel_mmi_ref.lock();
    
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    let swap_result = mod_mgmt::get_default_namespace().swap_crates(
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


const USAGE: &'static str = "Usage: swap (OLD1,NEW1[,NEW_NAME1]) [(OLD2,NEW2[,NEW_NAME2])]...
Swaps the pairwise list of modules, with NEW# replacing OLD# in each pair.
The OLD value is a crate name (\"my_crate\"), whereas the NEW value is a module file name (\"k#my_crate\").
A NEW_NAME string is optional, which will override the crate name derived from the NEW module.";
