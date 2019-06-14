#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mpmc;
extern crate volatile;
extern crate bit_field;
extern crate pci;
extern crate spin;
extern crate owning_ref;
extern crate irq_safety;

use core::ops::{Deref, DerefMut};
use alloc::{
    vec::Vec,
    collections::VecDeque
};
use memory::{create_contiguous_mapping, PhysicalAddress, EntryFlags, MappedPages};
use volatile::Volatile;
use owning_ref::BoxRefMut;

pub mod intel_ethernet;
use intel_ethernet::{RxDescriptor, TxDescriptor, NicInit, RxQueue, TxQueue};

/// The mapping flags used for pages that the NIC will map.
/// This should be a const, but Rust doesn't yet allow constants for the bitflags type
pub fn nic_mapping_flags() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE
}

/// A trait that defines the necessary minimum functions that all network interface card (NIC) drivers
/// should implement. 
pub trait NetworkInterfaceCard {
    /// Sends a packet contained in the given `transmit_buffer` out through this NetworkInterfaceCard. 
    /// Blocks until the packet has been successfully sent by the networking card hardware.
    fn send_packet(&self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>;

    /// Sends a packet on the specified transmit queue
    /// 
    /// # Arguments:
    /// * `txq`: transmit queue 
    /// * `max_tx_desc`: number of tx descriptors in the queue
    /// * `transmit_buffer`: buffer containing the packet to be sent
    fn send_on_queue<T: TxDescriptor>(txq: &mut TxQueue<T>, max_tx_desc: u16, transmit_buffer: TransmitBuffer) {
        txq.tx_descs[txq.tx_cur as usize].send(transmit_buffer);  
        // update the tx_cur value to hold the next free descriptor
        let old_cur = txq.tx_cur;
        txq.tx_cur = (txq.tx_cur + 1) % max_tx_desc;
        // update the tdt register by 1 so that it knows the previous descriptor has been used
        // and has a packet to be sent
        txq.update_tdt(txq.tx_cur as u32);
        // Wait for the packet to be sent
        txq.tx_descs[old_cur as usize].wait_for_packet_tx();
    }

    /// Returns the earliest `ReceivedFrame`, which is essentially a list of `ReceiveBuffer`s 
    /// that each contain an individual piece of the frame.
    fn get_received_frame(&mut self) -> Option<ReceivedFrame>;

    /// Poll the NIC for received frames. 
    /// Can be used as an alternative to interrupts, or as a supplement to interrupts.
    fn poll_receive(&self) -> Result<(), &'static str>;

    /// Retrieves the ethernet frames from one queue
    /// 
    /// # Arguments
    /// * `rxq`: receive queue to collect frames from 
    /// * `num_descs`: number of descriptors in the queue
    /// * `rx_buffer_pool`: pool which contains the receive buffers
    /// * `rx_buffer_size`: size of buffers in the 'rx_buffer_pool' in bytes
    fn collect_from_queue<T: RxDescriptor>(rxq: &mut RxQueue<T>, num_descs: u16, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>, rx_buffer_size: u16) -> Result<(), &'static str> {

        let mut cur = rxq.rx_cur as usize;
       
        let mut receive_buffers_in_frame: Vec<ReceiveBuffer> = Vec::new();
        let mut total_packet_length: u16 = 0;

        //print status of all packets until EoP
        while rxq.rx_descs[cur].descriptor_done() {
            // get information about the current receive buffer
            let length = rxq.rx_descs[cur].length();
            total_packet_length += length as u16;
            // debug!("collect_from_queue: received descriptor of length {}", length);
            
            // Now that we are "removing" the current receive buffer from the list of receive buffers that the NIC can use,
            // (because we're saving it for higher layers to use),
            // we need to obtain a new `ReceiveBuffer` and set it up such that the NIC will use it for future receivals.
            let new_receive_buf = match rx_buffer_pool.pop() {
                Some(rx_buf) => rx_buf,
                None => {
                    warn!("NIC RX BUF POOL WAS EMPTY.... reallocating! This means that no task is consuming the accumulated received ethernet frames.");
                    // if the pool was empty, then we allocate a new receive buffer
                    let len = rx_buffer_size;
                    let (mp, phys_addr) = create_contiguous_mapping(len as usize, nic_mapping_flags())?;
                    ReceiveBuffer::new(mp, phys_addr, len, rx_buffer_pool)
                }
            };

            // actually tell the NIC about the new receive buffer, and that it's ready for use now
            rxq.rx_descs[cur].set_packet_address(new_receive_buf.phys_addr);

            // Swap in the new receive buffer at the index corresponding to this current rx_desc's receive buffer,
            // getting back the receive buffer that is part of the received ethernet frame
            rxq.rx_bufs_in_use.push(new_receive_buf);
            let mut current_rx_buf = rxq.rx_bufs_in_use.swap_remove(cur); 
            current_rx_buf.length = length as u16; // set the ReceiveBuffer's length to the size of the actual packet received
            receive_buffers_in_frame.push(current_rx_buf);

            // move on to the next receive buffer to see if it's ready for us to take
            rxq.rx_cur = (cur as u16 + 1) % num_descs;
            rxq.update_rdt(cur as u32); 

            if rxq.rx_descs[cur].end_of_packet() {
                let buffers = core::mem::replace(&mut receive_buffers_in_frame, Vec::new());
                rxq.received_frames.push_back(ReceivedFrame(buffers));
            } else {
                warn!("NIC::collect_from_queue(): Received multi-rxbuffer frame, this scenario not fully tested!");
            }
            rxq.rx_descs[cur].reset_status();
            cur = rxq.rx_cur as usize;
        }

        Ok(())
    }

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
