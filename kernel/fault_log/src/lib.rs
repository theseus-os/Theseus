//! This crate logs all the faults occuring within Theseus. 
//! Maintains a list of exceptions and panics that has occured since booting up. 
//! This crate does not hold reference to any task or app. 
//! 

#![no_std]
#![feature(drain_filter)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate vga_buffer; // for println_raw!()
#[macro_use] extern crate print; // for regular println!()
#[macro_use] extern crate log;
extern crate alloc;
extern crate memory;
extern crate task;
extern crate apic;
extern crate irq_safety;

use alloc::{
    string::String,
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
    /// The core error occured
    pub core: Option<u8>,
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
            fault_type: fault_type,
            error_code: None,
            core: None,
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


lazy_static! {    
    /// The structure to hold the list of all faults so far occured in the system
    static ref FAULT_LIST: MutexIrqSafe<Vec<FaultEntry>> = MutexIrqSafe::new(Vec::new());
}

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
    fe.core = Some(get_my_apic_id());

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
        Some(curr_task.lock().name.clone())
    };

    // If task is from an application add application crate name. `None` if not 
    fe.running_app_crate = {
        let t = curr_task.lock();
        t.app_crate.as_ref().map(|x| x.lock_as_ref().crate_name.clone())
    };

    if let Some(instruction_pointer) = instruction_pointer {
        let instruction_pointer = VirtualAddress::new_canonical(instruction_pointer);
        fe.instruction_pointer = Some(instruction_pointer);
        fe.crate_error_occured = namespace.get_crate_containing_address(instruction_pointer.clone(), false)
                                        .map(|x| x.lock_as_ref().crate_name.clone());
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
    fe.address_accessed  = address_accessed.map(|address| VirtualAddress::new_canonical(address));
    update_and_insert_fault_entry_internal(fe, Some(instruction_pointer));
}

/// Add a new panic instance to the fault log. 
pub fn log_panic_entry() {
    let fe = FaultEntry::new(FaultType::Panic);
    update_and_insert_fault_entry_internal(fe, None);
}

/// Removes the unhandled faults from the fault log and returns. 
/// Is useful when we update the recovery detail about unhandled exceptions. 
pub fn remove_unhandled_exceptions() -> Vec<FaultEntry> {
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
pub fn print_fault_log() {
    println_both!("------------------ FAULT LOG ---------------------------");
    let list = FAULT_LIST.lock();
    for x in list.iter() {
        println_both!("{:?}", x);
    }
    println_both!("------------------ END OF LOG --------------------------");
}

/// null crate swap policy.
/// When this policy is enabled no crate swapping is performed
fn null_swap_policy() -> Option<String> {

    #[cfg(not(downtime_eval))]
    debug!("Running null swap policy");

    // We get all unhanlded faults
    let unhandled_list: Vec<FaultEntry> = remove_unhandled_exceptions();

    // No unhandled faults. Nothing to do. This happens when restartable tasks get killed or 
    // exits without encountering a panic or an exception
    if unhandled_list.is_empty(){
        debug!("No unhandled errors in the fault log");
        return None
    }
    // For the first fault we mark as task restarted.
    // For any subsequent fault we mark them as multiple fault recovery
    for (i, fe) in unhandled_list.iter().enumerate() {
        let mut fe = fe.clone();
        if i == 0 {
            fe.action_taken = RecoveryAction::TaskRestarted;
        } else {
            fe.action_taken = RecoveryAction::MultipleFaultRecovery;
        }
        FAULT_LIST.lock().push(fe);
    }
    return None
}

/// simple swap policy. 
/// When this swap policy is enabled always the crate which the last fault occurs 
/// is swapped.
fn simple_swap_policy() -> Option<String> {

    #[cfg(not(downtime_eval))]
    debug!("Running simple swap policy");

    // We get all unhanlded faults
    let unhandled_list: Vec<FaultEntry> = remove_unhandled_exceptions();
    if unhandled_list.is_empty(){
        debug!("No unhandled errors in the fault log");
        return None
    }

    // For the first fault we take action.
    // For any subsequent fault we mark them as multiple fault recovery
    for (i, fe) in unhandled_list.iter().enumerate() {
        let mut fe = fe.clone();
        if i == 0 {
            let crate_to_swap = unhandled_list[0].crate_error_occured.clone();

            // If the crate fault occured is not logged we can only restart
            if crate_to_swap.is_none() {
                debug!("No information on where the first failure occured");
                fe.action_taken = RecoveryAction::TaskRestarted;
            } else {
                // If the crate is logged we mark that as replaced crate
                fe.action_taken = RecoveryAction::FaultCrateReplaced;
                fe.replaced_crates.push(crate_to_swap.unwrap().clone());
            }

        } else {
            fe.action_taken = RecoveryAction::MultipleFaultRecovery;
        }
        FAULT_LIST.lock().push(fe);
    }

    let crate_to_swap = unhandled_list[0].crate_error_occured.clone();

    // We return None if the crate fault occured is not logged
    if crate_to_swap.is_none() {
        return None
    }

    // We return the crate name if the crate fault occured is logged
    let crate_name = crate_to_swap.unwrap();

    #[cfg(not(downtime_eval))]
    debug!("Repalce : {}",crate_name);

    Some(crate_name)
}

/// Provides the most recent entry in the log for given crate
/// Utility function for iterative crate replacement
fn get_the_most_recent_match(error_crate : &str) -> Option<FaultEntry> {

    #[cfg(not(downtime_eval))]
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

        #[cfg(not(downtime_eval))]
        debug!("No recent entries for the given crate {}", error_crate);
    }
    fe
}

/// slightly advanced swap policy. 
/// When this policy is enabled the following actions are taken in order to the number of time same fault is detected. 
/// 1) Restart the task
/// 2) Replace the crate fault is detected
/// 3) Replace the application crate
fn iterative_swap_policy() -> Option<String> {

    #[cfg(not(downtime_eval))]
    debug!("Running iterative swap policy");

    // We get all unhanlded faults
    let mut unhandled_list: Vec<FaultEntry> = remove_unhandled_exceptions();
    if unhandled_list.is_empty(){
        debug!("No unhandled errors in the fault log");
        return None
    }
    
    
    let mut crate_to_swap = None;

    // For the first fault we take action.
    // For any subsequent fault we mark them as multiple fault recovery
    for (i, fe) in unhandled_list.iter().enumerate() {
        let mut fe = fe.clone();
        if i == 0 {
            let error_crate = unhandled_list[0].crate_error_occured.clone();

            // If the crate fault occured is not logged we can only restart
            if error_crate.is_none() {
                debug!("No information on where the first failure occured");
                fe.action_taken = RecoveryAction::TaskRestarted;
            } else {
                let error_crate_name = error_crate.clone().unwrap();
                let error_crate_name_simple = error_crate_name.split("-").next().unwrap_or_else(|| &error_crate_name);

                // We check whether the fault is logged previously
                let fe_last_fault = get_the_most_recent_match(error_crate_name_simple);
                match fe_last_fault {

                    // If the fault is logged we check the action taken and then take the next drastic action.
                    Some(fault_entry) => {
                        if fault_entry.action_taken == RecoveryAction::FaultCrateReplaced && fault_entry.running_app_crate.is_some() {
                            // last action : 2  -> Next action : 3
                            crate_to_swap = fault_entry.running_app_crate.clone();
                            fe.action_taken = RecoveryAction::IterativelyCrateReplaced;
                            fe.replaced_crates.push(fault_entry.running_app_crate.clone().unwrap());
                        } else if fault_entry.action_taken == RecoveryAction::TaskRestarted {
                            // last action : 1  -> Next action : 2
                            crate_to_swap = error_crate.clone();
                            fe.action_taken = RecoveryAction::FaultCrateReplaced;
                            fe.replaced_crates.push(error_crate.clone().unwrap());
                        } else if fault_entry.action_taken == RecoveryAction::None || fault_entry.action_taken == RecoveryAction::MultipleFaultRecovery {
                            // last action : None  -> Next action : 1
                            crate_to_swap = None;
                            fe.action_taken = RecoveryAction::TaskRestarted;
                        } else {
                            // We have exhausted all our fault tolerance stages. So we just reuse stage 2 
                            crate_to_swap = error_crate.clone();
                            fe.action_taken = RecoveryAction::FaultCrateReplaced;
                            fe.replaced_crates.push(error_crate.clone().unwrap());
                        }
                    }

                    // If the fault is not logged we start from step 1
                    None => {
                        crate_to_swap = None;
                        fe.action_taken = RecoveryAction::TaskRestarted;
                    }
                }
            }
        } else {
            fe.action_taken = RecoveryAction::MultipleFaultRecovery;
        }
        FAULT_LIST.lock().push(fe);
    }

    // We return None if we didn't pick a crate to replace
    if crate_to_swap.is_none() {
        return None
    }

    let crate_name = crate_to_swap.unwrap();

    #[cfg(not(downtime_eval))]
    debug!("full {}",crate_name);

    Some(crate_name)
}

/// This function returns the name of the crate to replace if required. 
/// Returns none if no crate is needed to be replaced. 
pub fn get_crate_to_swap() -> Option<String> {

    #[cfg(not(use_crate_replacement))]
    {
        return null_swap_policy();
    }


    #[cfg(use_crate_replacement)]
    {
        #[cfg(not(use_iterative_replacement))]
        return simple_swap_policy();

        #[cfg(use_iterative_replacement)]
        return iterative_swap_policy() ;
    }
}
