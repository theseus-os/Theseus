//! This crate implements an interface/glue layer between our ethernet drivers
//! and the smoltcp network stack.
#![no_std]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate network_interface_card;
extern crate nic_buffers;
extern crate irq_safety;
extern crate network_manager;


use alloc::collections::BTreeMap;
use irq_safety::MutexIrqSafe;
use smoltcp::{
    socket::SocketSet,
    time::Instant,
    phy::DeviceCapabilities,
    wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address},
    iface::{EthernetInterface, EthernetInterfaceBuilder, NeighborCache, Routes},
};
use network_interface_card::NetworkInterfaceCard;
use nic_buffers::{TransmitBuffer, ReceivedFrame};
use network_manager::NetworkInterface;
use core::str::FromStr;

/// standard MTU for ethernet cards
const DEFAULT_MTU: usize = 1500;


/// A struct that implements the `NetworkInterface` trait for a NIC. 
/// There should be one instance of this struct per interface, i.e., an Ethernet port on the NIC.
pub struct EthernetNetworkInterface<N: NetworkInterfaceCard + 'static> {
    pub iface: EthernetInterface<'static, 'static, 'static, EthernetDevice<N>>,
}

impl<N: NetworkInterfaceCard + 'static> NetworkInterface for EthernetNetworkInterface<N> { 
    fn ethernet_addr(&self) -> EthernetAddress {
        self.iface.ethernet_addr()
    }

    fn set_ethernet_addr(&mut self, addr: EthernetAddress) {
        self.iface.set_ethernet_addr(addr)
    }

    fn poll(&mut self, sockets: &mut SocketSet, timestamp: Instant) -> smoltcp::Result<bool> {
        self.iface.poll(sockets, timestamp)
    }

    fn ip_addrs(&self) -> &[IpCidr] {
        self.iface.ip_addrs()
    }

    fn has_ip_addr(&self, addr: IpAddress) -> bool {
        self.iface.has_ip_addr(addr)
    }

    fn routes(&self) -> &Routes<'static> {
        self.iface.routes()
    }

    fn routes_mut(&mut self) -> &mut Routes<'static> {
        self.iface.routes_mut()
    }
}

impl<N: NetworkInterfaceCard + 'static > EthernetNetworkInterface<N> {
    /// Creates a new instance of an ethernet network interface, which can be used for handling sockets. 
    /// 
    /// Arguments: 
    /// * `nic`:  a reference to an initialized Ethernet NIC, which must implement the `NetworkInterfaceCard` trait.
    /// * `static_ip`: the IP that this network interface should locally use. If `None`, one will be assigned via DHCP.
    /// * `gateway_ip`: the IP of this network interface's local gateway (access point, router). If `None`, will be discovered via DHCP.
    /// 
    /// # Note
    /// Currently, `static_ip` and `gateway_ip` are required because we don't yet support DHCP.
    /// 
    pub fn new<G: Into<IpAddress>>(
        nic: &'static MutexIrqSafe<N>,
        static_ip: Option<IpCidr>,
        gateway_ip: Option<G>,
    ) -> Result<EthernetNetworkInterface<N>, &'static str> 
    {
        // here, we have to create the iface for the first time because it didn't yet exist
        let static_ip = static_ip.ok_or("static_ip is currently required because we do not yet support DHCP")?;
        let ip_addrs = vec![static_ip];

        let mut routes = Routes::new(BTreeMap::new());
        let res = match gateway_ip.ok_or("gateway_ip is currently required because we do not yet support DHCP")?.into() {
            IpAddress::Ipv4(ipv4) => routes.add_default_ipv4_route(ipv4),
            IpAddress::Ipv6(ipv6) => routes.add_default_ipv6_route(ipv6),
            _ => {
                return Err("gateway_ip must be an Ipv4Address or an Ipv6Address");
            }
        };
        res.map_err(|_e| {
            error!("ethernet_smoltcp_device(): couldn't set default gateway IP address: {:?}", _e);
            "couldn't set default gateway IP address"
        })?;

        let device = EthernetDevice::new(nic);
        let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
        // When creating an EthernetInterface, only the `ethernet_addr` and `neighbor_cache` are required.
        let iface = EthernetInterfaceBuilder::new(device)
            .ethernet_addr(hardware_mac_addr)
            .neighbor_cache(NeighborCache::new(BTreeMap::new()))
            .ip_addrs(ip_addrs)
            .routes(routes)
            .finalize();

        Ok(
            EthernetNetworkInterface { iface }
        )
    }

    /// Creates a new ethernet network interface with an ipv4 gateway address.
    /// The static and gateway ips are provided because DHCP is not enabled.
    /// 
    /// # Arguments
    /// * `nic_ref`: a reference to an initialized Ethernet NIC, which must implement the `NetworkInterfaceCard` trait.
    /// * `static_ip`: ip address to be assigned to this interface
    /// * `gateway_ip`: ipv4 gateway address for this interface
    pub fn new_ipv4_interface(
        nic_ref: &'static MutexIrqSafe<N>,
        static_ip: &str, 
        gateway_ip: &[u8]
    ) -> Result<EthernetNetworkInterface<N>, &'static str> 
    {
        let static_ip = IpCidr::from_str(static_ip).map_err(|_e| "couldn't parse static_ip_address")?;
        let gateway_ip = Ipv4Address::from_bytes(gateway_ip);

        Self::new(nic_ref, Some(static_ip), Some(gateway_ip))
    }
}


/// An implementation of smoltcp's `Device` trait, which enables smoltcp
/// to use our existing ethernet driver.
/// An instance of this `EthernetDevice` can be used in smoltcp's `EthernetInterface`.
pub struct EthernetDevice<N: NetworkInterfaceCard + 'static> { 
    nic_ref: &'static MutexIrqSafe<N>,
}
impl<N: NetworkInterfaceCard + 'static> EthernetDevice<N> {
    /// Create a new instance of the `EthernetDevice`.
    pub fn new(nic_ref: &'static MutexIrqSafe<N>) -> EthernetDevice<N> {
        EthernetDevice {
            nic_ref,
        }
    }
}


/// To connect the ethernet driver to smoltcp, 
/// we implement transmit and receive callbacks
/// that allow smoltcp to interact with the NIC.
impl<'d, N: NetworkInterfaceCard + 'static> smoltcp::phy::Device<'d> for EthernetDevice<N> {

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
                error!("EthernetDevice::receive(): error returned from poll_receive(): {}", _e);
                _e
            }).ok()?;
            nic.get_received_frame()?
        };

        // debug!("EthernetDevice::receive(): got Ethernet frame, consists of {} ReceiveBuffers.", received_frame.0.len());
        // TODO FIXME: add support for handling a frame that consists of multiple ReceiveBuffers
        if received_frame.0.len() > 1 {
            error!("EthernetDevice::receive(): WARNING: Ethernet frame consists of {} ReceiveBuffers, we currently only handle a single-buffer frame, so this may not work correctly!",  received_frame.0.len());
        }

        // Just create and return a pair of (receive token, transmit token), 
        // the actual rx buffer handling is done in the RxToken::consume() function
        Some((
            RxToken(received_frame),
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
            error!("EthernetDevice::transmit(): requested tx buffer size {} exceeds the max size of u16!", len);
            return Err(smoltcp::Error::Exhausted)
        }

        // debug!("EthernetDevice::transmit(): creating new TransmitBuffer of {} bytes, timestamp: {}", len, _timestamp);
        // create a new TransmitBuffer, cast it as a slice of bytes, call the passed `f` closure, and then send it!
        let mut txbuf = TransmitBuffer::new(len as u16).map_err(|e| {
            error!("EthernetDevice::transmit(): couldn't allocate TransmitBuffer of length {}, error {:?}", len, e);
            smoltcp::Error::Exhausted
        })?;

        let closure_retval = {
            f(&mut txbuf)?
        };
        self.nic_ref.lock()
            .send_packet(txbuf)
            .map_err(|e| {
                error!("EthernetDevice::transmit(): error sending Ethernet packet: {:?}", e);
                smoltcp::Error::Exhausted
            })?;
        
        Ok(closure_retval)
    }
}


/// The receive token type used by smoltcp, 
/// which contains only a `ReceivedFrame` to be consumed later.
pub struct RxToken(ReceivedFrame);

impl smoltcp::phy::RxToken for RxToken {
    fn consume<R, F>(mut self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
        where F: FnOnce(&mut [u8]) -> smoltcp::Result<R>
    {
        let first_buf = self.0.0.first_mut().ok_or_else(|| {
            error!("ReceivedFrame contained no receive buffers");
            smoltcp::Error::Exhausted
        })?;

        f(first_buf)
    }
}
