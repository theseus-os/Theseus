//! Simple routines to test various OTA live update scenarios. 
//! 

#![no_std]
#![feature(alloc)]
#![feature(try_from)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate network_manager;
extern crate mod_mgmt;
extern crate ota_update_client;

use alloc::{
    string::String,
    collections::BTreeSet,
};
use network_manager::{NetworkInterfaceRef};



// TODO: real update sequence: 
// (1) establish connection to build server
// (2) download the UPDATE MANIFEST, a file that describes which crates should replace which others
// (3) compare the crates in the given CrateNamespace with the latest available crates from the manifest
// (4) send HTTP requests to download the ones that differ 
// (5) swap in the new crates in place of the old crates



/// Implements a very simple update scenario that downloads the "keyboard_log" update build and deploys it. 
pub fn simple_keyboard_swap(iface: NetworkInterfaceRef) -> Result<(), &'static str> {
    let update_builds = ota_update_client::download_available_update_builds(&iface)?;

    warn!("AVAILABLE UPDATE BUILDS: {:?}", update_builds);
    let keyboard_log_ub = update_builds.iter()
        .find(|&e| e == "keyboard_log")
        .ok_or("build server did not have an update build called \"keyboard_log\"")?;

    let crates_to_include = {
        let mut set: BTreeSet<String> = BTreeSet::new();
        set.insert(String::from("k#keyboard-36be916209949cef.o"));
        // set.insert(String::from("k#atomic-905b6e75d16e053c.o"));
        // set.insert(String::from("k#bit_field-beaefef505b4f61a.o"));
        set.insert(String::from("k#dbus-495835640c757fc3.o"));
        set.insert(String::from("k#xmas_elf-3d0cd20d4e1d4ba9.o"));
        set
    };
    let new_crates = ota_update_client::download_crates(&iface, keyboard_log_ub, crates_to_include)?;

    warn!("DOWNLOADED CRATES:"); 
    for c in new_crates {
        warn!("\t{:?}: size {}", c.name, c.content.content().len());
    }
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
