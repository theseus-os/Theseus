//! This application is mostly for debugging usage, and allows a developer
//! to explore live dependencies between crates and sections at runtime.


#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate itertools;

extern crate getopts;
extern crate task;
extern crate memory;
extern crate mod_mgmt;
extern crate crate_name_utils;
extern crate spin;


use alloc::{
    string::{String},
    vec::Vec,
};
use spin::Once;
use getopts::{Matches, Options};
use mod_mgmt::{
    metadata::{StrongCrateRef, StrongSectionRef},
    CrateNamespace,
};
use crate_name_utils::get_containing_crate_name;


static VERBOSE: Once<bool> = Once::new();
macro_rules! verbose {
    () => (VERBOSE.try() == Some(&true));
}


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose output");
    opts.optopt ("s", "sections-in",  "output the sections that depend on the given SECTION (incoming weak dependents)",      "SECTION");
    opts.optopt ("S", "sections-out", "output the sections that the given SECTION depends on (outgoing strong dependencies)", "SECTION");
    opts.optopt ("c", "crates-in",    "output the crates that depend on the given CRATE (incoming weak dependents)",          "CRATE");
    opts.optopt ("C", "crates-out",   "output the crates that the given CRATE depends on (outgoing strong dependencies)",     "CRATE");
    

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

    VERBOSE.call_once(|| matches.opt_present("v"));

    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error:\n{}", e);
            -1
        }    
    }
}


fn rmain(matches: Matches) -> Result<(), String> {
    if verbose!() { println!("MATCHES: {:?}", matches.free); }

    if let Some(sec_name) = matches.opt_str("s") {
        sections_dependent_on_me(&sec_name)
    }
    else if let Some(sec_name) = matches.opt_str("S") {
        sections_i_depend_on(&sec_name)
    }
    else if let Some(crate_name) = matches.opt_str("c") {
        crates_dependent_on_me(&crate_name)
    }
    else if let Some(crate_name) = matches.opt_str("C") {
        crates_i_depend_on(&crate_name)
    }
    else {
        Err(format!("no supported options/arguments found."))
    }
}

/// Outputs the given section's weak dependents, i.e.,
/// the sections that depend on the given section.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn sections_dependent_on_me(section_name: &str) -> Result<(), String> {
    let sec_ref = find_section(section_name)?;
    println!("Sections that depend on {}  (weak dependents):", sec_ref.lock().name);
    for dependent_sec_ref in sec_ref.lock().sections_dependent_on_me.iter().filter_map(|weak_dep| weak_dep.section.upgrade()) {
        let dependent_sec = dependent_sec_ref.lock();
        if verbose!() { 
            println!("    {}  in {:?}", dependent_sec.name, dependent_sec.parent_crate.upgrade());
        } else {
            println!("    {}", dependent_sec.name);
        }
    }
    Ok(())
}


/// Outputs the given section's strong dependencies, i.e.,
/// the sections that the given section depends on.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn sections_i_depend_on(section_name: &str) -> Result<(), String> {
    let sec_ref = find_section(section_name)?;
    println!("Sections that {} depends on  (strong dependencies):", sec_ref.lock().name);
    for dependency_sec_ref in sec_ref.lock().sections_i_depend_on.iter().map(|dep| &dep.section) {
        let dependency_sec = dependency_sec_ref.lock();
        if verbose!() { 
            println!("    {}  in {:?}", dependency_sec.name, dependency_sec.parent_crate.upgrade());
        } else {
            println!("    {}", dependency_sec.name);
        }
    }
    Ok(())
}


/// Outputs the given crate's weak dependents, i.e.,
/// the crates that depend on the given crate.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching crate names separated by the newline character `'\n'`.
fn crates_dependent_on_me(_crate_name: &str) -> Result<(), String> {
    Err(format!("unimplemented"))
}


/// Outputs the given crate's strong dependencies, i.e.,
/// the crates that the given crate depends on.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching crate names separated by the newline character `'\n'`.
fn crates_i_depend_on(_crate_name: &str) -> Result<(), String> {
    Err(format!("unimplemented"))
}


/// Returns the section matching the given `section_name` if there is a single match.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn find_section(section_name: &str) -> Result<StrongSectionRef, String> {
    let namespace = get_my_current_namespace();
    let matching_symbols = namespace.find_symbols_starting_with(section_name);
    if matching_symbols.len() == 1 {
        return matching_symbols[0].1.upgrade()
            .ok_or_else(|| format!("Found matching symbol name but couldn't get reference to section"));
    } else if matching_symbols.len() > 1 {
        return Err(matching_symbols.into_iter().map(|(k, _v)| k).collect::<Vec<String>>().join("\n"));
    } else {
        // continue on
    }

    // If it wasn't a global section in the symbol map, then we need to find its containing crate
    // and search that crate's symbols manually.
    let containing_crate_ref = get_containing_crate_name(section_name).get(0)
        .and_then(|cname| namespace.get_crate_starting_with(&format!("{}-", cname)))
        .or_else(|| get_containing_crate_name(section_name).get(1)
            .and_then(|cname| namespace.get_crate_starting_with(&format!("{}-", cname)))
        )
        .map(|(_cname, crate_ref, _ns)| crate_ref)
        .ok_or_else(|| format!("Couldn't find section {} in symbol map, and couldn't get its containing crate", section_name))?;

    let mut matching_sections: Vec<(String, StrongSectionRef)> = containing_crate_ref.lock_as_ref().sections.values()
        .filter_map(|sec_ref| {
            let sec_name = sec_ref.lock().name.clone();
            if sec_name.starts_with(section_name) {
                Some((sec_name, sec_ref.clone()))
            } else {
                None 
            }
        })
        .collect();

    if matching_sections.len() == 1 { 
        Ok(matching_sections.remove(0).1)
    } else {
        Err(matching_sections.into_iter().map(|(name, _)| name).collect::<Vec<String>>().join("\n"))
    }
}


/// Returns the crate matching the given `crate_name` if there is a single match.
/// 
/// If there are multiple matches, this returns an Error containing all of the matching crate names,
/// separated by the newline character `'\n'`.
fn _find_crate(crate_name: &str) -> Result<StrongCrateRef, String> {
    let namespace = get_my_current_namespace();
    let matching_crate_names: Vec<String> = namespace.crate_names(true)
        .into_iter()
        .filter(|name| name.starts_with(crate_name))
        .collect();
    
    if matching_crate_names.len() == 1 {
        namespace.get_crate(&matching_crate_names[0])
            .ok_or_else(|| format!("Couldn't get crate {} from namespace {}", matching_crate_names[0], namespace.name))
    } else {
        Err(matching_crate_names.join("\n"))
    }
}


// TODO: fix this later once each task's environment contains a current namespace
fn get_my_current_namespace() -> &'static CrateNamespace {
    mod_mgmt::get_default_namespace().unwrap()
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: deps OPTION ARG
Outputs runtime dependency information and metadata known by Theseus's crate manager.";
