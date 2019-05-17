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
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [192, 168, 0, 102]; // this can change because machine gets its ip address from a DHCP server

/// The TCP port on the server that listens for data
const DEFAULT_DESTINATION_PORT: u16 = 8090;

/// The starting number for freely-available (non-reserved) standard TCP/UDP ports.
const STARTING_FREE_PORT: u16 = 49152;

/// RX and TX buffer sizes
const BUFFER_SIZE: usize = 1024;

macro_rules! hpet_ticks {
    () => {
        get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter()
    };
}

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

    // first, attempt to connect the socket to the remote server
    connect(&iface, &mut sockets, tcp_handle, remote_endpoint, local_port, startup_time)?;
    debug!("tcp_client: socket connected successfully!");

    // number of iterations needed to send all results
    let iter = (len as f32 / BUFFER_SIZE as f32).ceil() as usize;
    debug!("send message in {} iterations", iter);

    // let mut latest_packet_timestamp = startup_time;
    // let timeout_millis = 10000;

    // loop {
    //     _loop_ctr += 1;

    //     let _packet_io_occurred = poll_iface(&iface, sockets, startup_time)?;

    //     // check if we have timed out
    //     if let Some(t) = timeout_millis {
    //         if millis_since(latest_packet_timestamp)? > t {
    //             error!("http_client: timed out after {} ms, in state {:?}", t, state);
    //             return Err("http_client: timed out");
    //         }
    //     }

    //     let mut socket = sockets.get::<TcpSocket>(tcp_handle);

    //     if socket.can_send() {
    //         socket.send_slice(request.as_ref()).expect("http_client: cannot send request");
    //         latest_packet_timestamp = hpet_ticks!();
    //     }
    // }

    let mut latest_packet_timestamp = 0;
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
            if socket.is_active() {
                debug!("Socket is active!");
            }
            if socket.can_send() {
                let res = socket.send_slice(&data[(i*BUFFER_SIZE)..]).unwrap();
                latest_packet_timestamp = hpet_ticks!();
                debug!("bytes sent {}", res);
            }       
        }
    }

    // flush out TX buffer 

    loop {
        let timestamp = timestamp(startup_time)?;
        match iface.lock().poll(&mut sockets, Instant::from_millis(timestamp)) {
            Ok(_) => {},
            Err(e) => {
                debug!("poll error: {}", e);
                // return -1;
            }
        }

        if millis_since(latest_packet_timestamp)? > 5 {
            error!("tcp_client: timed out after 5 ms");
            return Err("tcp_client: timed out");
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

/// A convenience function for connecting a socket.
/// If the given socket is already open, it is forcibly closed immediately and reconnected.
pub fn connect(
    iface: &NetworkInterfaceRef,
    sockets: &mut SocketSet, 
    tcp_handle: SocketHandle,
    remote_endpoint: IpEndpoint,
    local_port: u16, 
    startup_time: u64,
) -> Result<(), &'static str> {
    if sockets.get::<TcpSocket>(tcp_handle).is_open() {
        return Err("tcp_client: when connecting socket, it was already open...");
    }

    let timeout_millis = 3000; // 3 second timeout
    let start = hpet_ticks!();
    
    debug!("tcp_client: connecting from {}:{} to {} ...",
        iface.lock().ip_addrs().get(0).map(|ip| format!("{}", ip)).unwrap_or_else(|| format!("ERROR")), 
        local_port, 
        remote_endpoint,
    );

    let _packet_io_occurred = poll_iface(&iface, sockets, startup_time)?;

    sockets.get::<TcpSocket>(tcp_handle).connect(remote_endpoint, local_port).map_err(|_e| {
        error!("tcp_client: failed to connect socket, error: {:?}", _e);
        "tcp_client: failed to connect socket"
    })?;

    loop {
        let _packet_io_occurred = poll_iface(&iface, sockets, startup_time)?;
        
        // if the socket actually connected, it should be able to send/recv
        let socket = sockets.get::<TcpSocket>(tcp_handle);
        if socket.may_send() && socket.may_recv() {
            break;
        }

        // check to make sure we haven't timed out
        if millis_since(start)? > timeout_millis {
            error!("tcp_client: failed to connect to socket, timed out after {} ms", timeout_millis);
            return Err("tcp_client: failed to connect to socket, timed out.");
        }
    }

    debug!("tcp_client: connected!  (took {} ms)", millis_since(start)?);
    Ok(())
}

/// A convenience function to poll the given network interface (i.e., flush tx/rx).
/// Returns true if any packets were sent or received through that interface on the given `sockets`.
pub fn poll_iface(iface: &NetworkInterfaceRef, sockets: &mut SocketSet, startup_time: u64) -> Result<bool, &'static str> {
    let timestamp: i64 = millis_since(startup_time)?
        .try_into()
        .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
    // debug!("calling iface.poll() with timestamp: {:?}", timestamp);
    let packets_were_sent_or_received = match iface.lock().poll(sockets, Instant::from_millis(timestamp)) {
        Ok(b) => b,
        Err(err) => {
            warn!("tcp_client: poll error: {}", err);
            false
        }
    };
    Ok(packets_were_sent_or_received)
}