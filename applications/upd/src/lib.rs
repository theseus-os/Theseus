//! This application offers a front-end for communicating with
//! Theseus's update server to download updated crate object files,
//! apply live updates to evolve Theseus, traverse update history, etc.


#![no_std]
#![feature(slice_concat_ext)]

extern crate alloc;
#[macro_use] extern crate app_io;
extern crate itertools;

extern crate getopts;
extern crate task;
extern crate ota_update_client;
extern crate net;
extern crate memory;
extern crate mod_mgmt;
extern crate crate_swap;
extern crate path;
extern crate memfs;
extern crate fs_node;
extern crate vfs_node;
extern crate spin;


use core::str::FromStr;
use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
    vec::Vec,
    collections::BTreeSet,
    sync::Arc,
};
use spin::Once;
use getopts::{Matches, Options};
use net::{get_default_interface, IpEndpoint};
use mod_mgmt::{
    CrateNamespace,
    NamespaceDir,
    IntoCrateObjectFile,
};
use crate_swap::{
    SwapRequest,
    SwapRequestList,
};
use memfs::MemFile;
use path::Path;
use vfs_node::VFSDirectory;
use fs_node::{FileOrDir, DirRef};
use ota_update_client::DIFF_FILE_NAME;



static VERBOSE: Once<bool> = Once::new();
macro_rules! verbose {
    () => (VERBOSE.get() == Some(&true));
}


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "verbose", "enable verbose logging");
    opts.optopt ("d", "destination", "specify the IP address (and optionally, the port) of the update server", "IP_ADDR[:PORT]");

    let matches = match opts.parse(args) {
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
            .map_err(|_e| "couldn't parse destination IP address/port".to_string())?
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
            Err(format!("unrecognized command {other:?}"))
        }
    }
}



/// Lists the set of crates in the given update_build,
/// or if no update build is specified, lists all available update builds by default.
fn list(remote_endpoint: IpEndpoint, update_build: Option<&String>) -> Result<(), String> {
    let iface = get_default_interface().ok_or_else(|| "couldn't get default interface".to_owned())?;

    if let Some(ub) = update_build {
        let listing = ota_update_client::download_listing(&iface, remote_endpoint, ub)
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
    let iface = get_default_interface().ok_or_else(|| "couldn't get default interface".to_owned())?;

    let file_str = ota_update_client::download_diff(&iface, remote_endpoint, update_build)
        .map_err(|e| e.to_string())?;
    println!("{}", file_str.join("\n"));

    Ok(())
} 


/// Downloads all of the new or changed crates from the `diff` file of the 
fn download(remote_endpoint: IpEndpoint, update_build: &str, crate_list: Option<&[String]>) -> Result<(), String> {
    let iface = get_default_interface().ok_or_else(|| "couldn't get default interface".to_owned())?;
    println!("Downloading crates...");
    let crate_list = if crate_list == Some(&[]) { None } else { crate_list };

    let mut diff_file_lines: Option<Vec<String>> = None;

    let crates = if let Some(crate_list) = crate_list {
        let crate_set = crate_list.iter().cloned().collect::<BTreeSet<String>>();
        ota_update_client::download_crates(&iface, remote_endpoint, update_build, crate_set).map_err(|e| e.to_string())?
    } else {
        let diff_lines = ota_update_client::download_diff(&iface, remote_endpoint, update_build)
            .map_err(|e| format!("failed to download diff file for {update_build}, error: {e}"))?;
        let diff = ota_update_client::parse_diff_lines(&diff_lines).map_err(|e| e.to_string())?;

        // download all of the new crates
        let new_crates_to_download: BTreeSet<String> = diff.pairs.iter().map(|(_old, new)| new.clone()).collect();
        let crates = ota_update_client::download_crates(&iface, remote_endpoint, update_build, new_crates_to_download).map_err(|e| e.to_string())?;
        diff_file_lines = Some(diff_lines);
        crates
    };
    
    // save each new crate to a file 
    let Ok(curr_dir) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        return Err("failed to get current task's working directory".to_string());
    };
    let new_namespace_dir = NamespaceDir::new(make_unique_directory(update_build, &curr_dir)?);
    for df in crates.into_iter() {
        let content = df.content.as_result_err_str()?;
        let size = content.len();
        // The name of the crate file that we downloaded is something like: "/keyboard_log/k#keyboard-36be916209949cef.o".
        // We need to get just the basename of the file, then remove the crate type prefix ("k#").
        let df_path = Path::new(df.name);
        let cfile = new_namespace_dir.write_crate_object_file(df_path.basename(), content)?;
        println!("Downloaded crate: {:?}, size {}", cfile.lock().get_absolute_path(), size);
    }

    // if downloaded, save the diff file into the base directory
    if let Some(diffs) = diff_file_lines {
        let cfile = MemFile::create(String::from(DIFF_FILE_NAME), &new_namespace_dir)?;
        cfile.lock().write_at(diffs.join("\n").as_bytes(), 0)?;
    }

    Ok(())
}


/// Applies an already-downloaded update according the "diff.txt" file
/// that must be in the given base directory.
fn apply(base_dir_path: &Path) -> Result<(), String> {
    if cfg!(not(loadable)) {
        return Err("Evolutionary updates can only be applied when Theseus is built in loadable mode.".to_string());
    }

    let kernel_mmi_ref = memory::get_kernel_mmi_ref().ok_or_else(|| "couldn't get kernel MMI".to_string())?;
    let Ok(curr_dir) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        return Err("failed to get current task's working directory".to_string());
    };
    let new_namespace_dir = match base_dir_path.get(&curr_dir) {
        Some(FileOrDir::Dir(d)) => NamespaceDir::new(d),
        _ => return Err(format!("cannot find an update base directory at path {base_dir_path}")),
    };
    let diff_file = match new_namespace_dir.lock().get(DIFF_FILE_NAME) { 
        Some(FileOrDir::File(f)) => f,
        _ => return Err(format!("cannot find diff file expected at {base_dir_path}/{DIFF_FILE_NAME}")),
    };
    let mut diff_content: Vec<u8> = alloc::vec::from_elem(0, diff_file.lock().len()); 
    let _bytes_read = diff_file.lock().read_at(&mut diff_content, 0)?;
    let diffs = ota_update_client::as_lines(&diff_content).map_err(|e| e.to_string())
        .and_then(|diff_lines| ota_update_client::parse_diff_lines(&diff_lines).map_err(|e| e.to_string()))?;


    // We can't immediately just replace the existing files in the current namespace 
    // because that would cause inconsistencies if another crate was loaded (using the new files)
    // before the currently-loaded ones were replaced. 
    // Instead, we need to just keep the new files in a new folder for now (which they already are),
    // and tell the crate swapping routine to use them. 
    // But first, we must create SwapRequests, which validates that the current namespace 
    // actually has all of the old crates that are expected/listed in the diff file.
    // After the live swap of all loaded crates in the namespace has completed,
    // it is safe to actually replace the old crate object files with the new ones. 
    
    let curr_namespace = get_my_current_namespace();
    // Now we create swap requests based on the contents of the diff file
    let mut swap_requests = SwapRequestList::new();

    for (old_crate_module_file_name, new_crate_module_file_name) in diffs.pairs.into_iter() {
        println!("Looking at diff {} -> {}", old_crate_module_file_name, new_crate_module_file_name);
        let old_crate_name = if old_crate_module_file_name.is_empty() {
            // An empty old_crate_name indicates that there is no old crate or object file to remove, we are just loading a new crate (or inserting its object file)
            None
        } else {
            let old_crate_name = mod_mgmt::crate_name_from_path(&Path::new(old_crate_module_file_name)).to_string();
            if curr_namespace.get_crate(&old_crate_name).is_none() {
                println!("\t Note: old crate {:?} was not currently loaded into namespace {:?}.", old_crate_name, curr_namespace.name());
            }
            Some(old_crate_name)
        };
        
        let new_crate_file = new_namespace_dir.get_crate_object_file(&new_crate_module_file_name).ok_or_else(|| 
            format!("cannot find new crate file {new_crate_module_file_name:?} in new namespace dir {base_dir_path}")
        )?;

        let swap_req = SwapRequest::new(
            old_crate_name.as_deref(),
            Arc::clone(&curr_namespace),
            IntoCrateObjectFile::File(new_crate_file),
            None, // all diff-based swaps occur within the same namespace
            false
        ).map_err(|invalid_req| format!("{invalid_req:#?}"))?;
        swap_requests.push(swap_req);
    }

    // now do the actual live crate swap at runtime
    crate_swap::swap_crates(
        &curr_namespace,
        swap_requests, 
        Some(new_namespace_dir), 
        diffs.state_transfer_functions, 
        kernel_mmi_ref,
        false, // verbose logging
        false, // enable_crate_cache
    ).map_err(|e| format!("crate swapping failed, error: {e}"))?;

    Ok(())
}


fn get_my_current_namespace() -> Arc<CrateNamespace> {
    task::with_current_task(|t| t.get_namespace().clone())
        .unwrap_or_else(|_|
            mod_mgmt::get_initial_kernel_namespace()
                .expect("BUG: initial kernel namespace wasn't initialized")
                .clone()
        )
}


/// Creates a new directory with a unique name in the given `parent_dir`. 
/// For example, given a base_name of "my_dir", 
/// it will create a directory "my_dir.2" if "my_dir" and "my_dir.1" already exist.
fn make_unique_directory(base_name: &str, parent_dir: &DirRef) -> Result<DirRef, &'static str> {
    if parent_dir.lock().get(base_name).is_none() {
        return VFSDirectory::create(base_name.to_string(), parent_dir);
    }
    for i in 1.. {
        let new_base_name = format!("{base_name}.{i}");
        if parent_dir.lock().get(&new_base_name).is_none() {
            return VFSDirectory::create(new_base_name, parent_dir);
        }   
    }

    Err("unable to create new unique directory")
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &str = "Usage: upd [OPTIONS] COMMAND
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
        in the given BASE_DIR, which contains the new crate object files to be used.";
