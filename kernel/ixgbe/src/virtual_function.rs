//! Interface for an application to request a `VirtualNIC` from the ixgbe device,
//! and implementation of the `PhysicalNic` trait for the ixgbe device.
//!
//! The `PhysicalNic` trait is required for returning virtual NIC resources to the ixgbe device when dropped.

use super::{get_ixgbe_nic, IxgbeNic, IxgbeRxQueueRegisters, IxgbeTxQueueRegisters};
use alloc::vec::Vec;
use intel_ethernet::descriptors::{AdvancedRxDescriptor, AdvancedTxDescriptor};
use network_interface_card::NetworkInterfaceCard;
use nic_queues::{RxQueue, TxQueue};
use pci::PciLocation;
use physical_nic::PhysicalNic;
use virtual_nic::VirtualNic;

/// Create a virtual NIC from the ixgbe device.
///
/// # Arguments
/// * `nic_id`: the ixgbe NIC we will take receive and transmit queue from.
/// * `ip_addresses`: set of ip addresses that will be assigned to the allocated receive queues.
///    The number of ip addresses is equal to the number of queue pairs that will be assigned to the vNIC.
///    Packets with the destination ip addresses specified here will be routed to the vNIC's queues.
/// * `default_rx_queue`: the queue that will be polled for packets when no other queue is specified.
/// * `default_tx_queue`: the queue that packets will be sent on when no other queue is specified.
pub fn create_virtual_nic(
    nic_id: PciLocation,
    ip_addresses: Vec<[u8; 4]>,
    default_rx_queue: usize,
    default_tx_queue: usize,
) -> Result<
    VirtualNic<
        IxgbeRxQueueRegisters,
        AdvancedRxDescriptor,
        IxgbeTxQueueRegisters,
        AdvancedTxDescriptor,
    >,
    &'static str,
> {
    let num_queues = ip_addresses.len();
    if num_queues == 0 {
        return Err("need to request >0 number of queues");
    }
    if (default_rx_queue >= num_queues) || (default_tx_queue >= num_queues) {
        return Err("default queue value is out of bounds");
    }

    let (rx_queues, tx_queues, mac_address) = {
        let mut nic = get_ixgbe_nic(nic_id)?.lock();
        // Allocate queues from the physical NIC
        let mut rx_queues = nic.take_rx_queues_from_physical_nic(num_queues)?;
        let tx_queues = nic.take_tx_queues_from_physical_nic(num_queues)?;
        // Set up the filters so that packets sent to `ip_addresses` are forwarded to these queues.
        for (queue, ip_address) in rx_queues.iter_mut().zip(ip_addresses.iter()) {
            let filter_num = nic.set_5_tuple_filter(
                None,
                Some(*ip_address),
                None,
                None,
                None,
                7, /*highest priority*/
                queue.id,
            )?;
            queue.filter_num = Some(filter_num);
        }
        (rx_queues, tx_queues, nic.mac_address())
    };

    VirtualNic::new(
        rx_queues,
        default_rx_queue,
        tx_queues,
        default_tx_queue,
        mac_address,
        get_ixgbe_nic(nic_id)?,
    )
}

impl
    PhysicalNic<
        IxgbeRxQueueRegisters,
        AdvancedRxDescriptor,
        IxgbeTxQueueRegisters,
        AdvancedTxDescriptor,
    > for IxgbeNic
{
    fn return_rx_queues(
        &mut self,
        mut queues: Vec<RxQueue<IxgbeRxQueueRegisters, AdvancedRxDescriptor>>,
    ) {
        // disable filters for all queues
        for queue in &queues {
            self.disable_5_tuple_filter(queue.filter_num.unwrap()); // safe to unwrap here because creation of virtualNIC ensures a filter
        }
        // return queues to physical nic
        self.rx_queues.append(&mut queues);
    }

    fn return_tx_queues(
        &mut self,
        mut queues: Vec<TxQueue<IxgbeTxQueueRegisters, AdvancedTxDescriptor>>,
    ) {
        // return queues to physical nic
        self.tx_queues.append(&mut queues);
    }
}
