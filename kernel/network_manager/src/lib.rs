#![no_std]

extern crate alloc;
// #[macro_use] extern crate log;
extern crate spin;
extern crate owning_ref;
extern crate smoltcp;

use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;
use smoltcp::{
    socket::SocketSet,
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr},
    iface::Routes,
};



/// A list of all of the available and initialized network interfaces that exist on this system.
pub static NETWORK_INTERFACES: Mutex<Vec<NetworkInterfaceRef>> = Mutex::new(Vec::new());

/// A trait that represents a Network Interface within Theseus. 
/// Currently this is a thin wrapper around what `smoltcp` offers. 
pub trait NetworkInterface {
    /// Get the Ethernet address of the interface.
    fn ethernet_addr(&self) -> EthernetAddress;

    /// Set the Ethernet address of the interface.
    fn set_ethernet_addr(&mut self, addr: EthernetAddress);

    /// Polls (or flushes) the network interface, which should be called in a periodically repeating fashion,
    /// to send queued outgoing packets and check for pending received packets in the given `sockets`. 
    /// 
    /// # Arguments
    /// * `sockets`: the set of sockets whose input and output queues should be flushed.
    /// * `timestamp`: a value representing the current system time with a monotonically increasing clock.
    /// 
    /// This is essentially a thin wrapper around smoltcp's 
    /// [`poll()`](https://docs.rs/smoltcp/0.5.0/smoltcp/iface/struct.EthernetInterface.html#method.poll) method.
    fn poll(&mut self, sockets: &mut SocketSet, timestamp: Instant) -> smoltcp::Result<bool>;

    /// Get the IP addresses of the interface.
    fn ip_addrs(&self) -> &[IpCidr];

    /// Check whether the interface has the given IP address assigned.
    fn has_ip_addr(&self, addr: IpAddress) -> bool;

    fn routes(&self) -> &Routes<'static>;

    fn routes_mut(&mut self) -> &mut Routes<'static>;
}

/// A trait object wrapped in an Arc and Mutex that allows 
/// arbitrary network interfaces to be shared in a thread-safe manner.
pub type NetworkInterfaceRef = Arc<Mutex<dyn NetworkInterface + Send>>;

/// Add a Nic to the global list of network interfaces.
/// The Nic must implement the NetworkInterface trait.
pub fn add_to_network_interfaces<T: NetworkInterface + 'static + Send> (iface: T) {
    NETWORK_INTERFACES.lock().push(Arc::new(Mutex::new(iface)));
}
