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
extern crate mellanox_ethernet;


use spin::Once; 
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use alloc::boxed::Box;
use memory::{PhysicalAddress, MappedPages, create_contiguous_mapping};
use pci::{PciDevice, PCI_INTERRUPT_LINE, PciConfigSpaceAccessMechanism};
use kernel_config::memory::PAGE_SIZE;
use owning_ref::BoxRefMut;
use interrupts::{eoi,register_interrupt};
use x86_64::structures::idt::{ExceptionStackFrame};
use network_interface_card:: NetworkInterfaceCard;
use nic_initialization::{NIC_MAPPING_FLAGS, allocate_memory, init_rx_buf_pool, init_rx_queue, init_tx_queue};
use intel_ethernet::descriptors::{LegacyRxDescriptor, LegacyTxDescriptor};
use nic_buffers::{TransmitBuffer, ReceiveBuffer, ReceivedFrame};
use nic_queues::{RxQueue, TxQueue, RxQueueRegisters, TxQueueRegisters};
use mellanox_ethernet::{CommandQueueEntry, InitializationSegment};


pub const MLX_VEND:           u16 = 0x15B3;  // Vendor ID for Mellanox
pub const CONNECTX5_DEV:      u16 = 0x1017;  // Device ID for the ConnectX-5 NIC

/// Assuming one of these nics for now
static CONNECTX5_NIC: Once<MutexIrqSafe<ConnectX5Nic>> = Once::new();

/// Returns a reference to the E1000Nic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_mlx5_nic() -> Option<&'static MutexIrqSafe<ConnectX5Nic>> {
    CONNECTX5_NIC.try()
}

// /// How many ReceiveBuffers are preallocated for this driver to use. 
// const RX_BUFFER_POOL_SIZE: usize = 256; 
// lazy_static! {
//     /// The pool of pre-allocated receive buffers that are used by the E1000 NIC
//     /// and temporarily given to higher layers in the networking stack.
//     static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE);
// }


/// Struct representing a connectx-5 network interface card.
pub struct ConnectX5Nic {
    /// Type of BAR0
    bar_type: u8,
    /// MMIO Base Address
    mem_base: PhysicalAddress,
}


/// Functions that setup the NIC struct and handle the sending and receiving of packets.
impl ConnectX5Nic {
    /// Initializes the new E1000 network interface card that is connected as the given PciDevice.
    pub fn init(mlx5_pci_dev: &PciDevice) -> Result<(), &'static str> { //Result<&'static MutexIrqSafe<ConnectX5Nic>, &'static str> {
        use pic::PIC_MASTER_OFFSET;

        let bar0 = mlx5_pci_dev.bars[0];
        // Determine the access mechanism from the base address register's bit 0
        let bar_type = (bar0 as u8) & 0x1;    

        // If the base address is not memory mapped then exit
        if bar_type == PciConfigSpaceAccessMechanism::IoPort as u8 {
            error!("mlx5::init(): BAR0 is of I/O type");
            return Err("mlx5::init(): BAR0 is of I/O type")
        }
  
        // memory mapped base address
        let mem_base = mlx5_pci_dev.determine_mem_base(0)?;

        // map pages to the physical address given by mem_base as that is the intialization segment
        let mut init_segment = ConnectX5Nic::mapped_init_segment(mem_base)?;

        // find number of entries in command queue and stride
        let max_cmdq_entries = init_segment.num_cmdq_entries() as usize;
        let cmdq_stride = init_segment.cmdq_entry_stride();

        // create command queue
        let size_in_bytes_of_cmdq = max_cmdq_entries * core::mem::size_of::<CommandQueueEntry>();
    
        // cmdp needs to be aligned, check??
        let (cmdq_mapped_pages, cmdq_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_cmdq, NIC_MAPPING_FLAGS)?;
    
        // cast our physically-contiguous MappedPages into a slice of command queue entries
        let mut cmdq = BoxRefMut::new(Box::new(cmdq_mapped_pages)).try_map_mut(|mp| mp.as_slice_mut::<CommandQueueEntry>(0, max_cmdq_entries))?;

        // write physical location of command queues to initialization segment
        init_segment.set_physical_address_of_cmdq(cmdq_starting_phys_addr);

        // Read initalizing field from initialization segment until it is cleared
        while init_segment.device_is_initializing() {}
        
        // Execute ENABLE_HCA command

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        mlx5_pci_dev.pci_set_command_bus_master_bit();

        Ok(())


    }
    
    fn mapped_init_segment(mem_base: PhysicalAddress) -> Result<BoxRefMut<MappedPages, InitializationSegment>, &'static str> {
        let mp = allocate_memory(mem_base, core::mem::size_of::<InitializationSegment>())?;
        BoxRefMut::new(Box::new(mp)).try_map_mut(|mp| mp.as_type_mut::<InitializationSegment>(0))
    }
}