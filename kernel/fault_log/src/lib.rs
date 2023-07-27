//! This crate logs all the faults occuring within Theseus. 
//! Maintains a list of exceptions and panics that has occured since booting up. 
//! This crate does not hold reference to any task or app. 
//! 

#![no_std]
#![feature(extract_if)]

extern crate alloc;

use alloc::{
    string::{String,ToString},
    vec::Vec,
};
use log::debug;
use cpu::CpuId;
use memory::VirtualAddress;
use sync_irq::IrqSafeMutex;
use core::panic::PanicInfo;

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
    UnknownException(u8)
}

/// Utility function to get Fault type from exception number. 
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
        num => FaultType::UnknownException(num),
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
    /// The ID of the CPU on which the error occured.
    pub cpu: Option<CpuId>,
    /// Task runnning immediately before the Exception
    pub running_task: Option<String>,
    /// If available the application crate that spawned the task
    pub running_app_crate: Option<String>,
    /// For page faults the address the program attempted to access. None for other faults
    pub address_accessed: Option<VirtualAddress>,
    /// Address at which exception occured
    pub instruction_pointer: Option<VirtualAddress>,    
    /// Crate the address at which exception occured located
    pub crate_error_occured: Option<String>,
    /// List of crates reloaded from memory to recover from fault
    pub replaced_crates: Vec<String>,
    /// Recovery Action taken as a result of the fault
    pub action_taken: RecoveryAction,
}

impl FaultEntry {
    /// Returns an empty `FaultEntry` with only `fault_type` field filled.
    pub fn new (
        fault_type: FaultType
    ) -> FaultEntry {
        FaultEntry {
            fault_type,
            error_code: None,
            cpu: None,
            running_task: None,
            running_app_crate: None,
            address_accessed: None,
            instruction_pointer: None,
            crate_error_occured: None,
            replaced_crates: Vec::<String>::new(),
            action_taken: RecoveryAction::None,
        }
    }
}


/// The structure to hold the list of all faults so far occured in the system
static FAULT_LIST: IrqSafeMutex<Vec<FaultEntry>> = IrqSafeMutex::new(Vec::new());

/// Clears the log of faults so far occured in the system 
pub fn clear_fault_log() {
    FAULT_LIST.lock().clear();
}

/// Internal function to populate the remaining fields of fault entry. 
/// Common logic for both panics and exceptions.
fn update_and_insert_fault_entry_internal(
    mut fe: FaultEntry,
    instruction_pointer: Option<usize>, 
) {

    // Add the core the fault was detected
    fe.cpu = Some(cpu::current_cpu());

    // If current task cannot be obtained we will just add `fault_entry` to 
    // the `fault_log` and return.
    let curr_task = match task::get_my_current_task() {
        Some(x) => x,
        _ => {
            FAULT_LIST.lock().push(fe);
            return
        },
    };

    let namespace = curr_task.get_namespace();

    // Add name of current task
    fe.running_task = {
        Some(curr_task.name.clone())
    };

    // If task is from an application add application crate name. `None` if not 
    fe.running_app_crate = {
        curr_task.app_crate.as_ref().map(|x| x.lock_as_ref().crate_name.to_string())
    };

    if let Some(instruction_pointer) = instruction_pointer {
        let instruction_pointer = VirtualAddress::new_canonical(instruction_pointer);
        fe.instruction_pointer = Some(instruction_pointer);
        fe.crate_error_occured = namespace.get_crate_containing_address(instruction_pointer, false)
                                        .map(|x| x.lock_as_ref().crate_name.to_string());
    };

    // Push the fault entry.
    FAULT_LIST.lock().push(fe);
}

/// Add a new exception instance to the fault log. 
/// Generally it will have `fault_type` and `instruction_pointer`. 
/// If `error_code` is provided with exception it will be send to 
/// the function as `Some(error_code)`. 
/// If the exception is a page fault the address attempted to accesss 
/// will also be send to the function as `Some(address_accessed)`
pub fn log_exception (
    fault_type: u8,
    instruction_pointer: usize,
    error_code: Option<u64>,
    address_accessed: Option<usize>
) {
    let mut fe = FaultEntry::new(from_exception_number(fault_type));
    fe.error_code = error_code;
    fe.address_accessed  = address_accessed.map(VirtualAddress::new_canonical);
    update_and_insert_fault_entry_internal(fe, Some(instruction_pointer));
}

/// Add a new panic instance to the fault log. 
pub fn log_panic_entry(panic_info: &PanicInfo) {
    let mut fe = FaultEntry::new(FaultType::Panic);
    if let Some(location) = panic_info.location() {
        // debug!("panic occurred in file '{}' at line {}", location.file(), location.line());
        let panic_file = location.file();
        let mut error_crate_iter = panic_file.split('/');
        error_crate_iter.next();
        let error_crate_name_simple = error_crate_iter.next().unwrap_or(panic_file);
        debug!("panic file {}",error_crate_name_simple);
        fe.crate_error_occured = Some(error_crate_name_simple.to_string());
    } else {
        debug!("panic occurred but can't get location information...");
    }
    update_and_insert_fault_entry_internal(fe, None);
}

/// Removes the unhandled faults from the fault log and returns. 
/// Is useful when we update the recovery detail about unhandled exceptions. 
pub fn remove_unhandled_exceptions() -> Vec<FaultEntry> {
    FAULT_LIST.lock().extract_if(|fe| fe.action_taken == RecoveryAction::None).collect::<Vec<_>>()
}

/// Prints to both the `early_printer` and the current terminal via `app_io`.
macro_rules! println_both {
    ($fmt:expr) => {
        early_printer::println!($fmt);
        app_io::println!($fmt);
    };
    ($fmt:expr, $($arg:tt)*) => {
        early_printer::println!($fmt, $($arg)*);
        app_io::println!($fmt, $($arg)*);
    };
}

/// Prints the fault log
pub fn print_fault_log() {
    println_both!("------------------ FAULT LOG ---------------------------");
    let list = FAULT_LIST.lock();
    for x in list.iter() {
        println_both!("{:?}", x);
    }
    println_both!("------------------ END OF LOG --------------------------");
}

/// Add a `FaultEntry` to fault log.
pub fn log_handled_fault(fe: FaultEntry){
    FAULT_LIST.lock().push(fe);
}

/// Provides the most recent entry in the log for given crate
/// Utility function for iterative crate replacement
pub fn get_the_most_recent_match(error_crate: &str) -> Option<FaultEntry> {
    debug!("getting the most recent match");

    let mut fe :Option<FaultEntry> = None;
    for fault_entry in FAULT_LIST.lock().iter() {
        if let Some(crate_error_occured) = &fault_entry.crate_error_occured {
            let error_crate_name = crate_error_occured.clone();
            let error_crate_name_simple = error_crate_name.split('-').next().unwrap_or(&error_crate_name);
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