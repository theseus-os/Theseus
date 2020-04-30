//! This crate logs all the faults occuring within Theseus. 
//! Maintains a list of exceptions and panics that has occured since booting up. 
//! This crate does not hold reference to any task or app. 
//! 

#![no_std]
#![feature(drain_filter)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate vga_buffer; // for println_raw!()
#[macro_use] extern crate print; // for regular println!()
extern crate alloc;
extern crate spin; 
extern crate memory;
extern crate task;
extern crate apic;
extern crate irq_safety;

use spin::Mutex;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use memory::VirtualAddress;
use apic::get_my_apic_id;
use irq_safety::MutexIrqSafe;

/// The possible faults (panics and exceptions) encountered 
/// during operations.
#[derive(Debug, Clone)]
pub enum FaultType {
    PageFault,
    GeneralProtectionFault,
    SegmentNotPresent,
    InvalidTSS,
    DoubleFault,
    DeviceNotAvailable,
    InvalidOpCode,
    BoundRangeExceeded,
    Overflow,
    NMI,
    DivideByZero,
    Panic,
    UnknownException
}

pub fn from_exception_number(num: u8) -> FaultType {
    match num {
        0x0 => FaultType::DivideByZero,
        0x2 => FaultType::NMI,
        0x4 => FaultType::Overflow,
        0x5 => FaultType::BoundRangeExceeded,
        0x6 => FaultType::InvalidOpCode,
        0x7 => FaultType::DeviceNotAvailable,
        0x8 => FaultType::DoubleFault,
        0xA => FaultType::InvalidTSS,
        0xB => FaultType::SegmentNotPresent,
        0xD => FaultType::GeneralProtectionFault,
        0xE => FaultType::PageFault,
        _   => FaultType::UnknownException,
    }
}

/// The different types of recovery procedures used for the 
/// observed fault
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    /// No action taken on this fault.
    None,
    /// Task restarted only. No crate replaced.
    TaskRestarted,
    /// Crate where fault is observed is replaced, and then task restarted.
    FaultCrateReplaced,
    /// Different Crate than the crate where fault is observed is restrated. 
    /// This option is taken when the same fault is observed multiple times.  
    IterativelyCrateReplaced,
    /// This fault is handled as a recovery for different fault. 
    /// Used when additional faults occur during unwinding.  
    MultipleFaultRecovery
}


/// A data structure to hold information about each fault. 
#[derive(Debug, Clone)]
pub struct FaultEntry {
    /// Type of fault
    pub fault_type: FaultType,
    /// Error code returned with the exception
    pub error_code: Option<u64>,
    /// The core error occured
    pub core: Option<u8>,
    /// Task runnning immediately before the Exception
    pub running_task: Option<String>,
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
    /// Recovery Action taken as a result of the fault
    pub action_taken : RecoveryAction,
}

impl FaultEntry {
    pub fn new(
        fault_type: FaultType
    ) -> FaultEntry {
        FaultEntry{
            fault_type: fault_type,
            error_code: None,
            core: None,
            running_task: None,
            running_app_crate: None,
            address_accessed: None,
            instruction_pointer : None,
            crate_error_occured : None,
            replaced_crates : Vec::<String>::new(),
            action_taken : RecoveryAction::None,
        }
    }
}


lazy_static! {    
    /// The structure to hold the list of all faults so far occured in the system
    static ref FAULT_LIST: MutexIrqSafe<Vec<FaultEntry>> = MutexIrqSafe::new(Vec::new());
}

/// Clears the log of faults so far occured in the system 
pub fn clear_fault_log() {
    FAULT_LIST.lock().clear();
}


fn update_and_insert_fault_entry_internal(
    mut fe: FaultEntry,
    instruction_pointer: Option<usize>, 
) -> () {
    fe.core = Some(get_my_apic_id());
    let curr_task = match task::get_my_current_task(){
        Some(x) => x,
        _ => {
            FAULT_LIST.lock().push(fe);
            return
        },
    };
    let namespace = curr_task.get_namespace();
    fe.running_task = {
        Some(curr_task.lock().name.clone())
    };
    fe.running_app_crate = {
        let t = curr_task.lock();
        t.app_crate.as_ref().map(|x| x.lock_as_ref().crate_name.clone())
    };
    match instruction_pointer {
        Some(instruction_pointer) => {
            let instruction_pointer = VirtualAddress::new_canonical(instruction_pointer);
            fe.instruction_pointer = Some(instruction_pointer);
            fe.crate_error_occured = match namespace.get_crate_containing_address(instruction_pointer.clone(),false){
                Some(cn) => {
                    Some(cn.lock_as_ref().crate_name.clone())
                }
                None => {
                    None
                }
            };
        },
        _=> {},
    };

    FAULT_LIST.lock().push(fe);
}

/// Add a new entry to the fault log. 
/// This function requires only the `fault_type` and `error_code`. 
/// Other entries will be marked as None to be filled later.
// Since all exceptions lead to calling `kill_and_halt` 
// we update the rest of the fields there.
pub fn log_exception (
    fault_type: u8,
    instruction_pointer: usize,
    error_code: Option<u64>,
    address_accessed: Option<usize>
)-> () {
    let mut fe = FaultEntry::new(from_exception_number(fault_type));
    fe.error_code = error_code;
    fe.address_accessed  = match address_accessed {
        Some(address) => Some(VirtualAddress::new_canonical(address)),
        _ => None,
    };

    update_and_insert_fault_entry_internal(fe,Some(instruction_pointer));
}

/// Add a panic occuring to fault log. 
pub fn log_panic_entry ()-> () {
    let fe = FaultEntry::new(FaultType::Panic);
    update_and_insert_fault_entry_internal(fe,None);
}

/// Removes the unhandled exception from the fault log and returns. 
/// Is useful when we update the recovery detail about unhandled exceptions
pub fn remove_unhandled_exception () -> Vec<FaultEntry> {
    FAULT_LIST.lock().drain_filter(|fe| fe.action_taken == RecoveryAction::None).collect::<Vec<_>>()
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
