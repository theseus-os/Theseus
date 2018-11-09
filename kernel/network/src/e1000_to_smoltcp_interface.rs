
use alloc::slice;
use alloc::vec::Vec;
use alloc::vec_deque::VecDeque;
use smoltcp::Error;
use smoltcp::phy::{Device, DeviceLimits};
use e1000::{E1000_NIC, NetworkCard};
use alloc::rc::Rc;
use core::cell::RefCell;


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
    type RxBuffer = Vec<u8>;
    type TxBuffer = TxBuffer;

    fn limits(&self) -> DeviceLimits {
        DeviceLimits {
            max_transmission_unit: 1536, // TODO: why 1536?
            ..DeviceLimits::default()
        }
    }

    fn receive(&mut self, _timestamp: u64) -> Result<Self::RxBuffer, Error> {
        let nic = E1000_NIC.try().ok_or(Error::Exhausted)?.lock();
        if nic.has_packet_arrived() {
            debug!("E1000Device::receive() packet has arrived");
            let (mp, len) = nic.get_latest_received_packet();
            mp.as_slice::<u8>(0, len as usize)
                .map(|slice| slice.to_vec())
                .map_err(|e| {
                    error!("E1000Device::receive(): error converting MappedPages to slice: {:?}", e);
                    Error::Exhausted
                })
        } else {
            // debug!("E1000Device::receive() packet has NOT arrived");
            Err(Error::Exhausted)
        }
    }

    fn transmit(&mut self, _timestamp: u64, length: usize) -> Result<Self::TxBuffer, Error> {
        if tx_empty() {
            debug!("E1000Device::transmit() tx was empty");
            let index = self.tx_next;
            Ok(TxBuffer {
               buffer : vec![0;length],
            })
        } else {
            debug!("E1000Device::transmit() tx was full");
            Err(Error::Exhausted)
        }
    }
}

/// The transmit buffer type used by smoltcp, which must implement two things:
/// * it must be representable as a slice of bytes, e.g., it must impl AsRef<[u8]>.
/// * it must actually send the packet when it is dropped.
pub struct TxBuffer {
    queue:  Rc<RefCell<VecDeque<Vec<u8>>>>,
    buffer: Vec<u8>
}

impl AsRef<[u8]> for TxBuffer {
    fn as_ref(&self) -> &[u8] { self.buffer.as_ref() }
}

impl AsMut<[u8]> for TxBuffer {
    fn as_mut(&mut self) -> &mut [u8] { self.buffer.as_mut() }
}

impl Drop for TxBuffer {
    fn drop(&mut self) {
        if let Some(e1000_nic) = E1000_NIC.try() {
            let res = e1000_nic.lock().send_packet(
                self.buffer.as_ptr() as usize, 
                self.buffer.len() as u16
            );
            if let Err(e) = res {
                error!("e1000_smoltcp: error sending Ethernet packet: {:?}", e);
            }
        } else {
            error!("BUG: e1000_smoltcp: E1000 NIC wasn't yet initialized!");
        }
    }

}

