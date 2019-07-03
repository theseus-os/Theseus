#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate mpmc;
extern crate nic_init;
extern crate nic_descriptors;
extern crate nic_queues;
extern crate nic_buffers;

use alloc::vec::Vec;
use memory::{create_contiguous_mapping};
use nic_init::{nic_mapping_flags};
use nic_descriptors:: {TxDescriptor, RxDescriptor};
use nic_queues::{RxQueue, TxQueue};
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};


/// A trait that defines the necessary minimum functions that all network interface card (NIC) drivers
/// should implement. 
pub trait NetworkInterfaceCard {
    /// Sends a packet contained in the given `transmit_buffer` out through this NetworkInterfaceCard. 
    /// Blocks until the packet has been successfully sent by the networking card hardware.
    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str>;

    /// Sends a packet on the specified transmit queue
    /// 
    /// # Arguments:
    /// * `txq`: transmit queue 
    /// * `max_tx_desc`: number of tx descriptors in the queue
    /// * `transmit_buffer`: buffer containing the packet to be sent
    fn send_on_queue<T: TxDescriptor>(txq: &mut TxQueue<T>, max_tx_desc: u16, transmit_buffer: TransmitBuffer) {
        txq.tx_descs[txq.tx_cur as usize].send(transmit_buffer.phys_addr, transmit_buffer.length);  
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
    fn poll_receive(&mut self) -> Result<(), &'static str>;

    /// Retrieves the ethernet frames from one queue
    /// 
    /// # Arguments
    /// * `rxq`: receive queue to collect frames from 
    /// * `num_descs`: number of descriptors in the queue
    /// * `rx_buffer_pool`: pool which contains the receive buffers
    /// * `rx_buffer_size`: size of buffers in the 'rx_buffer_pool' in bytes
    fn remove_frames_from_queue<T: RxDescriptor>(rxq: &mut RxQueue<T>, num_descs: u16, rx_buffer_pool: &'static mpmc::Queue<ReceiveBuffer>, rx_buffer_size: u16) -> Result<(), &'static str> {

        let mut cur = rxq.rx_cur as usize;
       
        let mut receive_buffers_in_frame: Vec<ReceiveBuffer> = Vec::new();
        let mut total_packet_length: u16 = 0;

        //print status of all packets until EoP
        while rxq.rx_descs[cur].descriptor_done() {
            // get information about the current receive buffer
            let length = rxq.rx_descs[cur].length();
            total_packet_length += length as u16;
            // debug!("remove_frames_from_queue: received descriptor of length {}", length);
            
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
                warn!("NIC::remove_frames_from_queue(): Received multi-rxbuffer frame, this scenario not fully tested!");
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
