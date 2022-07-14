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
use core::ops::Range;

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
use fault_log::{RecoveryAction, FaultEntry, remove_unhandled_exceptions, log_handled_fault};

/// A data structure to hold the ranges of memory used by the old crate and the new crate.
/// The crate only maintains the values as virtual addresses and holds no references to any
/// crate
#[derive(Clone, Default)]
pub struct SwapRanges{
    pub old_text : Option<Range<VirtualAddress>>,
    pub old_rodata : Option<Range<VirtualAddress>>,
    pub old_data : Option<Range<VirtualAddress>>,

    pub new_text : Option<Range<VirtualAddress>>,
    pub new_rodata : Option<Range<VirtualAddress>>,
    pub new_data : Option<Range<VirtualAddress>>,
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
    namespace: &Arc<CrateNamespace>,
    verbose_log: bool
) -> Result<SwapRanges, String> {

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
            Arc::clone(namespace),
            into_new_crate_file,
            new_namespace,
            false, //reexport
        ).map_err(|invalid_req| format!("{:#?}", invalid_req))?;

        #[cfg(not(downtime_eval))]
        debug!("swap call {:?}", swap_req);

        requests.push(swap_req);

        requests
    };


    let mut matching_crates = CrateNamespace::get_crates_starting_with(namespace, crate_name);

    // There can be only one matching crate for a given crate name
    if matching_crates.len() == 0 {
        return Err("No crates currently loaded matches ".to_string() + crate_name);
    }

    if matching_crates.len() > 1 {
        return Err("More than one crate matches ".to_string() + crate_name);
    }

    let mut return_struct = SwapRanges::default();

    let (_old_crate_name, old_crate_ref, _old_namespace) = matching_crates.remove(0);

    #[cfg(not(downtime_eval))]
    debug!("{:?}",old_crate_ref);

    {
        let old_crate = old_crate_ref.lock_as_ref();
        // Find the text, data and rodata ranges of old crate
        return_struct.old_text = old_crate.text_pages.as_ref().map(|(_mp, addr_range)| addr_range.clone());
        return_struct.old_rodata = old_crate.rodata_pages.as_ref().map(|(_mp, addr_range)| addr_range.clone());
        return_struct.old_data = old_crate.data_pages.as_ref().map(|(_mp, addr_range)| addr_range.clone());
    }


    // Swap crates
    let swap_result = swap_crates(
        namespace,
        swap_requests, 
        override_namespace_crate_dir,
        state_transfer_functions,
        &kernel_mmi_ref,
        verbose_log,
        false // enable crate_cahce
    );

    let ocn = crate_name;

    // Find the new crate loaded. It should have the exact same name as the old crate
    let mut matching_crates = CrateNamespace::get_crates_starting_with(namespace, ocn);

    // There can be only one matching crate for a given crate name
    if matching_crates.len() == 0 {
        return Err("No crates currently loaded matches ".to_string() + crate_name);
    }

    if matching_crates.len() > 1 {
        return Err("More than one crate matches ".to_string() + crate_name);
    }

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
    
    // Find the address range of the newly loaded crate
    {
        let new_crate = new_crate_ref.lock_as_ref();
        return_struct.new_text   = new_crate.text_pages  .as_ref().map(|(_mp, addr_range)| addr_range.clone());
        return_struct.new_rodata = new_crate.rodata_pages.as_ref().map(|(_mp, addr_range)| addr_range.clone());
        return_struct.new_data   = new_crate.data_pages  .as_ref().map(|(_mp, addr_range)| addr_range.clone());

        #[cfg(not(downtime_eval))]
        {
            if let (Some(old_text) , Some(new_text)) = (return_struct.old_text.clone(), return_struct.new_text.clone()) {
                    debug!("Text Range was {:X} - {:X}", old_text.start, old_text.end);
                    debug!("Text Range is {:X} - {:X}", new_text.start, new_text.end);
            }

            if let (Some(old_data) , Some(new_data)) = (return_struct.old_data.clone(), return_struct.new_data.clone()) {
                    debug!("Data Range was {:X} - {:X}", old_data.start, old_data.end);
                    debug!("Data Range is {:X} - {:X}", new_data.start, new_data.end);
            }

            if let (Some(old_rodata) , Some(new_rodata)) = (return_struct.old_rodata.clone(), return_struct.new_rodata.clone()) {
                    debug!("Rodata Range was {:X} - {:X}", old_rodata.start, old_rodata.end);
                    debug!("Rodata Range is {:X} - {:X}", new_rodata.start, new_rodata.end);
            }
        }
    }


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

    if swap_ranges.new_text.is_none(){
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

        if let (Some(old_text) , Some(new_text)) = (swap_ranges.old_text.clone(), swap_ranges.new_text.clone()) {
            if old_text.start < ra && old_text.end > ra {

                // debug!("Match text at address {:#X}, value  = {:#X}", x , n);

                // Note : Does not use saturation add since underflow is not possible and overflow could not be caused in proper execution
                let n_ra = ra - old_text.start + new_text.start;
                let p1 = (x) as *mut usize;
                unsafe { ptr::write(p1,n_ra.value()) };
                // For debug purposes we re-read teh value
                // #[cfg(not(downtime_eval))]
                // {
                //     let n =  unsafe { ptr::read(p) };
                //     debug!("New value at address {:#X}, value  = {:#X}", x , n);
                // }
            }
        }

        if let (Some(old_rodata) , Some(new_rodata)) = (swap_ranges.old_rodata.clone(), swap_ranges.new_rodata.clone()) {
            if old_rodata.start < ra && old_rodata.end > ra {

                // debug!("Match rodata at address {:#X}, value  = {:#X}", x , n);
                let n_ra = ra - old_rodata.start + new_rodata.start;
                let p1 = (x) as *mut usize;
                unsafe { ptr::write(p1,n_ra.value()) };

                // #[cfg(not(downtime_eval))]
                // {
                //     let n =  unsafe { ptr::read(p) };
                //     debug!("New value at address {:#X}, value  = {:#X}", x , n);
                // }
            }
        }

        if let (Some(old_data) , Some(new_data)) = (swap_ranges.old_data.clone(), swap_ranges.new_data.clone()) {
            if old_data.start < ra && old_data.end > ra {

                // debug!("Match data at address {:#X}, value  = {:#X}", x , n);
                let n_ra = ra - old_data.start + new_data.start;
                let p1 = (x) as *mut usize;
                unsafe { ptr::write(p1,n_ra.value()) };
                // #[cfg(not(downtime_eval))]
                // {
                //     let n =  unsafe { ptr::read(p) };
                //     debug!("New value at address {:#X}, value  = {:#X}", x , n);
                // }
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
pub fn self_swap_handler(crate_name: &str) -> Result<SwapRanges, String> {

    let taskref = task::get_my_current_task()
        .ok_or_else(|| format!("failed to get current task"))?;

    #[cfg(not(downtime_eval))]
    debug!("The taskref is {:?}",taskref);

    let curr_dir = Arc::clone(&taskref.get_env().lock().working_dir);

    let override_namespace_crate_dir = Option::<NamespaceDir>::None;

    let verbose = false;
    let state_transfer_functions: Vec<String> = Vec::new();

    let tuples: Vec<(&str, &str, bool)> = vec![(crate_name, crate_name , false)];

    #[cfg(not(downtime_eval))]
    debug!("tuples: {:?}", tuples);

    // 1) Call generic crate swapping routine
    let swap_result = do_self_swap(
        crate_name, 
        &curr_dir, 
        override_namespace_crate_dir,
        state_transfer_functions,
        &taskref.namespace,
        verbose
    );

    let swap_ranges = match swap_result {
        Ok(x) => {

            #[cfg(not(downtime_eval))]
            debug!("Swap operation complete");
            x
        }
        Err(e) => {
            debug!("SWAP FAILED at do_self_swap");
            return Err(e)
        }
    };

    for (_id, taskref) in task::TASKLIST.lock().iter() {
        let (bottom, top) = taskref.with_kstack(|kstack| 
            (kstack.bottom().value(), kstack.top_usable().value())
        ); 
        // debug!("Bottom and top of stack of task {} are {:X} {:X}", taskref.name, bottom, top);

        match constant_offset_fix(&swap_ranges, bottom, top) {
            Err (e) => {
                debug! {"Failed to perform constant offset fix for the stack for task {} due to {}", taskref.name, e.to_string()};
            },
            _ => {},
        }
    }

    Ok(swap_ranges)
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
#[cfg(use_crate_replacement)]
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

    #[cfg(not(downtime_eval))]
    debug!("Repalce : {:?}",crate_to_swap);

    crate_to_swap
}

/// slightly advanced swap policy. 
/// When this policy is enabled the following actions are taken in order to the number of time same fault is detected. 
/// 1) Restart the task
/// 2) Replace the crate fault is detected
/// 3) Replace the application crate
#[cfg(use_crate_replacement)]
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

            if let Some(error_crate) = unhandled_list[0].crate_error_occured.clone() {

                let error_crate_name = error_crate.clone();
                let error_crate_name_simple = error_crate_name.split("-").next().unwrap_or_else(|| &error_crate_name);

                // We check whether the fault is logged previously
                let fe_last_fault = fault_log::get_the_most_recent_match(error_crate_name_simple);
                match fe_last_fault {

                    // If the fault is logged we check the action taken and then take the next drastic action.
                    Some(fault_entry) => {
                        if fault_entry.action_taken == RecoveryAction::FaultCrateReplaced {
                            if let Some(app_crate) = fault_entry.running_app_crate {
                                // last action : 2  -> Next action : 3
                                crate_to_swap = Some(app_crate.clone());
                                fe.action_taken = RecoveryAction::IterativelyCrateReplaced;
                                fe.replaced_crates.push(app_crate.clone());
                            } else {
                                // last action : 2 -> Next action : 2 due to insufficient details
                                crate_to_swap = Some(error_crate.clone());
                                fe.action_taken = RecoveryAction::FaultCrateReplaced;
                                fe.replaced_crates.push(error_crate.clone());
                            }
                        } else if fault_entry.action_taken == RecoveryAction::TaskRestarted || fault_entry.action_taken == RecoveryAction::MultipleFaultRecovery {
                            // last action : 1  -> Next action : 2
                            crate_to_swap = Some(error_crate.clone());
                            fe.action_taken = RecoveryAction::FaultCrateReplaced;
                            fe.replaced_crates.push(error_crate.clone());
                        } else if fault_entry.action_taken == RecoveryAction::None {
                            // last action : None  -> Next action : 1
                            crate_to_swap = None;
                            fe.action_taken = RecoveryAction::TaskRestarted;
                        } else {
                            // We have exhausted all our fault tolerance stages. So we just reuse stage 2 
                            crate_to_swap = Some(error_crate.clone());
                            fe.action_taken = RecoveryAction::FaultCrateReplaced;
                            fe.replaced_crates.push(error_crate.clone());
                        }
                    }

                    // If the fault is not logged we start from step 1
                    None => {
                        crate_to_swap = None;
                        fe.action_taken = RecoveryAction::TaskRestarted;
                    }
                }
            } else {
                // If the crate fault occured is not logged we can only restart
                debug!("No information on where the first failure occured");
                fe.action_taken = RecoveryAction::TaskRestarted;
            }
        } else {
            fe.action_taken = RecoveryAction::MultipleFaultRecovery;
        }
        log_handled_fault(fe);
    }

    debug!("Repalce : {:?}",crate_to_swap);

    crate_to_swap
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
