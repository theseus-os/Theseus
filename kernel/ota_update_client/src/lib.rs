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
extern crate acpi;
extern crate owning_ref;
extern crate spawn;
extern crate task;
extern crate httparse;
extern crate sha3;
extern crate percent_encoding;
extern crate rand;
#[macro_use] extern crate http_client;


use core::str;
use alloc::{
    vec::Vec,
    collections::BTreeSet,
    string::{String, ToString},
};
use acpi::get_hpet;
use smoltcp::{
    wire::IpAddress,
    socket::{SocketSet, TcpSocket, TcpSocketBuffer, TcpState},
};
use sha3::{Digest, Sha3_512};
use percent_encoding::{DEFAULT_ENCODE_SET, utf8_percent_encode};
use network_manager::{NetworkInterfaceRef};
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use http_client::{HttpResponse, ConnectedTcpSocket, send_request, millis_since, check_http_request, poll_iface};


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

/// The name of the directory containing the checksums of each crate object file
/// inside of a given update build.
const CHECKSUMS_DIR_NAME: &'static str = "checksums";

/// The file extension that is appended onto each crate object file's checksum file.
const CHECKSUM_FILE_EXTENSION: &'static str = ".sha512";


    // TODO: real update sequence: 
    // (1) establish connection to build server
    // (2) download the UPDATE MANIFEST, a file that describes which crates should replace which others
    // (3) compare the crates in the given CrateNamespace with the latest available crates from the manifest
    // (4) send HTTP requests to download the ones that differ 
    // (5) swap in the new crates in place of the old crates



/// A file that has been downloaded over the network, 
/// including its name and a byte array containing its contents.
pub struct DownloadedFile {
    pub name: String,
    pub content: HttpResponse,
}


/// An enum used for specifying which crate files to download.
/// To download all crates, pass an empty `Exclude` set.
pub enum CrateSet {
    /// The set of crates to include, i.e., only these crates will be downloaded.
    Include(BTreeSet<String>),
    /// The set of crates to exclude, i.e., all crates except for these will be downloaded.
    Exclude(BTreeSet<String>),
}
impl CrateSet {
    /// Returns true if this `CrateSet` specifies that the given `crate_file_name` is included.
    pub fn includes(&self, crate_file_name: &str) -> bool {
        match self {
            CrateSet::Include(crates_to_include) => crates_to_include.contains(crate_file_name),
            CrateSet::Exclude(crates_to_exclude) => !crates_to_exclude.contains(crate_file_name),
        }
    }
}


/// Connects to the update server over the given network interface
/// and downloads the list of available update builds.
/// An update build is a compiled instance of Theseus that contains all crates' object files.
pub fn get_available_update_builds(
    iface: &NetworkInterfaceRef
) -> Result<Vec<String>, &'static str> {
    let updates_file = download_file(iface, UPDATE_BUILDS_PATH)?;
    let updates_str = str::from_utf8(updates_file.content.content())
        .map_err(|_e| "couldn't convert received list of update builds into a UTF8 string")?;

    Ok(
        updates_str.lines().map(|line| line.to_string()).collect()
    )
}


/// Connects to the update server over the given network interface
/// and downloads a subset of the compiled crate object files from the specified `update_build`,
/// based on the provided `crate_set`.
/// A list of available update builds can be obtained by calling 
/// `get_available_update_builds()`.
/// 
/// # Arguments
/// * `iface`: a reference to an initialized network interface for sockets to use.
/// * `update_build`: the string name of the update build that the downloaded crates will belong to.
/// * `crate_set`: a set of crate name strings that are used as a filter to specify which crates are downloaded.
/// 
/// Returns the list of crates (as `DownloadedFile`s) in the given `update_build` that differed 
/// from the given list of `existing_crates`.
pub fn download_crates(
    iface: &NetworkInterfaceRef, 
    update_build: &str,
    crate_set: CrateSet,
) -> Result<Vec<DownloadedFile>, &'static str> {

    // first, download the update_build's manifest (listing.txt) file
    debug!("ota_update_client: downloading the listing for update_build {:?}", update_build);
    let listing_file = download_file(iface, format!("/{}/{}", update_build, LISTING_FILE_NAME))?;
    
    // Iterate over the list of crate files in the listing, and build a vector of file paths to download. 
    // We need to download all crates that differ from the given list of `existing_crates`, 
    // plus the files containing those crates' sha512 checksums.
    let files_list = str::from_utf8(listing_file.content.content())
        .map_err(|_e| "couldn't convert received update listing file into a UTF8 string")?;

    let mut paths_to_download: Vec<String> = Vec::new();
    for file in files_list.lines()
        .map(|line| line.trim())
        .filter(|&line| crate_set.includes(line))
    {
        let path = format!("/{}/{}", update_build, file);
        let path_sha = format!("/{}/{}/{}{}", update_build, CHECKSUMS_DIR_NAME, file, CHECKSUM_FILE_EXTENSION);
        paths_to_download.push(path);
        paths_to_download.push(path_sha);
    }

    let mut crate_object_files = download_files(iface, paths_to_download)?;

    // iterate through each file downloaded, and verify the sha512 hashes
    for chunk in crate_object_files.chunks(2) {
        match chunk {
            [file, hash_file] => {
                let hash_file_str = str::from_utf8(hash_file.content.content())
                    .map_err(|_e| "couldn't convert downloaded hash file into a UTF8 string")?;
                    
                if let Some(hash_value) = hash_file_str.split_whitespace().next() {
                    if verify_hash(file.content.content(), hash_value) {
                        // success! the hash matched, so the file was properly downloaded
                    } else {
                        error!("ota_update_client: downloaded file {:?} did not match the expected hash value! Try downloading it again.", file.name);
                        return Err("ota_update_client: downloaded file did not match the expected hash value");
                    }
                } else {
                    error!("ota_update_client: hash file {:?} had unexpected contents: it should start with a 64-digit hex hash value.", hash_file.name);
                    return Err("ota_update_client: hash file had unexpected contents");
                }
            }
            _ => {
                error!("ota_update_client: missing crate object file or hash file for downloaded file pair {:?}, cannot verify file integrity.", 
                    chunk.iter().next().map(|f| &f.name)
                );
                return Err("ota_update_client: missing crate object file or hash file for downloaded file pair");
            }
        }
    }

    // now that we've verified all of the hashes, we can just return the crate object files themselves without the hash files
    crate_object_files.retain(|file| !file.name.ends_with(CHECKSUM_FILE_EXTENSION));

    Ok(crate_object_files)
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

    let mut downloaded_files: Vec<DownloadedFile> = Vec::with_capacity(absolute_paths.len());
    let last_index = absolute_paths.len() - 1;
    for (i, path) in absolute_paths.iter().enumerate() {
        let is_last_request = i == last_index;

        let http_request = {
            let method = "GET";
            let uri = utf8_percent_encode(path.as_ref(), DEFAULT_ENCODE_SET);
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

        downloaded_files.push(DownloadedFile {
            name: path.as_ref().into(),
            content: response,
        });
    }


    let mut _loop_ctr = 0;
    let mut issued_close = false;
    let timeout_millis = 3000; // 3 second timeout
    let start = hpet_ticks!();
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

        if socket.state() == TcpState::Closed {
            break;
        }

        if _loop_ctr % 50000 == 0 {
            debug!("ota_update_client: socket state (looping) is now {:?}", socket.state());
        }

        // check to make sure we haven't timed out
        if millis_since(start)? > timeout_millis {
            debug!("ota_update_client: timed out waiting to close socket, closing it manually with an abort.");
            socket.abort();
            // Don't break out of the loop here!  we need to let this socket actually send the close msg
            // to the remote endpoint, which requires another call to poll_iface (which will happen at the top of the loop).
            // Then, the socket will be in the Closed state, and that conditional above will break out of the loop. 
        }
    }

    Ok(downloaded_files)
}


/// Returns true if the SHA3 512-bit hash of the given `content` matches the given `hash` string.
/// The `hash` string must be 64 hexadecimal characters, otherwise `false` will be returned. 
fn verify_hash(content: &[u8], hash: &str) -> bool {
    let result = Sha3_512::digest(content);
    hash == format!("{:x}", result)
}
