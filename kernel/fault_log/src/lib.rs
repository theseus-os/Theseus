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
#[derive(Debug)]
pub struct FaultEntry {
    /// Machine excpetion number, 0xFF indicates panic
    pub exception_number: u8,

    /// Error code returned with the exception
    pub error_code: u64,

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

