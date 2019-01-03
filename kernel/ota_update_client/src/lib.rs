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
use alloc::vec::Vec;
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




/// Initialize the network live update client, 
/// which spawns a new thread to handle live update requests and notifications. 
/// 
/// # Arguments
/// * `iface`: a reference to an initialized network interface for sockets to use. 
/// 
pub fn init(iface: NetworkInterfaceRef) -> Result<(), &'static str> {

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

    let http_request = {
        let method = "GET";
        let uri = utf8_percent_encode("/a#hello.o", DEFAULT_ENCODE_SET);
        let version = "HTTP/1.1";
        let connection = "Connection: close";
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

    // second, send the HTTP request and obtain a response
    let response = {
        let mut connected_tcp_socket = ConnectedTcpSocket::new(&iface, &mut sockets, tcp_handle)?;
        send_request(http_request, &mut connected_tcp_socket, Some(HTTP_REQUEST_TIMEOUT_MILLIS))?
    };

    // calculate the sha3-512 hash of the HTTP response body (excluding headers)
    let mut hasher = Sha3_512::new();
    hasher.input(response.content());
    let result = hasher.result();
    info!("ota_update_client: sha3-512 hash of downloaded file: {:x}", result);


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
