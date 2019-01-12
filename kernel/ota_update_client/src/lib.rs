//! Functions to communicate with a network server that provides over-the-air live update functionality.
//! 

#![no_std]
#![feature(alloc)]
#![feature(try_from)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate network_manager;
extern crate spin;
extern crate acpi;
extern crate owning_ref;
extern crate spawn;
extern crate task;
extern crate httparse;
extern crate sha3;
extern crate percent_encoding;
extern crate rand;
#[macro_use] extern crate http_client;


use core::convert::TryInto;
use core::str;
use alloc::{
    vec::Vec,
    string::{String, ToString},
};
use spin::Once;
use acpi::get_hpet;
use smoltcp::{
    wire::IpAddress,
    socket::{SocketSet, TcpSocket, TcpSocketBuffer},
    time::Instant
};
use sha3::{Digest, Sha3_512};
use percent_encoding::{DEFAULT_ENCODE_SET, utf8_percent_encode};
use network_manager::{NetworkInterfaceRef};
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use http_client::{ConnectedTcpSocket, send_request, millis_since, check_http_request, poll_iface};


/// The IP address of the update server.
/// This is currently the static IP of `kevin.recg.rice.edu`.
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [168, 7, 138, 84];

/// The TCP port on the update server that listens for update requests 
const DEFAULT_DESTINATION_PORT: u16 = 8090;

/// The starting number for freely-available (non-reserved) standard TCP/UDP ports.
const STARTING_FREE_PORT: u16 = 49152;

/// The time limit in milliseconds to wait for a response to an HTTP request.
const HTTP_REQUEST_TIMEOUT_MILLIS: u64 = 10000;


/// The path of the update builds file, located at the root of the build server.
/// This file contains the list of all update build instances available,
/// listed in reverse chronological order (most recent builds first).
const UPDATE_BUILDS_PATH: &'static str = "/updates.txt";

/// The name (and relative path) of the listing file inside each update build directory,
/// which contains the names of all crate object files that were built.  
const LISTING_FILE_NAME: &'static str = "listings.txt";



    // TODO: real update sequence: 
    // (1) establish connection to build server
    // (2) download the UPDATE MANIFEST, a file that describes which crates should replace which others
    // (3) compare the crates in the given CrateNamespace with the latest available crates from the manifest
    // (4) send HTTP requests to download the ones that differ 
    // (5) swap in the new crates in place of the old crates




/// Connects to the update server over the given network interface
/// and downloads the list of available update builds.
/// An update build is a compiled instance of Theseus that contains all crates' object files.
pub fn get_available_update_builds(
    iface: &NetworkInterfaceRef
) -> Result<Vec<String>, &'static str> {
    let updates_file = download_file(iface, UPDATE_BUILDS_PATH)?;
    let updates_str = str::from_utf8(&updates_file.bytes)
        .map_err(|_e| "couldn't convert received list of update builds into a UTF8 string")?;

    Ok(
        updates_str.lines().map(|line| line.to_string()).collect()
    )
}



/// A file that has been downloaded over the network, 
/// including its name and a byte array containing its contents.
pub struct DownloadedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}


/// Connects to the update server over the given network interface
/// and downloads the compiled crate object files from the specified `update_build`. 
/// A list of available update builds can be obtained by calling 
/// `get_available_update_builds()`.
/// 
/// # Arguments
/// * `iface`: a reference to an initialized network interface for sockets to use.
/// * `update_build`: the string name of the update build that the downloaded crates will belong to.
/// * `existing_crates`: a list of the existing crate names, i.e., the crates to *not* download.
///    Passing an an empty list of crates will cause all crates in the given `update_build` to be downloaded.
/// 
/// Returns the list of crates (as `DownloadedFile`s) in the given `update_build` that differed 
/// from the given list of `existing_crates`.
pub fn download_differing_crates(
    iface: &NetworkInterfaceRef, 
    update_build: &str,
    existing_crates: Vec<String>,
) -> Result<Vec<DownloadedFile>, &'static str> {
    
    // first, download the update_build's manifest (listing.txt) file
    debug!("ota_update_client: downloading the listing for update_build {:?}", update_build);
    let listing_file = download_file(iface, format!("{}/{}", update_build, LISTING_FILE_NAME))?;
    
    // Iterate over the list of crate files in the listing, and build a vector of file paths to download. 
    // We need to download all crates that differ from the given list of `existing_crates`, 
    // plus the files containing those crates' sha512 checksums.




    // calculate the sha3-512 hash of the HTTP response body (excluding headers)
    let mut hasher = Sha3_512::new();
    hasher.input(response.content());
    let result = hasher.result();
    info!("ota_update_client: sha3-512 hash of downloaded file: {:x}", result);


    Err("unfinished")
}



/// A convenience function for downloading just one file. See `download_files()`.
fn download_file<S: AsRef<str>>(
    iface: &NetworkInterfaceRef, 
    absolute_path: S,
) -> Result<DownloadedFile, &'static str> {
    
    download_files(iface, vec![absolute_path])?
        .into_iter()
        .next()
        .ok_or("no file received from the server")
}


/// Connects to the update server over the given network interface
/// and downloads the given files, each specified with its full absolute path on the update server.
fn download_files<S: AsRef<str>>(
    iface: &NetworkInterfaceRef, 
    absolute_paths: Vec<S>,
) -> Result<Vec<DownloadedFile>, &'static str> {

    let dest_addr = IpAddress::v4(
        DEFAULT_DESTINATION_IP_ADDR[0],
        DEFAULT_DESTINATION_IP_ADDR[1],
        DEFAULT_DESTINATION_IP_ADDR[2],
        DEFAULT_DESTINATION_IP_ADDR[3],
    );
    let dest_port = DEFAULT_DESTINATION_PORT;

    let startup_time = hpet_ticks!();

    let mut rng = SmallRng::seed_from_u64(startup_time);
    let rng_upper_bound = u16::max_value() - STARTING_FREE_PORT;
    let local_port = STARTING_FREE_PORT + (rng.next_u32() as u16 % rng_upper_bound);

    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now
    let tcp_handle = sockets.add(tcp_socket);

    {
        info!("ota_update_client: connecting from {}:{} to {}:{}",
            iface.lock().ip_addrs().get(0).map(|ip| format!("{}", ip)).unwrap_or_else(|| format!("ERROR")), 
            local_port, 
            dest_addr, 
            dest_port
        );
    }


    // first, attempt to connect the socket to the remote server
    let timeout_millis = 3000; // 3 second timeout
    let start = hpet_ticks!();
    
    if sockets.get::<TcpSocket>(tcp_handle).is_active() {
        warn!("ota_update_client: when connecting socket, it was already active...");
    } else {
        debug!("ota_update_client: connecting socket...");
        sockets.get::<TcpSocket>(tcp_handle).connect((dest_addr, dest_port), local_port).map_err(|_e| {
            error!("ota_update_client: failed to connect socket, error: {:?}", _e);
            "ota_update_client: failed to connect socket"
        })?;

        loop {
            let _packet_io_occurred = poll_iface(&iface, &mut sockets, startup_time)?;
            
            // if the socket actually connected, it should be able to send/recv
            let socket = sockets.get::<TcpSocket>(tcp_handle);
            if socket.may_send() && socket.may_recv() {
                break;
            }

            // check to make sure we haven't timed out
            if millis_since(start)? > timeout_millis {
                error!("ota_update_client: failed to connect to socket, timed out after {} ms", timeout_millis);
                return Err("ota_update_client: failed to connect to socket, timed out.");
            }
        }
    }

    debug!("ota_update_client: socket connected successfully!");

    let last_index = absolute_paths.len() - 1;
    for (i, path) in absolute_paths.iter().enumerate() {
        let is_last_request = i == last_index;

        let http_request = {
            let method = "GET";
            let uri = utf8_percent_encode("/a#hello.o", DEFAULT_ENCODE_SET);
            let version = "HTTP/1.1";
            let connection = if is_last_request { "Connection: close" } else { "Connection: keep-alive" };
            format!("{} {} {}\r\n{}\r\n{}\r\n\r\n",
                method,
                uri,
                version,
                format_args!("Host: {}:{}", dest_addr, dest_port), // host
                connection
            )
        };

        if !check_http_request(http_request.as_bytes()) {
            error!("ota_update_client: created improper/incomplete HTTP request: {:?}.", http_request);
            return Err("ota_update_client: created improper/incomplete HTTP request");
        }

        // send the HTTP request and obtain a response
        let response = {
            let mut connected_tcp_socket = ConnectedTcpSocket::new(&iface, &mut sockets, tcp_handle)?;
            send_request(http_request, &mut connected_tcp_socket, Some(HTTP_REQUEST_TIMEOUT_MILLIS))?
        };

    }


    let mut _loop_ctr = 0;
    let mut issued_close = false;
    loop {
        _loop_ctr += 1;

        let _packet_io_occurred = poll_iface(&iface, &mut sockets, startup_time)?;

        let mut socket = sockets.get::<TcpSocket>(tcp_handle);
        if !issued_close {
            debug!("ota_update_client: socket state is {:?}", socket.state());

            debug!("ota_update_client: closing socket...");
            socket.close();
            debug!("ota_update_client: socket state (after close) is now {:?}", socket.state());
            issued_close = true;
        }

        if _loop_ctr % 50000 == 0 {
            debug!("ota_update_client: socket state (looping) is now {:?}", socket.state());
        }

    }

    Ok(())
    
}
