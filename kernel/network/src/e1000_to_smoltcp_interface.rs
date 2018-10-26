
use alloc::slice;
use alloc::vec::Vec;
use smoltcp::Error;
use smoltcp::phy::{Device, DeviceLimits};
use e1000::{nic_send_packet,nic_has_packet_arrived,nic_receive_packet};
//use e1000::E1000E_NIC;


/// platform-specific code to check if an incoming packet has arrived 
fn rx_full() -> bool {
    nic_has_packet_arrived()
}

///  platform-specific code to receive a packet into a buffer 
fn rx_setup() -> (*mut u8, usize) {
    nic_receive_packet()
}

/// platform-specific code to check if the outgoing packet was sent 
/// Always true as we are checking this in the server itself
fn tx_empty() -> bool {
    true
}

/// platform-specific code to send a buffer with a packet 
fn tx_setup(buf: *const u8, length: usize) {
    let addr: usize = buf as usize;
    nic_send_packet(addr, length);
}

pub struct EthernetDevice{
    pub tx_next: usize,
    pub rx_next: usize
}


/// Device trait for EthernetDEvice
/// Implementing transmit and receive
impl Device for EthernetDevice {
    type RxBuffer = &'static [u8];
    type TxBuffer = TxBuffer;

    fn limits(&self) -> DeviceLimits {
        let mut limits = DeviceLimits::default();
        limits.max_transmission_unit = 1536;
        limits.max_burst_size = Some(2);
        limits
    }

    fn receive(&mut self, _timestamp: u64) -> Result<Self::RxBuffer, Error> {
        if rx_full() {
            let (buf, length) = rx_setup();
            Ok(unsafe {
                slice::from_raw_parts(buf, length)
            })
        } else {
            Err(Error::Exhausted)
        }
    }

    fn transmit(&mut self, _timestamp: u64, length: usize) -> Result<Self::TxBuffer, Error> {
        if tx_empty() {
            let index = self.tx_next;
            Ok(TxBuffer {
               buffer : vec![0;length],
            })
        } else {
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

