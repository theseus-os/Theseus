//! Functions to communicate with a network server that provides over-the-air live update functionality.
//! 

#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate network_manager;
extern crate hpet;
extern crate spawn;
extern crate task;
extern crate sha3;
extern crate percent_encoding;
extern crate rand;
extern crate http_client;
extern crate itertools;
#[macro_use] extern crate smoltcp_helper;


use core::str;
use alloc::{
    vec::Vec,
    collections::BTreeSet,
    string::{String, ToString},
};
use itertools::Itertools;
use hpet::get_hpet;
use smoltcp::{
    wire::{Ipv4Address, IpEndpoint},
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
use http_client::{HttpResponse, ConnectedTcpSocket, send_request, check_http_request};
use smoltcp_helper::{STARTING_FREE_PORT, connect, millis_since, poll_iface};

/// The IP address of the update server.
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [10, 0, 2, 2]; // the IP of the host machine when running on QEMU.

/// The TCP port on the update server that listens for update requests 
const DEFAULT_DESTINATION_PORT: u16 = 8090;

/// The default remote endpoint, server IP and port, of the update server.
pub fn default_remote_endpoint() -> IpEndpoint {
    IpEndpoint::new(
        Ipv4Address::from_bytes(&DEFAULT_DESTINATION_IP_ADDR).into(), 
        DEFAULT_DESTINATION_PORT
    )
}

/// The time limit in milliseconds to wait for a response to an HTTP request.
const HTTP_REQUEST_TIMEOUT_MILLIS: u64 = 10000;

/// The path of the update builds file, located at the root of the build server.
/// This file contains the list of all update build instances available,
/// listed in reverse chronological order (most recent builds first).
const UPDATE_BUILDS_PATH: &str = "/updates.txt";

/// The name (and relative path) of the listing file inside each update build directory,
/// which contains the names of all crate object files that were built.  
const LISTING_FILE_NAME: &str = "listing.txt";

/// The name (and relative path) of the diff file inside each update build directory,
/// which contains the mapping of old crates to new crates, indicating how a swap should take place. 
pub const DIFF_FILE_NAME: &str = "diff.txt";

/// The name of the directory containing the checksums of each crate object file
/// inside of a given update build.
const CHECKSUMS_DIR_NAME: &str = "checksums";

/// The file extension that is appended onto each crate object file's checksum file.
const CHECKSUM_FILE_EXTENSION: &str = ".sha512";



/// A file that has been downloaded over the network, 
/// including its name and a byte array containing its contents.
pub struct DownloadedFile {
    pub name: String,
    pub content: HttpResponse,
}


/// An enum used for specifying which crate files to download from an update build's listing.
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
pub fn download_available_update_builds(
    iface: &NetworkInterfaceRef,
    remote_endpoint: IpEndpoint,
) -> Result<Vec<String>, &'static str> {
    download_string_file(iface, remote_endpoint, UPDATE_BUILDS_PATH)
}


/// Connects to the update server over the given network interface
/// and downloads the list of crates present in the given update build.
pub fn download_listing(
    iface: &NetworkInterfaceRef,
    remote_endpoint: IpEndpoint,
    update_build: &str,
) -> Result<Vec<String>, &'static str> {
    download_string_file(iface, remote_endpoint, &format!("/{update_build}/{LISTING_FILE_NAME}"))
}


/// Connects to the update server over the given network interface
/// and downloads the diff file in the given update build,
/// which dictates which crates should be swapped.
pub fn download_diff(
    iface: &NetworkInterfaceRef,
    remote_endpoint: IpEndpoint,
    update_build: &str,
) -> Result<Vec<String>, &'static str> {
    download_string_file(iface, remote_endpoint, &format!("/{update_build}/{DIFF_FILE_NAME}"))
}


/// Convenience function for downloading files and returning their contents as Strings per line. 
fn download_string_file(
    iface: &NetworkInterfaceRef,
    remote_endpoint: IpEndpoint,
    file_path: &str,
) -> Result<Vec<String>, &'static str> {
    let file = download_file(iface, remote_endpoint, file_path)?;
    let content = file.content.as_result_err_str()?;
    as_lines(content)
}


/// Convenience function for converting a byte stream
/// that is delimited by newlines into a list of Strings.
pub fn as_lines(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
    str::from_utf8(bytes)
        .map_err(|_e| "couldn't convert file into a UTF8 string")
        .map(|files| files.lines().map(String::from).collect())
}


/// A representation of an diff file used to define an evolutionary crate swapping update.
pub struct Diff {
    /// A list of tuples in which the first element is the old crate and the second element is the new crate.
    /// If the first element is the empty string, the second element is a new addition (replacing nothing),
    /// and if the second element is the empty string, the first element is to be removed without replacement.
    pub pairs: Vec<(String, String)>,
    /// The list of state transfer functions which should be applied at the end of a crate swapping operation.
    pub state_transfer_functions: Vec<String>,
}


/// Parses a series of diff lines into a representation of an update diff.
/// 
/// # Arguments
/// * `diffs`: a list of lines, in which each line is a diff entry.
/// 
/// # Example diff entry formats
/// ```
/// a#ps.o -> a#ps.o
/// k#runqueue-d04869f0baadb1bd.o -> k#runqueue-d04869f0baadb1bd.o
/// - k#scheduler-7f6134ffbb934a27.o
/// + k#spawn-f1c87a8fc4e03893.o
/// ```
pub fn parse_diff_lines(diffs: &Vec<String>) -> Result<Diff, &'static str> {
    let mut pairs: Vec<(String, String)> = Vec::with_capacity(diffs.len());
    let mut state_transfer_functions: Vec<String> = Vec::new();
    for diff in diffs {
        // addition of new crate
        if diff.starts_with('+') {
            pairs.push((
                String::new(), 
                diff.get(1..).ok_or("error parsing (+) diff line")?.trim().to_string(),
            ));
        } 
        // removal of old crate
        else if diff.starts_with('-') {
            pairs.push((
                diff.get(1..).ok_or("error parsing (-) diff line")?.trim().to_string(), 
                String::new()
            ));
        }
        // replacement of old crate with new crate
        else if let Some((old, new)) = diff.split("->").collect_tuple() {
            pairs.push((old.trim().to_string(), new.trim().to_string()));
        }
        // state transfer function
        else if diff.starts_with('@') {
            state_transfer_functions.push(
                diff.get(1..).ok_or("error parsing (@state_transfer) diff line")?.trim().to_string(),
            )
        }
        else {
            error!("parse_diff_lines(): error parsing diff line: {:?}", diff);
            return Err("error parsing diff line");
        }
    }

    Ok(Diff { pairs, state_transfer_functions })
}



/// Connects to the update server over the given network interface
/// and downloads the object files for the specified `crates`.
/// 
/// It also downloads the checksum file for each crate object file 
/// in order to verify that each object file was completely downloaded correctly.
/// 
/// A list of available update builds can be obtained by calling `download_available_update_builds()`.
/// 
/// # Arguments
/// * `iface`: a reference to an initialized network interface for sockets to use.
/// * `update_build`: the string name of the update build that the downloaded crates will belong to.
/// * `crates`: a set of crate names, e.g., "k#my_crate-3d0cd20d4e1d4ba9.o",
///    that will be downloaded from the given `update_build` on the server. 
/// 
/// Returns the list of crate object files (as `DownloadedFile`s) in the given `update_build`.
pub fn download_crates(
    iface: &NetworkInterfaceRef, 
    remote_endpoint: IpEndpoint,
    update_build: &str,
    crates: BTreeSet<String>,
) -> Result<Vec<DownloadedFile>, &'static str> {

    let mut paths_to_download: Vec<String> = Vec::new();
    for file_name in crates.iter() {
        let path = format!("/{update_build}/{file_name}");
        let path_sha = format!("/{update_build}/{CHECKSUMS_DIR_NAME}/{file_name}{CHECKSUM_FILE_EXTENSION}");
        paths_to_download.push(path);
        paths_to_download.push(path_sha);
    }

    let mut crate_object_files = download_files(iface, remote_endpoint, paths_to_download)?;

    // iterate through each file downloaded, and verify the sha512 hashes
    for chunk in crate_object_files.chunks(2) {
        match chunk {
            [file, hash_file] => {
                let hash_file_str = hash_file.content.as_result_err_str()
                    .and_then(|content| str::from_utf8(content)
                        .map_err(|_e| "couldn't convert downloaded hash file into a UTF8 string")
                    )?;
                    
                if let Some(hash_value) = hash_file_str.split_whitespace().next() {
                    if verify_hash(file.content.as_result_err_str()?, hash_value) {
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

    // All hashes are verified, just return the crate object files themselves without the hash files
    crate_object_files.retain(|file| !file.name.ends_with(CHECKSUM_FILE_EXTENSION));

    Ok(crate_object_files)
}


/// A convenience function for downloading just one file. See `download_files()`.
fn download_file<S: AsRef<str>>(
    iface: &NetworkInterfaceRef,
    remote_endpoint: IpEndpoint, 
    absolute_path: S,
) -> Result<DownloadedFile, &'static str> {
    
    download_files(iface, remote_endpoint, vec![absolute_path])?
        .into_iter()
        .next()
        .ok_or("no file received from the server")
}


/// Connects to the update server over the given network interface
/// and downloads the given files, each specified with its full absolute path on the update server.
/// 
/// Returns an error if any of the given `absolute_paths` didn't exist, 
/// or if there was any other error on the remote server.
fn download_files<S: AsRef<str>>(
    iface: &NetworkInterfaceRef,
    remote_endpoint: IpEndpoint,
    absolute_paths: Vec<S>,
) -> Result<Vec<DownloadedFile>, &'static str> {
    if absolute_paths.is_empty() { 
        return Err("no download paths given");
    }

    let startup_time = hpet_ticks!();
    let mut rng = SmallRng::seed_from_u64(startup_time);
    let rng_upper_bound = u16::max_value() - STARTING_FREE_PORT;

    // the below items may be overwritten on each loop iteration, if the socket was closed and we need to create a new one
    let mut local_port = STARTING_FREE_PORT + (rng.next_u32() as u16 % rng_upper_bound);
    let mut tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let mut tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let mut tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
    let mut sockets = SocketSet::new(Vec::with_capacity(1));
    let mut tcp_handle = sockets.add(tcp_socket);

    // first, attempt to connect the socket to the remote server
    connect(iface, &mut sockets, tcp_handle, remote_endpoint, local_port, startup_time)?;
    debug!("ota_update_client: socket connected successfully!");

    // iterate over the provided list of file paths, and retrieve each one via HTTP
    let mut downloaded_files: Vec<DownloadedFile> = Vec::with_capacity(absolute_paths.len());
    let last_index = absolute_paths.len() - 1;
    for (i, path) in absolute_paths.iter().enumerate() {
        let path = path.as_ref();
        let is_last_request = i == last_index;

        let http_request = {
            let method = "GET";
            let uri = utf8_percent_encode(path, DEFAULT_ENCODE_SET);
            let version = "HTTP/1.1";
            let connection = if is_last_request { "Connection: close" } else { "Connection: keep-alive" };
            format!("{} {} {}\r\n{}\r\n{}\r\n\r\n",
                method,
                uri,
                version,
                format_args!("Host: {}:{}", remote_endpoint.addr, remote_endpoint.port), // host
                connection
            )
        };
        if !check_http_request(http_request.as_bytes()) {
            error!("ota_update_client: created improper/incomplete HTTP request: {:?}.", http_request);
            return Err("ota_update_client: created improper/incomplete HTTP request");
        }

        // Check that the socket is still connected. If not, we need to create a new one and connect it before use. 
        // if !is_connected(&mut sockets, tcp_handle) {
        // for now, we just always close the old socket and connect a new one
        {
            debug!("ota_update_client: remote endpoint closed socket after response, opening a new socket.");
            // first, close the existing socket
            {
                let mut socket = sockets.get::<TcpSocket>(tcp_handle);
                socket.close(); 
                socket.abort(); 
            }
            let _packet_io_occurred = poll_iface(iface, &mut sockets, startup_time)?;
            
            // second, create an entirely new socket and connect it
            local_port = STARTING_FREE_PORT + (rng.next_u32() as u16 % rng_upper_bound);
            tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
            tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
            tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
            sockets = SocketSet::new(Vec::with_capacity(1));
            tcp_handle = sockets.add(tcp_socket);
            connect(iface, &mut sockets, tcp_handle, remote_endpoint, local_port, startup_time)?;
        }

        // send the HTTP request and obtain a response
        let response = {
            let mut connected_tcp_socket = ConnectedTcpSocket::new(iface, &mut sockets, tcp_handle)?;
            send_request(http_request, &mut connected_tcp_socket, Some(HTTP_REQUEST_TIMEOUT_MILLIS))?
        };

        if response.status_code != 200 {
            error!("ota_update_client: failed to download {:?}, Error {}: {}", path, response.status_code, response.reason);
            break;
        }

        downloaded_files.push(DownloadedFile {
            name: path.into(),
            content: response,
        });
    }


    let mut _loop_ctr = 0;
    let mut issued_close = false;
    let timeout_millis = 3000; // 3 second timeout
    let start = hpet_ticks!();
    loop {
        _loop_ctr += 1;

        let _packet_io_occurred = poll_iface(iface, &mut sockets, startup_time)?;

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

    if downloaded_files.len() != absolute_paths.len() {
        return Err("failed to download all specified files");
    }

    Ok(downloaded_files)
}


// /// A convenience function that checks whether a socket is connected, 
// /// which is currently true when both `may_send()` and `may_recv()` are true.
// fn is_connected(sockets: &mut SocketSet, tcp_handle: SocketHandle) -> bool {
//     let socket = sockets.get::<TcpSocket>(tcp_handle);
//     socket.may_send() && socket.may_recv()
// }


/// Returns true if the SHA3 512-bit hash of the given `content` matches the given `hash` string.
/// The `hash` string must be 64 hexadecimal characters, otherwise `false` will be returned. 
fn verify_hash(content: &[u8], hash: &str) -> bool {
    let result = Sha3_512::digest(content);
    hash == format!("{result:x}")
}
