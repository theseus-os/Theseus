//! Defines a trait `PhysicalNic` that must be implemented by any NIC driver that wants to support
//! language-level virtualization. This trait defines functions that can be called from a `VirtualNic`
//! drop handler to return NIC resources to the OS.

#![no_std]

extern crate alloc;
extern crate intel_ethernet;
extern crate nic_queues;

use alloc::vec::Vec;
use intel_ethernet::descriptors::{RxDescriptor, TxDescriptor};
use nic_queues::{RxQueue, RxQueueRegisters, TxQueue, TxQueueRegisters};

/// This trait must be implemented by any NIC driver that wants to support language-level virtualization.
/// It provides functions that are used to return Rx/Tx queues back to the physical NIC.
pub trait PhysicalNic<S: RxQueueRegisters, T: RxDescriptor, U: TxQueueRegisters, V: TxDescriptor> {
    /// Returns the `RxQueue`s owned by a virtual NIC back to the physical NIC.
    fn return_rx_queues(&mut self, queues: Vec<RxQueue<S, T>>);
    /// Returns the `TxQueue`s owned by a virtual NIC back to the physical NIC.
    fn return_tx_queues(&mut self, queues: Vec<TxQueue<U, V>>);
}
