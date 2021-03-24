#![no_std]

#![allow(dead_code)] //  to suppress warnings for unused functions/methods
#![allow(safe_packed_borrows)] // temporary, just to suppress unsafe packed borrows 
#![feature(rustc_private)]
#![feature(abi_x86_interrupt)]

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate static_assertions;
extern crate volatile;
extern crate zerocopy;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate kernel_config;
extern crate memory;
extern crate pci; 
extern crate owning_ref;
extern crate interrupts;
extern crate pic;
extern crate x86_64;
extern crate mpmc;
extern crate network_interface_card;
extern crate apic;
extern crate intel_ethernet;
extern crate nic_buffers;
extern crate nic_queues;
extern crate nic_initialization;

pub mod test_e1000_driver;
mod regs;
use regs::*;

use spin::Once; 
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{PhysicalAddress, MappedPages};
use pci::{PciDevice, PCI_INTERRUPT_LINE, PciConfigSpaceAccessMechanism};
use kernel_config::memory::PAGE_SIZE;
use owning_ref::BoxRefMut;
use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};
use network_interface_card:: NetworkInterfaceCard;
use nic_initialization::{allocate_memory, init_rx_buf_pool, init_rx_queue, init_tx_queue};
use intel_ethernet::descriptors::{LegacyRxDescriptor, LegacyTxDescriptor};
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};

pub const INTEL_VEND:           u16 = 0x8086;  // Vendor ID for Intel 
pub const E1000_DEV:            u16 = 0x100E;  // Device ID for the e1000 Qemu, Bochs, and VirtualBox emmulated NICs

/// TODO: in the future, we should support multiple NICs all stored elsewhere,
/// e.g., on the PCI bus or somewhere else.
static E1000_NIC: Once<MutexIrqSafe<E1000Nic>> = Once::new();

/// Returns a reference to the E1000Nic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_e1000_nic() -> Option<&'static MutexIrqSafe<E1000Nic>> {
    E1000_NIC.try()
}

/// How many ReceiveBuffers are preallocated for this driver to use. 
const RX_BUFFER_POOL_SIZE: usize = 256; 
lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the E1000 NIC
    /// and temporarily given to higher layers in the networking stack.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
}


/// Struct representing a connectx-5 network interface card.
pub struct ConnectX5Nic {
    /// Type of BAR0
    bar_type: u8,
    /// MMIO Base Address
    mem_base: PhysicalAddress,
    ///interrupt number
    interrupt_num: u8,
    /// The actual MAC address burnt into the hardware of this E1000 NIC.
    mac_hardware: [u8; 6],
    /// The optional spoofed MAC address to use in place of `mac_hardware` when transmitting.  
    mac_spoofed: Option<[u8; 6]>,
    /// Receive queue with descriptors
    rx_queue: RxQueue<E1000RxQueueRegisters,LegacyRxDescriptor>,
    /// Transmit queue with descriptors
    tx_queue: TxQueue<E1000TxQueueRegisters,LegacyTxDescriptor>,     
    /// memory-mapped control registers
    regs: BoxRefMut<MappedPages, E1000Registers>,
    /// memory-mapped registers holding the MAC address
    mac_regs: BoxRefMut<MappedPages, E1000MacRegisters>
}


/// Functions that setup the NIC struct and handle the sending and receiving of packets.
impl ConnectX5Nic {
    /// Initializes the new E1000 network interface card that is connected as the given PciDevice.
    pub fn init(mlx5_pci_dev: &PciDevice) -> Result<&'static MutexIrqSafe<ConnectX5Nic>, &'static str> {
        use pic::PIC_MASTER_OFFSET;

        let bar0 = mlx5_pci_dev.bars[0];
        // the initialization segment is located at offset 0 of the BAR
        // Map pages of the initialization segment


        // create command queues

        // write physical location of command queues to initialization segment

        // Read iniitalizing field from initialization segment until it is cleared

        // Execute ENABLE_HCA command

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        e1000_pci_dev.pci_set_command_bus_master_bit();


    }
    
}