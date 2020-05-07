//! Defines support functions needed for swapping of corrupted crates to a different address for fault tolerance
//! 

#![no_std]

#[macro_use] extern crate alloc;
#[macro_use] extern crate log; 
extern crate memory;
extern crate mod_mgmt;
extern crate fs_node;
extern crate path;
extern crate crate_swap;
extern crate task;
extern crate fault_log;

use core::ptr;

use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use memory::VirtualAddress;
use fs_node::{FileOrDir, DirRef};
use mod_mgmt::{
    CrateNamespace,
    NamespaceDir,
    IntoCrateObjectFile,
};
use path::Path;
use crate_swap::{SwapRequest, swap_crates};
use fault_log::{RecoveryAction, FaultEntry, remove_unhandled_exceptions, log_handled_fault, get_the_most_recent_match};


/// A data structure to hold the ranges of memory used by the old crate and the new crate.
/// The crate only maintains the values as virtual addresses and holds no references to any
/// crate
pub struct SwapRanges{
    pub old_text_low : Option<VirtualAddress>,
    pub old_rodata_low : Option<VirtualAddress>,
    pub old_data_low : Option<VirtualAddress>,

    pub old_text_high : Option<VirtualAddress>,
    pub old_rodata_high : Option<VirtualAddress>,
    pub old_data_high : Option<VirtualAddress>,

    pub new_text_low : Option<VirtualAddress>,
    pub new_rodata_low : Option<VirtualAddress>,
    pub new_data_low : Option<VirtualAddress>,

    pub new_text_high : Option<VirtualAddress>,
    pub new_rodata_high : Option<VirtualAddress>,
    pub new_data_high : Option<VirtualAddress>
}

impl SwapRanges {
    /// Returns an empty `SwapRanges` with all fields set to None
    pub fn new () -> SwapRanges {
        SwapRanges {
            old_text_low : None,
            old_rodata_low : None,
            old_data_low : None,
            old_text_high : None,
            old_rodata_high : None,
            old_data_high : None,
            new_text_low : None,
            new_rodata_low : None,
            new_data_low : None,
            new_text_high : None,
            new_rodata_high : None,
            new_data_high : None,
        }
    }
}

/// For swapping of a crate from the identical object file in the disk. 
/// Wraps arpund the crate_swap function by creating an appropriate  Swap request. 
/// Arguments identical to the ones of crate_swap except
/// namespace : Arc to the current namespace
/// Returns a `SwapRanges` struct indicating the old and new `VirtualAddress` range
/// of the swapped crate
pub fn do_self_swap(
    crate_name : &str, 
    curr_dir: &DirRef, 
    override_namespace_crate_dir: Option<NamespaceDir>, 
    state_transfer_functions: Vec<String>,
    namespace: Arc<CrateNamespace>,
    verbose_log: bool
) -> Result<(SwapRanges), String> {

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel_mmi_ref".to_string())?;

    // Create swap request
    let swap_requests = {
        let mut requests: Vec<SwapRequest> = Vec::with_capacity(1);
        // Check that the new crate file exists. It could be a regular path, or a prefix for a file in the namespace's dir.
        // If it's a full path, then we just check that the path points to a valid crate object file. 
        // Otherwise, we treat it as a prefix for a crate object file name that may be found 
        let (into_new_crate_file, new_namespace) = {
            if let Some(f) = override_namespace_crate_dir.as_ref().and_then(|ns_dir| ns_dir.get_file_starting_with(crate_name)) {
                (IntoCrateObjectFile::File(f), None)
            } else if let Some(FileOrDir::File(f)) = Path::new(String::from(crate_name)).get(curr_dir) {
                (IntoCrateObjectFile::File(f), None)
            } else {
                (IntoCrateObjectFile::Prefix(String::from(crate_name)), None)
            }
        };


        let swap_req = SwapRequest::new(
            Some(crate_name),
            Arc::clone(&namespace),
            into_new_crate_file,
            new_namespace,
            false, //reexport
        ).map_err(|invalid_req| format!("{:#?}", invalid_req))?;

        #[cfg(not(downtime_eval))]
        debug!("swap call {:?}", swap_req);

        requests.push(swap_req);

        requests
    };

    // Create a set of temporary variables to hold the data ranges
    let mut old_text_low : Option<VirtualAddress> = None;
    let mut old_rodata_low : Option<VirtualAddress> = None;
    let mut old_data_low : Option<VirtualAddress> = None;

    let mut old_text_high : Option<VirtualAddress> = None;
    let mut old_rodata_high : Option<VirtualAddress> = None;
    let mut old_data_high : Option<VirtualAddress> = None;

    let mut new_text_low : Option<VirtualAddress> = None;
    let mut new_rodata_low : Option<VirtualAddress> = None;
    let mut new_data_low : Option<VirtualAddress> = None;

    let mut new_text_high : Option<VirtualAddress> = None;
    let mut new_rodata_high : Option<VirtualAddress> = None;
    let mut new_data_high : Option<VirtualAddress> = None;

    let mut matching_crates = CrateNamespace::get_crates_starting_with(&namespace, crate_name);

    // There can be only one matching crate for a given crate name
    if matching_crates.len() == 1 {
        let (_old_crate_name, old_crate_ref, _old_namespace) = matching_crates.remove(0);

         #[cfg(not(downtime_eval))]
        debug!("{:?}",old_crate_ref);

        let old_crate = old_crate_ref.lock_as_mut().ok_or_else(|| {
            error!("Unimplemented: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exclusive mutable instance", old_crate_ref);
            "Unimplemented: swap_crates() doesn't yet support deep copying shared crates to get a new exclusive mutable instance"
        })?;


        // Find the text, data and rodata ranges of old crate
        {
            match &old_crate.text_pages {
                Some((_pages,address)) => {
                    #[cfg(not(downtime_eval))]
                    debug!("Text Range is {:X} - {:X}", address.start, address.end);
                    old_text_low = Some(address.start.clone());
                    old_text_high = Some(address.end.clone());
                }
                _=>{}
            };

            match &old_crate.rodata_pages {
                Some((_pages,address)) => {
                    #[cfg(not(downtime_eval))]
                    debug!("Rodata Range is {:X} - {:X}", address.start, address.end);
                    old_rodata_low = Some(address.start.clone());
                    old_rodata_high = Some(address.end.clone());
                }
                _=>{}
            };

            match &old_crate.data_pages {
                Some((_pages,address)) => {
                    #[cfg(not(downtime_eval))]
                    debug!("data Range is {:X} - {:X}", address.start, address.end);
                    old_data_low = Some(address.start.clone());
                    old_data_high = Some(address.end.clone());
                }
                _=>{}
            };
        }

    } else {
        return Err("More than 1 matching crates currently loaded".to_string());
    }

    // Swap crates
    let swap_result = swap_crates(
        &namespace,
        swap_requests, 
        override_namespace_crate_dir,
        state_transfer_functions,
        &kernel_mmi_ref,
        verbose_log,
        false // enable crate_cahce
    );

    debug!("Swap crates returned");

    let ocn = crate_name;

    // Find the new crate loaded. It should have the exact same name as the old crate
    let mut matching_crates = CrateNamespace::get_crates_starting_with(&namespace, ocn);

    // There can't be more than one match if swap occured correctly
    if matching_crates.len() == 1 {

        #[cfg(not(downtime_eval))]
         debug!("We got a match");

         let (new_crate_full_name, _ocr, real_new_namespace) = matching_crates.remove(0);
         let new_crate_ref = match CrateNamespace::get_crate_and_namespace(&real_new_namespace, &new_crate_full_name) {
            Some((ocr, _ns)) => {
                ocr
            }
            _ => {
                debug!("Match occurs but cannot get the crate");
                return Err("Cannot get reference".to_string());
            }
         };

        let new_crate = new_crate_ref.lock_as_mut().ok_or_else(|| {
            error!("Unimplemented: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exclusive mutable instance", new_crate_ref);
            "Unimplemented: swap_crates() doesn't yet support deep copying shared crates to get a new exclusive mutable instance"
        })?;

        // Find the address range of the newl loaded crate
        {
            match &new_crate.text_pages {
                Some((_pages, address)) => {

                    #[cfg(not(downtime_eval))]
                    {
                        debug!("Text Range was {:X} - {:X}", old_text_low.unwrap(), old_text_high.unwrap());
                        debug!("Text Range is  {:X} - {:X}", address.start, address.end);
                    }
                    new_text_low = Some(address.start.clone());
                    new_text_high = Some(address.end.clone());
                }
                _ => {}
            };

            match &new_crate.rodata_pages {
                Some((_pages,address)) => {
                    #[cfg(not(downtime_eval))]
                    {
                        debug!("Rodata Range was {:X} - {:X}", old_rodata_low.unwrap(), old_rodata_high.unwrap());
                        debug!("Rodata Range is  {:X} - {:X}", address.start, address.end);
                    }

                    new_rodata_low = Some(address.start.clone());
                    new_rodata_high = Some(address.end.clone());
                }
                _ => {}
            };

            match &new_crate.data_pages {
                Some((_pages,address)) => {

                    #[cfg(not(downtime_eval))]
                    {
                        debug!("data Range was {:X} - {:X}", old_data_low.unwrap(), old_data_high.unwrap());
                        debug!("data Range is  {:X} - {:X}", address.start, address.end);
                    }
                    new_data_low = Some(address.start.clone());
                    new_data_high = Some(address.end.clone());
                }
                _ => {}
            };
        }


     } else {
         debug!("No match");
     }

    let return_struct = SwapRanges{
        old_text_low : old_text_low,
        old_rodata_low : old_rodata_low,
        old_data_low : old_data_low,

        old_text_high : old_text_high,
        old_rodata_high : old_rodata_high,
        old_data_high :  old_data_high,

        new_text_low : new_text_low,
        new_rodata_low : new_rodata_low,
        new_data_low : new_data_low,

        new_text_high : new_text_high,
        new_rodata_high : new_rodata_high,
        new_data_high : new_data_high
    };



    match swap_result {
        Ok(()) => {

            #[cfg(not(downtime_eval))]
            debug!("Swap operation complete");

            Ok(return_struct)
        }
        Err(e) => {
            debug!("Swap FAILED");
            Err(e.to_string())
        }
    }


}

/// A support function for self swap of crate
/// Iterates through a given set of address from top to bottom and
/// checks whether they fall within the range of an old crate and 
/// shifts them by a constant offset to map them to correct region
/// in the new crate.
/// Fix is as follows if virtual address matches.
/// `new_address` = `old_address` - `start_address_of_old_text_region` + `start_address_of_new_text_region`.
/// Address ranges are provided via `SwapRanges` struct.
/// Uses unsafe logic for pointer manipulation
pub fn constant_offset_fix(
    swap_ranges : &SwapRanges, 
    bottom : usize,
    top : usize
) -> Result<(), String> {

    if top < bottom {
        return Err("In constant_offset_fix bottom must be less than or equal to top".to_string())
    }

    if swap_ranges.new_text_low.is_none(){
        // crate swap haven't happened. No constant_offset_fix to be done
        return Ok(())
    } 
    // start from the bottom value
    let mut x = bottom;
    loop {
        // read the value at that location
        let p = (x) as *const u64;
        let n =  unsafe { ptr::read(p) };
        //debug!("{:#X} = {:#X}", x ,n);
        let ra = memory::VirtualAddress::new_canonical(n as usize);

        if swap_ranges.old_text_low.is_some() && swap_ranges.new_text_low.is_some() {
            if swap_ranges.old_text_low.unwrap() < ra && swap_ranges.old_text_high.unwrap() > ra {

                #[cfg(not(downtime_eval))]
                debug!("Match text at address {:#X}, value  = {:#X}", x , n);

                // Note : Does not use saturation add since underflow is not possible and overflow could not be caused in proper execution
                let n_ra = ra - swap_ranges.old_text_low.unwrap() + swap_ranges.new_text_low.unwrap();
                let p1 = (x) as *mut usize;
                unsafe { ptr::write(p1,n_ra.value()) };
                // For debug purposes we re-read teh value
                #[cfg(not(downtime_eval))]
                {
                    let n =  unsafe { ptr::read(p) };
                    debug!("New value at address {:#X}, value  = {:#X}", x , n);
                }
            }
        }

        if swap_ranges.old_rodata_low.is_some() && swap_ranges.new_rodata_low.is_some() {
            if swap_ranges.old_rodata_low.unwrap() < ra && swap_ranges.old_rodata_high.unwrap() > ra {

                #[cfg(not(downtime_eval))]
                debug!("Match rodata at address {:#X}, value  = {:#X}", x , n);
                let n_ra = ra - swap_ranges.old_rodata_low.unwrap() + swap_ranges.new_rodata_low.unwrap();
                let p1 = (x) as *mut usize;
                unsafe { ptr::write(p1,n_ra.value()) };

                #[cfg(not(downtime_eval))]
                {
                    let n =  unsafe { ptr::read(p) };
                    debug!("New value at address {:#X}, value  = {:#X}", x , n);
                }
            }
        }

        if swap_ranges.old_data_low.is_some() && swap_ranges.new_data_low.is_some() {
            if swap_ranges.old_data_low.unwrap() < ra && swap_ranges.old_data_high.unwrap() > ra {

                #[cfg(not(downtime_eval))]
                debug!("Match data at address {:#X}, value  = {:#X}", x , n);
                let n_ra = ra - swap_ranges.old_data_low.unwrap() + swap_ranges.new_data_low.unwrap();
                let p1 = (x) as *mut usize;
                unsafe { ptr::write(p1,n_ra.value()) };
                #[cfg(not(downtime_eval))]
                {
                    let n =  unsafe { ptr::read(p) };
                    debug!("New value at address {:#X}, value  = {:#X}", x , n);
                }
            }
        }

        x = x + 8;
        if x > top {
            break;
        }
    }
    Ok(())
}

/// This function calls the crate swapping routine for a corrupted crate (referred to as self swap)
/// Swapping a crate with a new copy of object file includes following steps in high level
/// 1) Call generic crate swapping routine with self_swap = true
/// 2) Change the calues in the stack to the reloaded crate
/// 3) Change any other references in the heap to the reloaded crate
/// This function handles 1 and 2 operations and 3 is handled in the call site depending on necessity
pub fn self_swap_handler(crate_name: &str) -> Result<(SwapRanges), String> {

    let taskref = task::get_my_current_task()
        .ok_or_else(|| format!("failed to get current task"))?;

    #[cfg(not(downtime_eval))]
    debug!("The taskref is {:?}",taskref);

    let curr_dir = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    let override_namespace_crate_dir = Option::<NamespaceDir>::None;

    let verbose = false;
    let state_transfer_functions: Vec<String> = Vec::new();

    let mut tuples: Vec<(&str, &str, bool)> = Vec::new();

    tuples.push((crate_name, crate_name , false));

    #[cfg(not(downtime_eval))]
    debug!("tuples: {:?}", tuples);


    let namespace = task::get_my_current_task().ok_or("Couldn't get current task")?.get_namespace();    

    // 1) Call generic crate swapping routine
    let swap_result = do_self_swap(
        crate_name, 
        &curr_dir, 
        override_namespace_crate_dir,
        state_transfer_functions,
        namespace,
        verbose
    );

    // let mut rbp: usize;
    // let mut rsp: usize;
    // let mut rip: usize;

    // unsafe{
    //     asm!("lea $0, [rip]" : "=r"(rip), "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
    // }
    // debug!("rmain : register values: RIP: {:#X}, RSP: {:#X}, RBP: {:#X}", rip, rsp, rbp);

    let swap_ranges = match swap_result {
        Ok(x) => {

            #[cfg(not(downtime_eval))]
            debug!("Swap operation complete");
            x
        }
        Err(e) => {
            debug!("SWAP FAILED at do_self_swap");
            return Err(e.to_string())
        }
    };


    let taskref = task::get_my_current_task()
        .ok_or_else(|| format!("failed to get current task"))?;

    // debug!("The taskref is {:?}",taskref);

    // Find the range of stack to iterate
    let locked_task = taskref.lock();
    let bottom = locked_task.kstack.bottom().value();
    let top = locked_task.kstack.top_usable().value();

    #[cfg(not(downtime_eval))]
    debug!("Bottom and top of stack are{:X} {:X}", bottom, top);


    // On x86 you cannot directly read the value of the instruction pointer (RIP),
    // so we use a trick that exploits RIP-relateive addressing to read the current value of RIP (also gets RBP and RSP)
    // asm!("lea $0, [rip]" : "=r"(rip), "={rbp}"(rbp), "={rsp}"(rsp) : : "memory" : "intel", "volatile");
    // debug!("register values: RIP: {:#X}, RSP: {:#X}, RBP: {:#X}", rip, rsp, rbp);

    //let mut x = rsp - 8;
    #[cfg(not(downtime_eval))]
    debug!("Perform constant offset fix for stack");

    // Perform constant offset fix for the stack range
    match constant_offset_fix(&swap_ranges, bottom, top) {
        Ok(()) => {
            Ok(swap_ranges)
        }
        Err (e) => {
            debug! {"Failed to perform constant offset fix for the stack"};
            Err(e.to_string())
        }
    }
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
        log_handled_fault(fe);
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
        log_handled_fault(fe);
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

/// slightly advanced swap policy. 
/// When this policy is enabled the following actions are taken in order to the number of time same fault is detected. 
/// 1) Restart the task
/// 2) Replace the crate fault is detected
/// 3) Replace the application crate
fn iterative_swap_policy() -> Option<String> {

    #[cfg(not(downtime_eval))]
    debug!("Running iterative swap policy");

    // We get all unhanlded faults
    let unhandled_list: Vec<FaultEntry> = remove_unhandled_exceptions();
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
        log_handled_fault(fe);
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