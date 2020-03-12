//! This crate logs all the faults occuring within Theseus. 
//! Maintains a list of exceptions and panics that has occured since booting up. 
//! This crate does not hold reference to any task or app. 
//! 

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate vga_buffer; // for println_raw!()
#[macro_use] extern crate print; // for regular println!()
extern crate spin; 
extern crate memory;
extern crate hashbrown;
extern crate mod_mgmt;
extern crate fs_node;
extern crate qp_trie;
extern crate path;
extern crate by_address;
extern crate x86_64;

use spin::Mutex;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use memory::VirtualAddress;


/// A data structure to hold information about each fault. 
#[derive(Debug, Clone)]
pub struct FaultEntry {
    /// Machine excpetion number, 0xFF indicates panic
    pub exception_number: u8,

    /// Error code returned with the exception
    pub error_code: u64,

    /// The core error occured
    pub core: Option<u8>,

    /// Task runnning immediately before the Exception
    pub running_task: String,

    /// If available the application crate that spawned the task
    pub running_app_crate: Option<String>,

    /// For page faults the address the program attempted to access. None for other faults
    pub address_accessed: Option<VirtualAddress>,

    /// Address at which exception occured
    pub instruction_pointer : Option<VirtualAddress>,
    
    /// Crate the address at which exception occured located
    pub crate_error_occured : Option<String>,

    /// List of crates reloaded from memory to recover from fault
    pub replaced_crates : Vec<String>,

    /// true if crate reloading occurs as a result of the fault
    pub action_taken : bool,
}


lazy_static! {
    
    /// The structure to hold the list of all faults so far occured in the system
    static ref FAULT_LIST: Mutex<Vec<FaultEntry>> = Mutex::new(Vec::new());
}

/// Clears the log of faults so far occured in the system 
pub fn clear_fault_log() {
    FAULT_LIST.lock().clear();
}

/// Add a new entry to the fault log. 
/// This function requires all the entries in the `FaultEntry` as input.
pub fn add_error_to_fault_log (
    exception_number: u8,
    error_code: u64,
    core: Option<u8>,
    running_task: String,
    running_app_crate: Option<String>,
    address_accessed: Option<VirtualAddress>,
    instruction_pointer : Option<VirtualAddress>,
    crate_error_occured : Option<String>,
    replaced_crates : Vec<String>,
    action_taken : bool,
)-> () {

    let fe = FaultEntry{
        exception_number: exception_number,
        error_code: error_code,
        core: core,
        running_task: running_task,
        running_app_crate: running_app_crate,
        address_accessed: address_accessed,
        instruction_pointer : instruction_pointer,
        crate_error_occured : crate_error_occured,
        replaced_crates : replaced_crates,
        action_taken : action_taken,
    };
    FAULT_LIST.lock().push(fe);
}

/// Add a new entry to the fault log. 
/// This function requires only the `exception_number` and `error_code`. 
/// Other entries will be marked as None to be filled later.
pub fn add_error_simple (
    exception_number: u8,
    error_code: u64,
)-> () {

    let vec :Vec<String> = Vec::new();
    let fe = FaultEntry{
        exception_number: exception_number,
        error_code: error_code,
        core: None,
        running_task: "None".to_string(),
        running_app_crate: None,
        address_accessed: None,
        instruction_pointer : None,
        crate_error_occured : None,
        replaced_crates : vec,
        action_taken : false,
    };
    FAULT_LIST.lock().push(fe);
}

/// Removes the last fault entry from the fault log and returns
/// Is useful when information about the last fault needs to be updated
pub fn get_last_entry () -> Option<FaultEntry> {
    FAULT_LIST.lock().pop()
}

/// calls println!() and then println_raw!()
macro_rules! println_both {
    ($fmt:expr) => {
        print_raw!(concat!($fmt, "\n"));
        print!(concat!($fmt, "\n"));
    };
    ($fmt:expr, $($arg:tt)*) => {
        print_raw!(concat!($fmt, "\n"), $($arg)*);
        print!(concat!($fmt, "\n"), $($arg)*);
    };
}

/// Prints the fault log
pub fn print_fault_log() -> (){
    println_both!("------------------ FAULT LOG ---------------------------");
    let list = FAULT_LIST.lock();
    for x in list.iter() {
        println_both!("{:?}", x);
    }
    println_both!("------------------ END OF LOG --------------------------");
}

/// null crate swap policy.
/// When this policy is enabled no crate swapping is performed
/// However we still update the last error as handled since it is an intended action
pub fn null_swap_policy() -> Option<String> {
    debug!("Running null swap policy");
    let mut fe: FaultEntry = match get_last_entry() {
        Some(v) => {
            v
        }
        None => {
            debug!("Fault log is empty");
            return None
        }
    };
    if fe.action_taken == true {
        debug!("No unhandled errors in the fault log");
        return None
    }
    fe.action_taken = true;
    FAULT_LIST.lock().push(fe);
    return None
}

/// simple swap policy. 
/// When this swap policy is enabled always the crate which the last fault occurs 
/// is swapped.
pub fn simple_swap_policy() -> Option<String> {
    debug!("Running simple swap policy");
    let mut fe: FaultEntry = match get_last_entry() {
        Some(v) => {
            v
        }
        None => {
            debug!("Fault log is empty");
            return None
        }
    };
    if fe.action_taken == true {
        debug!("No unhandled errors in the fault log");
        return None
    }
    let crate_to_swap = fe.crate_error_occured.clone();
    if crate_to_swap.is_none() {
        debug!("No information on the last failed crate");
        return None
    }
    let crate_name = crate_to_swap.unwrap();
    fe.replaced_crates.push(crate_name.clone());
    fe.action_taken = true;
    FAULT_LIST.lock().push(fe);
    debug!("full {}",crate_name);
    let crate_name_simplified = crate_name.split("-").next().unwrap_or_else(|| &crate_name);
    debug!("simple {}",crate_name_simplified);
    Some(crate_name_simplified.to_string())
}

/// Provides the most recent entry in the log for given crate
pub fn get_the_most_recent_match(error_crate : &str) -> Option<FaultEntry> {
    debug!("getting the most recent match");
    let mut fe :Option<FaultEntry> = None;
    for fault_entry in FAULT_LIST.lock().iter() {
        if fault_entry.crate_error_occured.is_some(){
            let error_crate_name = fault_entry.crate_error_occured.clone().unwrap();
            let error_crate_name_simple = error_crate_name.split("-").next().unwrap_or_else(|| &error_crate_name);
            if error_crate_name_simple == error_crate {
                let item = fault_entry.clone();
                fe = Some(item);
            }
        }
    }
    if fe.is_none(){
        debug!("No recent entries for the given crate {}", error_crate);
    }
    fe
}

/// slightly advanced swap policy
/// When this swap policy is enabled at first attempt the crate where last fault occured is swapped.
/// If repetitive faults are detected at the same point if available the application crate is 
/// swapped at next attempt.
pub fn iterative_swap_policy() -> Option<String> {
    debug!("Running iterative swap policy");
    let mut fe: FaultEntry = match get_last_entry() {
        Some(v) => {
            v
        }
        None => {
            debug!("Fault log is empty");
            return None
        }
    };
    if fe.action_taken == true {
        debug!("No unhandled errors in the fault log");
        return None
    }
    if fe.crate_error_occured.is_none() {
        debug!("No information on the last failed crate");
        return None
    }
    let error_crate_name = fe.crate_error_occured.clone().unwrap();
    let error_crate_name_simple = error_crate_name.split("-").next().unwrap_or_else(|| &error_crate_name);
    let fe_old = get_the_most_recent_match(error_crate_name_simple);
    let crate_to_swap;
    if fe_old.is_some() && fe.running_app_crate.is_some(){
        crate_to_swap = fe.running_app_crate.clone();
    } else {
        crate_to_swap = fe.crate_error_occured.clone();
    }

    if crate_to_swap.is_none() {
        debug!("No information on the last failed crate");
        return None
    }
    let crate_name = crate_to_swap.unwrap();
    fe.replaced_crates.push(crate_name.clone());
    fe.action_taken = true;
    FAULT_LIST.lock().push(fe);
    debug!("full {}",crate_name);
    let crate_name_simplified = crate_name.split("-").next().unwrap_or_else(|| &crate_name);
    debug!("simple {}",crate_name_simplified);
    Some(crate_name_simplified.to_string())
}

pub fn get_crate_to_swap() -> Option<String> {
    // null_swap_policy()
    // simple_swap_policy()
    iterative_swap_policy() 
}
