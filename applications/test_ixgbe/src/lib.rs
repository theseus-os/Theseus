//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate print;
extern crate getopts;
extern crate ixgbe;
extern crate smoltcp;
extern crate ota_update_client;
extern crate network_manager;
extern crate rand;
#[macro_use] extern crate hpet;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::{Matches, Options};
use ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver;
use smoltcp::{
    wire::{IpAddress, Ipv4Address, IpEndpoint},
    socket::{SocketSet, SocketHandle, TcpSocket, TcpSocketBuffer, TcpState},
};
use ota_update_client::{STARTING_FREE_PORT, connect};
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use network_manager::{NetworkInterfaceRef, NETWORK_INTERFACES};
use hpet::get_hpet;
use core::str::FromStr;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    // test_nic_ixgbe_driver(None);  
    let mut opts = Options::new();
    opts.optopt ("d", "destination", "specify the IP address (and optionally, the port) of the update server", "IP_ADDR[:PORT]");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            // print_usage(opts);
            return -1; 
        }
    };

    let mut remote_endpoint = rmain(matches);
    if remote_endpoint.is_err() {
        println!("Couldn't create endpoint");
        return -1;
    }
    // // pass the port and ip address of applicatom
    // let remote_endpoint = IpEndpoint::new(
    //                             Ipv4Address::from_bytes(&DEFAULT_DESTINATION_IP_ADDR).into(), 
    //                             DEFAULT_DESTINATION_PORT
    //                         );

    let startup_time = get_hpet().as_ref().ok_or("coudln't get HPET timer").unwrap().get_counter();
    let mut rng = SmallRng::seed_from_u64(startup_time);
    let rng_upper_bound = u16::max_value() - STARTING_FREE_PORT;

    let iface = get_default_iface();
    if iface.is_err() {
        println!("No interface!");
        return -1;
    }

    let mut local_port = STARTING_FREE_PORT + (rng.next_u32() as u16 % rng_upper_bound);
    let mut tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let mut tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let mut tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
    let mut sockets = SocketSet::new(Vec::with_capacity(1));
    let mut tcp_handle = sockets.add(tcp_socket);

    let res = connect(&iface.unwrap(), &mut sockets, tcp_handle, remote_endpoint.unwrap(), local_port, startup_time);
    
    if res.is_err() {
        println!("Failed to connect to iface");
        return -1;
    }
    else {
        println!("socket connected successfully!");
    }

    let mut socket = sockets.get::<TcpSocket>(tcp_handle);
    if socket.can_send() {
        let data: [u8; 100] = [0; 100];
        let res = socket.send_slice(&data[..]).unwrap();

        println!("bytes sent {}", res);
    }

    0
}

fn rmain(matches: Matches) -> Result<IpEndpoint, String> {
    let mut remote_endpoint = if let Some(ip_str) = matches.opt_str("d") {
        IpEndpoint::from_str(&ip_str)
            .map_err(|_e| format!("couldn't parse destination IP address/port"))?
    } else {
        ota_update_client::default_remote_endpoint()
    };
    if remote_endpoint.port == 0 {
        remote_endpoint.port = ota_update_client::default_remote_endpoint().port;
    }

    Ok(remote_endpoint)
}

/// Returns the first network interface available in the system.
fn get_default_iface() -> Result<NetworkInterfaceRef, String> {
    NETWORK_INTERFACES.lock()
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| format!("no network interfaces available"))
}

// fn print_usage(opts: Options) {
//     println!("{}", opts.usage(USAGE));
// }
