//! Simple routines to test various OTA live update scenarios. 
//! 

#![no_std]
#![feature(alloc)]
#![feature(try_from)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate memory;
extern crate network_manager;
extern crate mod_mgmt;
extern crate ota_update_client;
extern crate vfs_node;
extern crate path;
extern crate memfs;

use core::ops::DerefMut;
use alloc::{
    string::String,
    collections::BTreeSet,
};
use network_manager::{NetworkInterfaceRef};
use vfs_node::VFSDirectory;
use memfs::MemFile;
use mod_mgmt::{SwapRequest, SwapRequestList, metadata::CrateType};
use path::Path;



// TODO: real update sequence: 
// (1) establish connection to build server
// (2) download the UPDATE MANIFEST, a file that describes which crates should replace which others
// (3) compare the crates in the given CrateNamespace with the latest available crates from the manifest
// (4) send HTTP requests to download the ones that differ 
// (5) swap in the new crates in place of the old crates



/// Implements a very simple update scenario that downloads the "keyboard_log" update build and deploys it. 
pub fn simple_keyboard_swap(iface: NetworkInterfaceRef) -> Result<(), &'static str> {
    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or("couldn't get kernel MMI")?;
    let default_namespace = mod_mgmt::get_default_namespace().ok_or("couldn't get default namespace")?;
    let namespaces_dir = mod_mgmt::get_namespaces_directory().ok_or("couldn't get directory of namespaces")?;

    let update_builds = ota_update_client::download_available_update_builds(&iface)?;
    warn!("AVAILABLE UPDATE BUILDS: {:?}", update_builds);
    let keyboard_log_ub = update_builds.iter()
        .find(|&e| e == "keyboard_log")
        .ok_or("build server did not have an update build called \"keyboard_log\"")?;

    let crates_to_include = {
        let mut set: BTreeSet<String> = BTreeSet::new();
        // this list is hardcoded right now based on what the build server offers
        set.insert(String::from("k#keyboard-36be916209949cef.o")); 
        set.insert(String::from("k#alloc-f655a0dd1878a29d.o"));
        set
    };
    let new_crates = ota_update_client::download_crates(&iface, keyboard_log_ub, crates_to_include)?;
    if new_crates.is_empty() {
        return Err("failed to download any crate files");
    }

    // save the newly-downloaded crates to the fs, and create swap requests based on them
    let update_build_dir = VFSDirectory::new(keyboard_log_ub.clone(), &namespaces_dir)?;
    let mut swap_requests = SwapRequestList::with_capacity(new_crates.len());

    warn!("DOWNLOADED CRATES:"); 
    for df in new_crates.into_iter() {
        let content = df.content.as_result_err_str()?;
        debug!("Saving downloaded crate to file: {:?}, size {}", df.name, content.len());
        // The name of the crate file that we downloaded is something like: "/keyboard_log/k#keyboard-36be916209949cef.o".
        // We need to get just the basename of the file, then remove the crate type prefix ("k#"), and then strip the trailing hash after the "-". 
        let df_path = Path::new(df.name);
        let (_crate_type, _prefix, objfilename) = CrateType::from_module_name(df_path.basename())?;
        let cname_no_hash = objfilename.split("-").next().ok_or("downloaded crate name couldn't be split at the '-' hash delimiter")?;
        debug!("\tobjfilename: {:?}, cname_no_hash: {:?}, crate type: {:?}, _prefix: {:?}", objfilename, cname_no_hash, _crate_type, _prefix);
        let cfile = MemFile::new(String::from(objfilename), content, &update_build_dir)?;
        debug!("\tcreated new file at path: {}", cfile.lock().get_path_as_string());
        let search_str = format!("{}-", cname_no_hash);
        debug!("\tsearch_str: {}", search_str);
        let old_crate_name = default_namespace.get_crate_starting_with(&search_str)
            .map(|(name, _old_crate_ref)| name)
            .ok_or("couldn't find matching old crate in namespace")?;
        let swap_req = SwapRequest::new(old_crate_name, Path::new(cfile.lock().get_path_as_string()), true);
        swap_requests.push(swap_req);
    }

    debug!("SWAP_REQUESTS: {:?}", swap_requests);
    default_namespace.swap_crates(swap_requests, Some(update_build_dir.clone()), kernel_mmi_ref.lock().deref_mut(), false)?;

    Err("unfinished")
}




// fn download_from_listing() {
//     // Iterate over the list of crate files in the listing, and build a vector of file paths to download. 
//     // We need to download all crates that differ from the given list of `existing_crates`, 
//     // plus the files containing those crates' sha512 checksums.
//     let files_list = str::from_utf8(listing_file.content.content())
//         .map_err(|_e| "couldn't convert received update listing file into a UTF8 string")?;

//     let mut paths_to_download: Vec<String> = Vec::new();
//     for file in files_list.lines()
//         .map(|line| line.trim())
//         .filter(|&line| crate_set.includes(line))
//     {

//     }
// }
