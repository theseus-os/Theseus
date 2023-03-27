//! Defines the receive and transmit queues that store a ring of DMA descriptors and related information.
//! 
//! Receive and transmit queues are used across all NICs to keep track of incoming and outgoing packets.
//! HW queues used by the NIC only consist of the ring of DMA descriptors.
//! The SW queues defined here hold the ring of DMA descriptors that it shares with the HW,
//! as well as other information such as the buffers received from the queues,
//! the tail register for each queue and the cpu the queue is mapped to.

#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate memory;
extern crate intel_ethernet;
extern crate nic_buffers;

use alloc::{
    vec::Vec,
    collections::VecDeque
};
use memory::{create_contiguous_mapping, BorrowedSliceMappedPages, Mutable};
use intel_ethernet::descriptors::{RxDescriptor, TxDescriptor};
use nic_buffers::{ReceiveBuffer, ReceivedFrame, TransmitBuffer};
pub use nic_buffers::NIC_MAPPING_FLAGS;

/// The register trait that gives access to only those registers required for receiving a packet.
/// The Rx queue control registers can only be accessed by the physical NIC.
pub trait RxQueueRegisters {
    fn set_rdbal(&mut self, value: u32);
    fn set_rdbah(&mut self, value: u32);
    fn set_rdlen(&mut self, value: u32);
    fn set_rdh(&mut self, value: u32);
    fn set_rdt(&mut self, value: u32);
}

/// The register trait that gives access to only those registers required for sending a packet.
/// The Tx queue control registers can only be accessed by the physical NIC.
pub trait TxQueueRegisters {
    fn set_tdbal(&mut self, value: u32);
    fn set_tdbah(&mut self, value: u32);
    fn set_tdlen(&mut self, value: u32);
    fn set_tdh(&mut self, value: u32);
    fn set_tdt(&mut self, value: u32);
}

/// A struct that holds all information for one receive queue.
/// There should be one such object per queue.
pub struct RxQueue<S: RxQueueRegisters, T: RxDescriptor> {
    /// The number of the queue, stored here for our convenience.
    pub id: u8,
    /// Registers for this receive queue
    pub regs: S,
    /// Receive descriptors
    pub rx_descs: BorrowedSliceMappedPages<T, Mutable>,
    /// The number of receive descriptors in the descriptor ring
    pub num_rx_descs: u16,
    /// Current receive descriptor index
    pub rx_cur: u16,
    /// The list of rx buffers, in which the index in the vector corresponds to the index in `rx_descs`.
    /// For example, `rx_bufs_in_use[2]` is the receive buffer that will be used when `rx_descs[2]` is the current rx descriptor (rx_cur = 2).
    pub rx_bufs_in_use: Vec<ReceiveBuffer>,
    pub rx_buffer_size_bytes: u16,
    /// The queue of received Ethernet frames, ready for consumption by a higher layer.
    /// Just like a regular FIFO queue, newly-received frames are pushed onto the back
    /// and frames are popped off of the front.
    /// Each frame is represented by a `Vec<ReceiveBuffer>`, because a single frame can span multiple receive buffers.
    /// TODO: improve this? probably not the best cleanest way to expose received frames to higher layers   
    pub received_frames: VecDeque<ReceivedFrame>,
    /// The cpu which this queue is mapped to. 
    /// This in itself doesn't guarantee anything, but we use this value when setting the cpu id for interrupts and DCA.
    pub cpu_id: Option<u8>,
    /// Pool where `ReceiveBuffer`s are stored.
    pub rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>,
    /// The filter id for the physical NIC filter that is set for this queue
    pub filter_num: Option<u8>
}

impl<S: RxQueueRegisters, T: RxDescriptor> RxQueue<S,T> {
    /// Polls the queue and removes all received packets from it.
    /// The received packets are stored in the receive queue's `received_frames` FIFO queue.
    pub fn poll_queue_and_store_received_packets(&mut self) -> Result<(), &'static str> {
        let mut cur = self.rx_cur as usize;
       
        let mut receive_buffers_in_frame: Vec<ReceiveBuffer> = Vec::new();
        let mut _total_packet_length: u16 = 0;

        while self.rx_descs[cur].descriptor_done() {
            // get information about the current receive buffer
            let length = self.rx_descs[cur].length();
            _total_packet_length += length as u16;
            // error!("poll_queue_and_store_received_packets {}: received descriptor of length {}", self.id, length);
            
            // Now that we are "removing" the current receive buffer from the list of receive buffers that the NIC can use,
            // (because we're saving it for higher layers to use),
            // we need to obtain a new `ReceiveBuffer` and set it up such that the NIC will use it for future receivals.
            let new_receive_buf = match self.rx_buffer_pool.pop() {
                Some(rx_buf) => rx_buf,
                None => {
                    warn!("NIC RX BUF POOL WAS EMPTY.... reallocating! This means that no task is consuming the accumulated received ethernet frames.");
                    // if the pool was empty, then we allocate a new receive buffer
                    let len = self.rx_buffer_size_bytes;
                    let (mp, phys_addr) = create_contiguous_mapping(len as usize, NIC_MAPPING_FLAGS)?;
                    ReceiveBuffer::new(mp, phys_addr, len, self.rx_buffer_pool)?
                }
            };

            // actually tell the NIC about the new receive buffer, and that it's ready for use now
            self.rx_descs[cur].set_packet_address(new_receive_buf.phys_addr());

            // Swap in the new receive buffer at the index corresponding to this current rx_desc's receive buffer,
            // getting back the receive buffer that is part of the received ethernet frame
            self.rx_bufs_in_use.push(new_receive_buf);
            let mut current_rx_buf = self.rx_bufs_in_use.swap_remove(cur); 
            current_rx_buf.set_length(length as u16)?; // set the ReceiveBuffer's length to the size of the actual packet received
            receive_buffers_in_frame.push(current_rx_buf);

            // move on to the next receive buffer to see if it's ready for us to take
            self.rx_cur = (cur as u16 + 1) % self.num_rx_descs;
            self.regs.set_rdt(cur as u32); 

            if self.rx_descs[cur].end_of_packet() {
                let buffers = core::mem::take(&mut receive_buffers_in_frame);
                self.received_frames.push_back(ReceivedFrame(buffers));
            } else {
                warn!("NIC::poll_queue_and_store_received_packets(): Received multi-rxbuffer frame, this scenario not fully tested!");
            }
            self.rx_descs[cur].reset_status();
            cur = self.rx_cur as usize;
        }

        Ok(())
    }

    /// Returns the earliest received ethernet frame.
    pub fn return_frame(&mut self) -> Option<ReceivedFrame> {
        self.received_frames.pop_front()
    }
}

/// A struct that holds all information for a transmit queue. 
/// There should be one such object per queue.
pub struct TxQueue<S: TxQueueRegisters, T: TxDescriptor> {
    /// The number of the queue, stored here for our convenience.
    pub id: u8,
    /// Registers for this transmit queue
    pub regs: S,
    /// Transmit descriptors 
    pub tx_descs: BorrowedSliceMappedPages<T, Mutable>,
    /// The number of transmit descriptors in the descriptor ring
    pub num_tx_descs: u16,
    /// Current transmit descriptor index
    pub tx_cur: u16,
    /// The cpu which this queue is mapped to. 
    /// This in itself doesn't guarantee anything but we use this value when setting the cpu id for interrupts and DCA.
    pub cpu_id : Option<u8>
}

impl<S: TxQueueRegisters, T: TxDescriptor> TxQueue<S,T> {
    /// Sends a packet on the transmit queue
    /// 
    /// # Arguments:
    /// * `transmit_buffer`: buffer containing the packet to be sent
    pub fn send_on_queue(&mut self, transmit_buffer: TransmitBuffer) {
        self.tx_descs[self.tx_cur as usize].send(transmit_buffer.phys_addr(), transmit_buffer.length());
        // update the tx_cur value to hold the next free descriptor
        let old_cur = self.tx_cur;
        self.tx_cur = (self.tx_cur + 1) % self.num_tx_descs;
        // update the tdt register by 1 so that it knows the previous descriptor has been used
        // and has a packet to be sent
        self.regs.set_tdt(self.tx_cur as u32);
        // Wait for the packet to be sent
        self.tx_descs[old_cur as usize].wait_for_packet_tx();
    }
}

