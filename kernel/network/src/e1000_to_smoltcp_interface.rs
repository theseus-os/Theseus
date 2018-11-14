
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::vec_deque::VecDeque;
use smoltcp::Error;
use smoltcp::phy::{Device, DeviceLimits};
use e1000::{E1000_NIC, NetworkInterfaceCard, ReceiveBuffer, ReceivedFrame, TransmitBuffer};
use alloc::rc::Rc;
use core::cell::RefCell;
use owning_ref::{BoxRef, BoxRefMut};


/// An implementation of smoltcp's `Device` trait, which enables smoltcp
/// to use our existing e1000 ethernet driver.
/// An instance of this `E1000Device` can be used in smoltcp's `EthernetInterface`.
pub struct E1000Device {
    tx_queue: Rc<RefCell<VecDeque<Vec<u8>>>>,
}
impl E1000Device {
    /// Create a new instance of the `E1000Device` with an empty transmit buffer.
    pub fn new() -> E1000Device {
        E1000Device {
            tx_queue: Rc::new(RefCell::new(VecDeque::new()))
        }
    }
}


/// Device trait for E1000Device
/// Implementing transmit and receive
impl Device for E1000Device {
    type RxBuffer = RxBuffer;
    type TxBuffer = TxBuffer;

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
        let mut nic = E1000_NIC.try().ok_or_else(|| {
            error!("E1000Device::receive(): E1000 NIC wasn't yet initialized.");
            Error::Exhausted
        })?.lock();
        let received_frame = nic.poll_rx().map_err(|_e| Error::Exhausted)?;
        debug!("E1000Device::receive(): got E1000 frame, consists of {} ReceiveBuffers.", received_frame.0.len());
        // convert the received frame into a smoltcp-understandable RxBuffer type
        BoxRef::new(Box::new(received_frame))
            .try_map(|rxbuf| {
                // TODO FIXME don't just use the first received buffer, which assumes the received frame only consists of a single receive buffer
                let first_buf_len = rxbuf.0[0].len();
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
            .map(|transmit_buf_bytes| TxBuffer(transmit_buf_bytes))
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
pub struct TxBuffer(BoxRefMut<TransmitBuffer, [u8]>);
impl AsRef<[u8]> for TxBuffer {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl AsMut<[u8]> for TxBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}
impl Drop for TxBuffer {
    fn drop(&mut self) {
        if let Some(e1000_nic) = E1000_NIC.try() {
            let res = e1000_nic.lock().send_packet(self.0.owner());
            if let Err(e) = res {
                error!("e1000_smoltcp: error sending Ethernet packet: {:?}", e);
            }
        } else {
            error!("BUG: e1000_smoltcp: E1000 NIC wasn't yet initialized!");
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
