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


    for req in &swap_requests {

        let SwapRequest { old_crate_name, old_namespace, new_crate_object_file: _, new_namespace: _, reexport_new_symbols_as_old: _ } = req;

        let old_crate_ref = match old_crate_name.as_deref().and_then(|ocn| CrateNamespace::get_crate_and_namespace(old_namespace, ocn)) {
            Some((ocr, _ns)) => {
                ocr
            }
            _ => {
                continue;
            }
        };

        debug!("{:?}",old_crate_ref);

        let old_crate = old_crate_ref.lock_as_mut().ok_or_else(|| {
            error!("Unimplemented: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exclusive mutable instance", old_crate_ref);
            "Unimplemented: swap_crates() doesn't yet support deep copying shared crates to get a new exclusive mutable instance"
        })?;


        // Find the text, data and rodata ranges of old crate
        {
            match &old_crate.text_pages {
                Some((_pages,address)) => {
                    debug!("Text Range is {:X} - {:X}", address.start, address.end);
                    old_text_low = Some(address.start.clone());
                    old_text_high = Some(address.end.clone());
                }
                _=>{}
            };

            match &old_crate.rodata_pages {
                Some((_pages,address)) => {
                    debug!("Rodata Range is {:X} - {:X}", address.start, address.end);
                    old_rodata_low = Some(address.start.clone());
                    old_rodata_high = Some(address.end.clone());
                }
                _=>{}
            };

            match &old_crate.data_pages {
                Some((_pages,address)) => {
                    debug!("data Range is {:X} - {:X}", address.start, address.end);
                    old_data_low = Some(address.start.clone());
                    old_data_high = Some(address.end.clone());
                }
                _=>{}
            };
        }

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

    let ocn = crate_name;

    // Find the new crate loaded. It should have the exact same name as the old crate
    let mut matching_crates = CrateNamespace::get_crates_starting_with(&namespace, ocn);

    // There can't be more than one match if swap occured correctly
    if matching_crates.len() == 1 {
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
                    debug!("Text Range was {:X} - {:X}", old_text_low.unwrap(), old_text_high.unwrap());
                    debug!("Text Range is  {:X} - {:X}", address.start, address.end);
                    new_text_low = Some(address.start.clone());
                    new_text_high = Some(address.end.clone());
                }
                _ => {}
            };

            match &new_crate.rodata_pages {
                Some((_pages,address)) => {
                    debug!("Rodata Range was {:X} - {:X}", old_rodata_low.unwrap(), old_rodata_high.unwrap());
                    debug!("Rodata Range is  {:X} - {:X}", address.start, address.end);
                    new_rodata_low = Some(address.start.clone());
                    new_rodata_high = Some(address.end.clone());
                }
                _ => {}
            };

            match &new_crate.data_pages {
                Some((_pages,address)) => {
                    debug!("data Range was {:X} - {:X}", old_data_low.unwrap(), old_data_high.unwrap());
                    debug!("data Range is  {:X} - {:X}", address.start, address.end);
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
            debug!("Swap operation complete");
            Ok(return_struct)
        }
        Err(e) => Err(e.to_string())
    }


}
