//! This application allows querying about and interacting with namespaces in Theseus, 
//! specifically `CrateNamespace`s.

#![no_std]
extern crate alloc;
#[macro_use] extern crate app_io;

extern crate getopts;
extern crate memory;
extern crate task;
extern crate mod_mgmt;
extern crate fs_node;
extern crate path;

use core::{
    ops::Deref,
    fmt::Write,
};
use alloc::{
    string::String,
    string::ToString,
    sync::Arc,
    vec::Vec,
};
use getopts::{Options, Matches};
use mod_mgmt::CrateNamespace;
use fs_node::FileRef;
use path::Path;


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("r", "recursive", "include recursive namespaces");
    opts.optflag("f", "files", "lists crate object files available in this namespace rather than currently-loaded crates");
    opts.optopt("", "load", "load a crate into the current namespace. Ignores all other arguments.", "CRATE_OBJ_FILE_PATH");

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
    let (namespace, curr_wd) = task::with_current_task(|t|
        (t.get_namespace().clone(), t.get_env().lock().working_dir.clone())
    ).map_err(|_| String::from("failed to get current task"))?;

    let recursive = matches.opt_present("r");
    let mut output = String::new();

    if let Some(crate_obj_file_path) = matches.opt_str("load") {
        let path = Path::new(crate_obj_file_path);
        let file = path.get_file(&curr_wd).ok_or_else(||
            format!("Couldn't resolve path to crate object file at {path:?}")
        )?;
        load_crate(&mut output, file, &namespace)?;
    } else if matches.opt_present("f") {
        print_files(&mut output, 0, namespace.deref(), recursive)
            .map_err(|_e| String::from("String formatting error"))?;
    } else {
        print_crates(&mut output, 0, namespace.deref(), recursive)
            .map_err(|_e| String::from("String formatting error"))?;
    }

    println!("{}", output);
    Ok(())
}


fn load_crate(output: &mut String, crate_file_ref: FileRef, namespace: &Arc<CrateNamespace>) -> Result<(), String> {
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "Cannot get kernel_mmi_ref".to_string())?;
    let (_new_crate_ref, _new_syms) = namespace.load_crate(
        &crate_file_ref,
        None,
        kernel_mmi_ref,
        false
    ).map_err(String::from)?;
    writeln!(output, "Loaded crate with {} new symbols from {}", _new_syms, crate_file_ref.lock().get_absolute_path()).unwrap();
    Ok(())
}


fn print_files(output: &mut String, indent: usize, namespace: &CrateNamespace, recursive: bool) -> core::fmt::Result {
    writeln!(output, "\n{:indent$}{} CrateNamespace has crate object files:", "", namespace.name(), indent = indent)?;
    let mut files = namespace.dir().lock().list();
    files.sort();
    for f in files {
        writeln!(output, "{:indent$}{}", "", f, indent = (indent + 4))?;
    }

    if recursive {
        if let Some(r_ns) = namespace.recursive_namespace() {
            print_files(output, indent + 2, r_ns.deref(), recursive)?;
        }
    }

    Ok(())
}


fn print_crates(output: &mut String, indent: usize, namespace: &CrateNamespace, recursive: bool) -> core::fmt::Result {
    writeln!(output, "\n{:indent$}{} CrateNamespace has loaded crates:", "", namespace.name(), indent = indent)?;
    let mut crates: Vec<String> = Vec::new();
    // We do recursion manually here so we can separately print each recursive namespace.
    namespace.for_each_crate(false, |crate_name, crate_ref| {
        crates.push(format!("{:indent$}{}     {:?}", "", crate_name, crate_ref.lock_as_ref().object_file.lock().get_absolute_path(), indent = (indent + 4)));
        true
    });
    crates.sort();
    for c in crates {
        writeln!(output, "{c}")?;
    }

    if recursive {
        if let Some(r_ns) = namespace.recursive_namespace() {
            print_crates(output, indent + 2, r_ns.deref(), recursive)?;
        }
    }

    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &str = "\nUsage: ns [OPTION]
Lists the crates that are loaded in the currently-active crate namespace.";
