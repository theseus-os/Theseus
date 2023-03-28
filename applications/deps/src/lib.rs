//! This application is mostly for debugging usage, and allows a developer
//! to explore live dependencies between crates and sections at runtime.


#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
extern crate alloc;
#[macro_use] extern crate app_io;
extern crate itertools;

extern crate getopts;
extern crate task;
extern crate memory;
extern crate mod_mgmt;
extern crate crate_name_utils;
extern crate spin;


use alloc::{
    collections::BTreeSet,
    string::{
        String,
        ToString,
    },
    vec::Vec,
    sync::Arc,
};
use memory::VirtualAddress;
use spin::Once;
use getopts::{Matches, Options};
use mod_mgmt::{
    StrongCrateRef,
    StrongSectionRef,
    CrateNamespace, StrRef,
};
use crate_name_utils::get_containing_crate_name;


/// calls println!() and then log!()
macro_rules! println_log {
    ($fmt:expr) => {
        debug!($fmt);
        print!(concat!($fmt, "\n"));
    };
    ($fmt:expr, $($arg:tt)*) => {
        debug!($fmt, $($arg)*);
        print!(concat!($fmt, "\n"), $($arg)*);
    };
}


static VERBOSE: Once<bool> = Once::new();
macro_rules! verbose {
    () => (VERBOSE.get() == Some(&true));
}


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help",             "print this help menu");
    opts.optflag("v", "verbose",          "enable verbose output");
    opts.optopt ("a", "address",          "output the section that contains the given ADDRESS",      "ADDRESS");
    opts.optopt ("s", "sections-in",      "output the sections that depend on the given SECTION (incoming weak dependents)",      "SECTION");
    opts.optopt ("S", "sections-out",     "output the sections that the given SECTION depends on (outgoing strong dependencies)", "SECTION");
    opts.optopt ("c", "crates-in",        "output the crates that depend on the given CRATE (incoming weak dependents)",          "CRATE");
    opts.optopt ("C", "crates-out",       "output the crates that the given CRATE depends on (outgoing strong dependencies)",     "CRATE");
    opts.optopt ("l", "list",             "list the public sections in the given crate", "CRATE");
    opts.optopt ("",  "list-all",         "list all sections in the given crate", "CRATE");
    opts.optopt ("",  "num-deps-crate",   "sum up the count of all dependencies for the given crate", "CRATE");
    opts.optopt ("",  "num-deps-section", "sum up the count of all dependencies for the given section", "SECTION");
    opts.optflag("",  "num-deps-all",     "sum up the count of all dependencies for all crates");
    opts.optflag("",  "num-rodata",       "count the private .rodata sections for all crates");
    

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

    if let Some(addr) = matches.opt_str("a") {
        section_containing_address(&addr)
    }
    else if let Some(sec_name) = matches.opt_str("s") {
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
    else if let Some(crate_name) = matches.opt_str("l") {
        sections_in_crate(&crate_name, false)
    }
    else if let Some(crate_name) = matches.opt_str("list-all") {
        sections_in_crate(&crate_name, true)
    }
    else if let Some(crate_name) = matches.opt_str("num-deps-crate") {
        num_deps_crate(&crate_name)
    }
    else if let Some(crate_name) = matches.opt_str("num-deps-section") {
        num_deps_section(&crate_name)
    }
    else if matches.opt_present("num-deps-all") {
        num_deps_all()
    }
    else if matches.opt_present("num-rodata") {
        count_private_rodata_sections()
    }
    else {
        Err("no supported options/arguments found.".to_string())
    }
}



/// Outputs the section containing the given address, i.e., symbolication.
/// 
fn section_containing_address(addr: &str) -> Result<(), String> {
    let addr = if addr.starts_with("0x") || addr.starts_with("0X") {
        &addr[2..]
    } else {
        addr
    };
    
    let virt_addr = VirtualAddress::new(
        usize::from_str_radix(addr, 16)
            .map_err(|_| format!("Error: address {addr:?} is not a valid hexademical usize value"))?
    ).ok_or_else(|| format!("Error: address {addr:?} is not a valid VirtualAddress"))?;

    if let Some((sec, offset)) = get_my_current_namespace().get_section_containing_address(virt_addr, false) {
        println!("Found {:>#018X} in {} + {:#X}, typ: {:?}", virt_addr, sec.name, offset, sec.typ);
        Ok(())
    } else {
        Err(format!("Couldn't find section containing address {virt_addr:>#018X}"))
    }
}

/// Outputs the given section's weak dependents, i.e.,
/// the sections that depend on the given section.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn sections_dependent_on_me(section_name: &str) -> Result<(), String> {
    let sec = find_section(section_name)?;
    println!("Sections that depend on {}  (weak dependents):", sec.name);
    for dependent_sec in sec.inner.read().sections_dependent_on_me.iter().filter_map(|weak_dep| weak_dep.section.upgrade()) {
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
    let sec = find_section(section_name)?;
    println!("Sections that {} depends on  (strong dependencies):", sec.name);
    for dependency_sec in sec.inner.read().sections_i_depend_on.iter().map(|dep| &dep.section) {
        if verbose!() { 
            println!("    {}  in {:?}", dependency_sec.name, dependency_sec.parent_crate.upgrade());
        } else {
            println!("    {}", dependency_sec.name);
        }
    }
    Ok(())
}


fn num_deps_crate(crate_name: &str) -> Result<(), String> {
    let (_cn, crate_ref) = find_crate(crate_name)?;
    let (s, w, i) = crate_dependency_count(&crate_ref);
    println!("Crate {}'s Dependency Count:\nStrong: {}\nWeak:   {}\nIntrnl: {}", crate_name, s, w, i);
    Ok(())
}

fn num_deps_section(section_name: &str) -> Result<(), String> {
    let section = find_section(section_name)?;
    let (s, w, i) = section_dependency_count(&section);
    println!("Section {}'s Dependency Count:\nStrong: {}\nWeak:   {}\nIntrnl: {}", section_name, s, w, i);
    Ok(())
}

fn num_deps_all() -> Result<(), String> {
    let namespace = get_my_current_namespace();
    let (mut s_total, mut w_total, mut i_total) = (0, 0, 0);
    let mut crate_count = 0;
    let mut section_count = 0;
    namespace.for_each_crate(true, |_crate_name, crate_ref| {
        crate_count += 1;
        let (s, w, i) = crate_dependency_count(crate_ref);
        section_count += crate_ref.lock_as_ref().sections.len();
        s_total += s;
        w_total += w;
        i_total += i;
        true // keep going
    });

    println!("Total Dependency Count for all {} crates ({} sections):\nStrong: {}\nWeak:   {}\nIntrnl: {}",
        crate_count, 
        section_count,    
        s_total, w_total, i_total
    );
    Ok(())
}

fn count_private_rodata_sections() -> Result<(), String> {
    let namespace = get_my_current_namespace();
    let mut private_rodata = 0;
    let mut public_rodata = 0;
    let mut crate_count = 0;
    let mut section_count = 0;
    let mut discardable = 0;
    namespace.for_each_crate(true, |_crate_name, crate_ref| {
        crate_count += 1;
        let mut prv = 0;
        let mut publ = 0;
        let mut disc = 0;
        for sec in crate_ref.lock_as_ref().sections.values().filter(|sec| sec.typ == mod_mgmt::SectionType::Rodata) {
            section_count += 1;
            let mut can_discard = true;
            if sec.global {
                trace!("\t public .rodata {:?}", sec);
                publ += 1;
                can_discard = false;
            } else {
                prv += 1;
                for strong_dep in sec.inner.read().sections_i_depend_on.iter() {
                    trace!("Private .rodata {:?} depends on {:?}", sec, strong_dep.section);
                    can_discard = false;
                }
                for weak_dep in sec.inner.read().sections_dependent_on_me.iter() {
                    error!("Logic error: Private .rodata {:?} has dependent {:?}", sec, weak_dep.section.upgrade());
                    can_discard = false;
                }
            }
            if can_discard {
                disc += 1;
            }
        }
        debug!("Crate {} has rodata sections: {} public, {} private, {} discardable", _crate_name, publ, prv, disc);
        private_rodata += prv;
        public_rodata += publ;
        discardable += disc;
        true // keep going
    });

    println!("Total of {} .rodata sections for all {} crates:  {} public, {} private, {} discardable",
        section_count,    
        crate_count, 
        public_rodata, 
        private_rodata,
        discardable
    );
    Ok(())
}

/// Returns the count of `(strong dependencies, weak dependents, internal dependencies)`
/// for all sections in the given crate. .
fn crate_dependency_count(crate_ref: &StrongCrateRef) -> (usize, usize, usize) {
    let res = crate_ref.lock_as_ref().sections.values()
        .map(section_dependency_count)
        .fold((0, 0, 0), |(acc_s, acc_w, acc_i), (s, w, i)| (acc_s + s, acc_w + w, acc_i + i));
    // trace!("crate {:?} has deps {:?}", crate_ref, res);
    res
}

/// Returns the given section's count of `(strong dependencies, weak dependents, internal dependencies)`.
fn section_dependency_count(sec: &StrongSectionRef) -> (usize, usize, usize) {
    let inner = sec.inner.read();
    (
        inner.sections_i_depend_on.len(),
        inner.sections_dependent_on_me.len(),
        #[cfg(internal_deps)]
        inner.internal_dependencies.len(),
        #[cfg(not(internal_deps))]
        0,
    )
}

/// Outputs the given crate's weak dependents, i.e.,
/// the crates that depend on the given crate.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching crate names separated by the newline character `'\n'`.
fn crates_dependent_on_me(_crate_name: &str) -> Result<(), String> {
    Err("unimplemented".to_string())
}


/// Outputs the given crate's strong dependencies, i.e.,
/// the crates that the given crate depends on.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching crate names separated by the newline character `'\n'`.
fn crates_i_depend_on(crate_prefix: &str) -> Result<(), String> {
    let (crate_name, crate_ref) = find_crate(crate_prefix)?;
    let mut crate_list = crate_ref
        .lock_as_ref()
        .crates_i_depend_on()
        .iter()
        .filter_map(|wc| wc.upgrade().map(|c| c.lock_as_ref().crate_name.clone()))
        .collect::<Vec<_>>();

    crate_list.sort_unstable();
    crate_list.dedup();


    println!("Crate {} has direct dependences:\n  {}", crate_name, crate_list.join("\n  "));
    Ok(())
}



/// Outputs the list of sections in the given crate.
/// 
/// # Arguments
/// * `all_sections`: If `true`, then all sections will be printed. 
///                   If `false`, then only public (global) sections will be printed.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn sections_in_crate(crate_name: &str, all_sections: bool) -> Result<(), String> {
    let (crate_name, crate_ref) = find_crate(crate_name)?;

    let mut containing_crates = BTreeSet::new();
    if all_sections {
        println_log!("Sections (all) in crate {}:", crate_name);
        for sec in crate_ref.lock_as_ref().sections.values() {
            println_log!("    {}", sec.name);
            for n in get_containing_crate_name(&sec.name) {
                containing_crates.insert(String::from(n));
            }
        }
    } else {
        println_log!("Sections (public-only) in crate {}:", crate_name);
        for sec in crate_ref.lock_as_ref().global_sections_iter() {
            println_log!("    {}", sec.name);
            for n in get_containing_crate_name(&sec.name) {
                containing_crates.insert(String::from(n));
            }
        }
    }

    let crates_list = containing_crates.into_iter().collect::<Vec<String>>().join("\n");
    println_log!("Constituent (or related) crates:\n{}", &crates_list);
    Ok(())
}


/// Returns the crate matching the given `crate_name` if there is a single match.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn find_crate(crate_name: &str) -> Result<(StrRef, StrongCrateRef), String> {
    let namespace = get_my_current_namespace();
    let mut matching_crates = CrateNamespace::get_crates_starting_with(&namespace, crate_name);
    match matching_crates.len() {
        0 => Err(format!("couldn't find crate matching {crate_name:?}")),
        1 => {
            let mc = matching_crates.swap_remove(0);
            Ok((mc.0, mc.1)) 
        }
        _ => Err(matching_crates.into_iter().map(|(crate_name, _crate_ref, _ns)| crate_name).collect::<Vec<_>>().join("\n")),
    }
}


/// Returns the section matching the given `section_name` if there is a single match.
/// 
/// If there are multiple matches, this returns an Error containing 
/// all of the matching section names separated by the newline character `'\n'`.
fn find_section(section_name: &str) -> Result<StrongSectionRef, String> {
    let namespace = get_my_current_namespace();
    let matching_symbols = namespace.find_symbols_starting_with(section_name);
    match matching_symbols.len() {
        1 => return matching_symbols[0].1
            .upgrade()
            .ok_or_else(|| "Found matching symbol name but couldn't get reference to section".to_string()),
        2.. => return Err(matching_symbols
            .into_iter()
            .map(|(k, _v)| k)
            .collect::<Vec<String>>()
            .join("\n")
        ),
        _ => { /* no matches, continue on */ },
    }

    // If it wasn't a global section in the symbol map, then we need to find its containing crate
    // and search that crate's symbols manually.
    let containing_crate_ref = get_containing_crate_name(section_name).first()
        .and_then(|cname| CrateNamespace::get_crate_starting_with(&namespace, &format!("{cname}-")))
        .or_else(|| get_containing_crate_name(section_name).get(1)
            .and_then(|cname| CrateNamespace::get_crate_starting_with(&namespace, &format!("{cname}-")))
        )
        .map(|(_cname, crate_ref, _ns)| crate_ref)
        .ok_or_else(|| format!("Couldn't find section {section_name} in symbol map, and couldn't get its containing crate"))?;

    let mut matching_sections: Vec<StrongSectionRef> = containing_crate_ref.lock_as_ref().sections.values()
        .filter_map(|sec| {
            if sec.name.starts_with(section_name) {
                Some(sec.clone())
            } else {
                None 
            }
        })
        .collect();

    if matching_sections.len() == 1 { 
        Ok(matching_sections.remove(0))
    } else {
        Err(matching_sections.into_iter().map(|sec| sec.name.clone()).collect::<Vec<_>>().join("\n"))
    }
}


fn get_my_current_namespace() -> Arc<CrateNamespace> {
    task::with_current_task(|t| t.get_namespace().clone())
        .or_else(|_| mod_mgmt::get_initial_kernel_namespace().cloned().ok_or(()))
        .map_err(|_| "couldn't get current task's namespace or default namespace")
        .unwrap()
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &str = "Usage: deps OPTION ARG
Outputs runtime dependency information and metadata known by Theseus's crate manager.";
