use super::{IxgbeNic, get_ixgbe_nic, IxgbeRxQueueRegisters, IxgbeTxQueueRegisters};
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

    fn power_down(&mut self) {
        error!("should power down now");
    }
}

pub fn create_virtual_nic(num_queues: usize, ip_addresses: Vec<[u8;4]>, filter_protocol: u8) -> Result<VirtualNic<IxgbeRxQueueRegisters, AdvancedRxDescriptor, IxgbeTxQueueRegisters, AdvancedTxDescriptor>, &'static str> {
    if num_queues == 0 {return Err("need to request >0 number of queues");}
    if num_queues != ip_addresses.len() { return Err("The number of queues requested does not match the number of ip_addresses");}
    
    let mut nic = get_ixgbe_nic().ok_or("Ixgbe nic not initialized")?.lock();
    let num_available_queues = nic.rx_queues.len();
    if num_available_queues - num_queues <=1  {
        return Err("Not enough rx queues for the NIC to remove any");
    }

    /// update the filter numbers in place so that queues are only removed when there's no failure.
    for i in 0..num_queues {
        let qid = nic.rx_queues[num_available_queues - 1 - i].id;
        let filter_num = nic.set_ip_dest_address_filter(ip_addresses[i], filter_protocol, 7 /*highest priority*/, qid)?;
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
        nic.wakelock.clone(),
        get_ixgbe_nic().ok_or("Ixgbe nic isn't initialized")?
    ))
}
