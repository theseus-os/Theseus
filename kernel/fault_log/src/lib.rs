//! Defines functions and types for crate swapping, used in live evolution and fault recovery.
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

use core::{
    fmt,
    ops::Deref,
    ptr
};
use spin::Mutex;
use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use hashbrown::HashMap;
use memory::{EntryFlags, MmiRef, VirtualAddress};
use fs_node::{FsNode, FileOrDir, FileRef, DirRef};
use qp_trie::wrapper::BString;
use mod_mgmt::{
    CrateNamespace,
    NamespaceDir,
    IntoCrateObjectFile,
    write_relocation,
    crate_name_from_path,
    replace_containing_crate_name,
    StrongSectionRef,
    WeakDependent,
};
use path::Path;
use by_address::ByAddress;

#[derive(Debug)]
pub struct FaultEntry {
    pub exception_number: u8,
    pub error_code: u64,
    pub running_task: String,
    pub running_app_crate: Option<String>,
    pub address_accessed: Option<VirtualAddress>,
    pub instruction_pointer : Option<VirtualAddress>,
    pub crate_error_occured : Option<String>,
    pub replaced_crates : Vec<String>,
    pub action_taken : bool,
}


lazy_static! {
    static ref FAULT_LIST: Mutex<Vec<FaultEntry>> = Mutex::new(Vec::new());
}

/// Clears the cache of unloaded (swapped-out) crates saved from previous crate swapping operations. 
pub fn clear_fault_log() {
    FAULT_LIST.lock().clear();
}

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

pub fn print_fault_log() -> (){
    println_both!("------------------ FAULT LOG ---------------------------");
    let list = FAULT_LIST.lock();
    for x in list.iter() {
        println_both!("{:?}", x);
    }
    println_both!("------------------ END OF LOG --------------------------");
}

// impl fmt::Debug for FaultEntry {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         f.debug_struct("FaultEntry")
//             .field("Excpetion : ", &self.exception_number)
//             .field("error_code : ", &self.exception_number)
//             .field("old_namespace", &self.old_namespace.name())
//             .field("new_crate", &self.new_crate_object_file.try_lock()
//                 .map(|f| f.get_absolute_path())
//                 .unwrap_or_else(|| format!("<Locked>"))
//             )
//             .field("new_namespace", &self.new_namespace.name())
//             .field("reexport_symbols", &self.reexport_new_symbols_as_old)
//             .finish()
//     }
// }
