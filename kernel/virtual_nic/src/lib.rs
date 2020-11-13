#![no_std]

extern crate nic_buffers;
extern crate nic_queues;
extern crate network_interface_card;
extern crate physical_nic;
extern crate intel_ethernet;
extern crate alloc;
extern crate irq_safety;

use nic_buffers::{TransmitBuffer, ReceivedFrame, ReceiveBuffer};
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};
use network_interface_card::{NetworkInterfaceCard};
use intel_ethernet::descriptors::{TxDescriptor, RxDescriptor};
use physical_nic::PhysicalNic;
use alloc::vec::Vec;
use alloc::sync::Arc;
use irq_safety::MutexIrqSafe;

pub struct VirtualNic<S: RxQueueRegisters + 'static, T: RxDescriptor + 'static, U: TxQueueRegisters + 'static, V: TxDescriptor + 'static> {
    rx_queues: Vec<RxQueue<S,T>>,
    default_rx_queue: usize,
    tx_queues: Vec<TxQueue<U,V>>,
    default_tx_queue: usize,
    mac_address: [u8; 6],
    physical_nic_ref: &'static MutexIrqSafe<dyn PhysicalNic<S,T,U,V>>
}

impl<S: RxQueueRegisters, T: RxDescriptor, U: TxQueueRegisters, V: TxDescriptor> VirtualNic<S,T,U,V> {
    pub fn new(rx_queues: Vec<RxQueue<S,T>>,
    default_rx_queue: usize,
    tx_queues: Vec<TxQueue<U,V>>,
    default_tx_queue: usize,
    mac_address: [u8; 6],
    physical_nic_ref: &'static MutexIrqSafe<dyn PhysicalNic<S,T,U,V>>) 
        -> VirtualNic<S,T,U,V> 
    {
        VirtualNic {
            rx_queues,
            default_rx_queue,
            tx_queues,
            default_tx_queue,
            mac_address,
            physical_nic_ref
        }

    }

    pub fn send_batch(&mut self, packets: &Vec<TransmitBuffer>) -> Result<(), &'static str> {
        self.tx_queues[self.default_tx_queue].send_batch_on_queue(packets);
        Ok(())
    }

    pub fn receive_batch(&mut self, batch_size: usize, buffers:&mut Vec<ReceiveBuffer>) -> Result<(), &'static str> {
        self.rx_queues[self.default_rx_queue].remove_batch_from_queue(batch_size, buffers)?;
        Ok(())
    }
}
impl<S: RxQueueRegisters, T: RxDescriptor, U: TxQueueRegisters, V: TxDescriptor> NetworkInterfaceCard for VirtualNic<S,T,U,V> {
    fn send_packet(&mut self, transmit_buffer: TransmitBuffer) -> Result<(), &'static str> {
        self.tx_queues[self.default_tx_queue].send_on_queue(transmit_buffer);
        Ok(())
    }

    // this function has only been tested with 1 Rx queue and is meant to be used with the smoltcp stack.
    fn get_received_frame(&mut self) -> Option<ReceivedFrame> {
        // return one frame from the queue's received frames
        self.rx_queues[self.default_rx_queue].received_frames.pop_front()
    }

    fn poll_receive(&mut self) -> Result<(), &'static str> {
        self.rx_queues[self.default_rx_queue].remove_frames_from_queue()?;
        Ok(())
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }
}

impl<S: RxQueueRegisters, T: RxDescriptor, U: TxQueueRegisters, V: TxDescriptor> Drop for VirtualNic<S,T,U,V> {
    fn drop(&mut self) {
        let mut nic = self.physical_nic_ref.lock();
        
        let mut rx_queues = Vec::new();
        let mut tx_queues = Vec::new();
        core::mem::swap(&mut rx_queues, &mut self.rx_queues);
        core::mem::swap(&mut tx_queues, &mut self.tx_queues);

        nic.return_rx_queues(rx_queues);
        nic.return_tx_queues(tx_queues);
    }
}