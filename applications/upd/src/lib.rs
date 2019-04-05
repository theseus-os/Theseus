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
extern crate ota_update_client;
extern crate network_manager;
extern crate memory;
extern crate smoltcp;


use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeSet,
    slice::SliceConcatExt,
};
use core::str::FromStr;
use getopts::{Matches, Options};
use network_manager::{NetworkInterfaceRef, NETWORK_INTERFACES};
use smoltcp::wire::IpEndpoint;



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

    // let taskref = match task::get_my_current_task() {
    //     Some(t) => t,
    //     None => {
    //         println!("failed to get current task");
    //         return -1;
    //     }
    // };
    // let curr_dir = {
    //     let locked_task = taskref.lock();
    //     let curr_env = locked_task.env.lock();
    //     Arc::clone(&curr_env.working_dir)
    // };

    let verbose = matches.opt_present("v");

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

    println!("MATCHES: {:?}", matches.free);

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

    println!("DOWNLOAD crate_list: {:?}", crate_list);

    let crates = if let Some(crate_list) = crate_list {
        let crate_set = crate_list.iter().cloned().collect::<BTreeSet<String>>();
        ota_update_client::download_crates(&iface, remote_endpoint, update_build, crate_set).map_err(|e| e.to_string())?
    } else {
        let diff_lines = ota_update_client::download_diff(&iface, remote_endpoint, update_build)
            .map_err(|e| format!("failed to download diff file for {}, error: {}", update_build, e))?;
        let diff_tuples = ota_update_client::parse_diff_file(&diff_lines).map_err(|e| e.to_string())?;

        println!("DIFF TUPLES:\n{:?}", diff_tuples);

        Vec::new() // TODO: download the actual crates
    };
    
    // TODO: save crates to file

    Ok(())
}


/// Returns the first network interface available in the system.
fn get_default_iface() -> Result<NetworkInterfaceRef, String> {
    NETWORK_INTERFACES.lock()
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| format!("no network interfaces available"))
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

    download UPDATE_BUILD [CRATES]
        Downloads all of different crates (from the diff file) for the given UPDATE_BUILD.
        If the list of CRATES is given, it downloads each one of those instead.";
