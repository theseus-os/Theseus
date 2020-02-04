
//! This application is for performing crate management, such as swapping.


#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate itertools;

extern crate getopts;
extern crate memory;
extern crate mod_mgmt;
extern crate hpet;
extern crate task;
extern crate path;
extern crate fs_node;

use alloc::{
    string::{String, ToString},
    vec::Vec,
    sync::Arc,
};
use getopts::{Options, Matches};
use mod_mgmt::{CrateNamespace, NamespaceDir, SwapRequest, IntoCrateObjectFile};
use hpet::get_hpet;
use path::Path;
use fs_node::{FileOrDir, DirRef};


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    #[cfg(not(loadable))] {
        println!("****************\nWARNING: Theseus was not built in 'loadable' mode, so crate swapping may not work.\n****************");
    }

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose logging of crate swapping actions");
    opts.optopt("d", "directory-crates", "the absolute path of the base directory where new crates will be loaded from", "PATH");
    opts.optmulti("t", "state-transfer", "the fully-qualified symbol names of state transfer functions, to be run in the order given", "SYMBOL");

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

    let override_namespace_crate_dir = if let Some(path) = matches.opt_str("d") {
        let path = Path::new(path);
        let dir = match path.get(&curr_dir) {
            Some(FileOrDir::Dir(dir)) => dir,
            _ => return Err(format!("Error: could not find specified namespace crate directory: {}.", path)),
        };
        Some(NamespaceDir::new(dir))
    } else {
        None
    };

    let verbose = matches.opt_present("v");
    let state_transfer_functions = matches.opt_strs("t");

    let free_args = matches.free.join(" ");
    println!("arguments: {}", free_args);

    let tuples = parse_input_tuples(&free_args)?;
    println!("tuples: {:?}", tuples);
        

    do_swap(
        tuples, 
        &curr_dir, 
        override_namespace_crate_dir,
        state_transfer_functions,
        verbose
    )
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
            _ => return Err("list of crate tuples is formatted incorrectly.".to_string()),
        }
    }

    if v.is_empty() {
        Err("no crate tuples specified.".to_string())
    }
    else {
        Ok(v)
    }
}


/// Performs the actual swapping of crates.
fn do_swap(
    tuples: Vec<(&str, &str, bool)>, 
    curr_dir: &DirRef, 
    override_namespace_crate_dir: Option<NamespaceDir>, 
    state_transfer_functions: Vec<String>,
    verbose_log: bool
) -> Result<(), String> {
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;
    let namespace = task::get_my_current_task().ok_or("Couldn't get current task")?.get_namespace();

    let swap_requests = {
        let mut requests: Vec<SwapRequest> = Vec::with_capacity(tuples.len());
        for (old_crate_name, new_crate_str, reexport) in tuples {

            // Check that the new crate file exists. It could be a regular path, or a prefix for a file in the namespace's dir.
            // If it's a full path, then we just check that the path points to a valid crate object file. 
            // Otherwise, we treat it as a prefix for a crate object file name that may be found 
            let (into_new_crate_file, new_namespace) = {
                if let Some(f) = override_namespace_crate_dir.as_ref().and_then(|ns_dir| ns_dir.get_file_starting_with(new_crate_str)) {
                    (IntoCrateObjectFile::File(f), None)
                } else if let Some(FileOrDir::File(f)) = Path::new(String::from(new_crate_str)).get(curr_dir) {
                    (IntoCrateObjectFile::File(f), None)
                } else {
                    (IntoCrateObjectFile::Prefix(String::from(new_crate_str)), None)
                }
            };
            
            let swap_req = SwapRequest::new(
                Some(old_crate_name),
                Arc::clone(&namespace),
                into_new_crate_file,
                new_namespace,
                reexport
            ).map_err(|_e| 
                format!("Failed to create SwapRequest: {:?}", _e)
                // use the below code to print out multiple matches
                // match CrateNamespace::get_crate_object_files_starting_with(&namespace, n).as_slice() {
                //     [(file, ns)] => (file.clone(), Some(Arc::clone(ns))),
                //     multiple_files => {
                //         let mut err_str = format!("Couldn't find single match for the new crate file {:?}. Matching files:", n);
                //         for (f, ns) in multiple_files {
                //             err_str = format!("{}\n    {} in namespace {}", err_str, f.lock().get_absolute_path(), ns.name);
                //         }
                //         return Err(err_str);
                //     }
                // }
            )?;
            requests.push(swap_req);
        }
        requests
    };
    
    let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

    let swap_result = CrateNamespace::swap_crates(
        &namespace,
        swap_requests, 
        override_namespace_crate_dir,
        state_transfer_functions,
        &kernel_mmi_ref,
        verbose_log
    );
    
    let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
    let hpet_period = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds();

    let elapsed_ticks = end - start;
    
    
    match swap_result {
        Ok(()) => {
            println!("Swap operation complete. Elapsed HPET ticks: {}, (HPET Period: {} femtoseconds)", 
                elapsed_ticks, hpet_period);
            Ok(())
        }
        Err(e) => Err(e.to_string())
    }
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: swap (OLD1, NEW1 [, true | false]) [(OLD2, NEW2 [, true | false])]...
Swaps the given list of crate tuples, with NEW# replacing OLD# in each tuple.
The OLD and NEW values are crate names, such as \"my_crate-<hash>\".
Both the old crate name and the new crate name can be prefixes, e.g., \"my_cra\" will find \"my_crate-<hash>\", 
but *only* if there is a single matching crate or object file.
A third element of each tuple is the optional 'reexport_new_symbols_as_old' boolean, which if true, 
will reexport new symbols under their old names, if those symbols match (excluding hashes).";
