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
extern crate spin;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::{Matches, Options};
use ixgbe::rx_poll_mq;
use ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver;
use smoltcp::{
    wire::{IpAddress, Ipv4Address, IpEndpoint},
    socket::{SocketSet, SocketHandle, TcpSocket, TcpSocketBuffer, TcpState},
    time::Instant,
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
use core::convert::TryInto;
use spin::Once;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    test_nic_ixgbe_driver(None);

    let _ = rx_poll_mq(None);

    0
}

fn rmain(matches: Matches) -> Result<IpEndpoint, String> {
    let mut remote_endpoint = if let Some(ip_str) = matches.opt_str("d") {
        IpEndpoint::from_str(&ip_str)
            .map_err(|_e| format!("couldn't parse destination IP address/port"))?
    } else {
        default_remote_endpoint()
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

fn timestamp(startup_time: u64) -> Result<i64, &'static str> {
    let timestamp: i64 = millis_since(startup_time)?
        .try_into()
        .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
    
    Ok(timestamp)
}

fn send(iface: &NetworkInterfaceRef, sockets: &mut SocketSet, startup_time: u64) -> Result<bool, &'static str>{
    let timestamp: i64 = millis_since(startup_time)?
        .try_into()
        .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;

    let packet_was_sent = match iface.lock().poll(sockets, Instant::from_millis(timestamp)) {
        Ok(b) => b,
        Err(err) => {
            println!("poll error: {}", err);
            false
        }
    };

    Ok(packet_was_sent)
}

/// Function to calculate the currently elapsed time (in milliseconds) since the given `start_time` (also milliseconds).
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

    let end_time: u64 = hpet_ticks!();
    // Convert to ms
    let diff = (end_time - start_time) * hpet_freq / FEMTOSECONDS_PER_MILLISECOND;
    Ok(diff)
}

/// The default remote endpoint, server IP and port, of the update server.
pub fn default_remote_endpoint() -> IpEndpoint {
    IpEndpoint::new(
        Ipv4Address::from_bytes(&DEFAULT_DESTINATION_IP_ADDR).into(), 
        DEFAULT_DESTINATION_PORT
    )
}