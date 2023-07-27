use alloc::{sync::Arc, vec::Vec};
use core::marker::PhantomData;

use smoltcp::{iface, phy::DeviceCapabilities, socket::AnySocket, wire};
pub use smoltcp::{
    iface::SocketSet,
    wire::{IpAddress, IpCidr},
};
use sync_block::Mutex;
use sync_irq::IrqSafeMutex;

use crate::{device::DeviceWrapper, NetworkDevice, Socket};

/// A network interface.
///
/// This is a wrapper around a network device which provides higher level
/// abstractions such as polling sockets.
pub struct NetworkInterface {
    pub(crate) inner: Mutex<iface::Interface>,
    device: &'static IrqSafeMutex<dyn crate::NetworkDevice>,
    pub(crate) sockets: Mutex<SocketSet<'static>>,
}

impl NetworkInterface {
    pub(crate) fn new<T>(device: &'static IrqSafeMutex<T>, ip: IpCidr, gateway: IpAddress) -> Self
    where
        T: NetworkDevice,
    {
        let hardware_addr = wire::EthernetAddress(device.lock().mac_address()).into();

        let mut wrapper = DeviceWrapper {
            inner: &mut *device.lock(),
        };

        let mut config = iface::Config::new(hardware_addr);
        config.random_seed = random::next_u64();

        let mut interface =
            iface::Interface::new(config, &mut wrapper, smoltcp::time::Instant::ZERO);
        interface.update_ip_addrs(|ip_addrs| {
            // NOTE: This won't fail as ip_addrs has a capacity of 2 (defined in smoltcp)
            // and this is the only address we are pushing.
            ip_addrs.push(ip).unwrap();
        });
        match gateway {
            IpAddress::Ipv4(addr) => interface.routes_mut().add_default_ipv4_route(addr),
            IpAddress::Ipv6(addr) => interface.routes_mut().add_default_ipv6_route(addr),
        }
        .expect("btree map route storage exhausted");

        Self {
            inner: Mutex::new(interface),
            device,
            sockets: Mutex::new(SocketSet::new(Vec::new())),
        }
    }

    /// Adds a socket to the interface.
    pub fn add_socket<T>(self: Arc<Self>, socket: T) -> Socket<T>
    where
        T: AnySocket<'static>,
    {
        let handle = self.sockets.lock().add(socket);
        Socket {
            handle,
            interface: self,
            phantom_data: PhantomData,
        }
    }

    /// Polls the sockets associated with the interface.
    ///
    /// Returns a boolean indicating whether the readiness of any socket may
    /// have changed.
    pub fn poll(&self) -> bool {
        let mut inner = self.inner.lock();
        let mut wrapper = DeviceWrapper {
            inner: &mut *self.device.lock(),
        };
        let mut sockets = self.sockets.lock();

        inner.poll(smoltcp::time::Instant::ZERO, &mut wrapper, &mut sockets)
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        self.device.lock().capabilities()
    }
}
