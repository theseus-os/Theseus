//! A mlx5 driver for a ConnectX-5 100GbE Network Interface Card.
//! 
//! Currently the following steps are completed 
//! * reading the device PCI space and mapping the initialization segment
//! * setting up a command queue to pass commands to the NIC
//! * setting up a single send and receive queue
//! * functions to send packets
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
extern crate nic_initialization;
extern crate mlx_ethernet;
extern crate kernel_config;
extern crate memory_structs;
extern crate nic_buffers;
extern crate mpmc;
#[macro_use] extern crate lazy_static;


use spin::Once; 
use alloc::vec::Vec;
use irq_safety::MutexIrqSafe;
use memory::{PhysicalAddress, MappedPages, create_contiguous_mapping, BorrowedMappedPages, Mutable};
use pci::PciDevice;
use nic_initialization::{NIC_MAPPING_FLAGS, allocate_memory, init_rx_buf_pool};
use mlx_ethernet::{
    command_queue::{AccessRegisterOpMod, CommandBuilder, CommandOpcode, CommandQueue, CommandQueueEntry, HCACapabilities, ManagePagesOpMod, QueryHcaCapCurrentOpMod, QueryHcaCapMaxOpMod, QueryPagesOpMod}, 
    completion_queue::{CompletionQueue, CompletionQueueEntry, CompletionQueueDoorbellRecord}, 
    event_queue::{EventQueue, EventQueueEntry}, 
    initialization_segment::InitializationSegment, 
    receive_queue::ReceiveQueue, 
    send_queue::SendQueue,
    work_queue::{WorkQueueEntrySend, WorkQueueEntryReceive, DoorbellRecord}
};
use kernel_config::memory::PAGE_SIZE;
use nic_buffers::{TransmitBuffer, ReceiveBuffer};

/// Vendor ID for Mellanox
pub const MLX_VEND:             u16 = 0x15B3;
/// Device ID for the ConnectX-5-EX NIC
pub const CONNECTX5_EX_DEV:     u16 = 0x1019; 
/// Device ID for the ConnectX-5 NIC
pub const CONNECTX5_DEV:        u16 = 0x1017;
/// For the send queue, we compress all the completion queue entries to the first entry.
const NUM_CQ_ENTRIES_SEND:      usize = 1; 

/// How many [`ReceiveBuffers`] are preallocated for this driver to use. 
/// 
/// # Warning
/// Right now we manually make sure this matches the mlx5 init arguments.
/// We need to update the whole RX buffer pool design to be per queue.
const RX_BUFFER_POOL_SIZE: usize = 512; 

lazy_static! {
    /// The pool of pre-allocated receive buffers that are used by the NIC
    /// and temporarily given to higher layers in the networking stack.
    /// 
    /// # Note
    /// The capacity always has to be greater than the number of buffers in the queue, which is why we multiply by 2.
    /// I'm not sure why that is, but if we try to add packets >= capacity, the addition does not make any progress.
    static ref RX_BUFFER_POOL: mpmc::Queue<ReceiveBuffer> = mpmc::Queue::with_capacity(RX_BUFFER_POOL_SIZE * 2);
}

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
    /// Initialization segment
    init_segment: BorrowedMappedPages<InitializationSegment, Mutable>,
    /// Command Queue
    command_queue: CommandQueue,
    /// Boot pages passed to the NIC. Once transferred, they should not be accessed by the driver.
    boot_pages: Vec<MappedPages>,
    /// Init pages passed to the NIC. Once transferred, they should not be accessed by the driver.
    init_pages: Vec<MappedPages>,
    /// Regular pages passed to the NIC. Once transferred, they should not be accessed by the driver.
    regular_pages: Vec<MappedPages>,
    /// MAC address of the device
    mac_addr: [u8; 6],
    /// The maximum capabilities supported by the device
    max_capabilities: HCACapabilities,
    /// The maximum MTU supported by the device
    max_mtu: u16,
    /// Event queue which currently only reports page request events
    event_queue: EventQueue,
    /// Buffer of transmit descriptors
    send_queue: SendQueue,
    /// The completion queue where packet transmission is reported
    send_completion_queue: CompletionQueue,
    /// Buffer of receive descriptors
    receive_queue: ReceiveQueue,
}

/// Functions that setup the NIC struct and transmit packets.
impl ConnectX5Nic {

    /// Initializes the new ConnectX-5 network interface card that is connected as the given PciDevice.
    /// (steps taken from the PRM, Section 7.2: HCA Driver Start-up)
    /// 
    /// # Arguments
    /// * `mlx5_pci_dev`: Contains the pci device information for this NIC.
    /// * `num_tx_descs`: The number of descriptors in each transmit queue.
    /// * `num_rx_descs`: The number of descriptors in each receive queue.
    /// * `mtu`: Maximum Transmission Unit in bytes
    pub fn init(
        mlx5_pci_dev: &PciDevice, 
        num_tx_descs: usize, 
        num_rx_descs: usize, 
        mtu: u16
    ) -> Result<&'static MutexIrqSafe<ConnectX5Nic> , &'static str> {
        let sq_size_in_bytes = num_tx_descs * core::mem::size_of::<WorkQueueEntrySend>();
        let rq_size_in_bytes = num_rx_descs * core::mem::size_of::<WorkQueueEntryReceive>();
        
        // because the RX and TX queues have to be contiguous and we are using MappedPages to split ownership of the queues,
        // the RX queue must end on a page boundary
        if rq_size_in_bytes % PAGE_SIZE != 0 {
            return Err("RQ size in bytes must be a multiple of the page size.");
        }

        if !num_tx_descs.is_power_of_two() || !num_rx_descs.is_power_of_two() {
            return Err("The number of descriptors must be a power of two.");
        } 

        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        mlx5_pci_dev.pci_set_command_bus_master_bit();

        // retrieve the memory-mapped base address of the initialization segment
        let mem_base = mlx5_pci_dev.determine_mem_base(0)?;
        trace!("mlx5 mem base = {}", mem_base);

        let mem_size = mlx5_pci_dev.determine_mem_size(0);
        trace!("mlx5 mem size = {}", mem_size);

        // map pages to the physical address given by mem_base as that is the intialization segment
        let mut init_segment = ConnectX5Nic::map_init_segment(mem_base)?;

        trace!("{:?}", &*init_segment);
        
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

        // calculate size of command queue
        let size_in_bytes_of_cmdq = num_cmdq_entries * cmdq_stride;
        trace!("total size in bytes of cmdq = {}", size_in_bytes_of_cmdq);
    
        // allocate mapped pages for the command queue
        let (cmdq_mapped_pages, cmdq_starting_phys_addr) = create_contiguous_mapping(size_in_bytes_of_cmdq, NIC_MAPPING_FLAGS)?;
        trace!("cmdq mem base = {}", cmdq_starting_phys_addr);
    
        // cast our physically-contiguous MappedPages into a slice of command queue entries
        let mut cmdq = CommandQueue::create(
            cmdq_mapped_pages.into_borrowed_slice_mut(0, num_cmdq_entries).map_err(|(_mp, err)| err)?,
            num_cmdq_entries
        )?;

        // write physical location of command queue to initialization segment
        init_segment.set_physical_address_of_cmdq(cmdq_starting_phys_addr)?;

        // Read initializing field from initialization segment until it is cleared
        while init_segment.device_is_initializing() {}
        trace!("initializing field is cleared.");

        // Execute ENABLE_HCA command
        let completed_cmd = cmdq.create_and_execute_command(CommandBuilder::new(CommandOpcode::EnableHca), &mut init_segment)?;
        trace!("EnableHCA: {:?}", cmdq.get_command_status(completed_cmd)?);

        // execute QUERY_ISSI
        let completed_cmd = cmdq.create_and_execute_command( CommandBuilder::new(CommandOpcode::QueryIssi), &mut init_segment)?;
        let (current_issi, available_issi, status) = cmdq.get_query_issi_command_output(completed_cmd)?;
        trace!("QueryISSI: {:?}, issi version :{}, available: {:#X}", status, current_issi, available_issi);
        
        // execute SET_ISSI
        const ISSI_VERSION_1: u8 = 0x2;
        if available_issi & ISSI_VERSION_1 == ISSI_VERSION_1 {
            let completed_cmd = cmdq.create_and_execute_command( CommandBuilder::new(CommandOpcode::SetIssi), &mut init_segment)?;
            trace!("SetISSI: {:?}", cmdq.get_command_status(completed_cmd)?);
        } else {
            return Err("ISSI indicated by PRM is not supported");
        }
        
        // Query pages for boot
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryPages).opmod(QueryPagesOpMod::BootPages as u16), 
            &mut init_segment
        )?;
        let (num_boot_pages, status) = cmdq.get_query_pages_command_output(completed_cmd)?;
        trace!("QueryPages: {:?}, num boot pages: {:?}", status, num_boot_pages);
        
        // Allocate pages for boot
        let (boot_mp, boot_pa) = Self::allocate_pages_for_nic(num_boot_pages as usize)?;
        
        // execute MANAGE_PAGES command to transfer boot pages to device
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::ManagePages)
                .opmod(ManagePagesOpMod::AllocationSuccess as u16)
                .allocated_pages(boot_pa), 
            &mut init_segment
        )?;
        trace!("ManagePages boot: {:?}", cmdq.get_command_status(completed_cmd)?);
        
        // Query current HCA capabilities
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryHcaCap)
                .opmod(QueryHcaCapCurrentOpMod::GeneralDeviceCapabilities as u16), 
            &mut init_segment
        )?;
        let (current_capabilities, status) = cmdq.get_device_capabilities(completed_cmd)?;
        trace!("QueryHCACap:{:?}, current capabilities: {:?}", status, current_capabilities);

        // Query maximum HCA capabilities
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryHcaCap)
                .opmod(QueryHcaCapMaxOpMod::GeneralDeviceCapabilities as u16), 
            &mut init_segment
        )?;
        let (max_capabilities, status) = cmdq.get_device_capabilities(completed_cmd)?;
        trace!("QueryHCACap:{:?}, max capabilities: {:?}", status, max_capabilities);

        // Query pages for init
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryPages)
                .opmod(QueryPagesOpMod::InitPages as u16), 
            &mut init_segment
        )?;
        let (num_init_pages, status) = cmdq.get_query_pages_command_output(completed_cmd)?;
        trace!("QueryPages: {:?}, num init pages: {:?}", status, num_init_pages);

        let init_mp = if num_init_pages != 0 {
            // Allocate pages for init
            let (init_mp, init_pa) = Self::allocate_pages_for_nic(num_init_pages as usize)?;

            // execute MANAGE_PAGES command to transfer init pages to device
            let completed_cmd = cmdq.create_and_execute_command(
                CommandBuilder::new(CommandOpcode::ManagePages)
                    .opmod(ManagePagesOpMod::AllocationSuccess as u16)
                    .allocated_pages(init_pa), 
                &mut init_segment
            )?;
            trace!("ManagePages init: {:?}", cmdq.get_command_status(completed_cmd)?);
            init_mp
        } else { 
            vec!(MappedPages::empty()) 
        };

        // execute INIT_HCA
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::InitHca), 
            &mut init_segment
        )?;
        trace!("InitHCA: {:?}", cmdq.get_command_status(completed_cmd)?);

        // Query regular pages
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryPages)
                .opmod(QueryPagesOpMod::RegularPages as u16), 
            &mut init_segment
        )?;
        let (num_reg_pages, status) = cmdq.get_query_pages_command_output(completed_cmd)?;
        trace!("QueryPages: {:?}, num regular pages: {:?}", status, num_reg_pages);

        let reg_mp = if num_reg_pages != 0 {
            // Allocate regular pages
            let (reg_mp, reg_pa) = Self::allocate_pages_for_nic(num_reg_pages as usize)?;

            // execute MANAGE_PAGES command to transfer regular pages to device
            let completed_cmd = cmdq.create_and_execute_command(
                CommandBuilder::new(CommandOpcode::ManagePages)
                    .opmod(ManagePagesOpMod::AllocationSuccess as u16)
                    .allocated_pages(reg_pa), 
                &mut init_segment
            )?;
            trace!("ManagePages regular: {:?}", cmdq.get_command_status(completed_cmd)?);
            reg_mp
        } else { 
            vec!(MappedPages::empty()) 
        };
        
        // execute QUERY_NIC_VPORT_CONTEXT to find the mac address
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryNicVportContext), 
            &mut init_segment
        )?;
        let (mac_addr, status) = cmdq.get_vport_mac_address(completed_cmd)?;
        trace!("QueryNicVportContext: {:?}, mac address: {:#X?}", status, mac_addr);

        // execute QUERY_VPORT_STATE to find the link status
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryVportState), 
            &mut init_segment
        )?;
        let (tx_speed, admin_state, state, status) = cmdq.get_vport_state(completed_cmd)?;
        trace!("QueryVportState: {:?}, tx_speed: {:#X}, admin_state:{:#X}, state: {:#X}", status, tx_speed, admin_state, state); 
        
        // execute ACCESS_REGISTER to get the maximum mtu
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::AccessRegister)
                .opmod(AccessRegisterOpMod::Read as u16),
            &mut init_segment
        )?;
        let (max_mtu, status) = cmdq.get_max_mtu(completed_cmd)?;
        trace!("AccessRegister: {:?}, max_mtu: {}", status, max_mtu); 

        // return an error if the MTU is greater than the maximum MTU
        if mtu > max_mtu { return Err("Required MTU is greater than maximum MTU!"); }

        // execute ACCESS_REGISTER to set the mtu
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::AccessRegister)
                .opmod(AccessRegisterOpMod::Write as u16)
                .mtu(mtu), 
            &mut init_segment
        )?;
        let status = cmdq.get_command_status(completed_cmd)?;
        trace!("AccessRegister: {:?}", status); 
    
        // execute MODIFY_NIC_VPORT_CONTEXT to set the mtu
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::ModifyNicVportContext)
                .mtu(mtu), 
            &mut init_segment
        )?;
        let status = cmdq.get_command_status(completed_cmd)?;
        trace!("ModifyNicVportContext: {:?}", status); 

        // execute ALLOC_UAR
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::AllocUar), 
            &mut init_segment
        )?;
        let (uar, status) = cmdq.get_uar(completed_cmd)?;
        trace!("AllocUar: {:?}, UAR: {}", status, uar);        

        // execute CREATE_EQ for page request event
        // 1. Allocate pages for EQ
        let num_eq_entries = 64;
        let (eq_mp, eq_pa) = create_contiguous_mapping(num_eq_entries * core::mem::size_of::<EventQueueEntry>(), NIC_MAPPING_FLAGS)?;
    
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateEq)
                .allocated_pages(vec!(eq_pa))
                .uar(uar)
                .queue_size(num_eq_entries as u32), 
            &mut init_segment
        )?;
        let (eqn, status) = cmdq.get_eq_number(completed_cmd)?;
        // 2. Initialize the EQ
        let event_queue = EventQueue::init(eq_mp, num_eq_entries, eqn)?;
        trace!("CreateEq: {:?}, eqn: {:?}", status, eqn); 

        // #[cfg(mlx_verbose_log)]
        // {
        //     event_queue.dump()
        // }

        // execute ALLOC_PD
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::AllocPd), 
            &mut init_segment
        )?;
        let (pd, status) = cmdq.get_protection_domain(completed_cmd)?;
        trace!("AllocPd: {:?}, protection domain: {:?}", status, pd);

        // execute ALLOC_TRANSPORT_DOMAIN
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::AllocTransportDomain), 
            &mut init_segment
        )?;
        let (td, status) = cmdq.get_transport_domain(completed_cmd)?;
        trace!("AllocTransportDomain: {:?}, transport domain: {:?}", status, td);

        // execute QUERY_SPECIAL_CONTEXTS
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QuerySpecialContexts), 
            &mut init_segment
        )?;
        let (rlkey, status) = cmdq.get_reserved_lkey(completed_cmd)?;
        trace!("QuerySpecialContexts: {:?}, rlkey: {:?}", status, rlkey);
        
        // execute CREATE_CQ for SQ 

        // 1. Allocate pages for CQ
        let (cq_mp, cq_pa) = create_contiguous_mapping(NUM_CQ_ENTRIES_SEND * core::mem::size_of::<CompletionQueueEntry>(), NIC_MAPPING_FLAGS)?;

        // 2. Allocate page for doorbell
        let (db_page, db_pa) = create_contiguous_mapping(core::mem::size_of::<CompletionQueueDoorbellRecord>(), NIC_MAPPING_FLAGS)?;
        
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateCq) 
                .allocated_pages(vec!(cq_pa))
                .uar(uar)
                .queue_size(NUM_CQ_ENTRIES_SEND as u32)
                .eqn(eqn)
                .db_page(db_pa)
                .collapsed_cq(), 
            &mut init_segment
        )?;
        let (cqn_s, status) = cmdq.get_cq_number(completed_cmd)?;
        // 3. Initialize the CQ
        let send_completion_queue = CompletionQueue::init(cq_mp, NUM_CQ_ENTRIES_SEND, db_page, cqn_s)?;
        trace!("CreateCq: {:?}, cqn_s: {:?}", status, cqn_s);

        // #[cfg(mlx_verbose_log)]
        // {
        //     send_completion_queue.dump()
        // }

        // execute CREATE_CQ for RQ

        // 1. Allocate pages for CQ
        let cq_entries_r = num_rx_descs; 
        let (cq_mp, cq_pa) = create_contiguous_mapping(cq_entries_r * core::mem::size_of::<CompletionQueueEntry>(), NIC_MAPPING_FLAGS)?;
        // 2. Allocate page for doorbell
        let (db_page, db_pa) = create_contiguous_mapping(core::mem::size_of::<CompletionQueueDoorbellRecord>(), NIC_MAPPING_FLAGS)?;
        
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateCq) 
                .allocated_pages(vec!(cq_pa))
                .uar(uar)
                .queue_size(cq_entries_r as u32)
                .eqn(eqn)
                .db_page(db_pa), 
            &mut init_segment
        )?;
        let (cqn_r, status) = cmdq.get_cq_number(completed_cmd)?;
        // 3. Initialize the CQ
        let completion_queue_r = CompletionQueue::init(cq_mp, cq_entries_r, db_page, cqn_r)?;
        trace!("CreateCq: {:?}, cqn_r: {:?}", status, cqn_r);
        
        // execute CREATE_TIS
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateTis)
                .td(td), 
            &mut init_segment
        )?;
        let (tisn, status) = cmdq.get_tis_context_number(completed_cmd)?;
        trace!("CreateTis: {:?}, tisn: {:?}", status, tisn);

        // Allocate pages for RQ and SQ, they have to be contiguous      
        let (q_mp, q_pa) = create_contiguous_mapping(rq_size_in_bytes + sq_size_in_bytes, NIC_MAPPING_FLAGS)?;
        let vaddr = q_mp.start_address();
        let (rq_mp, sq_mp) = q_mp.split(memory_structs::Page::containing_address(vaddr + rq_size_in_bytes))
            .map_err(|_e| "Could not split MappedPages")?;
        let sq_pa = q_pa + rq_size_in_bytes;
        debug!("RQ paddr: {:?}, SQ paddr: {:?}, RQ vaddr: {:?}, SQ vaddr: {:?}", q_pa, sq_pa, rq_mp.start_address(), sq_mp.start_address());

        // Allocate page for SQ/RQ doorbell
        let (db_page, db_pa) = create_contiguous_mapping(core::mem::size_of::<DoorbellRecord>(), NIC_MAPPING_FLAGS)?;
        debug!("doorbell: {:#x}", db_pa);

        // Allocate page for UAR. 
        // For the given uar number i, the page is the ith page from the memory base retrieved from the PCI BAR
        let uar_mem_base = mem_base + ((uar as usize) * PAGE_SIZE);
        let uar_page = allocate_memory(uar_mem_base, PAGE_SIZE)?;
        debug!("mmio: {:?}, uar: {:?}", mem_base, uar_mem_base);

        // Create the SQ
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateSq) 
                .allocated_pages(vec!(sq_pa)) 
                .uar(uar) 
                .queue_size(num_tx_descs as u32)
                .db_page(db_pa)
                .cqn(cqn_s) 
                .tisn(tisn) 
                .pd(pd), 
            &mut init_segment
        )?;
        let (sqn, status) = cmdq.get_send_queue_number(completed_cmd)?;
        let send_queue = SendQueue::create(
            sq_mp, 
            num_tx_descs, 
            db_page, 
            uar_page, 
            sqn, 
            tisn, 
            rlkey
        )?;
        trace!("Create SQ status: {:?}, number: {:?}", status, sqn);
        
        // #[cfg(mlx_verbose_log)]
        // {
        //     send_queue.dump()
        // }

        // initialize the rx buffer pool
        init_rx_buf_pool(num_rx_descs, mtu, &RX_BUFFER_POOL)?;

        // Create the RQ
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateRq) 
                .allocated_pages(vec!(q_pa)) 
                .queue_size(num_rx_descs as u32)
                .db_page(db_pa)
                .cqn(cqn_r) 
                .pd(pd), 
            &mut init_segment
        )?;
        let (rqn, status) = cmdq.get_receive_queue_number(completed_cmd)?;
        let mut receive_queue = ReceiveQueue::create(
            rq_mp, 
            num_rx_descs, 
            mtu as u32, 
            &RX_BUFFER_POOL, 
            rqn, 
            rlkey, 
            completion_queue_r
        )?;
        receive_queue.refill()?;
        trace!("Create RQ status: {:?}, number: {:?}", status, rqn);

        // MODIFY_SQ
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::ModifySq) 
                .sqn(sqn), 
            &mut init_segment
        )?;
        trace!("Modify SQ status: {:?}", cmdq.get_command_status(completed_cmd)?);

        // MODIFY_RQ
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::ModifyRq) 
                .rqn(rqn), 
            &mut init_segment
        )?;
        trace!("Modify RQ status: {:?}", cmdq.get_command_status(completed_cmd)?);

        // QUERY_SQ
        // let completed_cmd = cmdq.create_and_execute_command(
        //     CommandBuilder::new(CommandOpcode::QuerySq)
        //         .sqn(sqn), 
        //     &mut init_segment
        // )?;
        // let (state, status) = cmdq.get_sq_state(completed_cmd)?;
        // trace!("Query SQ status: {:?}, state: {:?}", status, state);

        // execute QUERY_VPORT_STATE
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::QueryVportState), 
            &mut init_segment
        )?;
        let (tx_speed, admin_state, state, status) = cmdq.get_vport_state(completed_cmd)?;
        trace!("Query Vport State status: {:?}, tx_speed: {:#X}, admin_state:{:#X}, state: {:#X}", status, tx_speed, admin_state, state); 

        // Create a flow table
        // currently we only create 1 rule, the wildcard rule.
        const NUM_RULES: u32 = 1;
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateFlowTable)
                .queue_size(NUM_RULES), 
            &mut init_segment
        )?;
        let (ft_id, status) = cmdq.get_flow_table_id(completed_cmd)?;
        trace!("Create FT status: {:?}, id: {:?}", status, ft_id);

        // create the wildcard flow group
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateFlowGroup)
                .flow_table_id(ft_id), 
            &mut init_segment
        )?;
        let (fg_id, status) = cmdq.get_flow_group_id(completed_cmd)?;
        trace!("Create FG status: {:?}, id: {:?}", status, fg_id);

        // create a TIR object for the RQ
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::CreateTir)
                .rqn(rqn)
                .td(td), 
            &mut init_segment
        )?;
        let (tirn, status) = cmdq.get_tir_context_number(completed_cmd)?;
        trace!("Create TIR status: {:?}, tirn: {:?}", status, tirn);

        // add the wildcard entry to the flow table
        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::SetFlowTableEntry)
                .flow_table_id(ft_id)
                .flow_group_id(fg_id)
                .tirn(tirn), 
            &mut init_segment
        )?;
        trace!("Set FT entry status: {:?}", cmdq.get_command_status(completed_cmd)?);

        let completed_cmd = cmdq.create_and_execute_command(
            CommandBuilder::new(CommandOpcode::SetFlowTableRoot)
                .flow_table_id(ft_id),
            &mut init_segment
        )?;
        trace!("Set FT root status: {:?}", cmdq.get_command_status(completed_cmd)?);


        let mlx5_nic = ConnectX5Nic {
            mem_base,
            init_segment,
            command_queue: cmdq, 
            boot_pages: boot_mp,
            init_pages: init_mp,
            regular_pages: reg_mp,
            mac_addr,
            max_capabilities,
            max_mtu,
            event_queue,
            send_completion_queue,
            send_queue,
            receive_queue
        };
        
        let nic_ref = CONNECTX5_NIC.call_once(|| MutexIrqSafe::new(mlx5_nic));
        Ok(nic_ref)
    }
    
    /// Returns the memory-mapped initialization segment of the NIC
    fn map_init_segment(mem_base: PhysicalAddress) -> Result<BorrowedMappedPages<InitializationSegment, Mutable>, &'static str> {
        allocate_memory(mem_base, core::mem::size_of::<InitializationSegment>())?
            .into_borrowed_mut(0)
            .map_err(|(_mp, err)| err)
    }

    /// Allocates `num_pages` [`MappedPages`] each of the standard kernel page size [`PAGE_SIZE`].
    /// Returns a vector of the [`MappedPages`] and a vector of the [`PhysicalAddress`] of the pages.
    fn allocate_pages_for_nic(num_pages: usize) -> Result<(Vec<MappedPages>, Vec<PhysicalAddress>), &'static str> {
        let mut mp = Vec::with_capacity(num_pages);
        let mut paddr = Vec::with_capacity(num_pages);
        for _ in 0..num_pages {
            let (page, pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            mp.push(page);
            paddr.push(pa);
        }
        Ok((mp, paddr))
    }

    /// Returns the MAC address of the physical function 
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_addr
    }

    /// Adds a packet to be sent to the transmit queue and returns once it is sent.
    pub fn send(&mut self, buffer: TransmitBuffer) -> Result<(), &'static str> {
        let wqe_counter = self.send_queue.send(buffer.phys_addr(), &buffer);
        // self.send_completion_queue.wqe_posted(wqe_counter);
        self.send_completion_queue.check_packet_transmission(0, wqe_counter);
        self.send_completion_queue.dump();
        Ok(())
    }

    /// Adds a packet to be sent to the transmit queue.
    pub fn send_fastpath(&mut self, buffer_addr: PhysicalAddress, buffer: &[u8]) {
        self.send_queue.send(buffer_addr, buffer);
    }
}
