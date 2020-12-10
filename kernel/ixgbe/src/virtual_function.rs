//! Interface for an application to request a `VirtualNIC` from the ixgbe device,
//! and implementation of the `PhysicalNic` trait for the ixgbe device.
//! The `PhysicalNic` trait is required for returning virtual NIC resources to the ixgbe device when dropped.

use super::{IxgbeNic, get_ixgbe_nic, IxgbeRxQueueRegisters, IxgbeTxQueueRegisters};
use physical_nic::PhysicalNic;
use virtual_nic::VirtualNic;
use intel_ethernet::descriptors::{AdvancedRxDescriptor, AdvancedTxDescriptor};
use nic_queues::{RxQueue, TxQueue};
use alloc::vec::Vec;
use network_interface_card::NetworkInterfaceCard;


pub fn create_virtual_nic(ip_addresses: Vec<[u8;4]>, default_rx_queue: usize, default_tx_queue: usize) 
    -> Result<VirtualNic<IxgbeRxQueueRegisters, AdvancedRxDescriptor, IxgbeTxQueueRegisters, AdvancedTxDescriptor>, &'static str> 
{
    let num_queues = ip_addresses.len();
    if num_queues == 0 {return Err("need to request >0 number of queues");}
    if (default_rx_queue >= num_queues) | (default_tx_queue >= num_queues) {
        return Err("default queue value is out of bounds");
    }
    
    let mut nic = get_ixgbe_nic().ok_or("Ixgbe nic not initialized")?.lock();
    // Allocate queues from the physical NIC
    let mut rx_queues = nic.remove_rx_queues(num_queues)?;
    let tx_queues = nic.remove_tx_queues(num_queues)?;
    // Set up the filters so that packets sent to `ip_addresses` are forwarded to these queues.
    for (queue, ip_address) in rx_queues.iter_mut().zip(ip_addresses.iter()) {
        let filter_num = nic.set_5_tuple_filter(None, Some(*ip_address), None, None, None, 7 /*highest priority*/, queue.id)?;
        queue.filter_num = Some(filter_num);
    }

    VirtualNic::new(
        rx_queues,
        default_rx_queue,
        tx_queues,
        default_tx_queue,
        nic.mac_address(),
        get_ixgbe_nic().ok_or("Ixgbe nic isn't initialized")?
    )
}


impl PhysicalNic<IxgbeRxQueueRegisters, AdvancedRxDescriptor, IxgbeTxQueueRegisters, AdvancedTxDescriptor> for IxgbeNic {
    fn return_rx_queues(&mut self, mut queues: Vec<RxQueue<IxgbeRxQueueRegisters, AdvancedRxDescriptor>>) {
        // disable filters for all queues
        for queue in &queues {
            self.disable_5_tuple_filter(queue.filter_num.unwrap()); // safe to unwrap here because creation of virtualNIC ensures a filter
        }
        // return queues to physical nic
        self.rx_queues.append(&mut queues);
    }

    fn return_tx_queues(&mut self, mut queues: Vec<TxQueue<IxgbeTxQueueRegisters, AdvancedTxDescriptor>>) {
        // return queues to physical nic
        self.tx_queues.append(&mut queues);
    }
}