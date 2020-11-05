#![no_std]

extern crate nic_buffers;
extern crate nic_queues;
extern crate network_interface_card;
extern crate physical_nic;
extern crate intel_ethernet;
extern crate alloc;
extern crate irq_safety;

use nic_buffers::{TransmitBuffer, ReceivedFrame};
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
    wakelock: Arc<u8>,
    physical_nic_ref: &'static MutexIrqSafe<dyn PhysicalNic<S,T,U,V>>
}

impl<S: RxQueueRegisters, T: RxDescriptor, U: TxQueueRegisters, V: TxDescriptor> VirtualNic<S,T,U,V> {
    pub fn new(rx_queues: Vec<RxQueue<S,T>>,
    default_rx_queue: usize,
    tx_queues: Vec<TxQueue<U,V>>,
    default_tx_queue: usize,
    mac_address: [u8; 6],
    wakelock: Arc<u8>,
    physical_nic_ref: &'static MutexIrqSafe<dyn PhysicalNic<S,T,U,V>>) 
        -> VirtualNic<S,T,U,V> 
    {
        VirtualNic {
            rx_queues,
            default_rx_queue,
            tx_queues,
            default_tx_queue,
            mac_address,
            wakelock,
            physical_nic_ref
        }

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

        if Arc::strong_count(&self.wakelock) == 2 {
            nic.power_down();
        }
    }
}