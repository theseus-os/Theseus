#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate nic_descriptors;
extern crate nic_buffers;

use core::ptr::write_volatile;
use memory::{VirtualAddress, MappedPages};
use nic_descriptors::{RxDescriptor, TxDescriptor};
use owning_ref::BoxRefMut;
use alloc::{
    vec::Vec,
    collections::VecDeque
};
use nic_buffers::{ReceiveBuffer, ReceivedFrame};

/// Set of functions to access Rx queue initialization registers.
pub trait RxQueueRegisters {
    /// write to the rdbal register to store the lower 32 bits of the buffer physical address
    fn rdbal(&mut self, val: u32);
    /// write to the rdbah register to store the higher 32 bits of the buffer physical address
    fn rdbah(&mut self, val: u32);
    /// write to the rdlen register to store the length of the queue in bytes
    fn rdlen(&mut self, val: u32);
    /// write to the rdh register to store the descriptor at the head of the queue
    fn rdh(&mut self, val: u32);
    /// write to the rdt register to store the descriptor at the tail of the queue
    fn rdt(&mut self, val: u32);
}

/// Set of functions to access Tx queue initialization registers.
pub trait TxQueueRegisters {
    /// write to the tdbal register to store the lower 32 bits of the buffer physical address
    fn tdbal(&mut self, val: u32);
    /// write to the tdbah register to store the higher 32 bits of the buffer physical address
    fn tdbah(&mut self, val: u32);
    /// write to the tdlen register to store the length of the queue in bytes
    fn tdlen(&mut self, val: u32);
    /// write to the tdh register to store the descriptor at the head of the queue
    fn tdh(&mut self, val: u32);
    /// write to the tdt register to store the descriptor at the tail of the queue
    fn tdt(&mut self, val: u32);
}

/// A struct that holds all information for one receive queue.
/// There should be one such object per queue
pub struct RxQueue<T: RxDescriptor> {
    /// The number of the queue, stored here for our convenience.
    /// It should match its index in the `queue` field of the RxQueues struct
    pub id: u8,
    /// Receive descriptors
    pub rx_descs: BoxRefMut<MappedPages, [T]>,
    /// Current receive descriptor index
    pub rx_cur: u16,
    /// The list of rx buffers, in which the index in the vector corresponds to the index in `rx_descs`.
    /// For example, `rx_bufs_in_use[2]` is the receive buffer that will be used when `rx_descs[2]` is the current rx descriptor (rx_cur = 2).
    pub rx_bufs_in_use: Vec<ReceiveBuffer>,
    /// The queue of received Ethernet frames, ready for consumption by a higher layer.
    /// Just like a regular FIFO queue, newly-received frames are pushed onto the back
    /// and frames are popped off of the front.
    /// Each frame is represented by a Vec<ReceiveBuffer>, because a single frame can span multiple receive buffers.
    /// TODO: improve this? probably not the best cleanest way to expose received frames to higher layers   
    pub received_frames: VecDeque<ReceivedFrame>,
    /// The cpu which this queue is mapped to. 
    /// This in itself doesn't guarantee anything, but we use this value when setting the cpu id for interrupts and DCA.
    pub cpu_id: u8,
    /// The address where the rdt register is located for this queue
    pub rdt_addr: VirtualAddress,
}

impl<T: RxDescriptor> RxQueue<T> {
    /// Updates the queue tail descriptor in the rdt register
    pub fn update_rdt(&self, val: u32) {
        unsafe { write_volatile((self.rdt_addr.value()) as *mut u32, val) }
    }
}



/// A struct that holds all information for a transmit queue. 
/// There should be one such object per queue.
pub struct TxQueue<T: TxDescriptor> {
    /// The number of the queue, stored here for our convenience.
    /// It should match its index in the `queue` field of the TxQueues struct
    pub id: u8,
    /// Transmit descriptors 
    pub tx_descs: BoxRefMut<MappedPages, [T]>,
    /// Current transmit descriptor index
    pub tx_cur: u16,
    /// The cpu which this queue is mapped to. 
    /// This in itself doesn't guarantee anything but we use this value when setting the cpu id for interrupts and DCA.
    pub cpu_id : u8,
    /// The address where the tdt register is located for this queue
    pub tdt_addr: VirtualAddress,
}

impl<T: TxDescriptor> TxQueue<T> {
    /// Updates the queue tail descriptor in the rdt register
    pub fn update_tdt(&self, val: u32) {
        unsafe { write_volatile((self.tdt_addr.value()) as *mut u32, val) }
    }
}
