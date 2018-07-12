
//! This application is for performing module management, such as swapping.


#![no_std]
#![feature(alloc)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate print;
extern crate itertools;

extern crate getopts;
extern crate memory;
extern crate mod_mgmt;

use core::ops::DerefMut;
use alloc::{Vec, String, BTreeMap};
use alloc::slice::SliceConcatExt;
use alloc::string::ToString;
use getopts::Options;
use memory::{get_module, ModuleArea};
use mod_mgmt::metadata::StrongCrateRef;
use itertools::Itertools;


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


/// Takes a string of arguments and parses it into a series of pairs, formatted as 
/// `(OLD,NEW) (OLD,NEW) (OLD,NEW)...`
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
fn swap_modules(pairs: Vec<(&str, &str, Option<String>)>, verbose_log: bool) -> Result<(), String> {
    let swap_pairs = {
        let mut mods: Vec<(StrongCrateRef, &ModuleArea, Option<String>)> = Vec::with_capacity(pairs.len());
        for (o, n, override_name) in pairs {
            println!("   Looking for ({},{})  [override: {:?}]", o, n, override_name);
            mods.push(
                (
                    mod_mgmt::get_default_namespace().get_crate(o).ok_or_else(|| format!("Couldn't find old crate \"{}\".", o))?,
                    get_module(n).ok_or_else(|| format!("Couldn't find new module file \"{}\".", n))?,
                    override_name
                )
            );
        }
        mods
    };

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;
    let mut kernel_mmi = kernel_mmi_ref.lock();
    mod_mgmt::get_default_namespace().swap_crates(
        swap_pairs, 
        kernel_mmi.deref_mut(), 
        verbose_log
    ).map_err(|e| e.to_string())
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: swap (OLD1,NEW1[,NEW_NAME1]) [(OLD2,NEW2[,NEW_NAME2])]...
Swaps the pairwise list of modules, with NEW# replacing OLD# in each pair.
The OLD value is a crate name (\"my_crate\"), whereas the NEW value is a module file name (\"__k_my_crate\").
A NEW_NAME string is optional, which will override the crate name derived from the NEW module.";
