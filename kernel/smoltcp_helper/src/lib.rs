
//! Collection of functions to set up a TCP connection using a smoltcp device

#![no_std]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate network_manager;
extern crate spin;
extern crate hpet;

use alloc::string::ToString;
use core::convert::TryInto;
use spin::Once;
use hpet::get_hpet;
use smoltcp::{
    wire::IpEndpoint,
    socket::{SocketSet, TcpSocket, SocketHandle},
    time::Instant
};
use network_manager::{NetworkInterfaceRef, NETWORK_INTERFACES};

/// The starting number for freely-available (non-reserved) standard TCP/UDP ports.
pub const STARTING_FREE_PORT: u16 = 49152;

/// A simple macro to get the current HPET clock ticks.
#[macro_export]
macro_rules! hpet_ticks {
    () => {
        get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter()
    };
}

/// Function to calculate the currently elapsed time (in milliseconds) since the given `start_time` (hpet ticks).
pub fn millis_since(start_time: u64) -> Result<u64, &'static str> {
    const FEMTOSECONDS_PER_MILLISECOND: u64 = 1_000_000_000_000;
    static HPET_PERIOD_FEMTOSECONDS: Once<u32> = Once::new();

    let hpet_freq = match HPET_PERIOD_FEMTOSECONDS.get() {
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


/// Returns the first network interface available in the system.
pub fn get_default_iface() -> Result<NetworkInterfaceRef, &'static str> {
    NETWORK_INTERFACES.lock()
        .iter()
        .next()
        .cloned()
        .ok_or("no network interfaces available")
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
        return Err("smoltcp_helper: when connecting socket, it was already open...");
    }

    let timeout_millis = 3000; // 3 second timeout
    let start = hpet_ticks!();
    
    debug!("smoltcp_helper: connecting from {}:{} to {} ...",
        iface.lock().ip_addrs().get(0).map(|ip| format!("{ip}")).unwrap_or_else(|| "ERROR".to_string()),
        local_port, 
        remote_endpoint,
    );

    let _packet_io_occurred = poll_iface(iface, sockets, startup_time)?;

    sockets.get::<TcpSocket>(tcp_handle).connect(remote_endpoint, local_port).map_err(|_e| {
        error!("smoltcp_helper: failed to connect socket, error: {:?}", _e);
        "smoltcp_helper: failed to connect socket"
    })?;

    loop {
        let _packet_io_occurred = poll_iface(iface, sockets, startup_time)?;
        
        // if the socket actually connected, it should be able to send/recv
        let socket = sockets.get::<TcpSocket>(tcp_handle);
        if socket.may_send() && socket.may_recv() {
            break;
        }

        // check to make sure we haven't timed out
        if millis_since(start)? > timeout_millis {
            error!("smoltcp_helper: failed to connect to socket, timed out after {} ms", timeout_millis);
            return Err("smoltcp_helper: failed to connect to socket, timed out.");
        }
    }

    debug!("smoltcp_helper: connected!  (took {} ms)", millis_since(start)?);
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
            warn!("smoltcp_helper: poll error: {}", err);
            false
        }
    };
    Ok(packets_were_sent_or_received)
}
