//! This crate implements an interface/glue layer between our e1000 driver
//! and the smoltcp network stack.
#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate smoltcp;
extern crate e1000;
extern crate network_interface_card;
extern crate irq_safety;


use irq_safety::MutexIrqSafe;
use smoltcp::Error;
use smoltcp::time::Instant;
use smoltcp::phy::DeviceCapabilities;
use network_interface_card::{NetworkInterfaceCard, TransmitBuffer};

/// standard MTU for ethernet cards
const DEFAULT_MTU: usize = 1500;


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
    type RxToken = RxToken<N>;
    /// The buffer type returned by the transmit callback.x
    type TxToken = TxToken<N>;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = DEFAULT_MTU;
        caps
    }

    fn receive(&mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        // Just create and return a pair of (receive token, transmit token), 
        // the actual rx buffer handling is done in the RxToken::consume() function
        Some((
            RxToken {
                nic_ref: self.nic_ref,
            },
            TxToken {
                nic_ref: self.nic_ref,
            },
        ))
    }

    fn transmit(&mut self) -> Option<Self::TxToken> {
        // Just create and return a transmit token, 
        // the actual tx buffer handling is done in the TxToken::consume() function
        Some(TxToken {
            nic_ref: self.nic_ref,
        })
    }
}


/// The transmit token type used by smoltcp, which contains only a reference to the relevant NIC 
/// because the actual transmit buffer is allocated dynamically.
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
            return Err(Error::Exhausted)
        }

        debug!("E1000Device::transmit(): creating new TransmitBuffer of {} bytes, timestamp: {}", len, _timestamp);
        // create a new TransmitBuffer, cast it as a slice of bytes, call the passed `f` closure, and then send it!
        let mut txbuf = TransmitBuffer::new(len as u16).map_err(|e| {
            error!("E1000Device::transmit(): couldn't allocate TransmitBuffer of length {}, error {:?}", len, e);
            Error::Exhausted
        })?;

        let closure_retval = {
            let txbuf_byte_slice = txbuf.as_slice_mut::<u8>(0, len).map_err(|e| {
                error!("E1000Device::transmit(): couldn't convert TransmitBuffer of length {} into byte slice, error {:?}", len, e);
                Error::Exhausted
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


/// The receive token type used by smoltcp, which contains only a reference to the relevant NIC 
/// because the actual receive buffer is retrieved dynamically from the NIC hardware.
pub struct RxToken<N: NetworkInterfaceCard + 'static> {
    nic_ref: &'static MutexIrqSafe<N>,
}
impl<N: NetworkInterfaceCard + 'static> smoltcp::phy::RxToken for RxToken<N> {
    fn consume<R, F>(self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
        where F: FnOnce(&[u8]) -> smoltcp::Result<R>
    {
        // According to the smoltcp documentation, this function must poll the ethernet driver
        // to see if a new packet (Ethernet frame) has arrived, and if so, take ownership of it
        // and return it. Otherwise, if no new packets have arrived, return Error::Exhausted. 
        let received_frame = {
            let mut nic = self.nic_ref.lock();
            nic.poll_receive().map_err(|_e| Error::Exhausted)?;
            nic.get_received_frame().ok_or(Error::Exhausted)?
        };

        debug!("E1000Device::receive(): got E1000 frame, consists of {} ReceiveBuffers at timestamp {}", received_frame.0.len(), _timestamp);
        if received_frame.0.len() > 1 {
            error!("E1000Device::receive(): WARNING: E1000 frame consists of {} ReceiveBuffers, we currently only handle a single-buffer frame, so this may not work correctly!",  received_frame.0.len());
        }

        let first_buf_len = received_frame.0[0].length;
        let rxbuf_byte_slice = received_frame.0[0].as_slice::<u8>(0, first_buf_len as usize).map_err(|e| {
            error!("E1000Device::receive(): couldn't convert receive buffer of length {} into byte slice, error {:?}", first_buf_len, e);
            Error::Exhausted
        })?;

        f(rxbuf_byte_slice)
    }
}