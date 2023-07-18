#![no_std]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};
use smoltcp::wire::Ipv4Address;
use spin::Mutex;
use sync_irq::IrqSafeMutex;

mod device;
mod error;
mod interface;
mod socket;

pub use device::{DeviceCapabilities, NetworkDevice};
pub use error::{Error, Result};
pub use interface::{IpAddress, IpCidr, NetworkInterface, SocketSet};
pub use smoltcp::{
    phy,
    socket::{icmp, tcp, udp},
    time::Instant,
    wire::{self, IpEndpoint},
};
pub use socket::{LockedSocket, Socket};

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
static NETWORK_INTERFACES: Mutex<Vec<Arc<NetworkInterface>>> = Mutex::new(Vec::new());

/// Registers a network device.
///
/// The function will convert the device to an interface and it will then be
/// accessible using [`get_interfaces()`].
pub fn register_device<T>(device: &'static IrqSafeMutex<T>) -> Arc<NetworkInterface>
where
    T: 'static + NetworkDevice + Send,
{
    let interface = NetworkInterface::new(
        device,
        // TODO: use DHCP to acquire an IP address and gateway.
        DEFAULT_LOCAL_IP.parse().unwrap(),
        DEFAULT_GATEWAY_IP,
    );

    let interface_arc = Arc::new(interface);
    NETWORK_INTERFACES.lock().push(interface_arc.clone());
    interface_arc
}

/// Returns a list of available interfaces behind a mutex.
pub fn get_interfaces() -> &'static Mutex<Vec<Arc<NetworkInterface>>> {
    &NETWORK_INTERFACES
}

/// Returns the first available interface.
pub fn get_default_interface() -> Option<Arc<NetworkInterface>> {
    NETWORK_INTERFACES.lock().get(0).cloned()
}

/// Returns a port in the range reserved for private, dynamic, and ephemeral
/// ports.
pub fn get_ephemeral_port() -> u16 {
    use rand::Rng;

    const RANGE_START: u16 = 49152;
    const RANGE_END: u16 = u16::MAX;

    // TODO: Doesn't need to be random (especially cryptographically random) but
    // reduces the likelihood of port clashes. Can remove randmoness after
    // implementing some kind of tracking of used ports.

    let mut rng = random::init_rng::<rand_chacha::ChaChaRng>().unwrap();
    rng.gen_range(RANGE_START..=RANGE_END)
}
