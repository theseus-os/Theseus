#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
extern crate owning_ref;
extern crate smoltcp;

use core::ops::{Deref, DerefMut};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::boxed::Box;
use owning_ref::BoxRefMut;

use smoltcp::{
    wire::{IpAddress, IpCidr},
    iface::{EthernetInterface, Neighbor, Route},
};


lazy_static! {
    /// Backing storage for the 
    static ref E1000_NETWORK_IFACE: OwnedInterfaceStorage = OwnedInterfaceStorage {
        neighbor_cache_storage: BTreeMap::new(),
        ip_addrs_storage: Vec::new(),
        routes_storage: BTreeMap::new(),
    };
}


/// A network interface object, of which there can be only at most one
/// for a single network interface card. 
/// This offers a way to use sockets through the network interface.
/// 
/// Under the hood, this is a wrapper for smoltcp's `Interface` type,
/// which itself alone is extremely difficult to deal with due to 
/// multiple complex lifetime parameters and constraints.
pub struct E1000NetworkInterface {
    // iface_ref: BoxRefMut<OwnedInterfaceStorage, EthernetInterface<'static, 'static, 'static, E1000Device<e1000::E1000Nic>>>,
}


/// The components that are owned 
struct OwnedInterfaceStorage {
    /// underlying owned storage for smoltcp's NeighborCache
    neighbor_cache_storage: BTreeMap<IpAddress, Neighbor>,
    /// underlying owned storage for smoltcp's NeighborCache
    ip_addrs_storage: Vec<IpCidr>,
    /// underlying owned storage for smoltcp's Routes
    routes_storage: BTreeMap<IpCidr, Route>,
}


/*

/// Returns the initialized e1000 network interface that can be used
/// to poll/flush sockets on that interface. 
/// 
/// If the interface has not yet been initialized, 
/// this function initializes it as a new one that uses the given `e1000_nic_ref`.
pub fn get_e1000_interface<N>(nic: &'static MutexIrqSafe<N>) 
    -> &'static Mutex<EthernetInterface<> 
    where N: NetworkInterfaceCard
{
    static E1000_IFACE: Once<Mutex<>> = Once::new();

    let neighbor_cache = NeighborCache::new(BTreeMap::new());
    
    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now
    let tcp_handle = sockets.add(tcp_socket);

    // Setup an interface/device that connects smoltcp to our e1000 driver
    let device = E1000Device::new(nic);
    let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
    // TODO FIXME: get the default gateway IP from DHCP
    let default_gateway = Ipv4Address::new(
        DEFAULT_LOCAL_GATEWAY_IP[0],
        DEFAULT_LOCAL_GATEWAY_IP[1],
        DEFAULT_LOCAL_GATEWAY_IP[2],
        DEFAULT_LOCAL_GATEWAY_IP[3],
    );
    let mut routes_storage = [None; 1];
    let mut routes = Routes::new(&mut routes_storage[..]);
    let _prev_default_gateway = routes.add_default_ipv4_route(default_gateway).map_err(|_e| {
        error!("ota_update_client: couldn't set default gateway IP address: {:?}", _e);
        "couldn't set default gateway IP address"
    })?;
    let mut iface = EthernetInterfaceBuilder::new(device)
        .ethernet_addr(hardware_mac_addr)
        .neighbor_cache(neighbor_cache)
        .ip_addrs(local_addr)
        .routes(routes)
        .finalize();

    E1000_IFACE.call_once(|| {
        // inside this closure, we should create a new interface based on the given 
    })
}

*/
