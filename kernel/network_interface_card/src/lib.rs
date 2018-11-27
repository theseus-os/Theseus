#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;

use core::ops::{Deref, DerefMut};
use alloc::vec::Vec;
use memory::{create_contiguous_mapping, PhysicalAddress, EntryFlags, MappedPages};



pub struct NicDeviceRef<'n, N: NetworkInterfaceCard + 'static> {
    nic_ref: &'n N,
}



/// A trait that defines for a NIC 
pub trait NetworkInterfaceCard {
    /// Sends a packet contained in the given `transmit_buffer` out through this NetworkInterfaceCard. 
    /// Blocks until the packet has been successfully sent by the networking card hardware.
    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>;

    /// Returns the earliest `ReceivedFrame`, which is essentially a list of `ReceiveBuffer`s 
    /// that each contain an individual piece of the frame.
    fn get_received_frame(&mut self) -> Option<ReceivedFrame>;

    /// Poll the NIC for recieved frames. 
    /// Can be used as an alternative to interrupts, or as a supplement to interrupts.
    fn poll_receive(&mut self) -> Result<(), &'static str>;

    /// Returns the MAC address that this NIC is configured with.
    /// If spoofed, it will return the spoofed MAC address, 
    /// otherwise it will return the regular MAC address defined by the NIC hardware.
    fn mac_address(&self) -> [u8; 6];
}


/// A buffer that stores a packet to be transmitted through the NIC
/// and is guaranteed to be contiguous in physical memory. 
/// Auto-dereferences into a `MappedPages` object that represents its underlying memory. 
pub struct TransmitBuffer {
    pub mp: MappedPages,
    pub phys_addr: PhysicalAddress,
    pub length: u16,
}
impl TransmitBuffer {
    /// Creates a new TransmitBuffer with the specified size in bytes.
    /// The size is a `u16` because that is the maximum size of an NIC transmit buffer. 
    pub fn new(size_in_bytes: u16) -> Result<TransmitBuffer, &'static str> {
        let (mp, starting_phys_addr) = create_contiguous_mapping(
            size_in_bytes as usize,
            EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE,
        )?;
        Ok(TransmitBuffer {
            mp: mp,
            phys_addr: starting_phys_addr,
            length: size_in_bytes,
        })
    }

    /// Send this `TransmitBuffer` out through the given `NetworkInterfaceCard`. 
    /// This function consumes this `TransmitBuffer`.
    pub fn send<N: NetworkInterfaceCard>(self, nic: &mut N) -> Result<(), &'static str> {
        nic.send_packet(self)
    }
}
impl Deref for TransmitBuffer {
    type Target = MappedPages;
    fn deref(&self) -> &MappedPages {
        &self.mp
    }
}
impl DerefMut for TransmitBuffer {
    fn deref_mut(&mut self) -> &mut MappedPages {
        &mut self.mp
    }
}


/// A buffer that stores a packet (a piece of an Ethernet frame) that has been received from the NIC
/// and is guaranteed to be contiguous in physical memory. 
/// Auto-dereferences into a `MappedPages` object that represents its underlying memory. 
/// When dropped, its underlying memory is automatically returned to the NIC driver for future reuse.
pub struct ReceiveBuffer {
    pub mp: MappedPages,
    pub phys_addr: PhysicalAddress,
    pub length: u16,
}
impl Deref for ReceiveBuffer {
    type Target = MappedPages;
    fn deref(&self) -> &MappedPages {
        &self.mp
    }
}
impl DerefMut for ReceiveBuffer {
    fn deref_mut(&mut self) -> &mut MappedPages {
        &mut self.mp
    }
}
impl Drop for ReceiveBuffer {
    fn drop(&mut self) {
        warn!("ReceiveBuffer at paddr {:#X} length {} was dropped, buffer re-use is not yet implemented!", self.phys_addr, self.length);
        // TODO FIXME: return dropped buffers back to the pool
        // RX_BUFFER_POOL.push(self.mp)
    }
}


/// A network (e.g., Ethernet) frame that has been received by the NIC.
pub struct ReceivedFrame(pub Vec<ReceiveBuffer>);
