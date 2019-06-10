#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mpmc;
extern crate volatile;


use core::ops::{Deref, DerefMut};
use alloc::vec::Vec;
use memory::{create_contiguous_mapping, PhysicalAddress, EntryFlags, MappedPages};
use volatile::Volatile;


/// A trait that defines the necessary minimum functions that all network interface card (NIC) drivers
/// should implement. 
pub trait NetworkInterfaceCard {
    /// Sends a packet contained in the given `transmit_buffer` out through this NetworkInterfaceCard. 
    /// Blocks until the packet has been successfully sent by the networking card hardware.
    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>;

    /// Helper function for sending a packet, updates the current tx descriptor number and the tdt register.
    /// 
    /// # Arguments:
    /// - `tx_cur`: the value which stores the next free descriptor to be used
    /// - `max_tx_desc`: the number of tx descriptors in the queue
    /// - `tdt': transmit descriptor tail register
    fn update_tdt(tx_cur: &mut u16, max_tx_desc: u16, tdt: &mut Volatile<u32>) {
        // update the tx_cur value to hold the next free descriptor
        *tx_cur = (*tx_cur + 1) % max_tx_desc;
        // update the tdt register by 1 so that it know the previous descriptor has been used
        // and has a packet to be sent
        tdt.write(*tx_cur as u32);
    }

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
    pool: &'static mpmc::Queue<ReceiveBuffer>,
}
impl ReceiveBuffer {
    /// Creates a new ReceiveBuffer with the given `MappedPages`, `PhysicalAddress`, and `length`. 
    /// When this ReceiveBuffer object is dropped, it will be returned to the given `pool`.
    pub fn new(mp: MappedPages, phys_addr: PhysicalAddress, length: u16, pool: &'static mpmc::Queue<ReceiveBuffer>) -> ReceiveBuffer {
        ReceiveBuffer {
            mp: mp,
            phys_addr: phys_addr,
            length: length,
            pool: pool,
        }
    }
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
        // trace!("ReceiveBuffer::drop(): length: {:5}, phys_addr: {:#X}, vaddr: {:#X}", self.length,  self.phys_addr, self.mp.start_address());

        // We need to return this ReceiveBuffer to its memory pool. We use a clever trick here:
        // Since we cannot move this receive buffer out of `self` because it's borrowed, 
        // we construct a new ReceiveBuffer object that is identical to this one being dropped,
        // and do an in-place replacement of its `MappedPages` object with an empty MP object,
        // allowing us to take ownership of the real MP object and put it into the new_rb. 
        let new_rb = ReceiveBuffer {
            mp: core::mem::replace(&mut self.mp, MappedPages::empty()),
            phys_addr: self.phys_addr,
            length: 0,
            pool: self.pool,
        };
        // we set the length to 0 as a quick way to "clear" the buffer. We could also zero out the whole MP. 

        // Now, we can add the new receive buffer to the pool 
        if let Err(_e) = self.pool.push(new_rb) {
            error!("NIC: couldn't return dropped ReceiveBuffer to pool, buf length: {}, phys_addr: {:#X}", _e.length, _e.phys_addr);
        }

        // `self` will be automatically dropped now, which only has the empty MP object.
    }
}


/// A network (e.g., Ethernet) frame that has been received by the NIC.
pub struct ReceivedFrame(pub Vec<ReceiveBuffer>);
