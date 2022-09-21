#![no_std]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};
use smoltcp::wire::Ipv4Address;
use spin::Mutex;

mod device;
mod error;
mod interface;

pub use device::*;
pub use error::{Error, Result};
pub use interface::*;

/// A randomly chosen IP address that must be outside of the DHCP range.
///
/// The default QEMU user-slirp network gives IP address of `10.0.2.*`.
const DEFAULT_LOCAL_IP: &str = "10.0.2.15/24";

/// Standard home router address.
///
/// `10.0.2.2` is the default QEMU user-slirp networking gateway IP.
const DEFAULT_GATEWAY_IP: IpAddress = IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 2));

// TODO: Non-spin mutex?
pub type InterfaceRef = Arc<Mutex<Interface>>;

// TODO: Make outer mutex rwlock?
static NETWORK_INTERFACES: Mutex<Vec<InterfaceRef>> = Mutex::new(Vec::new());

/// Registers a network device, returning the index.
///
/// The function will convert the device to an interface and it will then be
/// accessible using [`get_interface`].
pub fn register_device<T>(device: T) -> Result<usize>
where
    T: 'static + Device + Send,
{
    let interface = Arc::new(Mutex::new(Interface::new(
        device,
        // TODO: use DHCP to acquire an IP address and gateway.
        DEFAULT_LOCAL_IP.parse().unwrap(),
        DEFAULT_GATEWAY_IP,
    )));

    let mut interfaces = NETWORK_INTERFACES.lock();
    interfaces.push(interface);
    let index = interfaces.len() - 1;
    drop(interfaces);

    Ok(index)
}

/// Returns a list of available interfaces behind a mutex.
pub fn get_interfaces() -> &'static Mutex<Vec<InterfaceRef>> {
    &NETWORK_INTERFACES
}

/// Gets the interface with the specified `index`.
///
/// If `index` is `None` the first interface is returned.
pub fn get_interface(index: Option<usize>) -> Option<InterfaceRef> {
    let index = index.unwrap_or(0);
    let interfaces = NETWORK_INTERFACES.lock();
    let interface = interfaces.get(index).map(Arc::clone);
    drop(interfaces);
    interface
}
