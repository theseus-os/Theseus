use crate::{device::DeviceWrapper, NetworkDevice, Result, Socket};
use alloc::{sync::Arc, vec::Vec};
use core::marker::PhantomData;
use smoltcp::{iface, phy::DeviceCapabilities, socket::AnySocket, wire};
use sync_block::Mutex;
use sync_irq::IrqSafeMutex;

pub use smoltcp::iface::SocketSet;
pub use wire::{IpAddress, IpCidr};

/// A network interface.
///
/// This is a wrapper around a network device which provides higher level
/// abstractions such as polling sockets.
pub struct NetworkInterface {
    pub(crate) inner: Mutex<iface::Interface<'static>>,
    device: &'static IrqSafeMutex<dyn crate::NetworkDevice>,
    pub(crate) sockets: Mutex<SocketSet<'static>>,
}

impl NetworkInterface {
    pub(crate) fn new<T>(device: &'static IrqSafeMutex<T>, ip: IpCidr, gateway: IpAddress) -> Self
    where
        T: NetworkDevice,
    {
        let hardware_addr = wire::EthernetAddress(device.lock().mac_address()).into();

        let mut routes = iface::Routes::new();

        match gateway {
            IpAddress::Ipv4(addr) => routes.add_default_ipv4_route(addr),
            IpAddress::Ipv6(addr) => routes.add_default_ipv6_route(addr),
        }
        .expect("btree map route storage exhausted");

        let mut wrapper = DeviceWrapper {
            inner: &mut *device.lock(),
        };
        let inner = Mutex::new(
            iface::InterfaceBuilder::new()
                .random_seed(random::next_u64())
                .hardware_addr(hardware_addr)
                .ip_addrs(heapless::Vec::<_, 5>::from_slice(&[ip]).unwrap())
                .routes(routes)
                .neighbor_cache(iface::NeighborCache::new())
                .finalize(&mut wrapper),
        );

        let sockets = Mutex::new(SocketSet::new(Vec::new()));

        Self {
            inner,
            device,
            sockets,
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
    pub fn poll(&self) -> Result<()> {
        let mut inner = self.inner.lock();
        let mut wrapper = DeviceWrapper {
            inner: &mut *self.device.lock(),
        };
        let mut sockets = self.sockets.lock();

        inner.poll(smoltcp::time::Instant::ZERO, &mut wrapper, &mut sockets)?;

        Ok(())
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        self.device.lock().capabilities()
    }
}
