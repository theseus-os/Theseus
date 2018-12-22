#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate spin;
extern crate owning_ref;
extern crate smoltcp;
extern crate e1000;
// extern crate e1000_smoltcp_device;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use alloc::boxed::Box;
use spin::Mutex;
use smoltcp::{
    socket::SocketSet,
    time::Instant,
    phy::Device,
    wire::{EthernetAddress, IpAddress, Ipv4Address, IpCidr},
    iface::{EthernetInterface, EthernetInterfaceBuilder, NeighborCache, Routes},
};
use e1000::E1000Nic;
// use e1000_smoltcp_device::E1000Device;


/// A trait that represents a Network Interface within Theseus. 
/// Currently this is a thin wrapper around what `smoltcp` offers. 
pub trait NetworkInterface { 
    /// Flushes the network interface, which should be called in a periodically repeating fashion,
    /// to send queued outgoing packets and check for pending received packets in the given `sockets`. 
    /// 
    /// # Arguments
    /// * `sockets`: the set of sockets whose input and output queues should be flushed.
    /// * `timestamp`: a value representing the current system time with a monotonically increasing clock.
    /// 
    /// This is essentially a thin wrapper around smoltcp's 
    /// [`poll()`](https://docs.rs/smoltcp/0.5.0/smoltcp/iface/struct.EthernetInterface.html#method.poll) method.
    fn flush(&mut self, sockets: &mut SocketSet, timestamp: Instant) -> smoltcp::Result<bool>;
}

lazy_static! {
    /// A list of all of the available and initialized network interfaces that exist on this system.
    pub static ref NETWORK_INTERFACES: Mutex<Vec<NetworkInterfaceRef>> = Mutex::new(Vec::new());
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary network interfaces to be shared in a thread-safe manner.
pub type NetworkInterfaceRef = Arc<Mutex<NetworkInterface + Send>>;
// pub type NetworkInterfaceRef<D: for<'d> Device<'d>> = Arc<Mutex<NetworkInterface<D>>>;
