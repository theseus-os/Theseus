//! This crate implements an interface/glue layer between our e1000 driver
//! and the smoltcp network stack.
#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate smoltcp;
extern crate e1000;
extern crate network_interface_card;
extern crate irq_safety;
extern crate owning_ref;
extern crate network_manager;


use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::{Mutex, Once};
use irq_safety::MutexIrqSafe;
use smoltcp::{
    socket::SocketSet,
    time::Instant,
    phy::DeviceCapabilities,
    wire::{EthernetAddress, IpAddress, Ipv4Address, IpCidr},
    iface::{EthernetInterface, EthernetInterfaceBuilder, NeighborCache, Routes},
};
use network_interface_card::{NetworkInterfaceCard, TransmitBuffer, ReceivedFrame};
use e1000::E1000Nic;
use owning_ref::BoxRef;
use network_manager::NetworkInterface;

/// standard MTU for ethernet cards
const DEFAULT_MTU: usize = 1500;


// pub type E1000EthernetInterface<N: NetworkInterfaceCard + 'static> = EthernetInterface<'static, 'static, 'static, E1000Device<N>>;


pub struct E1000NetworkInterface<N: NetworkInterfaceCard + 'static> {
    pub iface: EthernetInterface<'static, 'static, 'static, E1000Device<N>>,
}

impl<N: NetworkInterfaceCard + 'static> NetworkInterface for E1000NetworkInterface<N> { 
    fn flush(&mut self, sockets: &mut SocketSet, timestamp: Instant) -> smoltcp::Result<bool> {
        self.iface.poll(sockets, timestamp)
    }
}



pub fn test_init() {
    let iface = create_interface(e1000::get_e1000_nic().unwrap(), None, None).unwrap();
    network_manager::NETWORK_INTERFACES.lock().push(Arc::new(Mutex::new(iface)));
}

/// Returns a reference to the e1000 network interface, which can be used for handling sockets. 
/// 
/// Arguments: 
/// * `nic`:  a reference to an initialized Ethernet NIC, which must implement the `NetworkInterfaceCard` trait.
/// * `static_ip`: the IP that this network interface should locally use. If `None`, one will be assigned via DHCP.
/// * `gateway_ip`: the IP of this network interface's local gateway (access point, router). If `None`, will be discovered via DHCP.
/// 
/// # Note
/// Currently, `static_ip` and `gateway_ip` are required because we don't yet support DHCP.
/// 
pub fn create_interface<N: NetworkInterfaceCard + 'static>(
    nic: &'static MutexIrqSafe<N>,
    static_ip: Option<IpAddress>,
    gateway_ip: Option<Ipv4Address>,
) -> Result<E1000NetworkInterface<N>, &'static str> 
{
    // here, we have to create the iface for the first time because it didn't yet exist
    let mut ip_addrs = Vec::new();
    ip_addrs.push(IpCidr::new(
        static_ip.ok_or("static_ip is currently required because we do not yet support DHCP")?,
        24
    ));

    let mut routes = Routes::new(BTreeMap::new());
    let _prev_default_gateway = routes.add_default_ipv4_route(
        gateway_ip.ok_or("gateway_ip is currently required because we do not yet support DHCP")?
    ).map_err(|_e| {
        error!("e1000_smoltcp_device(): couldn't set default gateway IP address: {:?}", _e);
        "couldn't set default gateway IP address"
    })?;

    let device = E1000Device::new(nic);
    let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
    let iface = EthernetInterfaceBuilder::new(device)
        .ethernet_addr(hardware_mac_addr)
        .neighbor_cache(NeighborCache::new(BTreeMap::new()))
        .ip_addrs(Vec::new())
        .routes(routes)
        .finalize();

    Ok(E1000NetworkInterface{iface})
}



/// An implementation of smoltcp's `Device` trait, which enables smoltcp
/// to use our existing e1000 ethernet driver.
/// An instance of this `E1000Device` can be used in smoltcp's `EthernetInterface`.
pub struct E1000Device<N: NetworkInterfaceCard + 'static> { 
    nic_ref: &'static MutexIrqSafe<N>,
}
impl<N: NetworkInterfaceCard + 'static> E1000Device<N> {
    /// Create a new instance of the `E1000Device`.
    pub fn new(nic_ref: &'static MutexIrqSafe<N>) -> E1000Device<N> {
        E1000Device {
            nic_ref: nic_ref,
        }
    }
}


/// To connect the e1000 driver to smoltcp, 
/// we implement transmit and receive callbacks
/// that allow smoltcp to interact with the e1000 NIC.
impl<'d, N: NetworkInterfaceCard + 'static> smoltcp::phy::Device<'d> for E1000Device<N> {

    /// The buffer type returned by the receive callback.
    type RxToken = RxToken;
    /// The buffer type returned by the transmit callback.x
    type TxToken = TxToken<N>;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = DEFAULT_MTU;
        caps
    }

    fn receive(&mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        // According to the smoltcp code, AFAICT, this function should poll the ethernet driver
        // to see if a new packet (Ethernet frame) has arrived, and if so, 
        // take ownership of it and return it inside of an RxToken.
        // Otherwise, if no new packets have arrived, return None.
        let received_frame = {
            let mut nic = self.nic_ref.lock();
            nic.poll_receive().map_err(|_e| {
                error!("E1000Device::receive(): error returned from poll_receive(): {}", _e);
                _e
            }).ok()?;
            nic.get_received_frame()?
        };

        // debug!("E1000Device::receive(): got E1000 frame, consists of {} ReceiveBuffers.", received_frame.0.len());
        // TODO FIXME: add support for handling a frame that consists of multiple ReceiveBuffers
        if received_frame.0.len() > 1 {
            error!("E1000Device::receive(): WARNING: E1000 frame consists of {} ReceiveBuffers, we currently only handle a single-buffer frame, so this may not work correctly!",  received_frame.0.len());
        }

        let first_buf_len = received_frame.0[0].length;
        let rxbuf_byte_slice = BoxRef::new(Box::new(received_frame))
            .try_map(|rxframe| rxframe.0[0].as_slice::<u8>(0, first_buf_len as usize))
            .map_err(|e| {
                error!("E1000Device::receive(): couldn't convert receive buffer of length {} into byte slice, error {:?}", first_buf_len, e);
                e
            })
            .ok()?;

        // Just create and return a pair of (receive token, transmit token), 
        // the actual rx buffer handling is done in the RxToken::consume() function
        Some((
            RxToken(rxbuf_byte_slice),
            TxToken {
                nic_ref: self.nic_ref,
            },
        ))
    }

    fn transmit(&mut self) -> Option<Self::TxToken> {
        // Just create and return a transmit token, 
        // the actual tx buffer creation is done in the TxToken::consume() function.
        // Also, we can't accurately create an actual transmit buffer here
        // because we don't yet know its required length.
        Some(TxToken {
            nic_ref: self.nic_ref,
        })
    }
}


/// The transmit token type used by smoltcp, which contains only a reference to the relevant NIC 
/// because the actual transmit buffer is allocated lazily only when it needs to be consumed.
pub struct TxToken<N: NetworkInterfaceCard + 'static> {
    nic_ref: &'static MutexIrqSafe<N>,
}
impl<N: NetworkInterfaceCard + 'static> smoltcp::phy::TxToken for TxToken<N> {
    fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> smoltcp::Result<R>
        where F: FnOnce(&mut [u8]) -> smoltcp::Result<R>
    {
        // According to the smoltcp documentation, this function must obtain a transmit buffer 
        // with the requested `length` (or one at least that big) and then return it. 
        // Because we can dynamically allocate transmit buffers, we just do that here.
        if len > (u16::max_value() as usize) {
            error!("E1000Device::transmit(): requested tx buffer size {} exceeds the max size of u16!", len);
            return Err(smoltcp::Error::Exhausted)
        }

        // debug!("E1000D/evice::transmit(): creating new TransmitBuffer of {} bytes, timestamp: {}", len, _timestamp);
        // create a new TransmitBuffer, cast it as a slice of bytes, call the passed `f` closure, and then send it!
        let mut txbuf = TransmitBuffer::new(len as u16).map_err(|e| {
            error!("E1000Device::transmit(): couldn't allocate TransmitBuffer of length {}, error {:?}", len, e);
            smoltcp::Error::Exhausted
        })?;

        let closure_retval = {
            let txbuf_byte_slice = txbuf.as_slice_mut::<u8>(0, len).map_err(|e| {
                error!("E1000Device::transmit(): couldn't convert TransmitBuffer of length {} into byte slice, error {:?}", len, e);
                smoltcp::Error::Exhausted
            })?;
            f(txbuf_byte_slice)?
        };
        self.nic_ref.lock()
            .send_packet(txbuf)
            .map_err(|e| {
                error!("E1000Device::transmit(): error sending Ethernet packet: {:?}", e);
                smoltcp::Error::Exhausted
            })?;
        
        Ok(closure_retval)
    }
}


/// The receive token type used by smoltcp, 
/// which contains only a `ReceivedFrame` to be consumed later.
pub struct RxToken(BoxRef<ReceivedFrame, [u8]>);

impl smoltcp::phy::RxToken for RxToken {
    fn consume<R, F>(self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
        where F: FnOnce(&[u8]) -> smoltcp::Result<R>
    {
        f(self.0.as_ref())
    }
}