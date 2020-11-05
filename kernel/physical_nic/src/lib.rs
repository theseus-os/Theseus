#![no_std]

extern crate nic_queues;
extern crate intel_ethernet;
extern crate alloc;

use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};
use intel_ethernet::descriptors::{TxDescriptor, RxDescriptor};
use alloc::vec::Vec;

pub trait PhysicalNic<S: RxQueueRegisters, T: RxDescriptor, U: TxQueueRegisters, V: TxDescriptor> {
    fn return_rx_queues(&mut self, queues: Vec<RxQueue<S,T>>);
    fn return_tx_queues(&mut self, queues: Vec<TxQueue<U,V>>);
    fn power_down(&mut self);
}