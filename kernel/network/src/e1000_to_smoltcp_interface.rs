
use alloc::slice;
use alloc::vec::Vec;
use smoltcp::Error;
use smoltcp::phy::{Device, DeviceLimits};
use e1000::{E1000_NIC, NetworkCard};


/// platform-specific code to check if the outgoing packet was sent 
/// Always true as we are checking this in the server itself
fn tx_empty() -> bool {
    true
    // E1000_NIC.lock().has_packet_been_sent()
}

/// platform-specific code to send a buffer with a packet 
fn tx_setup(buf: *const u8, length: usize) {
    let addr: usize = buf as usize;
    E1000_NIC.lock().send_packet(addr, length as u16).unwrap();
}

pub struct EthernetDevice{
    pub tx_next: usize,
    pub rx_next: usize
}


/// Device trait for EthernetDEvice
/// Implementing transmit and receive
impl Device for EthernetDevice {
    type RxBuffer = Vec<u8>;
    type TxBuffer = TxBuffer;

    fn limits(&self) -> DeviceLimits {
        let mut limits = DeviceLimits::default();
        limits.max_transmission_unit = 1536;
        limits.max_burst_size = Some(2);
        limits
    }

    fn receive(&mut self, _timestamp: u64) -> Result<Self::RxBuffer, Error> {
        let nic = E1000_NIC.lock();
        if nic.has_packet_arrived() {
            debug!("EthernetDevice::receive() packet has arrived");
            let (mp, len) = nic.get_latest_received_packet();
            mp.as_slice::<u8>(0, len as usize)
                .map(|slice| slice.to_vec())
                .map_err(|e| {
                error!("EthernetDevice::receive(): error converting MappedPages to slice: {:?}", e);
                Error::Exhausted
            })
        } else {
            // debug!("EthernetDevice::receive() packet has NOT arrived");
            Err(Error::Exhausted)
        }
    }

    fn transmit(&mut self, _timestamp: u64, length: usize) -> Result<Self::TxBuffer, Error> {
        if tx_empty() {
            debug!("EthernetDevice::transmit() tx was empty");
            let index = self.tx_next;
            Ok(TxBuffer {
               buffer : vec![0;length],
            })
        } else {
            debug!("EthernetDevice::transmit() tx was full");
            Err(Error::Exhausted)
        }
    }
}

pub struct TxBuffer {
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
        tx_setup(self.buffer.as_ptr(), self.buffer.len()) 
    }

}

