//! This crate implements an interface/glue layer between our e1000 driver
//! and the smoltcp network stack.
#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate e1000;
extern crate network_interface_card;
extern crate irq_safety;
extern crate owning_ref;


use alloc::boxed::Box;
use irq_safety::MutexIrqSafe;
use smoltcp::Error;
use smoltcp::phy::DeviceLimits;
use network_interface_card::{NetworkInterfaceCard, ReceivedFrame, TransmitBuffer};
use owning_ref::{BoxRef, BoxRefMut};


/// An implementation of smoltcp's `Device` trait, which enables smoltcp
/// to use our existing e1000 ethernet driver.
/// An instance of this `E1000Device` can be used in smoltcp's `EthernetInterface`.
pub struct E1000Device<'n, N: NetworkInterfaceCard + 'static> { 
    nic_ref: &'n MutexIrqSafe<N>,
}
impl<'n, N: NetworkInterfaceCard> E1000Device<'n, N> {
    /// Create a new instance of the `E1000Device`.
    pub fn new(nic_ref: &'n MutexIrqSafe<N>) -> E1000Device<'n, N> {
        E1000Device {
            nic_ref: nic_ref,
        }
    }
}


/// To connect the e1000 driver to smoltcp, 
/// we implement transmit and receive callbacks
/// that allow smoltcp to interact with the e1000 NIC.
impl<'n, N: NetworkInterfaceCard + 'static> smoltcp::phy::Device for E1000Device<'n, N> {

    /// The buffer type returned by the receive callback.
    type RxBuffer = RxBuffer;
    /// The buffer type returned by the transmit callback.
    type TxBuffer = TxBuffer<'n, N>;

    fn limits(&self) -> DeviceLimits {
        let mut limits = DeviceLimits::default();
        limits.max_transmission_unit = 1536; // TODO: why 1536?
        limits
    }

    fn receive(&mut self, _timestamp: u64) -> Result<Self::RxBuffer, Error> {
        // According to the smoltcp documentation, this function must poll the ethernet driver
        // to see if a new packet (Ethernet frame) has arrived, and if so, take ownership of it
        // and return it. Otherwise, if no new packets have arrived, return Error::Exhausted. 
        // Then, once the `RxBuffer` type that we return here gets dropped, we should allow the
        // ethernet driver to re-take ownership of that buffer (e.g., return it to the pool of rx buffers).
        let received_frame = {
            let mut nic = self.nic_ref.lock();
            nic.poll_receive().map_err(|_e| Error::Exhausted)?;
            nic.get_received_frame().ok_or(Error::Exhausted)?
        };
        debug!("E1000Device::receive(): got E1000 frame, consists of {} ReceiveBuffers.", received_frame.0.len());
        // convert the received frame into a smoltcp-understandable RxBuffer type
        BoxRef::new(Box::new(received_frame))
            .try_map(|rxbuf| {
                // TODO FIXME don't just use the first received buffer, which assumes the received frame only consists of a single receive buffer
                let first_buf_len = rxbuf.0[0].length;
                rxbuf.0[0].as_slice::<u8>(0, first_buf_len as usize)
            })
            .map(|box_ref| RxBuffer(box_ref))
            .map_err(|_e| {
                error!("E1000Device::receive():");
                Error::Exhausted
            })
    }

    fn transmit(&mut self, _timestamp: u64, length: usize) -> Result<Self::TxBuffer, Error> {
        // According to the smoltcp documentation, this function must obtain a transmit buffer 
        // with the requested `length` (or one at least that big) and then return it. 
        // Because we can dynamically allocate transmit buffers, we just do that here.
        if length > (u16::max_value() as usize) {
            error!("E1000Device::transmit(): requested tx buffer size {} exceeds the max size of u16!", length);
            return Err(Error::Exhausted)
        }

        debug!("E1000Device::transmit(): creating new TransmitBuffer of {} bytes, timestamp: {}", length, _timestamp);
        // create a new TransmitBuffer, cast it as a slice of bytes, and wrap it in a TxBuffer
        TransmitBuffer::new(length as u16)
            .and_then(|transmit_buf| {
                BoxRefMut::new(Box::new(transmit_buf))
                    .try_map_mut(|mp| mp.as_slice_mut::<u8>(0, length))
            })
            .map(|transmit_buf_bytes| TxBuffer {
                nic_ref: self.nic_ref,
                buffer: transmit_buf_bytes
            })
            .map_err(|e| {
                error!("E1000Device::transmit(): couldn't allocate TransmitBuffer of length {}, error {:?}", length, e);
                Error::Exhausted
            })
    }
}


/// The transmit buffer type used by smoltcp, which must implement two things:
/// * it must be representable as a slice of bytes, e.g., it must impl AsRef<[u8]> and AsMut<[u8]>.
/// * it must actually send the packet when it is dropped.
/// Internally, we use `BoxRefMut<_, [u8]>` because it implements both AsRef<[u8]> and AsMut<[u8]>.
pub struct TxBuffer<'n, N: NetworkInterfaceCard + 'static> {
    nic_ref: &'n MutexIrqSafe<N>,
    buffer: BoxRefMut<TransmitBuffer, [u8]>,
}
impl<'n, N: NetworkInterfaceCard + 'static> AsRef<[u8]> for TxBuffer<'n, N> {
    fn as_ref(&self) -> &[u8] {
        self.buffer.as_ref()
    }
}
impl<'n, N: NetworkInterfaceCard + 'static> AsMut<[u8]> for TxBuffer<'n, N> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.buffer.as_mut()
    }
}
impl<'n, N: NetworkInterfaceCard + 'static> Drop for TxBuffer<'n, N> {
    fn drop(&mut self) {
        let res = self.nic_ref.lock().send_packet(self.buffer.owner());
        if let Err(e) = res {
            error!("e1000_smoltcp: error sending Ethernet packet: {:?}", e);
        }
    }

}


/// The receive buffer type used by smoltcp, which must be representable as a slice of bytes,
/// e.g., it must impl AsRef<[u8]>.
/// Under the hood, it's a simple wrapper around a received frame.
pub struct RxBuffer(BoxRef<ReceivedFrame, [u8]>);
impl AsRef<[u8]> for RxBuffer {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
