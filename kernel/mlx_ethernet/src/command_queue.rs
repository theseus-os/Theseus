 //! Defines the Command Queue that is used to pass commands from the driver to the NIC.
 //! Also defines multiple enums that specify the valid input and output values for different commands.

use alloc::{
    vec::Vec,
    boxed::Box
};
use memory::{PhysicalAddress, MappedPages, create_contiguous_mapping};
use volatile::Volatile;
use bit_field::BitField;
use zerocopy::*;
use byteorder::BigEndian;
use owning_ref:: BoxRefMut;
use nic_initialization::NIC_MAPPING_FLAGS;
use kernel_config::memory::PAGE_SIZE;
use core::fmt;
use num_enum::TryFromPrimitive;
use core::convert::TryFrom;
use crate::event_queue::*;
use crate::completion_queue::*;
use crate::send_queue::*;
use crate::work_queue::*;
use crate::receive_queue::*;

/// Size of mailboxes, including both control fields and data.
#[allow(dead_code)]
const MAILBOX_SIZE_IN_BYTES:        usize = 576;
/// Number of bytes in the mailbox that are actually used to pass data.
const MAILBOX_DATA_SIZE_IN_BYTES:   usize = 512;
/// Mailboxes are aligned at 4 KiB, so they are always present at offset 0 in a page.
const MAILBOX_OFFSET_IN_PAGE:       usize = 0;
/// Each physical address takes 8 bytes
const SIZE_PADDR_IN_BYTES:          usize = 8; 
/// When passing physical addresses to the NIC, we always send the higher 4 bytes in one field.
const SIZE_PADDR_H_IN_BYTES:        usize = 4; 
/// When passing physical addresses to the NIC, we always send the lower 4 bytes in one field.
const SIZE_PADDR_L_IN_BYTES:        usize = 4; 

/// Type of transport that carries the command.
pub enum CommandTransportType {
    PCIe = 0x7 << 24
}

/// Possible reasons for failure when executing a command
pub enum CommandQueueError {
    /// All command entries are currently being used
    NoCommandEntryAvailable,
    /// Allocated pages are not passed to a command that requires them
    MissingInputPages,
    /// Any other input is not passed to a command that requires them
    MissingInput,
    /// Opcode value in the command entry is not what was expected
    IncorrectCommandOpcode,
    /// Opcode in the command entry is not a valid value
    InvalidCommandOpcode,
    /// Delivery status in the command entry is not a valid value
    InvalidCommandDeliveryStatus,
    /// Return status in the command entry is not a valid value
    InvalidCommandReturnStatus,
    /// Trying to access the command entry before HW is done processing it
    CommandNotCompleted,
    /// Offset in a page is too large to map a [`CommandInterfaceMailbox`] to that offset
    InvalidMailboxOffset,
    /// A call to create a [`MappedPages`] failed
    PageAllocationFailed,
    /// Initializing a comand entry for the given opcode has not been implemented
    UnimplementedOpcode,
    /// Some function has not been implemented for the given opcode
    NotImplemented,
    /// The returned port type is not a valid value
    InvalidPortType,
}

impl From<CommandQueueError> for &'static str {
    fn from(error: CommandQueueError) -> Self {
        match error {
            CommandQueueError::NoCommandEntryAvailable => "No command entry is available",
            CommandQueueError::MissingInputPages => "No pages were passed to the command",
            CommandQueueError::MissingInput => "An input was not passed to a command that required it",
            CommandQueueError::IncorrectCommandOpcode => "Incorrect command opcode",
            CommandQueueError::InvalidCommandOpcode => "Invalid command opcode. This could be because the value is invalid, 
                                                        or because the driver currently doesn't support the opcode.",
            CommandQueueError::InvalidCommandDeliveryStatus => "Invalid command delivery status",
            CommandQueueError::InvalidCommandReturnStatus => "Invalid command return status",
            CommandQueueError::CommandNotCompleted => "Command not complete yet",
            CommandQueueError::InvalidMailboxOffset => "Invalid offset for mailbox in a page",
            CommandQueueError::PageAllocationFailed => "Failed to allocate MappedPages",
            CommandQueueError::UnimplementedOpcode => "Opcode is not implemented",
            CommandQueueError::NotImplemented => "Function not implemented for the given opcode",
            CommandQueueError::InvalidPortType => "Invalid port type"
        }
    }
}


/// Return codes written by HW in the delivery status field of the command entry.
/// See [`CommandQueueEntry::token_signature_status_own`].
#[derive(Debug, TryFromPrimitive)]
#[repr(u32)]
pub enum CommandDeliveryStatus {
    Success             = 0x0,
    SignatureErr        = 0x1,
    TokenErr            = 0x2,
    BadBlockNumber      = 0x3,
    BadOutputPointer    = 0x4,
    BadInputPointer     = 0x5,
    InternalErr         = 0x6,
    InputLenErr         = 0x7,
    OutputLenErr        = 0x8,
    ReservedNotZero     = 0x9,
    BadCommandType      = 0x10,
}


/// Command opcode written by SW in opcode field of the input data in the command entry.
/// See [`CommandQueueEntry::command_input_opcode`].
#[derive(PartialEq, Debug, TryFromPrimitive, Copy, Clone)]
#[repr(u32)]
pub enum CommandOpcode {
    QueryHcaCap             = 0x100,
    InitHca                 = 0x102,
    EnableHca               = 0x104,
    QueryPages              = 0x107,
    ManagePages             = 0x108,
    QueryIssi               = 0x10A,
    SetIssi                 = 0x10B,
    SetDriverVersion        = 0x10D,
    QuerySpecialContexts    = 0x203,
    CreateEq                = 0x301,
    CreateCq                = 0x400,
    QueryVportState         = 0x750,
    QueryNicVportContext    = 0x754,
    ModifyNicVportContext   = 0x755,
    AllocPd                 = 0x800,
    AllocUar                = 0x802,
    AllocTransportDomain    = 0x816,
    CreateTir               = 0x900,
    CreateSq                = 0x904,
    ModifySq                = 0x905,
    QuerySq                 = 0x907,
    CreateRq                = 0x908,
    ModifyRq                = 0x909,
    QueryRq                 = 0x90B,
    CreateTis               = 0x912,
}

impl CommandOpcode {
    fn input_bytes(&self, num_pages: Option<usize>) -> Result<u32, CommandQueueError> {
        let len = match self {
            Self::QueryHcaCap => 12,
            Self::InitHca => 12,
            Self::EnableHca => 12,
            Self::QueryPages => 12,
            Self::ManagePages => {
                let num_pages = num_pages.ok_or(CommandQueueError::MissingInput)? as u32;
                0x10 + num_pages * SIZE_PADDR_IN_BYTES as u32
            }
            Self::QueryIssi => 8, 
            Self::SetIssi => 12,
            Self::SetDriverVersion =>  0x50,    
            Self::QuerySpecialContexts => 8,              
            Self::CreateEq => {
                let num_pages = num_pages.ok_or(CommandQueueError::MissingInput)? as u32;
                0x110 + num_pages * SIZE_PADDR_IN_BYTES as u32
            }
            Self::CreateCq => {
                let num_pages = num_pages.ok_or(CommandQueueError::MissingInput)? as u32;
                0x110 + num_pages * SIZE_PADDR_IN_BYTES as u32
            }
            Self::QueryVportState => 12,        
            Self::QueryNicVportContext => 16,  
            Self::ModifyNicVportContext => 0x100 + 0x108,        
            Self::AllocPd => 8,               
            Self::AllocUar => 8,              
            Self::AllocTransportDomain => 8,
            Self::CreateTir => 0x20 + 0xF0,
            Self::CreateSq => {
                let num_pages = num_pages.ok_or(CommandQueueError::MissingInput)? as u32;
                0x20 + 0x30 + 0xC0 + (num_pages * SIZE_PADDR_IN_BYTES as u32)
            }
            Self::ModifySq => 0x20 + 0x30 + 0xC0,
            Self::QuerySq => 12,
            Self::CreateRq => {
                let num_pages = num_pages.ok_or(CommandQueueError::MissingInput)? as u32;
                0x20 + 0x30 + 0xC0 + num_pages * SIZE_PADDR_IN_BYTES as u32
            },
            Self::ModifyRq => 0x20 + 0x30 + 0xC0,
            Self::QueryRq => 12,
            Self::CreateTis => 0x20 + 0xA0,
            _ => return Err(CommandQueueError::NotImplemented)           
        };
        Ok(len)
    }

    fn output_bytes(&self) -> Result<u32, CommandQueueError> {
        let len = match self {
            Self::QueryHcaCap => 16 + 0x100,
            Self::InitHca => 8,
            Self::EnableHca => 8,
            Self::QueryPages => 16,
            Self::ManagePages => 16,
            Self::QueryIssi => 0x70, 
            Self::SetIssi => 8,   
            Self::SetDriverVersion =>  8,
            Self::QuerySpecialContexts => 16,              
            Self::CreateEq => 16,
            Self::CreateCq => 16,
            Self::QueryVportState => 16,   
            Self::QueryNicVportContext => 16 + 0x108,      
            Self::ModifyNicVportContext => 16,        
            Self::AllocPd => 12,               
            Self::AllocUar => 12,              
            Self::AllocTransportDomain => 12,
            Self::CreateTir => 16,
            Self::CreateSq => 16,
            Self::ModifySq => 8,
            Self::QuerySq => 0x10 + MAILBOX_DATA_SIZE_IN_BYTES as u32,
            Self::CreateRq => 16,
            Self::ModifyRq => 16,
            Self::QueryRq => 0x10 + MAILBOX_DATA_SIZE_IN_BYTES as u32,
            Self::CreateTis => 16,
            _ => return Err(CommandQueueError::NotImplemented)         
        };
        Ok(len)
    }
}


/// Command status written by HW in status field of the output data in the command entry.
/// See [`CommandQueueEntry::command_output_status`].
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum CommandReturnStatus {
    OK                  = 0x00,
    InternalError       = 0x01,
    BadOp               = 0x02,
    BadParam            = 0x03,
    BadSysState         = 0x04,
    BadResource         = 0x05,
    ResourceBusy        = 0x06,
    ExceedLim           = 0x08,
    BadResState         = 0x09,
    BadIndex            = 0x0A,
    NoResources         = 0x0F,
    BadInputLen         = 0x50,
    BadOutputLen        = 0x51,
    BadResourceState    = 0x10,
    BadPkt              = 0x30,
    BadSize             = 0x40,
}


/// Possible values of the opcode modifer when the opcode is [`CommandOpcode::ManagePages`].
pub enum ManagePagesOpMod {
    AllocationFail      = 0,
    AllocationSuccess   = 1,
    HcaReturnPages      = 2
}

/// Possible values of the opcode modifer when the opcode is [`CommandOpcode::QueryPages`].
pub enum QueryPagesOpMod {
    BootPages       = 1,
    InitPages       = 2,
    RegularPages    = 3
}

/// Possible values of the opcode modifer when the opcode is [`CommandOpcode::QueryVportState`].
pub enum QueryVportStateOpMod {
    VnicVport       = 0,
    EswVport        = 1,
    Uplink          = 2,
}


/// Possible values of the opcode modifer when the opcode is [`CommandOpcode::QueryHcaCap`] and we want to retrieve maximum values of capabilities.
#[derive(Copy, Clone)]
pub enum QueryHcaCapMaxOpMod {
    GeneralDeviceCapabilities       = (0x0 << 1),
    EthernetOffloadCapabilities     = (0x1 << 1)
}

/// Possible values of the opcode modifer when the opcode is [`CommandOpcode::QueryHcaCap`] and we want to retrieve current values of capabilities.
#[derive(Copy, Clone)]
pub enum QueryHcaCapCurrentOpMod {
    GeneralDeviceCapabilities       = (0x0 << 1) | 0x1,
    EthernetOffloadCapabilities     = (0x1 << 1) | 0x1
}


/// Possible values of the port type field returned when retrieving device capabilities using the command [`CommandOpcode::QueryHcaCap`].
#[derive(Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum HcaPortType {
    /// Infiniband
    IB          = 0x0,
    Ethernet    = 0x1
}

/// Mailboxes can be used for both input data passed to HW, and output data passed from HW to SW.
#[derive(PartialEq)]
enum MailboxType {
    Input,
    Output
}

/// The layout of output data for the command [`CommandOpcode::QueryNicVportContext`].
/// This command is mainly used to retrieve the mac address of the NIC when working with the physical function.
/// (Section 8.6: NIC_Vport Context)
#[derive(FromBytes)]
#[repr(C)]
struct NicVportContext {
    /// This field is not necessarily padding, just unused fields that are not relevant.
    _padding0: [u8; 244],
    /// The upper bytes of the NIC's MAC address
    permanent_address_h: Volatile<U32<BigEndian>>,
    /// The lower four bytes of the NIC's MAC address
    permanent_address_l: Volatile<U32<BigEndian>>,
}

const_assert_eq!(core::mem::size_of::<NicVportContext>(), 252);

struct MailboxBuffer {
    mp: MappedPages,
    addr: PhysicalAddress
}

pub struct InitializedCommand {
    pub(crate) entry_num: usize,
    input_mailbox_buffers: Vec<MailboxBuffer>,
    output_mailbox_buffers: Vec<MailboxBuffer>,
}

impl InitializedCommand {
    fn new(
        entry_num: usize, 
        mut entry: CommandQueueEntry, 
        input_mailbox_buffers: Vec<MailboxBuffer>,
        output_mailbox_buffers: Vec<MailboxBuffer>,
        command_queue: &mut BoxRefMut<MappedPages, [CommandQueueEntry]>
    ) -> Self {
        core::mem::swap(&mut entry, &mut command_queue[entry_num]);
        InitializedCommand {
            entry_num,
            input_mailbox_buffers,
            output_mailbox_buffers
        }
    }

    pub(crate) fn post(self) -> PostedCommand {
        PostedCommand { 
            entry_num: self.entry_num,
            input_mailbox_buffers: self.input_mailbox_buffers, 
            output_mailbox_buffers: self.output_mailbox_buffers 
        }
    }
}

pub struct PostedCommand {
    entry_num: usize,
    input_mailbox_buffers: Vec<MailboxBuffer>,
    output_mailbox_buffers: Vec<MailboxBuffer>,
}

impl PostedCommand {
    fn complete(self) -> CompletedCommand {
        CompletedCommand { 
            entry_num: self.entry_num,
            input_mailbox_buffers: self.input_mailbox_buffers, 
            output_mailbox_buffers: self.output_mailbox_buffers 
        }
    }
}

pub struct CompletedCommand {
    entry_num: usize,
    input_mailbox_buffers: Vec<MailboxBuffer>,
    output_mailbox_buffers: Vec<MailboxBuffer>,
}

#[derive(Debug)]
pub struct CommandCompletionStatus {
    delivery_status: CommandDeliveryStatus,
    return_status: CommandReturnStatus
}

pub struct CommandBuilder {
    opcode:                         CommandOpcode,
    opmod:                          Option<u16>,
    allocated_pages:                Option<Vec<PhysicalAddress>>,
    user_access_region:             Option<u32>,
    log_queue_size:                 Option<u8>,
    event_queue_num:                Option<u8>, 
    doorbell_page:                  Option<PhysicalAddress>,
    transport_domain:               Option<u32>,
    completion_queue_num:           Option<u32>,
    transport_interface_send_num:   Option<u32>,
    protection_domain:              Option<u32>,
    send_queue_num:                 Option<u32>,
    collapsed_cq:                   bool,
    receive_queue_num:              Option<u32>
}

impl CommandBuilder {
    pub fn new(opcode: CommandOpcode) -> CommandBuilder {
        CommandBuilder { 
            opcode: opcode, 
            opmod: None, 
            allocated_pages: None, 
            user_access_region: None, 
            log_queue_size: None, 
            event_queue_num: None, 
            doorbell_page: None, 
            transport_domain: None, 
            completion_queue_num: None, 
            transport_interface_send_num: None, 
            protection_domain: None, 
            send_queue_num: None, 
            collapsed_cq: false,
            receive_queue_num: None
        }
    }

    pub fn opmod(mut self, opmod: u16) -> CommandBuilder {
        self.opmod = Some(opmod);
        self
    }

    pub fn allocated_pages(mut self, allocated_pages: Vec<PhysicalAddress>) -> CommandBuilder {
        self.allocated_pages = Some(allocated_pages);
        self
    }

    pub fn uar(mut self, uar: u32) -> CommandBuilder {
        self.user_access_region = Some(uar);
        self
    }

    pub fn log_queue_size(mut self, size: u8) -> CommandBuilder {
        self.log_queue_size = Some(size);
        self
    }

    pub fn eqn(mut self, eqn: u8) -> CommandBuilder {
        self.event_queue_num = Some(eqn);
        self
    }

    pub fn db_page(mut self, db_page: PhysicalAddress) -> CommandBuilder {
        self.doorbell_page = Some(db_page);
        self
    }

    pub fn td(mut self, td: u32) -> CommandBuilder {
        self.transport_domain = Some(td);
        self
    }

    pub fn cqn(mut self, cqn: u32) -> CommandBuilder {
        self.completion_queue_num = Some(cqn);
        self
    }

    pub fn tisn(mut self, tisn: u32) -> CommandBuilder {
        self.transport_interface_send_num = Some(tisn);
        self
    }

    pub fn pd(mut self, pd: u32) -> CommandBuilder {
        self.protection_domain = Some(pd);
        self
    }

    pub fn sqn(mut self, sqn: u32) -> CommandBuilder {
        self.send_queue_num = Some(sqn);
        self
    }

    pub fn collapsed_cq(mut self) -> CommandBuilder {
        self.collapsed_cq = true;
        self
    }
    
    pub fn rqn(mut self, rqn: u32) -> CommandBuilder {
        self.receive_queue_num = Some(rqn);
        self
    }
}

/// A buffer of fixed-size entries that is used to pass commands to the HCA.
/// It resides in a physically contiguous 4 KiB memory chunk.
/// (Section 8.24.1: HCA Command Queue)
pub struct CommandQueue {
    /// Physically-contiguous command queue entries
    entries: BoxRefMut<MappedPages, [CommandQueueEntry]>,
    /// Per-entry boolean flags to keep track of which entries are in use
    available_entries: Vec<bool>,
    /// A random number that needs to be different for every command, and the same for all mailboxes that are part of a command.
    token: u8,
    /// Pages used for mailboxes.
    mailbox_buffers: Vec<MailboxBuffer>, 
}

impl CommandQueue {

    /// Create a command queue object.
    ///
    /// ## Arguments
    /// * `entries`: physically contiguous memory that is mapped as a slice of command queue entries.
    /// * `num_cmdq_entries`: number of entries in the queue.
    pub fn create(entries: BoxRefMut<MappedPages, [CommandQueueEntry]>, num_cmdq_entries: usize) -> Result<CommandQueue, &'static str> {
        
        // initially, all command entries are available
        let available_entries = vec![true; num_cmdq_entries];

        // start off by pre-allocating one page per entry
        let mut mailbox_buffers= Vec::with_capacity(num_cmdq_entries);
        for _ in 0..num_cmdq_entries {
            let (mp, addr) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            mailbox_buffers.push(MailboxBuffer{mp, addr});
        }

        // Assign a random number to token, another OS (Snabb) starts off with 0xAA
        let token = 0xAA;

        Ok(CommandQueue{ entries, available_entries, token, mailbox_buffers })
    }

    /// Find an command queue entry that is not in use
    fn find_free_command_entry(&self) -> Option<usize> {
        self.available_entries.iter().position(|&x| x == true)
    }

    /// Fill in the fields of a command queue entry.
    /// At the end of the function, the command is ready to be posted using the doorbell in the initialization segment. 
    /// Returns an error if no entry is available to use.
    ///
    /// ## Arguments
    /// * `parameters`:
    pub fn create_command(&mut self, parameters: CommandBuilder) -> Result<InitializedCommand, CommandQueueError> 
    {
        let entry_num = self.find_free_command_entry().ok_or(CommandQueueError::NoCommandEntryAvailable)?; 
        let num_pages = parameters.allocated_pages.as_ref().and_then(|pages| Some(pages.len())); 

        // create a command queue entry with the fields initialized for the given opcode and opmod
        let mut cmdq_entry = CommandQueueEntry::init(parameters.opcode, parameters.opmod, self.token, num_pages)?;
        let mut input_mailbox_buffers: Vec<MailboxBuffer> = Vec::new();
        let mut output_mailbox_buffers: Vec<MailboxBuffer> = Vec::new();

        match parameters.opcode {
            CommandOpcode::EnableHca => {}
            CommandOpcode::InitHca => {}
            CommandOpcode::QuerySpecialContexts => {}
            CommandOpcode::QueryPages => {}
            CommandOpcode::AllocUar => {},
            CommandOpcode::QueryVportState => { /* only accesses your own vport */ },
            CommandOpcode::AllocPd => {},
            CommandOpcode::AllocTransportDomain => {},
            CommandOpcode::QueryHcaCap => {
                const NUM_OUTPUT_MAILBOXES_QUERY_HCA_CAP: usize = 1;
                self.initialize_mailboxes(NUM_OUTPUT_MAILBOXES_QUERY_HCA_CAP, &mut output_mailbox_buffers)?;
                cmdq_entry.set_output_mailbox_pointer(output_mailbox_buffers[0].addr);
            },
            CommandOpcode::QueryIssi => {
                const NUM_OUTPUT_MAILBOXES_QUERY_ISSI: usize = 1;
                self.initialize_mailboxes(NUM_OUTPUT_MAILBOXES_QUERY_ISSI, &mut output_mailbox_buffers)?;
                cmdq_entry.set_output_mailbox_pointer(output_mailbox_buffers[0].addr);
            }
            CommandOpcode::SetIssi => {
                // setting to 1 by default
                const ISSI_VERSION_1: u32 = 1;
                cmdq_entry.set_input_inline_data_0(ISSI_VERSION_1);
            }
            CommandOpcode::SetDriverVersion => {
                const NUM_INPUT_MAILBOXES_SET_DRIVER_VERSION: usize = 1;
                self.initialize_mailboxes(NUM_INPUT_MAILBOXES_SET_DRIVER_VERSION, &mut input_mailbox_buffers)?;

                Self::write_driver_version_to_mailbox(&mut input_mailbox_buffers)?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);
            }
            CommandOpcode::ManagePages => {
                let pages_pa = parameters.allocated_pages.ok_or(CommandQueueError::MissingInputPages)?;
                cmdq_entry.set_input_inline_data_1(pages_pa.len() as u32);

                let num_mailboxes = libm::ceilf((pages_pa.len() * SIZE_PADDR_IN_BYTES) as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
                self.initialize_mailboxes(num_mailboxes, &mut input_mailbox_buffers)?;
                Self::write_page_addrs_to_mailboxes(&mut input_mailbox_buffers, pages_pa)?; 
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);
            }
            CommandOpcode::QueryNicVportContext => { // only accesses your own vport
                const NUM_OUTPUT_MAILBOXES_QUERY_NIC_VPORT_CONTEXT: usize = 1;
                self.initialize_mailboxes(NUM_OUTPUT_MAILBOXES_QUERY_NIC_VPORT_CONTEXT, &mut output_mailbox_buffers)?;
                cmdq_entry.set_output_mailbox_pointer(output_mailbox_buffers[0].addr);
            },
            CommandOpcode::CreateEq => {
                let pages_pa = parameters.allocated_pages.ok_or(CommandQueueError::MissingInputPages)?;    

                let size_of_mailbox_data = (0x110 - 0x10) + SIZE_PADDR_IN_BYTES * pages_pa.len();
                let num_mailboxes = libm::ceilf(size_of_mailbox_data as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
                self.initialize_mailboxes(num_mailboxes, &mut input_mailbox_buffers)?;          

                Self::write_event_queue_context_to_mailbox(
                    &mut input_mailbox_buffers, 
                    pages_pa,
                    parameters.user_access_region.ok_or(CommandQueueError::MissingInput)?,
                    parameters.log_queue_size.ok_or(CommandQueueError::MissingInput)?
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);
            },
            CommandOpcode::CreateCq => {
                let pages_pa = parameters.allocated_pages.ok_or(CommandQueueError::MissingInputPages)?;
                        
                let size_of_mailbox_data = (0x110 - 0x10) + SIZE_PADDR_IN_BYTES * pages_pa.len();
                let num_mailboxes = libm::ceilf(size_of_mailbox_data as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
                self.initialize_mailboxes(num_mailboxes, &mut input_mailbox_buffers)?;
                
                Self::write_completion_queue_context_to_mailbox(
                    &mut input_mailbox_buffers,
                    pages_pa,
                    parameters.user_access_region.ok_or(CommandQueueError::MissingInput)?,
                    parameters.log_queue_size.ok_or(CommandQueueError::MissingInput)?,
                    parameters.event_queue_num.ok_or(CommandQueueError::MissingInput)?,
                    parameters.doorbell_page.ok_or(CommandQueueError::MissingInput)?,
                    parameters.collapsed_cq
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);

            },
            CommandOpcode::CreateTis => {
                const NUM_INPUT_MAILBOXES_CREATE_TIS: usize = 1;
                self.initialize_mailboxes(NUM_INPUT_MAILBOXES_CREATE_TIS,&mut input_mailbox_buffers)?;

                const TIS_MAILBOX_INDEX: usize = 0;
                Self::write_transport_interface_send_context_to_mailbox(
                    &mut input_mailbox_buffers[TIS_MAILBOX_INDEX],
                    parameters.transport_domain.ok_or(CommandQueueError::MissingInput)?
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);
            },
            CommandOpcode::CreateSq => {
                let pages_pa = parameters.allocated_pages.ok_or(CommandQueueError::MissingInputPages)?;

                let size_of_mailbox_data = 0x10 + 0x30 + 0xC0 + SIZE_PADDR_IN_BYTES * pages_pa.len();
                let num_mailboxes = libm::ceilf(size_of_mailbox_data as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
                self.initialize_mailboxes(num_mailboxes, &mut input_mailbox_buffers)?;

                Self::write_send_queue_context_to_mailbox(
                    &mut input_mailbox_buffers,
                    pages_pa, 
                    parameters.completion_queue_num.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.transport_interface_send_num.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.protection_domain.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.user_access_region.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.doorbell_page.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.log_queue_size.ok_or(CommandQueueError::MissingInput)?
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);

            },
            CommandOpcode::ModifySq => {
                let sq_state = 0 << 28;
                cmdq_entry.set_input_inline_data_0(sq_state | parameters.send_queue_num.ok_or(CommandQueueError::MissingInput)?);

                const NUM_INPUT_MAILBOXES_MODIFY_SQ: usize = 1;
                self.initialize_mailboxes(NUM_INPUT_MAILBOXES_MODIFY_SQ, &mut input_mailbox_buffers)?;
        
                const SQ_CONTEXT_MAILBOX_INDEX: usize = 0;
                Self::modify_sq_state(
                    &mut input_mailbox_buffers[SQ_CONTEXT_MAILBOX_INDEX],
                    parameters.completion_queue_num.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.transport_interface_send_num.ok_or(CommandQueueError::MissingInput)?, 
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);

            },
            CommandOpcode::QuerySq => {
                cmdq_entry.set_input_inline_data_0(parameters.send_queue_num.ok_or(CommandQueueError::MissingInput)?);
                const NUM_OUTPUT_MAILBOXES_QUERY_SQ: usize = 1;
                self.initialize_mailboxes(NUM_OUTPUT_MAILBOXES_QUERY_SQ, &mut output_mailbox_buffers)?;
                cmdq_entry.set_output_mailbox_pointer(output_mailbox_buffers[0].addr);
            },
            CommandOpcode::CreateRq => {
                let pages_pa = parameters.allocated_pages.ok_or(CommandQueueError::MissingInputPages)?;

                let size_of_mailbox_data = 0x10 + 0x30 + 0xC0 + SIZE_PADDR_IN_BYTES * pages_pa.len();
                let num_mailboxes = libm::ceilf(size_of_mailbox_data as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
                self.initialize_mailboxes(num_mailboxes, &mut input_mailbox_buffers)?;

                Self::write_receive_queue_context_to_mailbox(
                    &mut input_mailbox_buffers,
                    pages_pa, 
                    parameters.completion_queue_num.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.protection_domain.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.doorbell_page.ok_or(CommandQueueError::MissingInput)?, 
                    parameters.log_queue_size.ok_or(CommandQueueError::MissingInput)?
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);
            }
            CommandOpcode::ModifyRq => {
                let rq_state = 0 << 28;
                cmdq_entry.set_input_inline_data_0(rq_state | parameters.receive_queue_num.ok_or(CommandQueueError::MissingInput)?);

                const NUM_INPUT_MAILBOXES_MODIFY_RQ: usize = 1;
                self.initialize_mailboxes(NUM_INPUT_MAILBOXES_MODIFY_RQ, &mut input_mailbox_buffers)?;
        
                const RQ_CONTEXT_MAILBOX_INDEX: usize = 0;
                Self::modify_rq_state(
                    &mut input_mailbox_buffers[RQ_CONTEXT_MAILBOX_INDEX]
                )?;
                cmdq_entry.set_input_mailbox_pointer(input_mailbox_buffers[0].addr);

            },
            // CommandOpcode::QuerySq => {
            //     cmdq_entry.set_input_inline_data_0(parameters.send_queue_num.ok_or(CommandQueueError::MissingInput)?);
            //     const NUM_OUTPUT_MAILBOXES_QUERY_SQ: usize = 1;
            //     self.initialize_mailboxes(NUM_OUTPUT_MAILBOXES_QUERY_SQ, &mut output_mailbox_buffers)?;
            //     cmdq_entry.set_output_mailbox_pointer(output_mailbox_buffers[0].addr);
            // },
            _=> {
                error!("unimplemented opcode");
                return Err(CommandQueueError::UnimplementedOpcode);
            }
        }    
        
        // Write the command to the actual place in memory, 
        // and return the command object indicating an initialized command queue entry which is still in SW ownership
        let initialized_entry = InitializedCommand::new(entry_num, cmdq_entry, input_mailbox_buffers, output_mailbox_buffers, &mut self.entries);
        
        // update token to be used in the next command
        self.token = self.token.wrapping_add(1);

        // claim the command entry as in use
        self.available_entries[entry_num] = false;

        debug!("command INPUT: {:?}", parameters.opcode);
        self.entries[entry_num].dump_command();
        for mb in &initialized_entry.input_mailbox_buffers {
            mb.mp.as_type::<CommandInterfaceMailbox>(0)
                .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?
                .dump_mailbox()
        }

        Ok(initialized_entry)
    }
    
    /// Allocates all the mailboxes required for a command and then initializes all the non-data fields (token, block number and next address fields).
    fn initialize_mailboxes(&mut self, num_mailboxes: usize, command_mailbox_buffers: &mut Vec<MailboxBuffer>) -> Result<(), CommandQueueError> {
        // Add pre-allocated mailbox buffers to the mailbox list for this command and add extra mailbox pages if required.      
        // We create the list first since each mailbox needs to know the starting address of the next mailbox.
        for _ in 0..num_mailboxes {
            if let Some(mb_buffer) = self.mailbox_buffers.pop() {
                command_mailbox_buffers.push(mb_buffer);
            } else {
                let (mp, addr) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)
                    .map_err(|_e| CommandQueueError::PageAllocationFailed)?;
                command_mailbox_buffers.push(MailboxBuffer{mp, addr});
            }
        }
        // Initialize the mailboxes
        for block_num in 0..num_mailboxes {
            // record the next page address to set pointer for the next mailbox
            let next_mb_addr = if block_num < (num_mailboxes - 1) {
                command_mailbox_buffers[block_num + 1].addr.value()
            } else {
                0
            };
            // initialize the mailbox fields except for data.
            let mailbox = command_mailbox_buffers[block_num].mp.as_type_mut::<CommandInterfaceMailbox>(MAILBOX_OFFSET_IN_PAGE)
                .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
            mailbox.init(block_num as u32, self.token, next_mb_addr);
        }
        Ok(())
    }
    
    /*** Functions to write input values to mailboxes ***/

    /// Initialize input mailboxes with the driver version for the [`CommandOpcode::SetDriverVersion`] command.
    fn write_driver_version_to_mailbox(input_mailbox_buffers: &mut Vec<MailboxBuffer>) -> Result<(), CommandQueueError> {
        let driver_version = "Theseus,mlx5,1.0.0".as_bytes(); // TODO: take as argument
        let mut buffer: [u8; 512] = [0; 512];
        buffer[0..driver_version.len()].copy_from_slice(driver_version);
        
        let mailbox = input_mailbox_buffers[0].mp.as_type_mut::<CommandInterfaceMailbox>(0)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        mailbox.mailbox_data.write(buffer);
        Ok(())
    }
    
    /// Initialize input mailboxes with the physical addresses of pages passed to the NIC.
    fn write_page_addrs_to_mailboxes(input_mailbox_buffers: &mut [MailboxBuffer], mut pages: Vec<PhysicalAddress>) -> Result<(), CommandQueueError> {
        for mb_buffer in input_mailbox_buffers {  
            let mb_page = &mut mb_buffer.mp;
            let mailbox_data = mb_page.as_type_mut::<[u8; MAILBOX_DATA_SIZE_IN_BYTES]>(MAILBOX_OFFSET_IN_PAGE)
                .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
            
            Self::write_page_addrs_to_data_buffer(mailbox_data, &mut pages);
        }
        Ok(())
    }
    
    /// Fills a data buffer with physical address values in the format specified by the PRM.
    fn write_page_addrs_to_data_buffer(data_buffer: &mut [u8], pages:  &mut Vec<PhysicalAddress>) {
        let paddr_per_buffer = data_buffer.len() / SIZE_PADDR_IN_BYTES;
        for paddr_index in 0..paddr_per_buffer {
            if let Some(paddr) = pages.pop() {
                let start_offset = paddr_index * SIZE_PADDR_IN_BYTES;
                Self::write_page_addr(paddr, &mut data_buffer[start_offset..]);
            } else {
                break;
            }
        }
    }
    
    /// Writes the physical address `paddr` to the beginning of the `data` buffer.
    /// The format specified in the PRM is that the 4 higher bytes are written first at the starting address in big endian format,
    /// then the four lower bytes are written in the next 4 bytes in big endian format.
    fn write_page_addr(paddr: PhysicalAddress, data: &mut [u8]) {
        let end_offset_h = SIZE_PADDR_H_IN_BYTES;
        let addr = (paddr.value() >> 32) as u32;
        data[0..end_offset_h].copy_from_slice(&addr.to_be_bytes());

        let start_offset_l = end_offset_h;
        let end_offset_l = start_offset_l + SIZE_PADDR_L_IN_BYTES;
        let addr = (paddr.value() & 0xFFFF_FFFF) as u32;
        data[start_offset_l..end_offset_l].copy_from_slice(&addr.to_be_bytes());
    }

    fn write_event_queue_context_to_mailbox(
        input_mailbox_buffers: &mut Vec<MailboxBuffer>,
        mut pages: Vec<PhysicalAddress>, 
        uar: u32, 
        log_eq_size: u8
    ) -> Result<(), CommandQueueError> 
    {    
        const EVENT_QUEUE_CONTEXT_MAILBOX_INDEX: usize = 0;
        let mb_page = &mut input_mailbox_buffers[EVENT_QUEUE_CONTEXT_MAILBOX_INDEX].mp;
        
        // initialize the event queue context
        const EVENT_QUEUE_CONTEXT_OFFSET: usize = 0;
        let eq_context = mb_page.as_type_mut::<EventQueueContext>(EVENT_QUEUE_CONTEXT_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        eq_context.init(uar, log_eq_size);

        // initialize the bitmask. this function only activates the page request event
        const BITMASK_OFFSET: usize  = 0x58 - 0x10;
        let eq_bitmask = mb_page.as_type_mut::<U64<BigEndian>>(BITMASK_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        const PAGE_REQUEST_BIT: u64 = 1 << 0xB;
        *eq_bitmask = U64::new(PAGE_REQUEST_BIT);

        // Now use the remainder of the mailbox for page entries
        const EQ_PADDR_OFFSET: usize = 0x110 - 0x10;
        let data_buffer = mb_page.as_type_mut::<[u8;256]>(EQ_PADDR_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        Self::write_page_addrs_to_data_buffer(data_buffer, &mut pages);

        // Write the remaining addresses to the next mailboxes
        Self::write_page_addrs_to_mailboxes(&mut input_mailbox_buffers[1..], pages)?;
        Ok(())
    }


    fn write_completion_queue_context_to_mailbox(
        input_mailbox_buffers: &mut Vec<MailboxBuffer>,
        mut pages: Vec<PhysicalAddress>, 
        uar: u32, 
        log_cq_size: u8,
        c_eqn: u8,
        doorbell_pa: PhysicalAddress,
        collapsed: bool
    ) -> Result<(), CommandQueueError> {
        const COMPLETION_QUEUE_CONTEXT_MAILBOX_INDEX: usize = 0;
        let mb_page = &mut input_mailbox_buffers[COMPLETION_QUEUE_CONTEXT_MAILBOX_INDEX].mp;
        
        // initialize the completion queue context
        const COMPLETION_QUEUE_CONTEXT_OFFSET: usize = 0;
        let cq_context = mb_page.as_type_mut::<CompletionQueueContext>(COMPLETION_QUEUE_CONTEXT_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        cq_context.init(uar, log_cq_size, c_eqn, doorbell_pa, collapsed);

        // Now use the remainder of the mailbox for page entries
        const CQ_PADDR_OFFSET: usize = 0x110 - 0x10;
        let data_buffer = mb_page.as_type_mut::<[u8;256]>(CQ_PADDR_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        Self::write_page_addrs_to_data_buffer(data_buffer, &mut pages);

        // Write the remaining addresses to the next mailboxes
        Self::write_page_addrs_to_mailboxes(&mut input_mailbox_buffers[1..], pages)?;

        Ok(())
    }

    fn write_transport_interface_send_context_to_mailbox(input_mailbox_buffer: &mut MailboxBuffer, td: u32) -> Result<(), CommandQueueError> {
        const TIS_OFFSET: usize = 0x10;
        // initialize the TIS context
        let tis_context = input_mailbox_buffer.mp.as_type_mut::<TransportInterfaceSendContext>(TIS_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        tis_context.init(td);
        Ok(())
    }

    fn write_send_queue_context_to_mailbox(
        input_mailbox_buffers: &mut Vec<MailboxBuffer>,
        mut pages: Vec<PhysicalAddress>, 
        cqn: u32,
        tisn: u32,
        pd: u32,
        uar_page: u32,
        db_addr: PhysicalAddress,
        log_wq_size: u8
    ) -> Result<(), CommandQueueError> {
        const SEND_QUEUE_CONTEXT_MAILBOX_INDEX: usize = 0;
        let mb_page = &mut input_mailbox_buffers[SEND_QUEUE_CONTEXT_MAILBOX_INDEX].mp;
            
        // initialize the send queue context
        const SEND_QUEUE_CONTEXT_OFFSET: usize = 0x10;
        let sq_context = mb_page.as_type_mut::<SendQueueContext>(SEND_QUEUE_CONTEXT_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        sq_context.init(cqn, tisn);

        // initialize the work queue
        const WORK_QUEUE_OFFSET: usize = 0x10 + 0x30;
        let wq = mb_page.as_type_mut::<WorkQueue>(WORK_QUEUE_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        wq.init_sq(pd, uar_page, db_addr, log_wq_size);

        // Now use the remainder of the mailbox for page entries
        const SQ_PADDR_OFFSET: usize = 0x10 + 0x30 + 0xC0;
        let data_buffer = mb_page.as_type_mut::<[u8;256]>(SQ_PADDR_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        Self::write_page_addrs_to_data_buffer(data_buffer, &mut pages);

        // Write the remaining addresses to the next mailboxes
        Self::write_page_addrs_to_mailboxes(&mut input_mailbox_buffers[1..], pages)?;
        Ok(())
    }

    fn modify_sq_state(input_mailbox_buffer: &mut MailboxBuffer, cqn: u32, tisn: u32) -> Result<(), CommandQueueError> {        
        const SEND_QUEUE_CONTEXT_OFFSET:usize = 0x10;
        // initialize the TIS context
        let sq_context = input_mailbox_buffer.mp.as_type_mut::<SendQueueContext>(SEND_QUEUE_CONTEXT_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        // sq_context.init(cqn, tisn);
        sq_context.set_state(SendQueueState::Ready);

        Ok(())
    }

    fn modify_rq_state(input_mailbox_buffer: &mut MailboxBuffer) -> Result<(), CommandQueueError> {        
        const RECEIVE_QUEUE_CONTEXT_OFFSET:usize = 0x10;
        let rq_context = input_mailbox_buffer.mp.as_type_mut::<ReceiveQueueContext>(RECEIVE_QUEUE_CONTEXT_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        // sq_context.init(cqn, tisn);
        rq_context.set_state(ReceiveQueueState::Ready);

        Ok(())
    }

    fn write_receive_queue_context_to_mailbox(
        input_mailbox_buffers: &mut Vec<MailboxBuffer>,
        mut pages: Vec<PhysicalAddress>, 
        cqn: u32,
        pd: u32,
        db_addr: PhysicalAddress,
        log_wq_size: u8
    ) -> Result<(), CommandQueueError> {
        const RECEIVE_QUEUE_CONTEXT_MAILBOX_INDEX: usize = 0;
        let mb_page = &mut input_mailbox_buffers[RECEIVE_QUEUE_CONTEXT_MAILBOX_INDEX].mp;
            
        // initialize the send queue context
        const RECEIVE_QUEUE_CONTEXT_OFFSET: usize = 0x10;
        let rq_context = mb_page.as_type_mut::<ReceiveQueueContext>(RECEIVE_QUEUE_CONTEXT_OFFSET)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        rq_context.init(cqn);

        // initialize the work queue
        const WORK_QUEUE_OFFSET: usize = 0x10 + 0x30;
        let wq = mb_page.as_type_mut::<WorkQueue>(WORK_QUEUE_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        wq.init_rq(pd, db_addr, log_wq_size);

        // Now use the remainder of the mailbox for page entries
        const SQ_PADDR_OFFSET: usize = 0x10 + 0x30 + 0xC0;
        let data_buffer = mb_page.as_type_mut::<[u8;256]>(SQ_PADDR_OFFSET).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        Self::write_page_addrs_to_data_buffer(data_buffer, &mut pages);

        // Write the remaining addresses to the next mailboxes
        Self::write_page_addrs_to_mailboxes(&mut input_mailbox_buffers[1..], pages)?;
        Ok(())
    }

    /*** Functions to retrieve output values ***/

    /// Waits for ownership bit to be cleared, and then returns the command delivery status and the command return status.
    pub fn wait_for_command_completion(&mut self, command: PostedCommand) -> CompletedCommand {
        while self.entries[command.entry_num].owned_by_hw() {}
        command.complete()
    }

    pub fn get_command_status(&mut self, command: CompletedCommand) -> Result<CommandCompletionStatus, CommandQueueError> {
        debug!("command OUTPUT");
        
        self.entries[command.entry_num].dump_command();
        for mb in command.output_mailbox_buffers {
            mb.mp.as_type::<CommandInterfaceMailbox>(0)
                .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?
                .dump_mailbox()
        }

        self.available_entries[command.entry_num] = true;
        let delivery_status = self.entries[command.entry_num].get_delivery_status()?;
        let return_status = self.entries[command.entry_num].get_return_status()?;
        Ok(CommandCompletionStatus{ delivery_status, return_status })
    }

    /// When retrieving the output data of a command, checks that the correct opcode is written and that the command has completed.
    fn check_command_output_validity(&self, entry_num: usize, cmd_opcode: CommandOpcode) -> Result<(), CommandQueueError> {
        if self.entries[entry_num].owned_by_hw() {
            error!("the command hasn't completed yet!");
            return Err(CommandQueueError::CommandNotCompleted);
        }
        if self.entries[entry_num].get_command_opcode()? != cmd_opcode {
            error!("Incorrect Command!: {:?}", self.entries[entry_num].get_command_opcode()?);
            return Err(CommandQueueError::IncorrectCommandOpcode);
        }
        Ok(())
    }

    /// Get the number of pages requested by the NIC, which is the output of the [`CommandOpcode::QueryHcaCap`] command.  
    pub fn get_port_type(&mut self, command: CompletedCommand) -> Result<(HcaPortType, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QueryHcaCap)?;

        const DATA_OFFSET_IN_MAILBOX: usize = 0x34; 
        let mailbox = &command.output_mailbox_buffers[0].mp;
        let port_type = (
            mailbox.as_type::<U32<BigEndian>>(DATA_OFFSET_IN_MAILBOX)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?
        ).get();
        HcaPortType::try_from(((port_type & 0x300) >> 8) as u8)
            .map_err(|_e| CommandQueueError::InvalidPortType)
            .and_then(|port_type| Ok((port_type, self.get_command_status(command)?)))
    }

    /// Get the device capabilities, which is the output of the [`CommandOpcode::QueryHcaCap`] command.  
    pub fn get_device_capabilities(&mut self, command: CompletedCommand) -> Result<(HCACapabilities, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QueryHcaCap)?;

        const DATA_OFFSET_IN_MAILBOX: usize = 0; 
        let mailbox = &command.output_mailbox_buffers[0].mp;
        let capabilities = mailbox.as_type::<HCACapabilitiesLayout>(DATA_OFFSET_IN_MAILBOX)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;

        Ok((capabilities.get_capabilities(), self.get_command_status(command)?))
    }

    /// Get the current ISSI version and the supported ISSI versions, which is the output of the [`CommandOpcode::QueryIssi`] command.  
    pub fn get_query_issi_command_output(&mut self, command: CompletedCommand) -> Result<(u16, u8, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QueryIssi)?;

        const DATA_OFFSET_IN_MAILBOX: usize = 0x6C - 0x10; 
        let mailbox = &command.output_mailbox_buffers[0].mp;
        let supported_issi = (
            mailbox.as_type::<U32<BigEndian>>(DATA_OFFSET_IN_MAILBOX)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?
        ).get();

        let current_issi = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((current_issi as u16, supported_issi as u8, self.get_command_status(command)?))
    }

    /// Get the number of pages requested by the NIC, which is the output of the [`CommandOpcode::QueryPages`] command.  
    pub fn get_query_pages_command_output(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QueryPages)?;

        let num_pages = self.entries[command.entry_num].get_output_inline_data_1();
        Ok((num_pages, self.get_command_status(command)?))
    }

    /// Get the User Access Region (UAR) number, which is the output of the [`CommandOpcode::AllocUar`] command.  
    pub fn get_uar(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::AllocUar)?;
        let uar = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((uar & 0xFF_FFFF, self.get_command_status(command)?))
    }

    /// Get the protection domain number, which is the output of the [`CommandOpcode::AllocPd`] command.  
    pub fn get_protection_domain(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::AllocPd)?;

        let pd = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((pd & 0xFF_FFFF, self.get_command_status(command)?))
    }

    /// Get the transport domain number, which is the output of the [`CommandOpcode::AllocTransportDomain`] command.  
    pub fn get_transport_domain(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::AllocTransportDomain)?;

        let td = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((td & 0xFF_FFFF, self.get_command_status(command)?))
    }

    /// Get the value of the reserved Lkey for Base Memory Management Extension, which is used when we are using physical addresses.
    /// It is taken as the output of the [`CommandOpcode::QuerySpecialContexts`] command.
    pub fn get_reserved_lkey(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QuerySpecialContexts)?;

        let resd_lkey = self.entries[command.entry_num].get_output_inline_data_1();
        Ok((resd_lkey, self.get_command_status(command)?))
    }

    /// Get the Vport state in the format (max_tx_speed, admin_state, state)
    /// It is taken as the output of the [`CommandOpcode::QueryVportState`] command.
    pub fn get_vport_state(&mut self, command: CompletedCommand) -> Result<(u16, u8, u8, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QueryVportState)?;

        let state = self.entries[command.entry_num].get_output_inline_data_1();
        Ok(((state  >> 16) as u16, (state as u8) >> 4, state as u8 & 0xF, self.get_command_status(command)?))
    }

    /// Get the port mac address, which is the output of the [`CommandOpcode::QueryNicVportContext`] command.  
    pub fn get_vport_mac_address(&mut self, command: CompletedCommand) -> Result<([u8; 6], CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QueryNicVportContext)?;

        const DATA_OFFSET_IN_MAILBOX: usize = 0x0;
        let mailbox = &command.output_mailbox_buffers[0].mp;
        let nic_vport_context = mailbox.as_type::<NicVportContext>(DATA_OFFSET_IN_MAILBOX).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        let mac_address_h = nic_vport_context.permanent_address_h.read().get();
        let mac_address_l = nic_vport_context.permanent_address_l.read().get();

        Ok(([
            (mac_address_h >> 8) as u8,
            (mac_address_h) as u8,
            (mac_address_l >> 24) as u8,
            (mac_address_l >> 16) as u8,
            (mac_address_l >> 8) as u8,
            mac_address_l as u8
        ], self.get_command_status(command)?))
    }

    pub fn get_eq_number(&mut self, command: CompletedCommand) -> Result<(u8, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::CreateEq)?;

        let eq_number = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((eq_number as u8, self.get_command_status(command)?))
    }

    pub fn get_cq_number(&mut self, command:CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::CreateCq)?;

        let cq_number = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((cq_number & 0xFF_FFFF, self.get_command_status(command)?))
    }

    pub fn get_tis_context_number(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError>  {
        self.check_command_output_validity(command.entry_num, CommandOpcode::CreateTis)?;
        
        let tisn = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((tisn & 0xFF_FFFF, self.get_command_status(command)?))
    }

    pub fn get_send_queue_number(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError>  {
        self.check_command_output_validity(command.entry_num, CommandOpcode::CreateSq)?;

        let sqn = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((sqn & 0xFF_FFFF, self.get_command_status(command)?))
    }

    pub fn get_receive_queue_number(&mut self, command: CompletedCommand) -> Result<(u32, CommandCompletionStatus), CommandQueueError>  {
        self.check_command_output_validity(command.entry_num, CommandOpcode::CreateRq)?;

        let rqn = self.entries[command.entry_num].get_output_inline_data_0();
        Ok((rqn & 0xFF_FFFF, self.get_command_status(command)?))
    }

    pub fn get_sq_state(&mut self, command: CompletedCommand) -> Result<(u8, CommandCompletionStatus), CommandQueueError> {
        self.check_command_output_validity(command.entry_num, CommandOpcode::QuerySq)?;

        let mailbox = &command.output_mailbox_buffers[0];
        let sq_context = mailbox.mp.as_type::<SendQueueContext>(0x10).map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
        let state = sq_context.get_state();
        Ok((state, self.get_command_status(command)?))
    }
}

/// Layout of a command passed to the NIC.
/// The fields include control information for the command as well as actual command input and output.
/// The first 16 bytes of the actual command input are part of the entry. The remaining data is written in mailboxes.
/// Similarly, the first 16 bytes of the command output are part of the entry and remaining data is written in mailboxes.
#[derive(FromBytes,Default)]
#[repr(C)]
pub struct CommandQueueEntry {
    /// Type of transport that carries the command
    type_of_transport:              Volatile<U32<BigEndian>>,
    /// Input command length in bytes.
    input_length:                   Volatile<U32<BigEndian>>,
    /// Pointer to input mailbox, upper 4 bytes
    input_mailbox_pointer_h:        Volatile<U32<BigEndian>>,
    /// Pointer to input mailbox, lower 4 bytes
    input_mailbox_pointer_l:        Volatile<U32<BigEndian>>,
    /// Command opcode
    command_input_opcode:           Volatile<U32<BigEndian>>,
    /// Opcode modifier for the command
    command_input_opmod:            Volatile<U32<BigEndian>>,
    /// First 4 bytes of command input data.
    command_input_inline_data_0:    Volatile<U32<BigEndian>>,
    /// Second 4 bytes of command input data.
    command_input_inline_data_1:    Volatile<U32<BigEndian>>,
    /// Command return status
    command_output_status:          Volatile<U32<BigEndian>>,
    /// Syndrome on the command, valid only if status = 0x0
    command_output_syndrome:        Volatile<U32<BigEndian>>,
    /// First 4 bytes of command output data, valid only if status = 0x0
    command_output_inline_data_0:   Volatile<U32<BigEndian>>,
    /// Second 4 bytes of command output data, valid only if status = 0x0
    command_output_inline_data_1:   Volatile<U32<BigEndian>>,
    /// Pointer to output mailbox, upper 4 bytes
    output_mailbox_pointer_h:       Volatile<U32<BigEndian>>,
    /// Pointer to output mailbox, lower 4 bytes
    output_mailbox_pointer_l:       Volatile<U32<BigEndian>>,
    /// Output command length in bytes
    output_length:                  Volatile<U32<BigEndian>>,
    /// * Token: Token of the command, should have the same value in the command and the mailbox blocks. 
    /// * Signature: 8 bit signature of the command queue entry.
    /// * Status: command delivery status.
    /// * Ownership: bit 0 of field. When set, indicates that HW owns the command entry.
    token_signature_status_own:     Volatile<U32<BigEndian>>
}

const_assert_eq!(core::mem::size_of::<CommandQueueEntry>(), 64);

impl fmt::Debug for CommandQueueEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CommandQueueEntry")
            .field("type of transport", &self.type_of_transport.read().get())
            .field("input length", &self.input_length.read().get())
            .field("input_mailbox_ptr_h", &self.input_mailbox_pointer_h.read().get())
            .field("input_mailbox_ptr_l", &self.input_mailbox_pointer_l.read().get())
            .field("command_input_opcode",&self.command_input_opcode.read().get())
            .field("command_input_opmod",&self.command_input_opmod.read().get())
            .field("command_input_inline_data_0",&self.command_input_inline_data_0.read().get())
            .field("command_input_inline_data_1",&self.command_input_inline_data_1.read().get())
            .field("command_output_status",&self.command_output_status.read().get())
            .field("command_output_syndrome",&self.command_output_syndrome.read().get())
            .field("command_output_inline_data_0",&self.command_output_inline_data_0.read().get())
            .field("command_output_inline_data_1",&self.command_output_inline_data_1.read().get())
            .field("output_mailbox_pointer_h",&self.output_mailbox_pointer_h.read().get())
            .field("output_mailbox_pointer_l",&self.output_mailbox_pointer_l.read().get())
            .field("output_length",&self.output_length.read().get())
            .field("token_signature_status_own",&self.token_signature_status_own.read().get())
            .finish()
    }
}

impl CommandQueueEntry {
    /// Creates a command queue entry and initializes it for the given `opcode` and `opmod`.
    /// ## Arguments
    /// * `opcode`: value identifying which command has to be carried out
    /// * `opmod`: opcode modifier. If None, field will be set to zero.
    /// * `token`: token of the command, a random value that should be the same in the command entry and all linked mailboxes.
    /// * `num_pages`: number of 4 KiB pages being to passed to the HW with this command.
    fn init(opcode: CommandOpcode, opmod: Option<u16>, token: u8, num_pages: Option<usize>) -> Result<Self, CommandQueueError> {
        let mut cmdq_entry = Self::default();

        // set the type of transport 
        cmdq_entry.type_of_transport.write(U32::new(CommandTransportType::PCIe as u32));
        
        // Sets the token of the command.
        // This is a random value that should be the same in the command entry and all linked mailboxes.
        let val = cmdq_entry.token_signature_status_own.read().get();
        cmdq_entry.token_signature_status_own.write(U32::new(val | ((token as u32) << 24)));

        // set the ownership bit to HW-owned.
        // This bit will be cleared when the command is complete.
        cmdq_entry.change_ownership_to_hw();
        
        // Sets length of input data in bytes. This value is different for every command, and can be taken from Chapter 23 of the PRM. 
        cmdq_entry.input_length.write(U32::new(opcode.input_bytes(num_pages)?));

        // Sets length of output data in bytes. This value is different for every command, and can be taken from Chapter 23 of the PRM. 
        cmdq_entry.output_length.write(U32::new(opcode.output_bytes()?));

        cmdq_entry.command_input_opcode.write(U32::new((opcode as u32) << 16));
        cmdq_entry.command_input_opmod.write(U32::new(opmod.unwrap_or(0) as u32));

        Ok(cmdq_entry)
    }

    /// Sets the first 4 bytes of actual command input data that are written inline in the command.
    /// The valid values for each field are different for every command, and can be taken from Chapter 23 of the PRM.
    ///
    /// ## Arguments
    /// * `command0`: the first 4 bytes of actual command data.
    fn set_input_inline_data_0(&mut self, command0: u32) {
        self.command_input_inline_data_0.write(U32::new(command0));
    }

    /// Sets the second 4 bytes of actual command input data that are written inline in the command.
    /// The valid values for each field are different for every command, and can be taken from Chapter 23 of the PRM.
    ///
    /// ## Arguments
    /// * `command1`: the second 4 bytes of actual command data.
    fn set_input_inline_data_1(&mut self, command1: u32) {
        self.command_input_inline_data_1.write(U32::new(command1));
    }

    /// Sets the mailbox pointer in a command entry with the physical address of the first mailbox.
    fn set_input_mailbox_pointer(&mut self, mailbox_ptr: PhysicalAddress) {
        self.input_mailbox_pointer_h.write(U32::new((mailbox_ptr.value() >> 32) as u32));
        self.input_mailbox_pointer_l.write(U32::new((mailbox_ptr.value() & 0xFFFF_FFFF) as u32));
    }

    /// Sets the mailbox pointer in a command entry with the physical address of the first mailbox.
    fn set_output_mailbox_pointer(&mut self, mailbox_ptr: PhysicalAddress) {
        self.output_mailbox_pointer_h.write(U32::new((mailbox_ptr.value() >> 32) as u32));
        self.output_mailbox_pointer_l.write(U32::new((mailbox_ptr.value() & 0xFFFF_FFFF) as u32));
    }

    /// Returns the value written to the input opcode field of the command.
    fn get_command_opcode(&self) -> Result<CommandOpcode, CommandQueueError> {
        let opcode = self.command_input_opcode.read().get() >> 16;
        CommandOpcode::try_from(opcode).map_err(|_e| CommandQueueError::InvalidCommandOpcode)
    }

    /// Returns the first 16 bytes of output data that are written inline in the command.
    /// The valid values for each field are different for every command, and can be taken from Chapter 23 of the PRM.
    fn get_output_inline_data(&self) -> (u8, u32, u32, u32) {
        (   
            (self.command_output_status.read().get() >> 24) as u8,
            self.command_output_syndrome.read().get(),
            self.command_output_inline_data_0.read().get(),
            self.command_output_inline_data_1.read().get()
        )
    }

    /// Gets the first 4 bytes of actual command output data that are written inline in the command.
    /// The valid values for each field are different for every command, and can be taken from Chapter 23 of the PRM.
    fn get_output_inline_data_0(&self) -> u32 {
        self.command_output_inline_data_0.read().get()
    }

    /// Gets the second 4 bytes of actual command output data that are written inline in the command.
    /// The valid values for each field are different for every command, and can be taken from Chapter 23 of the PRM.
    fn get_output_inline_data_1(&self) -> u32 {
        self.command_output_inline_data_1.read().get()
    }
   
    /// Returns the status of command delivery.
    /// This only informs us if the command was delivered to the NIC successfully, not if it was completed successfully.
    pub fn get_delivery_status(&self) -> Result<CommandDeliveryStatus, CommandQueueError> {
        let status = (self.token_signature_status_own.read().get() & 0xFE) >> 1;
        CommandDeliveryStatus::try_from(status).map_err(|_e| CommandQueueError::InvalidCommandDeliveryStatus)
    }

    /// Sets the ownership bit so that HW can take control of the command entry
    fn change_ownership_to_hw(&mut self) {
        let ownership = self.token_signature_status_own.read().get() | 0x1;
        self.token_signature_status_own.write(U32::new(ownership));
    }

    /// Returns true if the command is currently under the ownership of HW (SW should not touch the fields).
    pub fn owned_by_hw(&self) -> bool {
        self.token_signature_status_own.read().get().get_bit(0)
    }

    /// Returns the status of command execution.
    /// A `None` returned value indicates that there was no valid value in the bitfield.
    pub fn get_return_status(&self) -> Result<CommandReturnStatus, CommandQueueError> {
        let (status, _syndrome, _, _) = self.get_output_inline_data();
        CommandReturnStatus::try_from(status).map_err(|_e| CommandQueueError::InvalidCommandReturnStatus)
    }

    fn dump_command(&self) {
        unsafe {
            let ptr = self as *const CommandQueueEntry as *const u32;
            debug!("000: {:#010X} {:#010X} {:#010X} {:#010X}", (*ptr).to_be(), (*ptr.offset(1)).to_be(), (*ptr.offset(2)).to_be(), (*ptr.offset(3)).to_be());
            debug!("010: {:#010X} {:#010X} {:#010X} {:#010X}", (*ptr.offset(4)).to_be(), (*ptr.offset(5)).to_be(), (*ptr.offset(6)).to_be(), (*ptr.offset(7)).to_be());
            debug!("020: {:#010X} {:#010X} {:#010X} {:#010X}", (*ptr.offset(8)).to_be(), (*ptr.offset(9)).to_be(), (*ptr.offset(10)).to_be(), (*ptr.offset(11)).to_be());
            debug!("030: {:#010X} {:#010X} {:#010X} {:#010X}", (*ptr.offset(12)).to_be(), (*ptr.offset(13)).to_be(), (*ptr.offset(14)).to_be(), (*ptr.offset(15)).to_be());
        }
    }
}


/// Layout of mailbox used to pass extra input and output command data that doesn't fit into the command entry.
#[derive(FromBytes)]
#[repr(C)]
struct CommandInterfaceMailbox {
    /// Data in the mailbox
    mailbox_data:           Volatile<[u8; 512]>,
    _padding:               [u8; 48],
    /// MSBs of pointer to the next mailbox page (if needed). 
    /// If no additional block is needed, the pointer should be 0.
    next_pointer_h:         Volatile<U32<BigEndian>>,
    /// LSBs of pointer to the next mailbox page (if needed). 
    /// If no additional block is needed, the pointer should be 0.
    next_pointer_l:         Volatile<U32<BigEndian>>,
    /// Sequence number of the block.
    /// Starting by 0 and increment for each block on the linked list of blocks.
    block_number:           Volatile<U32<BigEndian>>,
    /// Token of the command.
    /// Should have the same value in the command and the mailbox blocks.
    token_ctrl_signature:   Volatile<U32<BigEndian>>
}
const_assert_eq!(core::mem::size_of::<CommandInterfaceMailbox>(), MAILBOX_SIZE_IN_BYTES);

impl CommandInterfaceMailbox {
    /// Sets all fields of the mailbox to 0.
    fn clear_all_fields(&mut self) {
        self. mailbox_data.write([0;512]);
        self.next_pointer_h.write(U32::new(0));
        self.next_pointer_l.write(U32::new(0));
        self.block_number.write(U32::new(0));
        self.token_ctrl_signature.write(U32::new(0));
    }

    fn init(&mut self, block_num: u32, token: u8, next_mb_addr: usize) {
        self.clear_all_fields();
        self.block_number.write(U32::new(block_num));
        self.token_ctrl_signature.write(U32::new((token as u32) << 16));
        self.next_pointer_h.write(U32::new((next_mb_addr >> 32) as u32));
        self.next_pointer_l.write(U32::new((next_mb_addr & 0xFFFF_FFFF) as u32));
    }

    fn dump_mailbox(&self) {
        unsafe {
            let ptr = self as *const CommandInterfaceMailbox as *const u32;
            for i in 0..MAILBOX_SIZE_IN_BYTES/16 {
                let x = (i * 4) as isize;
                debug!("{:#03}: {:#010X} {:#010X} {:#010X} {:#010X}", i*16, (*ptr.offset(x)).to_be(), (*ptr.offset(x+1)).to_be(), (*ptr.offset(x+2)).to_be(), (*ptr.offset(x+3)).to_be());
            }
        }
    }
}

impl fmt::Debug for CommandInterfaceMailbox {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CommandInterfaceMailbox")
            .field("mailbox_data", &self.mailbox_data.read())
            .field("next pointer h", &self.next_pointer_h.read().get())
            .field("next pointer l", &self.next_pointer_l.read().get())
            .field("block number", &self.block_number.read().get())
            .field("token ctrl signature", &self.token_ctrl_signature.read().get())
            .finish()
    }
}

#[derive(FromBytes)]
#[repr(C)]
struct HCACapabilitiesLayout {
    vhca_resource_manager:          Volatile<U32<BigEndian>>,
    transpose_max_element_size:     Volatile<U32<BigEndian>>,
    transpose_max_size:             Volatile<U32<BigEndian>>,
    _padding0:                      u32,
    log_max_qp:                     Volatile<U32<BigEndian>>,
    scatter_fcs:                    Volatile<U32<BigEndian>>,
    log_max_cq:                     Volatile<U32<BigEndian>>,
    log_max_eq:                     Volatile<U32<BigEndian>>,
    log_max_klm:                    Volatile<U32<BigEndian>>,
    log_max_ra_res_dc:              Volatile<U32<BigEndian>>,
    log_max_ra_res_qp:              Volatile<U32<BigEndian>>,
    gid_table_size:                 Volatile<U32<BigEndian>>,
    pkey_table_size:                Volatile<U32<BigEndian>>,
    num_ports:                      Volatile<U32<BigEndian>>,
    wol_p:                          Volatile<U32<BigEndian>>,
    cqe_version:                    Volatile<U32<BigEndian>>,
    extended_retry_count:           Volatile<U32<BigEndian>>,
    rc:                             Volatile<U32<BigEndian>>,
    log_pg_sz:                      Volatile<U32<BigEndian>>,
    lag_native:                     Volatile<U32<BigEndian>>,
    max_wqe_sz_sq:                  Volatile<U32<BigEndian>>,
    max_wqe_sz_rq:                  Volatile<U32<BigEndian>>,
    max_wqe_sz_sq_dc:               Volatile<U32<BigEndian>>,
    max_qp_mcg:                     Volatile<U32<BigEndian>>,
    log_max_mcg:                    Volatile<U32<BigEndian>>,
    log_max_xrcd:                   Volatile<U32<BigEndian>>,
    max_flow_counter_15_0:          Volatile<U32<BigEndian>>,
    log_max_tis:                    Volatile<U32<BigEndian>>,
    log_max_tis_per_sq:             Volatile<U32<BigEndian>>,
    log_min_stride_sz_sq:           Volatile<U32<BigEndian>>,
    log_max_wq_sz:                  Volatile<U32<BigEndian>>,
    log_max_current_uc_list:        Volatile<U32<BigEndian>>,
    _padding1:                      u64,
    create_qp_start_hint:           Volatile<U32<BigEndian>>,
    max_num_eqs:                    Volatile<U32<BigEndian>>,
    log_uar_page_sz:                Volatile<U32<BigEndian>>,
    _padding2:                      u32,
    device_frequency_mhz:           Volatile<U32<BigEndian>>,
    device_frequency_khz:           Volatile<U32<BigEndian>>,
    nvmf_target:                    Volatile<U32<BigEndian>>,
    _padding3:                      u32,
    flex_parser_protocols:          Volatile<U32<BigEndian>>,
    flex_parser_header:             Volatile<U32<BigEndian>>,
    _padding4:                      u32,
    cqe_compression:                Volatile<U32<BigEndian>>,
    cqe_compression_max_num:        Volatile<U32<BigEndian>>,
    log_max_xrq:                    Volatile<U32<BigEndian>>,
    sw_owner_id:                    Volatile<U32<BigEndian>>,
    num_ppcnt:                      Volatile<U32<BigEndian>>,
    num_q:                          Volatile<U32<BigEndian>>,
    max_num_sf:                     Volatile<U32<BigEndian>>,
    _padding5:                      u32,
    flex_parser_id:                 Volatile<U32<BigEndian>>,
    sf_base_id:                     Volatile<U32<BigEndian>>,
    num_total_dynamic:              Volatile<U32<BigEndian>>,
    dynmaic_msix_table:             Volatile<U32<BigEndian>>,
    max_dynamic_vf:                 Volatile<U32<BigEndian>>,
    max_flow_execute:               Volatile<U32<BigEndian>>,
    _padding6:                      u64,
    match_definer:                  Volatile<U32<BigEndian>>, 
}

const_assert_eq!(core::mem::size_of::<HCACapabilitiesLayout>(), 256);

#[derive(Debug)]
pub struct HCACapabilities {
    log_max_cq_sz:                  u8,
    log_max_cq:                     u8,               
    log_max_eq_sz:                  u8,            
    log_max_mkey:                   u8,             
    log_max_eq:                     u8,               
    max_indirection:                u8,          
    log_max_mrw_sz:                 u8,           
    log_max_klm_list_size:          u8,
    end_pad:                        bool,   
    start_pad:                      bool,                
    cache_line_128byte:             bool,       
    vport_counters:                 bool,           
    vport_group_manager:            bool,      
    nic_flow_table:                 bool,           
    port_type:                      u8,                
    num_ports:                      u8,                
    log_max_msg:                    u8,              
    max_tc:                         u8,                   
    cqe_version:                    u8,              
    cmdif_checksum:                 u8,           
    wq_signature:                   bool,             
    sctr_data_cqe:                  bool,            
    eth_net_offloads:               bool,         
    cq_oi:                          bool,                    
    cq_resize:                      bool,                
    cq_moderation:                  bool,            
    cq_eq_remap:                    bool,              
    scqe_break_moderation:          bool,    
    cq_period_start_from_cqe:       bool, 
    imaicl:                         bool,                   
    xrc:                            bool,                      
    ud:                             bool,                       
    uc:                             bool,                       
    rc:                             bool,                       
    uar_sz:                         u8,                   
    log_pg_sz:                      u8,                
    bf:                             bool,                       
    driver_version:                 bool,           
    pad_tx_eth_packet:              bool,        
    log_bf_reg_size:                u8,          
    log_max_transport_domain:       u8, 
    log_max_pd:                     u8,               
    max_flow_counter:               u16,         
    log_max_rq:                     u8,               
    log_max_sq:                     u8,               
    log_max_tir:                    u8,              
    log_max_tis:                    u8,              
    basic_cyclic_rcv_wqe:           bool,     
    log_max_rmp:                    u8,              
    log_max_rqt:                    u8,              
    log_max_rqt_size:               u8,         
    log_max_tis_per_sq:             u8,       
    log_max_stride_sz_rq:           u8,     
    log_min_stride_sz_rq:           u8,     
    log_max_stride_sz_sq:           u8,     
    log_min_stride_sz_sq:           u8,     
    log_max_wq_sz:                  u8,            
    log_max_vlan_list:              u8,        
    log_max_current_mc_list:        u8,  
    log_max_current_uc_list:        u8,  
    log_max_l2_table:               u8,         
    log_uar_page_sz:                u16,          
    device_frequency_mhz:           u32,     
}

impl HCACapabilitiesLayout {
    fn get_capabilities(&self) -> HCACapabilities {
        HCACapabilities {
            log_max_cq_sz:                  ((self.log_max_cq.read().get() >> 16) & 0xFF) as u8,
            log_max_cq:                     (self.log_max_cq.read().get() & 0x1F) as u8,               
            log_max_eq_sz:                  ((self.log_max_eq.read().get() >> 24) & 0xFF) as u8,            
            log_max_mkey:                   ((self.log_max_eq.read().get() >> 16) & 0x3F) as u8,             
            log_max_eq:                     (self.log_max_eq.read().get() & 0xF) as u8,               
            max_indirection:                ((self.log_max_klm.read().get() >> 24) & 0xFF) as u8,          
            log_max_mrw_sz:                 ((self.log_max_klm.read().get() >> 16) & 0x7F) as u8,               
            log_max_klm_list_size:          (self.log_max_klm.read().get() & 0x3F) as u8,
            end_pad:                        self.gid_table_size.read().get().get_bit(31),   
            start_pad:                      self.gid_table_size.read().get().get_bit(28),                
            cache_line_128byte:             self.gid_table_size.read().get().get_bit(27),       
            vport_counters:                 self.pkey_table_size.read().get().get_bit(30),           
            vport_group_manager:            self.num_ports.read().get().get_bit(31),      
            nic_flow_table:                 self.num_ports.read().get().get_bit(25),           
            port_type:                      ((self.num_ports.read().get() >> 8) & 0x3) as u8,                
            num_ports:                      (self.num_ports.read().get() & 0xFF) as u8,                
            log_max_msg:                    ((self.wol_p.read().get() >> 24) & 0x1F) as u8,              
            max_tc:                         ((self.wol_p.read().get() >> 16) & 0xF) as u8,                   
            cqe_version:                    (self.cqe_version.read().get() & 0xF) as u8,              
            cmdif_checksum:                 ((self.extended_retry_count.read().get() >> 14) & 0x3) as u8,           
            wq_signature:                   self.extended_retry_count.read().get().get_bit(11),             
            sctr_data_cqe:                  self.extended_retry_count.read().get().get_bit(10),            
            eth_net_offloads:               self.extended_retry_count.read().get().get_bit(3),         
            cq_oi:                          self.rc.read().get().get_bit(31),                    
            cq_resize:                      self.rc.read().get().get_bit(30),                
            cq_moderation:                  self.rc.read().get().get_bit(29),            
            cq_eq_remap:                    self.rc.read().get().get_bit(25),              
            scqe_break_moderation:          self.rc.read().get().get_bit(21),    
            cq_period_start_from_cqe:       self.rc.read().get().get_bit(20), 
            imaicl:                         self.rc.read().get().get_bit(14),                   
            xrc:                            self.rc.read().get().get_bit(3),                      
            ud:                             self.rc.read().get().get_bit(2),                       
            uc:                             self.rc.read().get().get_bit(1),                       
            rc:                             self.rc.read().get().get_bit(0),                       
            uar_sz:                         ((self.log_pg_sz.read().get() >> 16) & 0x3F) as u8,                   
            log_pg_sz:                      (self.log_pg_sz.read().get() & 0xFF) as u8,                
            bf:                             self.lag_native.read().get().get_bit(31),                       
            driver_version:                 self.lag_native.read().get().get_bit(30),           
            pad_tx_eth_packet:              self.lag_native.read().get().get_bit(29),        
            log_bf_reg_size:                ((self.lag_native.read().get() >> 16) & 0x1F) as u8,          
            log_max_transport_domain:       ((self.log_max_xrcd.read().get() >> 24) & 0x1F) as u8, 
            log_max_pd:                     ((self.log_max_xrcd.read().get() >> 16) & 0x1F) as u8,               
            max_flow_counter:               (self.max_flow_counter_15_0.read().get() & 0xFFFF) as u16,         
            log_max_rq:                     ((self.log_max_tis.read().get() >> 24) & 0x1F) as u8,               
            log_max_sq:                     ((self.log_max_tis.read().get() >> 16) & 0x1F) as u8,               
            log_max_tir:                    ((self.log_max_tis.read().get() >> 8) & 0x1F) as u8,              
            log_max_tis:                    (self.log_max_tis.read().get() & 0x1F) as u8,              
            basic_cyclic_rcv_wqe:           self.log_max_tis_per_sq.read().get().get_bit(31),     
            log_max_rmp:                    ((self.log_max_tis_per_sq.read().get() >> 24) & 0x1F) as u8,              
            log_max_rqt:                    ((self.log_max_tis_per_sq.read().get() >> 16) & 0x1F) as u8,              
            log_max_rqt_size:               ((self.log_max_tis_per_sq.read().get() >> 8) & 0x1F) as u8,         
            log_max_tis_per_sq:             (self.log_max_tis_per_sq.read().get() & 0x1F) as u8,       
            log_max_stride_sz_rq:           ((self.log_min_stride_sz_sq.read().get() >> 24) & 0x1F) as u8,     
            log_min_stride_sz_rq:           ((self.log_min_stride_sz_sq.read().get() >> 16) & 0x1F) as u8,     
            log_max_stride_sz_sq:           ((self.log_min_stride_sz_sq.read().get() >> 8) & 0x1F) as u8,     
            log_min_stride_sz_sq:           (self.log_min_stride_sz_sq.read().get() & 0x1F) as u8,     
            log_max_wq_sz:                  (self.log_max_wq_sz.read().get() & 0x1F) as u8,
            log_max_vlan_list:              ((self.log_max_current_uc_list.read().get() >> 16) & 0x1F) as u8,
            log_max_current_mc_list:        ((self.log_max_current_uc_list.read().get() >> 8) & 0x1F) as u8,  
            log_max_current_uc_list:        (self.log_max_current_uc_list.read().get() & 0x1F) as u8,
            log_max_l2_table:               ((self.log_uar_page_sz.read().get() >> 24) & 0x1F) as u8,         
            log_uar_page_sz:                (self.log_uar_page_sz.read().get() & 0xFFFF) as u16,          
            device_frequency_mhz:           self.device_frequency_mhz.read().get()     
        }
    }
}