use crate::{DeviceWrapper, Device};
use alloc::{collections::BTreeMap, sync::Arc};
use irq_safety::MutexIrqSafe;
use smoltcp::{iface, wire};

pub use smoltcp::iface::SocketSet;
pub use wire::{IpAddress, IpCidr};

#[derive(Clone)]
pub struct Interface {
    // FIXME: Can this be a regular mutex?
    inner: Arc<MutexIrqSafe<iface::Interface<'static>>>,
    pub(crate) device: &'static MutexIrqSafe<dyn crate::Device>,
}

impl Interface {
    pub(crate) fn new<T>(device: &'static MutexIrqSafe<T>, ip: IpCidr, gateway: IpAddress) -> Self
    where
        T: Device,
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
        let inner = Arc::new(MutexIrqSafe::new(
            iface::InterfaceBuilder::new()
                .random_seed(random::next_u64())
                .hardware_addr(hardware_addr)
                .ip_addrs([ip])
                .routes(routes)
                .neighbor_cache(iface::NeighborCache::new(BTreeMap::new()))
                .finalize(&mut wrapper),
        ));

        Self {
            inner,
            device,
        }
    }

    pub fn poll(&self, sockets: &mut SocketSet) {
        let mut wrapper = DeviceWrapper {
            inner: &mut *self.device.lock(),
        };
        // FIXME: Timestamp
        self.inner
            .lock()
            .poll(smoltcp::time::Instant::ZERO, &mut wrapper, sockets)
            .unwrap();
    }
}
