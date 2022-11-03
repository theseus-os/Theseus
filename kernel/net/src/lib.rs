#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use irq_safety::MutexIrqSafe;
use smoltcp::wire::Ipv4Address;
use spin::Mutex;

mod device;
mod error;
mod interface;

pub use device::{DeviceCapabilities, NetworkDevice};
pub use error::{Error, Result};
pub use interface::{IpAddress, IpCidr, NetworkInterface, SocketSet};
pub use smoltcp::{phy, socket, time::Instant, wire};

/// A randomly chosen IP address that must be outside of the DHCP range.
///
/// The default QEMU user-slirp network gives IP address of `10.0.2.*`.
const DEFAULT_LOCAL_IP: &str = "10.0.2.15/24";

/// Standard home router address.
///
/// `10.0.2.2` is the default QEMU user-slirp networking gateway IP.
const DEFAULT_GATEWAY_IP: IpAddress = IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 2));

// TODO: Make mutex rwlock?
// TODO: Use atomic append-only vec?
static NETWORK_INTERFACES: Mutex<Vec<NetworkInterface>> = Mutex::new(Vec::new());

/// Registers a network device.
///
/// The function will convert the device to an interface and it will then be
/// accessible using [`get_interface`].
pub fn register_device<T>(device: &'static MutexIrqSafe<T>)
where
    T: 'static + NetworkDevice + Send,
{
    let interface = NetworkInterface::new(
        device,
        // TODO: use DHCP to acquire an IP address and gateway.
        DEFAULT_LOCAL_IP.parse().unwrap(),
        DEFAULT_GATEWAY_IP,
    );

    NETWORK_INTERFACES.lock().push(interface);
}

/// Returns a list of available interfaces behind a mutex.
pub fn get_interfaces() -> &'static Mutex<Vec<NetworkInterface>> {
    &NETWORK_INTERFACES
}

/// Gets the first available interface.
pub fn get_default_interface() -> Option<NetworkInterface> {
    NETWORK_INTERFACES.lock().get(0).cloned()
}
