use crate::{device::DeviceWrapper, NetworkDevice, Result, Socket};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
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
    sockets: MutexSleep<Vec<Arc<MutexSleep<Socket<dyn AnySocket<'static> + Send>>>>>,
}

impl NetworkInterface {
    pub(crate) fn new<T>(device: &'static MutexIrqSafe<T>, ip: IpCidr, gateway: IpAddress) -> Self
    where
        T: NetworkDevice,
    {
        let hardware_addr = wire::EthernetAddress(device.lock().mac_address()).into();

        let mut routes = iface::Routes::new(BTreeMap::new());

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
                .ip_addrs([ip])
                .routes(routes)
                .neighbor_cache(iface::NeighborCache::new(BTreeMap::new()))
                .finalize(&mut wrapper),
        );

        let sockets = MutexSleep::new(Vec::new());

        Self {
            inner,
            device,
            sockets,
        }
    }

    /// Adds a socket to the interface.
    pub fn add_socket<T>(&self, socket: T) -> Arc<MutexSleep<Socket<T>>>
    where
        T: AnySocket<'static> + Send,
    {
        let mut socket_set = SocketSet::new([iface::SocketStorage::default(); 1]);
        socket_set.add(socket);
        let socket_arc = Arc::new(MutexSleep::new(Socket::<T>::new(socket_set)));
        self.sockets
            .lock()
            .expect("failed to lock interface sockets")
            // SAFETY: Socket has a transparent representation, so the memory layout is identical
            // regardless of T. The Send bound on T ensures that transmuting to a type that
            // implements Send is sound.
            // NOTE: We are transmuting an arc, not the underlying type. That doesn't change the
            // safety requirements though.
            .push(unsafe { core::mem::transmute(socket_arc.clone()) });
        socket_arc
    }

    /// Polls the sockets associated with the interface.
    ///
    /// This should only be called by NIC drivers when new data is received.
    pub fn poll(&self) -> Result<()> {
        let mut inner = self.inner.lock().expect("failed to lock inner interface");
        let mut wrapper = DeviceWrapper {
            inner: &mut *self.device.lock(),
        };

        // NOTE: If some application decides to lock a socket for an extended period of
        // time, this will prevent all other sockets from being polled. Not sure if
        // there's a great way around this.
        for socket in self
            .sockets
            .lock()
            .expect("failed to lock interface sockets")
            .iter()
        {
            let mut socket = socket.lock().expect("failed to lock socket");
            inner.poll(
                smoltcp::time::Instant::ZERO,
                &mut wrapper,
                &mut socket.inner,
            )?;
        }

        Ok(())
    }

    // TODO: This is a temporary workaround while crate::Socket dereferences to a
    // smoltcp socket. In the future, the send method on crate::Socket will poll the
    // socket automatically.
    pub fn poll_socket<T>(&self, socket: &mut Socket<T>) -> Result<()>
    where
        T: AnySocket<'static>,
    {
        let mut inner = self.inner.lock().expect("failed to lock inner interface");
        let mut wrapper = DeviceWrapper {
            inner: &mut *self.device.lock(),
        };
        match inner.poll(
            smoltcp::time::Instant::ZERO,
            &mut wrapper,
            &mut socket.inner,
        ) {
            Ok(_) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        self.device.lock().capabilities()
    }
}
