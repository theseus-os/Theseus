//! Functions for creating and sending HTTP requests and receiving responses.
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
extern crate hpet;
extern crate libm;
extern crate rand;

use core::convert::TryInto;
use core::str;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Once;
use hpet::get_hpet;
use smoltcp::{
    wire::{IpAddress, Ipv4Address, IpEndpoint},
    socket::{SocketSet, TcpSocket, SocketHandle, TcpSocketBuffer},
    time::Instant
};
use libm::F32Ext;
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};
use network_manager::{NetworkInterfaceRef, NETWORK_INTERFACES};


/// The IP address of the server.
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [11, 11, 11, 13]; // this can change because machine gets its ip address from a DHCP server

/// The TCP port on the server that listens for data
const DEFAULT_DESTINATION_PORT: u16 = 8090;

/// The starting number for freely-available (non-reserved) standard TCP/UDP ports.
const STARTING_FREE_PORT: u16 = 49152;

/// RX and TX buffer sizes
const BUFFER_SIZE: usize = 4096;


pub fn send_results(data: &[u8], len: u16) -> Result<(), &'static str> {

    // create ip endpoint for the listening server
    let remote_endpoint = IpEndpoint::new(
                                Ipv4Address::from_bytes(&DEFAULT_DESTINATION_IP_ADDR).into(), 
                                DEFAULT_DESTINATION_PORT
                            );
    

    // get interface for 82599 NIC
    let iface = get_default_iface()?; 

    // initialize the rng
    let startup_time = get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter();
    let mut rng = SmallRng::seed_from_u64(startup_time);
    let rng_upper_bound = u16::max_value() - STARTING_FREE_PORT;

    // assign a random port number
    let mut local_port = STARTING_FREE_PORT + (rng.next_u32() as u16 % rng_upper_bound);

    // create TCP socket
    let mut tcp_rx_buffer = TcpSocketBuffer::new(vec![0; BUFFER_SIZE]);
    let mut tcp_tx_buffer = TcpSocketBuffer::new(vec![0; BUFFER_SIZE]);
    let mut tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
    let mut sockets = SocketSet::new(Vec::with_capacity(1));
    let mut tcp_handle = sockets.add(tcp_socket);

    // connect to server, server should have called listen() function by now
    {
        let mut socket = sockets.get::<TcpSocket>(tcp_handle);
        let res = match socket.connect(remote_endpoint, local_port) {
            Ok(b) => b,
            Err(err) => {
                warn!("Failed to connect to socket");
                return Err("Failed to connect to socket");
            }
        };
    }

    // number of iterations needed to send all results
    let iter = (len as f32 / BUFFER_SIZE as f32).ceil() as usize;
    debug!("send message in {} iterations", iter);
    for i in 0..iter {
        let timestamp = timestamp(startup_time)?; 

        match iface.lock().poll(&mut sockets, Instant::from_millis(timestamp)) {
            Ok(_) => {},
            Err(e) => {
                debug!("poll error: {}", e);
                // return -1;
            }
        }

        {
            let mut socket = sockets.get::<TcpSocket>(tcp_handle);
            if socket.can_send() {
                let res = socket.send_slice(&data[(i*BUFFER_SIZE)..]).unwrap();

                debug!("bytes sent {}", res);
            }       
        }
    }

    // flush out TX buffer 
    let timestamp = timestamp(startup_time)?;
    match iface.lock().poll(&mut sockets, Instant::from_millis(timestamp)) {
        Ok(_) => {},
        Err(e) => {
            debug!("poll error: {}", e);
            // return -1;
        }
    }

    //close socket
    let mut socket = sockets.get::<TcpSocket>(tcp_handle);
    socket.close();
    
    Ok(())
}

/// Returns the first network interface available in the system.
/// TODO: should return the iface we want
fn get_default_iface() -> Result<NetworkInterfaceRef, &'static str> {
    NETWORK_INTERFACES.lock()
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| "no network interfaces available")
}

/// Function to calculate the currently elapsed time (in milliseconds) since the given `start_time` (hpet ticks).
pub fn millis_since(start_time: u64) -> Result<u64, &'static str> {
    const FEMTOSECONDS_PER_MILLISECOND: u64 = 1_000_000_000_000;
    static HPET_PERIOD_FEMTOSECONDS: Once<u32> = Once::new();

    let hpet_freq = match HPET_PERIOD_FEMTOSECONDS.try() {
        Some(period) => period,
        _ => {
            let freq = get_hpet().as_ref().ok_or("couldn't get HPET")?.counter_period_femtoseconds();
            HPET_PERIOD_FEMTOSECONDS.call_once(|| freq)
        }
    };
    let hpet_freq = *hpet_freq as u64;

    let end_time: u64 = get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter();
    // Convert to ms
    let diff = (end_time - start_time) * hpet_freq / FEMTOSECONDS_PER_MILLISECOND;
    Ok(diff)
}

fn timestamp(startup_time: u64) -> Result<i64, &'static str> {
    let timestamp: i64 = millis_since(startup_time)?
        .try_into()
        .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
    
    Ok(timestamp)
}