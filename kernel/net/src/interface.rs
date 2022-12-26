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
    sockets: MutexIrqSafe<Vec<Arc<MutexIrqSafe<Socket<dyn AnySocket<'static> + Send>>>>>,
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

        let sockets = MutexIrqSafe::new(Vec::new());

        Self {
            inner,
            device,
            sockets,
        }
    }

    /// Adds a socket to the interface.
    pub fn add_socket<T>(&self, socket: T) -> Arc<MutexIrqSafe<T>>
    where
        T: AnySocket<'static> + Send,
    {
        let socket_arc = Arc::new(MutexIrqSafe::new(socket));
        self.sockets
            .lock()
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

        let sockets = self.sockets.lock();
        let mut socket_guards = Vec::with_capacity(sockets.len());
        let mut socket_set = SocketSet::new(Vec::with_capacity(sockets.len()));

        for socket in sockets.iter() {
            let mut guard = socket.lock();
            let socket = guard.inner.take().unwrap();
            let handle = match socket {
                smoltcp::socket::Socket::Raw(socket) => socket_set.add(socket),
                smoltcp::socket::Socket::Icmp(socket) => socket_set.add(socket),
                smoltcp::socket::Socket::Udp(socket) => socket_set.add(socket),
                smoltcp::socket::Socket::Tcp(socket) => socket_set.add(socket),
            };
            socket_guards.push((guard, handle));
        }

        // NOTE: If some application decides to lock a socket for an extended period of
        // time, this will prevent all other sockets from being polled. Not sure if
        // there's a great way around this.
        inner.poll(smoltcp::time::Instant::ZERO, &mut wrapper, &mut socket_set)?;

        for (mut guard, handle) in socket_guards {
            guard.inner = Some(socket_set.remove(handle));
        }

        Ok(())
    }

    pub fn capabilities(&self) -> DeviceCapabilities {
        self.device.lock().capabilities()
    }
}
