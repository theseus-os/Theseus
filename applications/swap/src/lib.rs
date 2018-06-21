//! This application is for performing module management, such as swapping.


#![no_std]
#![feature(alloc)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate console;
extern crate itertools;

extern crate getopts;
extern crate memory;
extern crate mod_mgmt;

use alloc::{Vec, String};
use alloc::slice::SliceConcatExt;
use alloc::string::ToString;
use getopts::Options;
use memory::{get_module, ModuleArea};
use itertools::Itertools;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");


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

    match swap_modules(mod_pairs) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }
    }
}


fn parse_module_pairs<'a>(args: &'a str) -> Result<Vec<(&'a str, &'a str)>, String> {
    let mut v: Vec<(&str, &str)> = Vec::new();
    let mut open_paren_iter = args.match_indices('(');

    // looking for open parenthesis
    while let Some((paren_start, _)) = open_paren_iter.next() {
        let the_rest = args.get((paren_start + 1) ..).ok_or("unmatched open parenthesis.".to_string())?;
        // find close parenthesis
        let parsed = the_rest.find(')')
            .and_then(|end_index| the_rest.get(.. end_index))
            .and_then(|inside_paren| {
                inside_paren.split(',')
                    .map(str::trim)
                    .next_tuple()
            });
        if let Some((o, n)) = parsed {
            println!("found pair: {},{}", o, n);
            v.push((o, n));
        }
        else {
            return Err("list of module pairs is formatted incorrectly.".to_string());
        }
    }

    if v.is_empty() {
        Err("no module pairs specified.".to_string())
    }
    else {
        Ok(v)
    }
}


fn swap_modules(pairs: Vec<(&str, &str)>) -> Result<(), String> {
    let modules = {
        let mut mods: Vec<(&ModuleArea, &ModuleArea)> = Vec::with_capacity(pairs.len());
        for (o, n) in pairs {
            mods.push(
                (
                    get_module(o).ok_or(format!("Couldn't find old module \"{}\".", o))?,
                    get_module(n).ok_or(format!("Couldn't find new module \"{}\".", n))?
                )
            );
        }
        mods
    };

    for (old_mod, new_mod) in modules {
        println!("Replacing old module {:?} with new module {:?}", old_mod.name(), new_mod.name());
    }

    Err("swap_modules() is unimplemented!".to_string())

}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: swap (OLD1,NEW1) [(OLD2,NEW2)]...
Swaps the pairwise list of modules, with NEW# replacing OLD# in each pair.";
