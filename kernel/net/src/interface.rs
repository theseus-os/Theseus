use crate::DeviceWrapper;
use alloc::collections::BTreeMap;
use irq_safety::MutexIrqSafe;
use smoltcp::{iface, wire};

pub use smoltcp::iface::SocketSet;
pub use wire::{IpAddress, IpCidr};

pub struct Interface {
    inner: iface::Interface<'static>,
    pub(crate) device: DeviceWrapper,
}

impl Interface {
    pub(crate) fn new<T>(device: &'static MutexIrqSafe<T>, ip: IpCidr, gateway: IpAddress) -> Self
    where
        T: 'static + crate::Device,
    {
        let hardware_addr = wire::EthernetAddress(device.lock().mac_address()).into();

        let mut wrapper = DeviceWrapper::new(device);
        let mut routes = iface::Routes::new(BTreeMap::new());

        match gateway {
            IpAddress::Ipv4(addr) => routes.add_default_ipv4_route(addr),
            IpAddress::Ipv6(addr) => routes.add_default_ipv6_route(addr),
        }
        .expect("btree map route storage exhausted");

        let inner = iface::InterfaceBuilder::new()
            .random_seed(random::next_u64())
            .hardware_addr(hardware_addr)
            .ip_addrs([ip])
            .routes(routes)
            .neighbor_cache(iface::NeighborCache::new(BTreeMap::new()))
            .finalize(&mut wrapper);

        Self {
            inner,
            device: wrapper,
        }
    }

    pub fn poll(&mut self, sockets: &mut SocketSet) {
        // FIXME: Timestamp
        self.inner
            .poll(smoltcp::time::Instant::ZERO, &mut self.device, sockets)
            .unwrap();
    }
}
