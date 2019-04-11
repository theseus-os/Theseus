//! This application offers a front-end for communicating with
//! Theseus's update server to download updated crate object files,
//! apply live updates to evolve Theseus, traverse update history, etc.


#![no_std]
#![feature(alloc)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate itertools;

extern crate getopts;
extern crate task;
extern crate ota_update_client;
extern crate network_manager;
extern crate memory;
extern crate mod_mgmt;
extern crate smoltcp;
extern crate path;
extern crate memfs;
extern crate fs_node;
extern crate vfs_node;
extern crate spin;


use core::str::FromStr;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeSet,
    slice::SliceConcatExt,
};
use spin::Once;
use getopts::{Matches, Options};
use network_manager::{NetworkInterfaceRef, NETWORK_INTERFACES};
use smoltcp::wire::IpEndpoint;
use mod_mgmt::{
    CrateNamespace,
    NamespaceDirectorySet,
    SwapRequest, SwapRequestList,
};
use memfs::MemFile;
use path::Path;
use vfs_node::VFSDirectory;
use fs_node::{FileOrDir, DirRef};
use ota_update_client::DIFF_FILE_NAME;



static VERBOSE: Once<bool> = Once::new();
macro_rules! verbose {
    () => (VERBOSE.try() == Some(&true));
}


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose logging");
    opts.optopt ("d", "destination", "specify the IP address (and optionally, the port) of the update server", "IP_ADDR[:PORT]");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    if matches.free.is_empty() { 
        println!("Error: missing command.");
        print_usage(opts);
        return -1;
    }

    VERBOSE.call_once(|| matches.opt_present("v"));

    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }    
    }
}


fn rmain(matches: Matches) -> Result<(), String> {
    let mut remote_endpoint = if let Some(ip_str) = matches.opt_str("d") {
        IpEndpoint::from_str(&ip_str)
            .map_err(|_e| format!("couldn't parse destination IP address/port"))?
    } else {
        ota_update_client::default_remote_endpoint()
    };
    if remote_endpoint.port == 0 {
        remote_endpoint.port = ota_update_client::default_remote_endpoint().port;
    }

    if verbose!() { println!("MATCHES: {:?}", matches.free); }

    match &*matches.free[0] {
        "list" | "ls" => {
            list(remote_endpoint, matches.free.get(1))
        }
        "list-diff" | "ls-diff" => {
            let update_build = matches.free.get(1).ok_or_else(|| String::from("missing UPDATE_BUILD argument"))?;
            diff(remote_endpoint, update_build)
        }
        "download" | "dl" => {
            let update_build = matches.free.get(1).ok_or_else(|| String::from("missing UPDATE_BUILD argument"))?;
            download(remote_endpoint, update_build, matches.free.get(2..))
        }
        "apply" | "ap" => {
            let base_dir_path = matches.free.get(1).ok_or_else(|| String::from("missing BASE_DIR path argument"))?;
            apply(&Path::new(base_dir_path.clone()))
        }
        other => {
            Err(format!("unrecognized command {:?}", other))
        }
    }
}



/// Lists the set of crates in the given update_build,
/// or if no update build is specified, lists all available update builds by default.
fn list(remote_endpoint: IpEndpoint, update_build: Option<&String>) -> Result<(), String> {
    let iface = get_default_iface()?;

    if let Some(ub) = update_build {
        let listing = ota_update_client::download_listing(&iface, remote_endpoint, &*ub)
            .map_err(|e| e.to_string())?;
        println!("{}", listing.join("\n"));
    } else {
        let update_builds = ota_update_client::download_available_update_builds(&iface, remote_endpoint)
            .map_err(|e| e.to_string())?;
        println!("{}", update_builds.join("\n"));
    }

    Ok(())
} 


/// Lists the contents of the diff file for the given update build.
fn diff(remote_endpoint: IpEndpoint, update_build: &str) -> Result<(), String> {
    let iface = get_default_iface()?;

    let file_str = ota_update_client::download_diff(&iface, remote_endpoint, update_build)
        .map_err(|e| e.to_string())?;
    println!("{}", file_str.join("\n"));

    Ok(())
} 


/// Downloads all of the new or changed crates from the `diff` file of the 
fn download(remote_endpoint: IpEndpoint, update_build: &str, crate_list: Option<&[String]>) -> Result<(), String> {
    let iface = get_default_iface()?;
    let crate_list = if crate_list == Some(&[]) { None } else { crate_list };

    let mut diff_file_lines: Option<Vec<String>> = None;

    let crates = if let Some(crate_list) = crate_list {
        let crate_set = crate_list.iter().cloned().collect::<BTreeSet<String>>();
        ota_update_client::download_crates(&iface, remote_endpoint, update_build, crate_set).map_err(|e| e.to_string())?
    } else {
        let diff_lines = ota_update_client::download_diff(&iface, remote_endpoint, update_build)
            .map_err(|e| format!("failed to download diff file for {}, error: {}", update_build, e))?;
        let diff = ota_update_client::parse_diff_lines(&diff_lines).map_err(|e| e.to_string())?;

        // download all of the new crates
        let new_crates_to_download: BTreeSet<String> = diff.pairs.iter().map(|(_old, new)| new.clone()).collect();
        let crates = ota_update_client::download_crates(&iface, remote_endpoint, update_build, new_crates_to_download).map_err(|e| e.to_string())?;
        diff_file_lines = Some(diff_lines);
        crates
    };
    
    // save each new crate to a file 
    let curr_dir = task::get_my_working_dir().ok_or_else(|| format!("couldn't get my current working directory"))?;
    let new_namespace_dir = make_unique_directory(update_build, &curr_dir)?;
    let new_namespace_name = new_namespace_dir.lock().get_name();
    let new_namespace = CrateNamespace::new(new_namespace_name, NamespaceDirectorySet::new(new_namespace_dir)?);
    for df in crates.into_iter() {
        let content = df.content.as_result_err_str()?;
        let size = content.len();
        // The name of the crate file that we downloaded is something like: "/keyboard_log/k#keyboard-36be916209949cef.o".
        // We need to get just the basename of the file, then remove the crate type prefix ("k#").
        let df_path = Path::new(df.name);
        let cfile = new_namespace.dirs().insert_crate_object_file(df_path.basename(), content)?;
        println!("Downloaded crate: {:?}, size {}", cfile.lock().get_absolute_path(), size);
    }

    // if downloaded, save the diff file into the base directory
    if let Some(diffs) = diff_file_lines {
        let cfile = MemFile::new(String::from(DIFF_FILE_NAME), new_namespace.dirs().base_directory())?;
        cfile.lock().write(diffs.join("\n").as_bytes(), 0)?;
    }

    Ok(())
}


/// Applies an already-downloaded update according the "diff.txt" file
/// that must be in the given base directory.
fn apply(base_dir_path: &Path) -> Result<(), String> {
    if cfg!(not(loadable)) {
        return Err(format!("Evolutionary updates can only be applied when Theseus is built in loadable mode."));
    }

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| format!("couldn't get kernel MMI"))?;
    let curr_dir = task::get_my_working_dir().ok_or_else(|| format!("couldn't get my current working directory"))?;
    let base_dir = match base_dir_path.get(&curr_dir) {
        Ok(FileOrDir::Dir(d)) => d,
        _ => return Err(format!("cannot find base directory at path {}", base_dir_path)),
    };
    let new_namespace_dirs = NamespaceDirectorySet::from_existing_base_dir(base_dir.clone()).map_err(|e| e.to_string())?;

    let diff_file = match base_dir.lock().get(DIFF_FILE_NAME) { 
        Some(FileOrDir::File(f)) => f,
        _ => return Err(format!("cannot find diff file at {}/{}", base_dir_path, DIFF_FILE_NAME)),
    };
    let mut diff_content: Vec<u8> = alloc::vec::from_elem(0, diff_file.lock().size()); 
    let _bytes_read = diff_file.lock().read(&mut diff_content, 0)?;
    let diffs = ota_update_client::as_lines(&diff_content).map_err(|e| e.to_string())
        .and_then(|diff_lines| ota_update_client::parse_diff_lines(&diff_lines).map_err(|e| e.to_string()))?;


    // We can't immediately just replace the existing files in the current namespace 
    // because that would cause inconsistencies if another crate was loaded (using the new files)
    // before the currently-loaded ones were replaced. 
    // Instead, we need to just keep the new files in a new folder for now (which they already are),
    // and tell the crate swapping routine to use them. 
    // But first, we must check to make sure that the current namespace actually has all of the old crates
    // that are expected/listed in the diff file.
    // After the live swap of all loaded crates in the namespace has completed,
    // it is safe to actually replace the old crate object files with the new ones. 
    
    let curr_namespace = get_my_current_namespace();
    // create swap requests to replace the currently loaded old crates with the new crates 
    let mut swap_requests = SwapRequestList::new();
    for (old_crate_file_name, new_crate_file_name) in &diffs.pairs {
        println!("Looking at diff {} -> {}", old_crate_file_name, new_crate_file_name);
        // first, check to make sure the old crate actually exists
        let old_crate_file = curr_namespace.dirs().get_crate_object_file(old_crate_file_name)
            .ok_or_else(|| format!("cannot find old crate file {:?} in namespace {:?}", old_crate_file_name, curr_namespace.name))?;
        // the old needs to be swapped if it's currently loaded
        let old_crate_file_name = Path::new(old_crate_file.lock().get_name());
        let old_crate_name = old_crate_file_name.file_stem().to_string();
        if let Some(_old_crate) = curr_namespace.get_crate(&old_crate_name) {
            let new_crate_file = new_namespace_dirs.get_crate_object_file(new_crate_file_name)
                .ok_or_else(|| format!("cannot find new crate file {:?} in new namespace dirs {}", new_crate_file_name, base_dir_path))?;
            let swap_req = SwapRequest::new(old_crate_name, Path::new(new_crate_file.lock().get_absolute_path()), false)?;
            swap_requests.push(swap_req);
        }
        else {
            println!("   not swapping non-loaded old crate: {}", old_crate_name);
        }
    }

    // now do the actual live crate swap at runtime
    curr_namespace.swap_crates(
        swap_requests, 
        Some(new_namespace_dirs), 
        diffs.state_transfer_functions, 
        &mut kernel_mmi_ref.lock(), 
        false)
        .map_err(|e| format!("crate swapping failed, error: {}", e))?;

    Ok(())
}


// TODO: fix this later once each task's environment contains a current namespace
fn get_my_current_namespace() -> &'static CrateNamespace {
    mod_mgmt::get_default_namespace().unwrap()
}


/// Returns the first network interface available in the system.
fn get_default_iface() -> Result<NetworkInterfaceRef, String> {
    NETWORK_INTERFACES.lock()
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| format!("no network interfaces available"))
}


/// Creates a new directory with a unique name in the given `parent_dir`. 
/// For example, given a base_name of "my_dir", 
/// it will create a directory "my_dir.2" if "my_dir" and "my_dir.1" already exist.
fn make_unique_directory(base_name: &str, parent_dir: &DirRef) -> Result<DirRef, &'static str> {
    if parent_dir.lock().get(base_name).is_none() {
        return VFSDirectory::new(base_name.to_string(), parent_dir);
    }
    for i in 1.. {
        let new_base_name = format!("{}.{}", base_name, i);
        if parent_dir.lock().get(&new_base_name).is_none() {
            return VFSDirectory::new(new_base_name, parent_dir);
        }   
    }

    Err("unable to create new unique directory")
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: upd [OPTIONS] COMMAND
Runs the given update-related COMMAND. Choices include:
    list
        Lists all available updates from the update server.

    list UPDATE_BUILD
        Lists all crate files available for download from the given UPDATE_BUILD.

    list-diff UPDATE_BUILD
        Lists the patch-like diff for the given UPDATE_BUILD,
        which specifies which crates should be swapped with other crates.

    download UPDATE_BUILD
        Downloads all different crates (from the diff file) for the given UPDATE_BUILD.
        
    download UPDATE_BUILD CRATES
        Downloads the given list of CRATES for the given UPDATE_BUILD.
        
    apply BASE_DIR
        Applies the evolutionary update specified by the diff file 
        in the given BASE_DIR, which also must contain a set of 
        CrateNamespace subdirectories: \"kernel\" and \"applications\", at the least.";
