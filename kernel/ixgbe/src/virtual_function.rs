//! Interface for an application to request a virtual NIC from the ixgbe device,
//! and implementation of the PhysicalNic trait for the ixgbe device,
//! The PhysicalNic trait is required for returning virtual NIC resources to the ixgbe device when dropped.

use super::{IxgbeNic, get_ixgbe_nic,IxgbeRxQueueRegisters, IxgbeTxQueueRegisters
    // queue_registers::{IxgbeRxQueueRegisters, IxgbeTxQueueRegisters}
};
use physical_nic::PhysicalNic;
use virtual_nic::VirtualNic;
use intel_ethernet::{
    descriptors::{AdvancedRxDescriptor, AdvancedTxDescriptor, TxDescriptor, RxDescriptor},
    types::Rdt,
};  
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};
use alloc::vec::Vec;
use alloc::sync::Arc;
use network_interface_card::NetworkInterfaceCard;


pub fn create_virtual_nic(num_queues: usize, ip_addresses: Vec<[u8;4]>) 
    -> Result<VirtualNic<IxgbeRxQueueRegisters, AdvancedRxDescriptor, IxgbeTxQueueRegisters, AdvancedTxDescriptor>, &'static str> 
{
    if num_queues == 0 {return Err("need to request >0 number of queues");}
    if num_queues != ip_addresses.len() { return Err("The number of queues requested does not match the number of ip_addresses");}
    
    let mut nic = get_ixgbe_nic().ok_or("Ixgbe nic not initialized")?.lock();
    let num_available_queues = nic.rx_queues.len();
    if num_available_queues <= num_queues  {
        return Err("Not enough rx queues to create a vNIC");
    }

    /// update the filter numbers in place so that queues are only removed when there's no failure.
    for i in 0..num_queues {
        let qid = nic.rx_queues[num_available_queues - 1 - i].id;
        let filter_num = nic.set_5_tuple_filter(None, Some(ip_addresses[i]), None, None, None, 7 /*highest priority*/, qid)?;
        nic.rx_queues[num_available_queues -1 - i].filter_num = Some(filter_num);
    }

    let rx_queues = nic.remove_rx_queues(num_queues)?;
    let tx_queues = nic.remove_tx_queues(num_queues)?;


    Ok(VirtualNic::new(
        rx_queues,
        0,
        tx_queues,
        0,
        nic.mac_address(),
        get_ixgbe_nic().ok_or("Ixgbe nic isn't initialized")?
    ))
}


impl PhysicalNic<IxgbeRxQueueRegisters, AdvancedRxDescriptor, IxgbeTxQueueRegisters, AdvancedTxDescriptor> for IxgbeNic {
    fn return_rx_queues(&mut self, mut queues: Vec<RxQueue<IxgbeRxQueueRegisters, AdvancedRxDescriptor>>) {
        for queue in &queues {
            self.disable_5_tuple_filter(queue.filter_num.unwrap())
        }

        while !queues.is_empty() {
            self.rx_queues.push(queues.pop().unwrap());
        }
    }

    fn return_tx_queues(&mut self, mut queues: Vec<TxQueue<IxgbeTxQueueRegisters, AdvancedTxDescriptor>>) {
        while !queues.is_empty() {
            self.tx_queues.push(queues.pop().unwrap());
        }
    }
}