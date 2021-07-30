//! A mlx5 driver for a ConnectX-5 100GbE Network Interface Card.
//! 
//! Currently we only support reading the device PCI space, mapping the initialization segment,
//! and setting up a command queue to pass commands to the NIC.
//! 
//! All information is taken from the Mellanox Adapters Programmer’s Reference Manual (PRM) Rev 0.54,
//! unless otherwise specified. 

#![no_std]

#[macro_use] extern crate log;
extern crate alloc;
extern crate spin;
extern crate irq_safety;
extern crate memory;
extern crate pci; 
extern crate owning_ref;
extern crate nic_initialization;
extern crate mlx_ethernet;
extern crate kernel_config;

use spin::Once; 
use alloc::{
    vec::Vec,
    boxed::Box
};
use irq_safety::MutexIrqSafe;
use memory::{PhysicalAddress, MappedPages, create_contiguous_mapping};
use pci::PciDevice;
use owning_ref::BoxRefMut;
use nic_initialization::{NIC_MAPPING_FLAGS, allocate_memory};
use mlx_ethernet::{
    InitializationSegment, 
    command_queue::{CommandQueueEntry, CommandQueue, CommandOpcode, ManagePagesOpMod, QueryPagesOpMod}
};
use kernel_config::memory::PAGE_SIZE;

/// Vendor ID for Mellanox
pub const MLX_VEND:           u16 = 0x15B3;
/// Device ID for the ConnectX-5 NIC
pub const CONNECTX5_DEV:      u16 = 0x1019;

/// The singleton connectx-5 NIC.
/// TODO: Allow for multiple NICs
static CONNECTX5_NIC: Once<MutexIrqSafe<ConnectX5Nic>> = Once::new();

/// Returns a reference to the NIC wrapped in a MutexIrqSafe,
/// if it exists and has been initialized.
pub fn get_mlx5_nic() -> Option<&'static MutexIrqSafe<ConnectX5Nic>> {
    CONNECTX5_NIC.get()
}

/// Struct representing a ConnectX-5 network interface card.
#[allow(dead_code)]
pub struct ConnectX5Nic {
    /// Initialization segment base address
    mem_base: PhysicalAddress,
    /// Initialization Segment
    init_segment: BoxRefMut<MappedPages, InitializationSegment>,
    /// Command Queue
    command_queue: CommandQueue,
    /// Boot pages passed to the NIC. Once transferred, they should not be accessed by the driver.
    boot_pages: Vec<MappedPages>,
    /// Init pages passed to the NIC. Once transferred, they should not be accessed by the driver.
    init_pages: Vec<MappedPages>,
}


/// Functions that setup the NIC struct.
impl ConnectX5Nic {

    /// Initializes the new ConnectX-5 network interface card that is connected as the given PciDevice.
    /// (steps taken from the PRM, Section 7.2: HCA Driver Start-up)
    pub fn init(mlx5_pci_dev: &PciDevice) -> Result<&'static MutexIrqSafe<ConnectX5Nic>, &'static str> {

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        mlx5_pci_dev.pci_set_command_bus_master_bit();

        // retrieve the memory-mapped base address of the initialization segment
        let mem_base = mlx5_pci_dev.determine_mem_base(0)?;
        trace!("mlx5 mem base = {}", mem_base);

        // map pages to the physical address given by mem_base as that is the intialization segment
        let mut init_segment = ConnectX5Nic::map_init_segment(mem_base)?;

        trace!("{:?}", init_segment);
        
        // find number of entries in command queue and stride
        let num_cmdq_entries = init_segment.num_cmdq_entries() as usize;
        trace!("mlx5 cmdq entries = {}", num_cmdq_entries);

        // find command queue entry stride, the number of bytes between the start of two adjacent entries.
        let cmdq_stride = init_segment.cmdq_entry_stride() as usize;
        trace!("mlx5 cmdq stride = {}", cmdq_stride);
        
        // We assume that the stride is equal to the size of the entry.
        if cmdq_stride != core::mem::size_of::<CommandQueueEntry>() {
            error!("Command Queue layout is no longer accurate due to invalid assumption.");
            return Err("Command Queue layout is no longer accurate due to invalid assumption.");
        }

        // create command queue
        let size_in_bytes_of_cmdq = num_cmdq_entries * cmdq_stride;
        trace!("total size in bytes of cmdq = {}", size_in_bytes_of_cmdq);
    
        // allocate mapped pages for the command queue
        let (cmdq_mapped_pages, cmdq_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_cmdq, NIC_MAPPING_FLAGS)?;
        trace!("cmdq mem base = {}", cmdq_starting_phys_addr);
    
        // cast our physically-contiguous MappedPages into a slice of command queue entries
        let mut cmdq = CommandQueue::create(
            BoxRefMut::new(Box::new(cmdq_mapped_pages)).try_map_mut(|mp| mp.as_slice_mut::<CommandQueueEntry>(0, num_cmdq_entries))?,
            num_cmdq_entries
        )?;

        // write physical location of command queue to initialization segment
        init_segment.set_physical_address_of_cmdq(cmdq_starting_phys_addr)?;

        // Read initalizing field from initialization segment until it is cleared
        while init_segment.device_is_initializing() { trace!("device is initializing"); }
        trace!("initializing field is cleared.");

        // Execute ENABLE_HCA command
        let cmdq_entry = cmdq.create_command(CommandOpcode::EnableHca, None, None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        trace!("EnableHCA: {:?}", status);

        // execute QUERY_ISSI
        let cmdq_entry = cmdq.create_command(CommandOpcode::QueryIssi, None, None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        let (current_issi, available_issi) = cmdq.get_query_issi_command_output(cmdq_entry)?;
        trace!("QueryISSI: {:?}, issi version :{}, available: {:#X}", status, current_issi, available_issi);

        // execute SET_ISSI
        let cmdq_entry = cmdq.create_command(CommandOpcode::SetIssi, None, None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        trace!("SetISSI: {:?}", status);

        // Query pages for boot
        let cmdq_entry = cmdq.create_command(CommandOpcode::QueryPages, Some(QueryPagesOpMod::BootPages as u16), None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        let num_boot_pages = cmdq.get_query_pages_command_output(cmdq_entry)?;
        trace!("Query pages status: {:?}, Boot pages: {:?}", status, num_boot_pages);

        // Allocate pages for boot
        let mut boot_mp = Vec::with_capacity(num_boot_pages as usize);
        let mut boot_pa = Vec::with_capacity(num_boot_pages as usize);
        for _ in 0..num_boot_pages {
            let (page, pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            boot_mp.push(page);
            boot_pa.push(pa);
        }

        // execute MANAGE_PAGES command to transfer boot pages to device
        let cmdq_entry = cmdq.create_command(
            CommandOpcode::ManagePages, 
            Some(ManagePagesOpMod::AllocationSuccess as u16), 
            Some(boot_pa), 
            None, 
            None
        )?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        trace!("Manage pages boot status: {:?}", status);

        // Query pages for init
        let cmdq_entry = cmdq.create_command(CommandOpcode::QueryPages, Some(QueryPagesOpMod::InitPages as u16), None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        let num_init_pages = cmdq.get_query_pages_command_output(cmdq_entry)?;
        trace!("Query pages status: {:?}, init pages: {:?}", status, num_init_pages);

        let mut init_mp = Vec::with_capacity(num_init_pages as usize);
        if num_init_pages != 0 {
            // Allocate pages for init
            let mut init_pa = Vec::with_capacity(num_init_pages as usize);
            for _ in 0..num_init_pages {
                let (page, pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
                init_mp.push(page);
                init_pa.push(pa);
            }

            // execute MANAGE_PAGES command to transfer init pages to device
            let cmdq_entry = cmdq.create_command(
                CommandOpcode::ManagePages, 
                Some(ManagePagesOpMod::AllocationSuccess as u16), 
                Some(init_pa), 
                None, 
                None
            )?;
            init_segment.post_command(cmdq_entry);
            let status = cmdq.wait_for_command_completion(cmdq_entry)?;
            trace!("Manage pages init status: {:?}", status);
        }

        // execute INIT_HCA
        let cmdq_entry = cmdq.create_command(CommandOpcode::InitHca, None, None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        trace!("Init HCA status: {:?}", status);

        // execute ALLOC_UAR
        let cmdq_entry = cmdq.create_command(CommandOpcode::AllocUar, None, None, None, None)?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        trace!("UAR status: {:?}", status);        

        let uar = cmdq.get_uar(cmdq_entry)?;
        trace!("UAR status: {:?}, UAR: {}", status, uar);        

        // execute CREATE_EQ for page request event
        // Allocate pages for EQ
        let num_eq_pages = 1;
        let mut eq_mp = Vec::with_capacity(num_eq_pages as usize);
        let mut eq_pa = Vec::with_capacity(num_eq_pages as usize);
        for _ in 0..num_eq_pages {
            let (page, pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;
            eq_mp.push(page);
            eq_pa.push(pa);
        }
        let cmdq_entry = cmdq.create_command(CommandOpcode::CreateEq, None, Some(eq_pa), Some(uar), Some(7))?;
        init_segment.post_command(cmdq_entry);
        let status = cmdq.wait_for_command_completion(cmdq_entry)?;
        let eq_number = cmdq.get_eq_number(cmdq_entry)?;
        trace!("Create EQ status: {:?}, number: {}", status, eq_number);

        let mlx5_nic = ConnectX5Nic {
            mem_base: mem_base,
            init_segment: init_segment,
            command_queue: cmdq, 
            boot_pages: boot_mp,
            init_pages: init_mp
        };
        
        let nic_ref = CONNECTX5_NIC.call_once(|| MutexIrqSafe::new(mlx5_nic));
        Ok(nic_ref)
    }
    
    /// Returns the memory-mapped initialization segment of the NIC
    fn map_init_segment(mem_base: PhysicalAddress) -> Result<BoxRefMut<MappedPages, InitializationSegment>, &'static str> {
        let mp = allocate_memory(mem_base, core::mem::size_of::<InitializationSegment>())?;
        BoxRefMut::new(Box::new(mp)).try_map_mut(|mp| mp.as_type_mut::<InitializationSegment>(0))
    }
}
