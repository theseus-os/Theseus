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


use memory::allocate_pages;
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
use mellanox_ethernet::{CommandQueueEntry, InitializationSegment, CommandQueue, CommandOpcode, ManagePagesOpmod, QueryPagesOpmod};


pub const MLX_VEND:           u16 = 0x15B3;  // Vendor ID for Mellanox
pub const CONNECTX5_DEV:      u16 = 0x1019;  // Device ID for the ConnectX-5 NIC

/// Assuming one of these nics for now
static CONNECTX5_NIC: Once<MutexIrqSafe<ConnectX5Nic>> = Once::new();

/// Returns a reference to the E1000Nic wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_mlx5_nic() -> Option<&'static MutexIrqSafe<ConnectX5Nic>> {
    CONNECTX5_NIC.get()
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
        trace!("init segment size = {}", core::mem::size_of::<mellanox_ethernet::InitializationSegment>());
  

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        mlx5_pci_dev.pci_set_command_bus_master_bit();

        // memory mapped base address
        let mem_base = mlx5_pci_dev.determine_mem_base(0)?;
        trace!("mlx5 mem base = {}", mem_base);
        // map pages to the physical address given by mem_base as that is the intialization segment
        let mut init_segment = ConnectX5Nic::mapped_init_segment(mem_base)?;

        // init_segment.pf_reset();
        init_segment.print();
        
        // find number of entries in command queue and stride
        let cmdq_entries = init_segment.num_cmdq_entries() as usize;
        trace!("mlx5 cmdq entries = {}", cmdq_entries);
        let cmdq_stride = init_segment.cmdq_entry_stride() as usize;
        trace!("mlx5 cmdq stride = {}", cmdq_stride);
        
        if cmdq_stride != core::mem::size_of::<mellanox_ethernet::CommandQueueEntry>() {
            error!("Command Queue layout is no longer accurate due to invalid assumption.");
            return Err("Command Queue layout is no longer accurate due to invalid assumption.");
        }
        // create command queue
        let size_in_bytes_of_cmdq = cmdq_entries * cmdq_stride;
        trace!("total size in bytes of cmdq = {}", size_in_bytes_of_cmdq);
    
        // cmdp needs to be aligned, check??
        let (cmdq_mapped_pages, cmdq_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_cmdq, NIC_MAPPING_FLAGS)?;
        trace!("cmdq mem base = {}", cmdq_starting_phys_addr);
    
        // cast our physically-contiguous MappedPages into a slice of command queue entries
        let mut cmdq = CommandQueue::create(
            BoxRefMut::new(Box::new(cmdq_mapped_pages)).try_map_mut(|mp| mp.as_slice_mut::<CommandQueueEntry>(0, cmdq_entries))?,
            cmdq_entries
        )?;

        // write physical location of command queues to initialization segment
        init_segment.set_physical_address_of_cmdq(cmdq_starting_phys_addr)?;

        // Read initalizing field from initialization segment until it is cleared
        while init_segment.device_is_initializing() { trace!("device is initializing");}
        trace!("initializing field is cleared.");

        
        // Execute ENABLE_HCA command
        let cmdq_entry = cmdq.create_command(CommandOpcode::EnableHca, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry);
        trace!("EnableHCA: {:?}", status);

        // execute query ISSI
        let cmdq_entry = cmdq.create_command(CommandOpcode::QueryIssi, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry);
        let issi = cmdq.get_query_issi_command_output(cmdq_entry)?;
        trace!("SetISSI: {:?}, issi version :{}", status, issi);

        // execute set ISSI
        let cmdq_entry = cmdq.create_command(CommandOpcode::SetIssi, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry);
        trace!("SetISSI: {:?}", status);

        // Query pages for boot
        let cmdq_entry = cmdq.create_command(CommandOpcode::QueryPages, Some(QueryPagesOpmod::BootPages as u16), None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry);
        let num_boot_pages = cmdq.get_query_pages_command_output(cmdq_entry)?;
        trace!("Query pages status: {:?}, Boot pages: {:?}", status, num_boot_pages);

        // Allocate pages for boot
        let mut boot_mp = Vec::with_capacity(num_boot_pages as usize);
        let mut boot_pa = Vec::with_capacity(num_boot_pages as usize);
        for _ in 0..num_boot_pages {
            let (page, pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;
            boot_mp.push(page);
            boot_pa.push(pa);
        }
        let cmdq_entry = cmdq.create_command(CommandOpcode::ManagePages, Some(ManagePagesOpmod::AllocationSuccess as u16), Some(boot_pa))?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry);
        trace!("Manage pages boot status: {:?}", status);



        Ok(())


    }
    
    fn mapped_init_segment(mem_base: PhysicalAddress) -> Result<BoxRefMut<MappedPages, InitializationSegment>, &'static str> {
        let mp = allocate_memory(mem_base, core::mem::size_of::<InitializationSegment>())?;
        BoxRefMut::new(Box::new(mp)).try_map_mut(|mp| mp.as_type_mut::<InitializationSegment>(0))
    }
}