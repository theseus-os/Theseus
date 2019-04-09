
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
extern crate task;
extern crate path;
extern crate fs_node;

use core::ops::DerefMut;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    sync::Arc,
    slice::SliceConcatExt,
};
use getopts::{Options, Matches};
use mod_mgmt::{SwapRequest, NamespaceDirectorySet};
use acpi::get_hpet;
use path::Path;
use fs_node::{FileOrDir, FsNode, DirRef};


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose logging of crate swapping actions");
    opts.optopt("d", "directory-crates", "specify the absolute path of the base directory where new crates will be loaded from", "PATH");

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

    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e); 
            -1
        }
    }
}


fn rmain(matches: Matches) -> Result<(), String> {

    let taskref = task::get_my_current_task()
        .ok_or_else(|| format!("failed to get current task"))?;

    let curr_dir = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    let override_namespace_crate_dirs = if let Some(path) = matches.opt_str("d") {
        let path = Path::new(path);
        let base_dir = match path.get(&curr_dir) {
            Ok(FileOrDir::Dir(dir)) => dir,
            _ => return Err(format!("Error: could not find specified namespace crate directory: {}.", path)),
        };
        Some(NamespaceDirectorySet::from_existing_base_dir(base_dir).map_err(|e| e.to_string())?)
    } else {
        None
    };

    let verbose = matches.opt_present("v");

    let matches = matches.free.join(" ");
    println!("matches: {}", matches);

    let tuples = parse_input_tuples(&matches)?;
    println!("tuples: {:?}", tuples);

    swap_modules(tuples, &curr_dir, override_namespace_crate_dirs, verbose)
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


/// Performs the actual swapping of crates.
fn swap_modules(
    tuples: Vec<(&str, &str, bool)>, 
    curr_dir: &DirRef, 
    override_namespace_crate_dirs: Option<NamespaceDirectorySet>, 
    verbose_log: bool
) -> Result<(), String> {
    let namespace = mod_mgmt::get_default_namespace().ok_or("Couldn't get default crate namespace")?;

    let swap_requests = {
        let mut mods: Vec<SwapRequest> = Vec::with_capacity(tuples.len());
        for (o, n, r) in tuples {
            // 1) check that the old crate exists and is loaded into the namespace
            let old_crate = namespace.get_crate_starting_with(o)
                .map(|(_name, crate_ref)| crate_ref)
                .ok_or_else(|| format!("Couldn't find old crate loaded into namespace that matched {:?}", o))?;

            // 2) check that the new crate file exists. It could be a regular path, or a prefix for a file in the namespace's kernel dir
            let new_crate_abs_path = match Path::new(String::from(n)).get(curr_dir) {
                Ok(FileOrDir::File(f)) => Ok(Path::new(f.lock().get_absolute_path())),
                _ => namespace.get_kernel_file_starting_with(n)
                        .and_then(|p| p.get(namespace.dirs().kernel_directory()).map(|f_or_d| Path::new(f_or_d.get_absolute_path())).ok())
                        .ok_or_else(|| format!("Couldn't find new kernel crate file {:?}.", n))
            }?;
            mods.push(
                SwapRequest::new(old_crate.lock_as_ref().crate_name.clone(), new_crate_abs_path, r)
                    .map_err(|_e| format!("BUG: the path of the new crate (passed in as {:?}) was not an absolute Path.", n))?
            );
        }
        mods
    };

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;
    let mut kernel_mmi = kernel_mmi_ref.lock();
    
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    let swap_result = namespace.swap_crates(
        swap_requests, 
        override_namespace_crate_dirs,
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
will reexport new symbols under their old names, if those symbols match (excluding hashes).";
