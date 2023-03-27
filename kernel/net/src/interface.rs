use crate::{device::DeviceWrapper, NetworkDevice, Result, Socket};
use alloc::vec::Vec;
use core::marker::PhantomData;
use irq_safety::MutexIrqSafe;
use mutex_sleep::MutexSleep;
use smoltcp::{iface, phy::DeviceCapabilities, socket::AnySocket, wire};

pub use smoltcp::iface::SocketSet;
pub use wire::{IpAddress, IpCidr};

/// A network interface.
///
/// This is a wrapper around a network device which provides higher level
/// abstractions such as polling sockets.
pub struct NetworkInterface {
    inner: MutexSleep<iface::Interface<'static>>,
    device: &'static MutexIrqSafe<dyn crate::NetworkDevice>,
    sockets: MutexSleep<SocketSet<'static>>,
}

impl NetworkInterface {
    pub(crate) fn new<T>(device: &'static MutexIrqSafe<T>, ip: IpCidr, gateway: IpAddress) -> Self
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
        let inner = MutexSleep::new(
            iface::InterfaceBuilder::new()
                .random_seed(random::next_u64())
                .hardware_addr(hardware_addr)
                .ip_addrs(heapless::Vec::<_, 5>::from_slice(&[ip]).unwrap())
                .routes(routes)
                .neighbor_cache(iface::NeighborCache::new())
                .finalize(&mut wrapper),
        );

        let sockets = MutexSleep::new(SocketSet::new(Vec::new()));

        Self {
            inner,
            device,
            sockets,
        }
    }

    /// Adds a socket to the interface.
    pub fn add_socket<T>(&self, socket: T) -> Socket<T>
    where
        T: AnySocket<'static>,
    {
        let handle = self
            .sockets
            .lock()
            .expect("failed to lock sockets")
            .add(socket);
        Socket {
            handle,
            sockets: &self.sockets,
            phantom_data: PhantomData,
        }
    }

    /// Polls the sockets associated with the interface.
    pub fn poll(&self) -> Result<()> {
        let mut inner = self.inner.lock().expect("failed to lock inner interface");
        let mut wrapper = DeviceWrapper {
            inner: &mut *self.device.lock(),
        };
        let mut sockets = self.sockets.lock().expect("failed to lock sockets");

        inner.poll(smoltcp::time::Instant::ZERO, &mut wrapper, &mut sockets)?;

        Ok(())
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        self.device.lock().capabilities()
    }
}
