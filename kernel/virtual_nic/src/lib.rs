//! This crate defines a struct that enables language-level virtualization of the NIC.
//! A `VirtualNIC` contains a subset of physical NIC resources that are sufficient for an application,
//! that has mutable access to a `VirtualNIC`, to send and receive packets without kernel mediation.
//! The resources of a `VirtualNIC` are a set of `RxQueue`s and `TxQueue`s that are passed to
//! it by a physical NIC. On creation of a `VirtualNIC`, hardware filters are set to route packets 
//! sent to a specified IP address to the `VirtualNIC` queues. When a `VirtualNIC` is dropped,
//! its resources are returned to the physical NIC.
 
#![no_std]

extern crate nic_buffers;
extern crate nic_queues;
extern crate net;
extern crate physical_nic;
extern crate intel_ethernet;
extern crate alloc;
extern crate sync_irq;

use nic_buffers::{TransmitBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};
use intel_ethernet::descriptors::{TxDescriptor, RxDescriptor};
use physical_nic::PhysicalNic;
use alloc::vec::Vec;
use sync_irq::IrqSafeMutex;

/// A structure that contains a set of `RxQueue`s and `TxQueue`s that can be used to send and receive packets.
pub struct VirtualNic<S, T, U, V>
where
    S: RxQueueRegisters + 'static,
    T: RxDescriptor + 'static,
    U: TxQueueRegisters + 'static,
    V: TxDescriptor + 'static,
{
    /// The virtual NIC id is set to the id of its first receive queue
    id: u8, 
    /// Set of `RxQueue`s assigned to a virtual NIC
    rx_queues: Vec<RxQueue<S,T>>,
    /// The queue that a packet is received on if no other queue is specified
    default_rx_queue: usize,
    /// Set of `TxQueue`s assigned to a virtual NIC
    tx_queues: Vec<TxQueue<U,V>>,
    /// The queue that a packet is sent on if no other queue is specified
    default_tx_queue: usize,
    /// MAC address of the NIC
    mac_address: [u8; 6],
    /// Reference to the physical NIC that Rx/Tx queues will be returned to.
    physical_nic_ref: &'static IrqSafeMutex<dyn PhysicalNic<S, T, U, V> + Send>
}

impl<S, T, U, V> VirtualNic<S, T, U, V>
where
    S: RxQueueRegisters + 'static,
    T: RxDescriptor + 'static,
    U: TxQueueRegisters + 'static,
    V: TxDescriptor + 'static,
{
    /// Create a new `VirtualNIC` with the given parameters.
    /// For now we require that there is at least one Rx and one Tx queue.
    pub fn new(
        rx_queues: Vec<RxQueue<S,T>>,
        default_rx_queue: usize, 
        tx_queues: Vec<TxQueue<U,V>>,
        default_tx_queue: usize, 
        mac_address: [u8; 6], 
        physical_nic_ref: &'static IrqSafeMutex<dyn PhysicalNic<S,T,U,V> + Send>
    ) -> Result<VirtualNic<S,T,U,V>, &'static str> {

        if rx_queues.is_empty() || tx_queues.is_empty() { 
            return Err("Must have at least one Rx and Tx queue to create virtual NIC");
        }

        Ok(VirtualNic {
            id: rx_queues[0].id,
            rx_queues,
            default_rx_queue,
            tx_queues,
            default_tx_queue,
            mac_address,
            physical_nic_ref
        })
    }

    pub fn id(&self) -> u8 {
        self.id
    }

    /// Send a packet on the specified queue.
    #[allow(dead_code)]
    pub fn send_packet_on_queue(&mut self, qid: usize, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        if qid >= self.tx_queues.len() { return Err("Invalid qid"); }
        self.tx_queues[qid].send_on_queue(transmit_buffer);
        Ok(())
    }

    /// Retrieve a received frame from the specified queue.
    #[allow(dead_code)]
    fn get_received_frame_from_queue(&mut self, qid: usize) -> Result<ReceivedFrame, &'static str> {
        if qid >= self.rx_queues.len() { return Err("Invalid qid"); }
        // return one frame from the queue's received frames
        self.rx_queues[qid].received_frames.pop_front().ok_or("No frames received")
    }

    /// Poll the specified queue to check if any packets have been received.
    #[allow(dead_code)]
    fn poll_receive_queue(&mut self, qid: usize) -> Result<(), &'static str> {
        if qid >= self.rx_queues.len() { return Err("Invalid qid"); }
        self.rx_queues[qid].poll_queue_and_store_received_packets()?;
        Ok(())
    }
}

impl<S, T, U, V> net::NetworkDevice for VirtualNic<S, T, U, V>
where
    S: RxQueueRegisters + Send + Sync + 'static,
    T: RxDescriptor + Send + Sync + 'static,
    U: TxQueueRegisters + Send + Sync + 'static,
    V: TxDescriptor + Send + Sync + 'static,
{
    fn send(&mut self, buf: TransmitBuffer) {
        self.tx_queues[self.default_tx_queue].send_on_queue(buf);
    }

    fn receive(&mut self) -> Option<ReceivedFrame> {
        // poll_queue_and_store_received_packets will only return an error if it fails
        // to create a contiguous mapping, or if the mapping created is of the wrong
        // length, indicating a logic bug.
        self.rx_queues[self.default_rx_queue]
            .poll_queue_and_store_received_packets()
            .expect("failed to poll virtual NIC queue");
        self.rx_queues[self.default_rx_queue].received_frames.pop_front()
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }
}

impl<S, T, U, V> Drop for VirtualNic<S, T, U, V>
where
    S: RxQueueRegisters + 'static,
    T: RxDescriptor + 'static,
    U: TxQueueRegisters + 'static,
    V: TxDescriptor + 'static,
{
    // Right now we assume that a `virtualNIC` is only dropped when all packets have been removed from queues.
    // TODO: check that queues are empty before returning to the NIC.
    fn drop(&mut self) {
        // get access to the physical NIC
        let mut nic = self.physical_nic_ref.lock();
        // remove queues from virtual NIC
        let mut rx_queues = Vec::new();
        let mut tx_queues = Vec::new();
        core::mem::swap(&mut rx_queues, &mut self.rx_queues);
        core::mem::swap(&mut tx_queues, &mut self.tx_queues);
        // return queues to physical NIC
        nic.return_rx_queues(rx_queues);
        nic.return_tx_queues(tx_queues);
    }
}