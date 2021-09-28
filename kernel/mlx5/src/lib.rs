//! A mlx5 driver for a ConnectX-5 100GbE Network Interface Card.
//! 
//! Currently we only support reading the device PCI space, mapping the initialization segment,
//! and setting up a command queue to pass commands to the NIC.
//! 
//! All information is taken from the Mellanox Adapters Programmer’s Reference Manual (PRM) Rev 0.54,
//! unless otherwise specified. 

#![no_std]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
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
use mlx_ethernet::{InitializationSegment, command_queue::{CommandBuilder, CommandOpcode, CommandQueue, CommandQueueEntry, ManagePagesOpMod, QueryHcaCapCurrentOpMod, QueryHcaCapMaxOpMod, QueryPagesOpMod, AccessRegisterOpMod}, 
    completion_queue::CompletionQueue, 
    event_queue::EventQueue, 
    send_queue::SendQueue,
    receive_queue::ReceiveQueue
};
use kernel_config::memory::PAGE_SIZE;
use core::sync::atomic::fence;
use core::sync::atomic::Ordering;

/// Vendor ID for Mellanox
pub const MLX_VEND:           u16 = 0x15B3;
/// Device ID for the ConnectX-5 NIC
pub const CONNECTX5_DEV:      u16 = 0x1019; //7;

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
    event_queue: EventQueue,
    completion_queue: CompletionQueue,
    send_queue: SendQueue
}


/// Functions that setup the NIC struct.
impl ConnectX5Nic {

    /// Initializes the new ConnectX-5 network interface card that is connected as the given PciDevice.
    /// (steps taken from the PRM, Section 7.2: HCA Driver Start-up)
    pub fn init(mlx5_pci_dev: &PciDevice) -> Result</*&'static MutexIrqSafe<ConnectX5Nic>*/ (), &'static str> {
        let sq_size = 1024;
        let rq_size = 1024;

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        mlx5_pci_dev.pci_set_command_bus_master_bit();

        // retrieve the memory-mapped base address of the initialization segment
        let mem_base = mlx5_pci_dev.determine_mem_base(0)?;
        trace!("mlx5 mem base = {}", mem_base);

        let mem_size = mlx5_pci_dev.determine_mem_size(0);
        trace!("mlx5 mem size = {}", mem_size);

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
        let init_cmd = cmdq.create_command(CommandBuilder::new(CommandOpcode::EnableHca))?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        trace!("EnableHCA: {:?}", cmdq.get_command_status(completed_cmd)?);

        // // execute QUERY_ISSI
        // let init_cmd = cmdq.create_command( CommandBuilder::new(CommandOpcode::QueryIssi))?;
        // let posted_cmd = init_segment.post_command(init_cmd);
        // let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        // let (current_issi, available_issi, status) = cmdq.get_query_issi_command_output(completed_cmd)?;
        // trace!("QueryISSI: {:?}, issi version :{}, available: {:#X}", status, current_issi, available_issi);
        let available_issi = 0x2;

        // execute SET_ISSI
        const ISSI_VERSION_1: u8 = 0x2;
        if available_issi & ISSI_VERSION_1 == ISSI_VERSION_1 {
            let init_cmd = cmdq.create_command( CommandBuilder::new(CommandOpcode::SetIssi))?;
            let posted_cmd = init_segment.post_command(init_cmd);
            let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
            trace!("SetISSI: {:?}", cmdq.get_command_status(completed_cmd)?);
        } else {
            return Err("ISSI indicated by PRM is not supported");
        }

        // Query pages for boot
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryPages).opmod(QueryPagesOpMod::BootPages as u16)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (num_boot_pages, status) = cmdq.get_query_pages_command_output(completed_cmd)?;
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
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::ManagePages)
                .opmod(ManagePagesOpMod::AllocationSuccess as u16)
                .allocated_pages(boot_pa)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        trace!("Manage pages boot status: {:?}", cmdq.get_command_status(completed_cmd)?);

        // Query HCA capabilities
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryHcaCap)
                .opmod(QueryHcaCapCurrentOpMod::GeneralDeviceCapabilities as u16), 
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (current_capabilities, status) = cmdq.get_device_capabilities(completed_cmd)?;
        trace!("Query HCA cap status:{:?}, current_capabilities: {:?}", status, current_capabilities);

        // Query HCA capabilities
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryHcaCap)
                .opmod(QueryHcaCapMaxOpMod::GeneralDeviceCapabilities as u16), 
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (max_capabilities, status) = cmdq.get_device_capabilities(completed_cmd)?;
        trace!("Query HCA cap status:{:?}, max_capabilities: {:?}", status, max_capabilities);

        // Query pages for init
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryPages)
                .opmod(QueryPagesOpMod::InitPages as u16)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (num_init_pages, status) = cmdq.get_query_pages_command_output(completed_cmd)?;
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
            let init_cmd = cmdq.create_command(
                CommandBuilder::new(CommandOpcode::ManagePages)
                    .opmod(ManagePagesOpMod::AllocationSuccess as u16)
                    .allocated_pages(init_pa)
            )?;
            let posted_cmd = init_segment.post_command(init_cmd);
            let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
            trace!("Manage pages init status: {:?}", cmdq.get_command_status(completed_cmd)?);
        }

        // execute INIT_HCA
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::InitHca) 
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        trace!("Init HCA status: {:?}", cmdq.get_command_status(completed_cmd)?);

        // Query regular pages
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryPages)
                .opmod(QueryPagesOpMod::RegularPages as u16)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (num_reg_pages, status) = cmdq.get_query_pages_command_output(completed_cmd)?;
        trace!("Query pages status: {:?}, regular pages: {:?}", status, num_reg_pages);

        let mut reg_mp = Vec::with_capacity(num_reg_pages as usize);
        if num_reg_pages != 0 {
            // Allocate pages for init
            let mut reg_pa = Vec::with_capacity(num_reg_pages as usize);
            for _ in 0..num_reg_pages {
                let (page, pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
                reg_mp.push(page);
                reg_pa.push(pa);
            }

            // execute MANAGE_PAGES command to transfer init pages to device
            let init_cmd = cmdq.create_command(
                CommandBuilder::new(CommandOpcode::ManagePages)
                    .opmod(ManagePagesOpMod::AllocationSuccess as u16)
                    .allocated_pages(reg_pa)
            )?;
            let posted_cmd = init_segment.post_command(init_cmd);
            let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
            trace!("Manage pages reg status: {:?}", cmdq.get_command_status(completed_cmd)?);
        }

        // // Set driver version 
        // let init_cmd = cmdq.create_command(
        //     CommandBuilder::new(CommandOpcode::SetDriverVersion)
        // )?;
        // let posted_cmd = init_segment.post_command(init_cmd);
        // let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        // trace!("Set Driver Version: {:?}", cmdq.get_command_status(completed_cmd)?);
        
        // execute QUERY_NIC_VPORT_CONTEXT
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryNicVportContext)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (mac, status) = cmdq.get_vport_mac_address(completed_cmd)?;
        trace!("Query Nic Vport context status: {:?}, mac address: {:#X?}", status, mac);

        // execute QUERY_VPORT_STATE
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryVportState)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (tx_speed, admin_state, state, status) = cmdq.get_vport_state(completed_cmd)?;
        trace!("Query Vport State status: {:?}, tx_speed: {:#X}, admin_state:{:#X}, state: {:#X}", status, tx_speed, admin_state, state); 

        // execute ACCESS_REGISTER to set the mtu
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::AccessRegister)
                .opmod(AccessRegisterOpMod::Write as u16)
                .mtu(9000)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let status = cmdq.get_command_status(completed_cmd)?;
        trace!("Access register status: {:?}", status); 
    
        // execute modify nic vport context to set the mtu
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::ModifyNicVportContext)
                .mtu(9000)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let status = cmdq.get_command_status(completed_cmd)?;
        trace!("modify nic vport status: {:?}", status); 

        // execute ALLOC_UAR
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::AllocUar)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (uar, status) = cmdq.get_uar(completed_cmd)?;
        trace!("UAR status: {:?}, UAR: {}", status, uar);        

        // execute CREATE_EQ for page request event
        // Allocate pages for EQ
        let num_eq_pages = 2;
        let mut eq_mp = Vec::with_capacity(num_eq_pages as usize);
        let mut eq_pa = Vec::with_capacity(num_eq_pages as usize);
        for _ in 0..num_eq_pages {
            let (page, pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;
            eq_mp.push(page);
            eq_pa.push(pa);
        }
        // set the ownership bit of the EQE to HW owned
        let mut event_queue = EventQueue::create(eq_mp)?;
        event_queue.init();

        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::CreateEq)
                .allocated_pages(eq_pa)
                .uar(uar)
                .log_queue_size(7)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (eq_number, status) = cmdq.get_eq_number(completed_cmd)?;
        trace!("Create EQ status: {:?}, number: {}", status, eq_number); 

        #[cfg(mlx_logger)]
        {
            event_queue.dump()
        }

        // execute ALLOC_PD
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::AllocPd)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (pd, status) = cmdq.get_protection_domain(completed_cmd)?;
        trace!("Alloc PD status: {:?}, protection domain num: {}", status, pd);

        // execute ALLOC_TRANSPORT_DOMAIN
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::AllocTransportDomain)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (td, status) = cmdq.get_transport_domain(completed_cmd)?;
        trace!("Alloc TD status: {:?}, transport domain num: {}", status, td);

        // execute QUERY_SPECIAL_CONTEXTS
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QuerySpecialContexts)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (rlkey, status) = cmdq.get_reserved_lkey(completed_cmd)?;
        trace!("Query Special Contexts status: {:?}, rlkey: {}", status, rlkey);
        
        // execute CREATE_CQ for SQ
        // Allocate pages for CQ
        let size_cq = 1; 
        let (cq_mp, cq_pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;

        // Allocate page for doorbell
        let (db_page, db_pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;
        
        // Initialize the CQ
        let mut completion_queue = CompletionQueue::create(cq_mp, size_cq, db_page)?;
        completion_queue.init();

        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::CreateCq) 
                .allocated_pages(vec!(cq_pa))
                .uar(uar)
                .log_queue_size(0)
                .eqn(eq_number)
                .db_page(db_pa)
                .collapsed_cq()
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (cq_number_s, status) = cmdq.get_cq_number(completed_cmd)?;
        trace!("Create CQ status: {:?}, number: {}", status, cq_number_s);

        #[cfg(mlx_logger)]
        {
            completion_queue.dump()
        }

        // execute CREATE_CQ for RQ
        // Allocate pages for CQ
        let size_cq = rq_size; 
        let (cq_mp, cq_pa) = create_contiguous_mapping(size_cq * 64, NIC_MAPPING_FLAGS)?;
        // Allocate page for doorbell
        let (db_page, db_pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;
        
        // Initialize the CQ
        let mut completion_queue_r = CompletionQueue::create(cq_mp, size_cq, db_page)?;
        completion_queue_r.init();

        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::CreateCq) 
                .allocated_pages(vec!(cq_pa))
                .uar(uar)
                .log_queue_size(10)
                .eqn(eq_number)
                .db_page(db_pa)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (cq_number_r, status) = cmdq.get_cq_number(completed_cmd)?;
        trace!("Create CQ status: {:?}, number: {}", status, cq_number_r);

        // execute CREATE_TIS
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::CreateTis)
                .td(td),
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (tisn, status) = cmdq.get_tis_context_number(completed_cmd)?;
        trace!("Create TIS status: {:?}, tisn: {}", status, tisn);

        // Allocate pages for RQ and SQ
        let sq_size_in_bytes = sq_size * 64;
        
        let rq_size_in_bytes = rq_size * 64;
        
        let (rq_mp, rq_pa) = create_contiguous_mapping(rq_size_in_bytes, NIC_MAPPING_FLAGS)?;
        let sq_pa = PhysicalAddress::new(rq_pa.value() + rq_size_in_bytes).ok_or("Could not create starting address for SQ")?; 
        let sq_mp = allocate_memory(sq_pa, sq_size_in_bytes)?;
        
        // Allocate page for SQ/RQ doorbell
        let (db_page, db_pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;

        // Allocate page for UAR
        let uar_mem_base = mem_base.value() + ((uar as usize) * 4096);
        let uar_page = allocate_memory(PhysicalAddress::new(uar_mem_base).ok_or("Could not create starting address for uar")?, 4096)?;
        
        debug!("mmio: {:#x}, uar: {:#x}", mem_base.value(), uar_mem_base);
        // Create the SQ
        let mut send_queue = SendQueue::create(sq_mp, db_page, uar_page, sq_size)?;

        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::CreateSq) 
                .allocated_pages(vec!(sq_pa)) 
                .uar(uar) 
                .log_queue_size(10)
                .db_page(db_pa)
                .cqn(cq_number_s) 
                .tisn(tisn) 
                .pd(pd)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (sq_number, status) = cmdq.get_send_queue_number(completed_cmd)?;
        trace!("Create SQ status: {:?}, number: {}", status, sq_number);

        #[cfg(mlx_logger)]
        {
            send_queue.dump()
        }

        // Create the RQ
        let mut receive_queue = ReceiveQueue::create(rq_mp)?;

        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::CreateRq) 
                .allocated_pages(vec!(rq_pa)) 
                .log_queue_size(10)
                .db_page(db_pa)
                .cqn(cq_number_r) 
                .pd(pd)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (rq_number, status) = cmdq.get_receive_queue_number(completed_cmd)?;
        trace!("Create RQ status: {:?}, number: {}", status, rq_number);

        // MODIFY_SQ
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::ModifySq) 
                .cqn(cq_number_s) 
                .tisn(tisn) 
                .sqn(sq_number)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        trace!("Modify SQ status: {:?}", cmdq.get_command_status(completed_cmd)?);

        // MODIFY_RQ
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::ModifyRq) 
                .rqn(rq_number)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        trace!("Modify RQ status: {:?}", cmdq.get_command_status(completed_cmd)?);

        // QUERY_SQ
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QuerySq)
                .sqn(sq_number)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (state, status) = cmdq.get_sq_state(completed_cmd)?;
        trace!("Query SQ status: {:?}, state: {}", status, state);

        // execute QUERY_VPORT_STATE
        let init_cmd = cmdq.create_command(
            CommandBuilder::new(CommandOpcode::QueryVportState)
        )?;
        let posted_cmd = init_segment.post_command(init_cmd);
        let completed_cmd = cmdq.wait_for_command_completion(posted_cmd);
        let (tx_speed, admin_state, state, status) = cmdq.get_vport_state(completed_cmd)?;
        trace!("Query Vport State status: {:?}, tx_speed: {:#X}, admin_state:{:#X}, state: {:#X}", status, tx_speed, admin_state, state); 

        // let (mut packet, pa) = create_contiguous_mapping(4096, NIC_MAPPING_FLAGS)?;
        // let buffer: &mut [u8] = packet.as_slice_mut(0, 298)?;
        
        // let dhcp_packet: [u8; 298] = [ 
        //     0x01, 0x2c, 0xa8, 0x36, 0x00, 0x00, 0xfa, 0x11, 0x17, 0x8b, 0x00, 0x00, 0x00, 0x00,
        // 0xff, 0xff, 0xff, 0xff, 0x00, 0x44, 0x00, 0x43, 0x01, 0x18, 0x59, 0x1f, 0x01, 0x01, 0x06,
        // 0x00, 0x00, 0x00, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f, 0xc6, 0x9c, 0x89,
        // 0x4c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x63, 0x82, 0x53, 0x63, 0x35, 0x01, 0x01,
        // 0x3d, 0x07, 0x01, 0x00, 0x1f, 0xc6, 0x9c, 0x89, 0x4c, 0x32, 0x04, 0x00, 0x00, 0x00, 0x00,
        // 0x37, 0x04, 0x01, 0x03, 0x06, 0x2a, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        // ];
        
        // buffer.copy_from_slice(&dhcp_packet);

        // send_queue.send(sq_number, tisn, rlkey, pa)?;
        send_queue.nop(sq_number, tisn, rlkey)?;
        while completion_queue.hw_owned(0) {}
        // completion_queue.check_packet_transmission();

        // let mlx5_nic = ConnectX5Nic {
        //     mem_base: mem_base,
        //     init_segment: init_segment,
        //     command_queue: cmdq, 
        //     boot_pages: boot_mp,
        //     init_pages: init_mp,
        //     event_queue: event_queue,
        //     completion_queue: completion_queue,
        //     send_queue: send_queue
        // };
        
        // let nic_ref = CONNECTX5_NIC.call_once(|| MutexIrqSafe::new(mlx5_nic));
        // Ok(nic_ref)

        Ok(())
    }
    
    /// Returns the memory-mapped initialization segment of the NIC
    fn map_init_segment(mem_base: PhysicalAddress) -> Result<BoxRefMut<MappedPages, InitializationSegment>, &'static str> {
        let mp = allocate_memory(mem_base, core::mem::size_of::<InitializationSegment>())?;
        BoxRefMut::new(Box::new(mp)).try_map_mut(|mp| mp.as_type_mut::<InitializationSegment>(0))
    }
}
