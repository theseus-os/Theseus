 //! Defines the Command Queue that is used to pass commands from the driver to the NIC.

use alloc::vec::Vec;
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
    QueryAdapter            = 0x101,
    InitHca                 = 0x102,
    TeardownHca             = 0x103,
    EnableHca               = 0x104,
    DisableHca              = 0x105,
    QueryPages              = 0x107,
    ManagePages             = 0x108,
    QueryIssi               = 0x10A,
    SetIssi                 = 0x10B,
    QuerySpecialContexts    = 0x203,
    CreateEq                = 0x301,
    CreateCq                = 0x400,
    QueryVportState         = 0x751,
    QueryNicVportContext    = 0x754,
    AllocPd                 = 0x800,
    AllocUar                = 0x802,
    AllocTransportDomain    = 0x816,
    CreateSq                = 0x904,
    ModifySq                = 0x905,
    CreateRq                = 0x908,
    ModifyRq                = 0x909,
    CreateTis               = 0x912,
}

impl CommandOpcode {
    fn input_bytes(&self, num_pages: Option<usize>) -> Result<u32, CommandQueueError> {
        let len = match self {
            Self::InitHca => 12,
            Self::EnableHca => 12,
            Self::QueryPages => 12,
            Self::ManagePages => {
                let num_pages = num_pages.ok_or(CommandQueueError::MissingInput)? as u32;
                0x10 + num_pages * SIZE_PADDR_IN_BYTES as u32
            }
            Self::QueryIssi => 8, 
            Self::SetIssi => 12,    
            Self::QuerySpecialContexts => 8,              
            Self::QueryVportState => 12,        
            Self::AllocUar => 8,              
            Self::AllocPd => 8,               
            Self::AllocTransportDomain => 8,
            _ => return Err(CommandQueueError::NotImplemented)           
        };
        Ok(len)
    }

    fn output_bytes(&self) -> Result<u32, CommandQueueError> {
        let len = match self {
            Self::InitHca => 8,
            Self::EnableHca => 8,
            Self::QueryPages => 16,
            Self::ManagePages => 16,
            Self::QueryIssi => 112, 
            Self::SetIssi => 8,    
            Self::QuerySpecialContexts => 12,              
            Self::QueryVportState => 16,        
            Self::AllocUar => 12,              
            Self::AllocPd => 12,               
            Self::AllocTransportDomain => 12,
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

/// Mailboxes can be used for both input data passed to HW, and output data passed from HW to SW.
#[derive(PartialEq)]
enum MailboxType {
    Input,
    Output
}


/// A buffer of fixed-size entries that is used to pass commands to the HCA.
/// It resides in a physically contiguous 4 KiB memory chunk.
/// (Section 8.24.1: HCA Command Queue)
#[repr(C)]
pub struct CommandQueue {
    /// Physically-contiguous command queue entries
    entries: BoxRefMut<MappedPages, [CommandQueueEntry]>,
    /// Per-entry boolean flags to keep track of which entries are in use
    available_entries: Vec<bool>,
    /// A random number that needs to be different for every command, and the same for all mailboxes that are part of a command.
    token: u8,
    /// Pages used for input mailboxes 
    mailbox_buffers_input: Vec<Vec<(MappedPages, PhysicalAddress)>>, 
    /// Pages used for output mailboxes 
    mailbox_buffers_output: Vec<Vec<(MappedPages, PhysicalAddress)>> 
}

impl CommandQueue {

    /// Create a command queue object.
    ///
    /// # Arguments
    /// * `entries`: physically contiguous memory that is mapped as a slice of command queue entries.
    /// * `num_cmdq_entries`: number of entries in the queue.
    pub fn create(entries: BoxRefMut<MappedPages, [CommandQueueEntry]>, num_cmdq_entries: usize) -> Result<CommandQueue, &'static str> {
        
        // initially, all command entries are available
        let available_entries = vec![true; num_cmdq_entries];

        // start off by pre-allocating one page for input and output mailboxes per entry
        let mut mailbox_buffers_input = Vec::with_capacity(num_cmdq_entries);
        let mut mailbox_buffers_output = Vec::with_capacity(num_cmdq_entries);
        for _ in 0..num_cmdq_entries {
            let (mailbox_mp, mailbox_pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            mailbox_buffers_input.push(vec!((mailbox_mp, mailbox_pa)));

            let (mailbox_mp, mailbox_pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            mailbox_buffers_output.push(vec!((mailbox_mp, mailbox_pa)));
        }

        // Assign a random number to token, another OS (Snabb) starts off with 0xAA
        let token = 0xAA;

        Ok(CommandQueue{ entries, available_entries, token, mailbox_buffers_input, mailbox_buffers_output })
    }

    /// Find an command queue entry that is not in use
    fn find_free_command_entry(&self) -> Option<usize> {
        self.available_entries.iter().position(|&x| x == true)
    }

    /// Fill in the fields of a command queue entry.
    /// At the end of the function, the command is ready to be posted using the doorbell in the initialization segment. 
    /// Returns an error if no entry is available to use.
    ///
    /// # Arguments
    /// * `opcode`: [`CommandOpcode`] for the command that the driver wants to execute
    /// * `opmod`: opcode modifer, only applicable for certain commands
    /// * `allocated_pages`: physical address of pages that need to be passed to the NIC. Only used in the [`CommandOpcode::ManagePages`] command.
    pub fn create_command(
        &mut self, 
        opcode: CommandOpcode, 
        opmod: Option<u16>, 
        allocated_pages: Option<Vec<PhysicalAddress>>,
    ) -> Result<usize, CommandQueueError> 
    {
        let entry_num = self.find_free_command_entry().ok_or(CommandQueueError::NoCommandEntryAvailable)?; 
        
        // clear the fields of the command
        let mut cmdq_entry = CommandQueueEntry::default();

        // set the type of transport and token
        cmdq_entry.set_type_of_transport();
        cmdq_entry.set_token(self.token);

        // set the ownership bit to HW-owned.
        // This bit will be cleared when the command is complete.
        cmdq_entry.change_ownership_to_hw();

        match opcode {
            CommandOpcode::EnableHca => {
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            }
            CommandOpcode::QueryIssi => {
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
                
                self.init_query_issi_output_mailbox_buffers(entry_num)?;
                self.set_mailbox_pointer_in_cmd_entry(&mut cmdq_entry, entry_num, MailboxType::Output);
            }
            CommandOpcode::SetIssi => {
                // setting to 1 by default
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, Some(1), None);
            }
            CommandOpcode::InitHca => {
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            }
            CommandOpcode::QuerySpecialContexts => {
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            }
            CommandOpcode::QueryPages => {
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            }
            CommandOpcode::ManagePages => {
                let pages_pa = allocated_pages.ok_or(CommandQueueError::MissingInputPages)?;
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(Some(pages_pa.len()))?); 
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(
                    opcode, 
                    opmod, 
                    None, 
                    Some(pages_pa.len() as u32)
                );

                self.init_manage_pages_input_mailbox_buffers(entry_num, pages_pa)?;
                self.set_mailbox_pointer_in_cmd_entry(&mut cmdq_entry, entry_num, MailboxType::Input);
            }
            CommandOpcode::AllocUar => {
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            },
            CommandOpcode::QueryVportState => { // only accesses your own vport
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            },
            CommandOpcode::AllocPd => { 
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            },
            CommandOpcode::AllocTransportDomain => { 
                cmdq_entry.set_input_length_in_bytes(opcode.input_bytes(None)?);
                cmdq_entry.set_output_length_in_bytes(opcode.output_bytes()?);
                cmdq_entry.set_input_inline_data(opcode, opmod, None, None);
            },
            _=> {
                error!("unimplemented opcode");
                return Err(CommandQueueError::UnimplementedOpcode);
            }
        }    
        
        // write command to actual place in memory
        core::mem::swap(&mut cmdq_entry, &mut self.entries[entry_num]);
        
        // update token to be used in the next command
        self.token = self.token.wrapping_add(1);

        // claim the command entry as in use
        self.available_entries[entry_num] = false;

        Ok(entry_num)
    }

    /// Sets the mailbox pointer in a command entry with the physical address of the first mailbox.
    fn set_mailbox_pointer_in_cmd_entry(&mut self, cmdq_entry: &mut CommandQueueEntry, entry_num: usize, mailbox_type: MailboxType) {
        if mailbox_type == MailboxType::Input {
            let mailbox_ptr = self.mailbox_buffers_input[entry_num][0].1.value();
            cmdq_entry.input_mailbox_pointer_h.write(U32::new((mailbox_ptr >> 32) as u32));
            cmdq_entry.input_mailbox_pointer_l.write(U32::new((mailbox_ptr & 0xFFFF_FFFF) as u32));
        } else {
            let mailbox_ptr = self.mailbox_buffers_output[entry_num][0].1.value();
            cmdq_entry.output_mailbox_pointer_h.write(U32::new((mailbox_ptr >> 32) as u32));
            cmdq_entry.output_mailbox_pointer_l.write(U32::new((mailbox_ptr & 0xFFFF_FFFF) as u32));
        };

    }

    /// Initialize output mailboxes for the [`CommandOpcode::QueryIssi`] command.
    fn init_query_issi_output_mailbox_buffers(&mut self, entry_num: usize) -> Result<(), CommandQueueError> {
        const NUM_MAILBOXES_QUERY_ISSI: usize = 1;
        self.initialize_mailboxes(entry_num, NUM_MAILBOXES_QUERY_ISSI, MailboxType::Output)?;
        Ok(())
    }

    /// Initialize input mailboxes for the [`CommandOpcode::ManagePages`] command.
    /// We write that physical address of the pages passed to the NIC to the mailbox data field.
    fn init_manage_pages_input_mailbox_buffers(&mut self, entry_num: usize, mut pages: Vec<PhysicalAddress>) -> Result<(), CommandQueueError> {
        
        let num_mailboxes = libm::ceilf((pages.len() * SIZE_PADDR_IN_BYTES) as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
        self.initialize_mailboxes(entry_num, num_mailboxes, MailboxType::Input)?;

        let mailbox_pages = &mut self.mailbox_buffers_input[entry_num];
        let paddr_per_mailbox = MAILBOX_DATA_SIZE_IN_BYTES / SIZE_PADDR_IN_BYTES;

        for block_num in 0..num_mailboxes {  
            let (mb_page, _mb_page_starting_addr) = &mut mailbox_pages[block_num];
            let mailbox = mb_page.as_type_mut::<CommandInterfaceMailbox>(MAILBOX_OFFSET_IN_PAGE)
                .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;

            let mut data = [0; 512];
            for page in 0..paddr_per_mailbox {
                match pages.pop() {
                    Some(paddr) => {
                        Self::write_paddr_in_mailbox_data(page * SIZE_PADDR_IN_BYTES, paddr, &mut data);
                    },
                    None => { 
                        // trace!("breaking out of loop on mailbox: {} and paddr: {}", block_num, page);
                        break; 
                    }
                }
            }
            mailbox.mailbox_data.write(data);
        }

        Ok(())
    }

    /// Waits for ownership bit to be cleared, and then returns the command delivery status and the command return status.
    pub fn wait_for_command_completion(&mut self, entry_num: usize) -> Result<(CommandDeliveryStatus, CommandReturnStatus), CommandQueueError> {
        while self.entries[entry_num].owned_by_hw() {}
        self.available_entries[entry_num] = true;
        let delivery_status = self.entries[entry_num].get_delivery_status().ok_or(CommandQueueError::InvalidCommandDeliveryStatus)?;
        let return_status = self.entries[entry_num].get_return_status().ok_or(CommandQueueError::InvalidCommandReturnStatus)?;
        Ok((delivery_status, return_status))
    }

    /// Get the current ISSI version and the supported ISSI versions, which is the output of the [`CommandOpcode::QueryIssi`] command.  
    pub fn get_query_issi_command_output(&self, entry_num: usize) -> Result<(u16, u8), CommandQueueError> {
        self.check_command_output_validity(entry_num, CommandOpcode::QueryIssi)?;

        const DATA_OFFSET_IN_MAILBOX: usize = 0x20 - 0x10;
        let mailbox = &self.mailbox_buffers_output[entry_num][0].0;
        let supported_issi = (
            mailbox.as_type::<u32>(DATA_OFFSET_IN_MAILBOX)
            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?
        ).to_le();

        let (_status, _syndrome, current_issi , _command1) = self.entries[entry_num].get_output_inline_data();
        Ok((current_issi as u16, supported_issi as u8))
    }

    /// Get the number of pages requested by the NIC, which is the output of the [`CommandOpcode::QueryPages`] command.  
    pub fn get_query_pages_command_output(&self, entry_num: usize) -> Result<u32, CommandQueueError> {
        self.check_command_output_validity(entry_num, CommandOpcode::QueryPages)?;
        let (_status, _syndrome, _function_id, num_pages) = self.entries[entry_num].get_output_inline_data();
        Ok(num_pages)
    }

    /// Get the User Access Region (UAR) number, which is the output of the [`CommandOpcode::AllocUar`] command.  
    pub fn get_uar(&self, entry_num: usize) -> Result<u32, CommandQueueError> {
        self.check_command_output_validity(entry_num, CommandOpcode::AllocUar)?;
        let (_status, _syndrome, uar, _reserved) = self.entries[entry_num].get_output_inline_data();
        Ok(uar & 0xFF_FFFF)
    }

    /// Get the protection domain number, which is the output of the [`CommandOpcode::AllocPd`] command.  
    pub fn get_protection_domain(&self, entry_num: usize) -> Result<u32, CommandQueueError> {
        self.check_command_output_validity(entry_num, CommandOpcode::AllocPd)?;
        let (_status, _syndrome, pd, _reserved) = self.entries[entry_num].get_output_inline_data();
        Ok(pd & 0xFF_FFFF)
    }

    /// Get the transport domain number, which is the output of the [`CommandOpcode::AllocTransportDomain`] command.  
    pub fn get_transport_domain(&self, entry_num: usize) -> Result<u32, CommandQueueError> {
        self.check_command_output_validity(entry_num, CommandOpcode::AllocTransportDomain)?;
        let (_status, _syndrome, td, _reserved) = self.entries[entry_num].get_output_inline_data();
        Ok(td & 0xFF_FFFF)
    }

    /// Get the value of the reserved Lkey for Base Memory Management Extension, which is used when we are using physical addresses.
    /// It is taken as the output of the [`CommandOpcode::QuerySpecialContexts`] command.
    pub fn get_reserved_lkey(&self, entry_num: usize) -> Result<u32, CommandQueueError> {
        self.check_command_output_validity(entry_num, CommandOpcode::QuerySpecialContexts)?;
        let (_status, _syndrome, _dump_fill_mkey, resd_lkey) = self.entries[entry_num].get_output_inline_data();
        Ok(resd_lkey)
    }

    /// When retrieving the output data of a command, checks that the correct opcode is written and that the command has completed.
    fn check_command_output_validity(&self, entry_num: usize, cmd_opcode: CommandOpcode) -> Result<(), CommandQueueError> {
        if self.entries[entry_num].owned_by_hw() {
            error!("the command hasn't completed yet!");
            return Err(CommandQueueError::CommandNotCompleted);
        }
        if self.entries[entry_num].get_command_opcode().ok_or(CommandQueueError::InvalidCommandOpcode)? != cmd_opcode {
            error!("Incorrect Command!: {:?}", self.entries[entry_num].get_command_opcode());
            return Err(CommandQueueError::IncorrectCommandOpcode);
        }
        Ok(())
    }

    /// Writes the physical address `paddr` to the given `start_offset` in the `data` buffer.
    /// The format specified in the PRM is that the 4 higher bytes are written first at the starting address in big endian format,
    /// then the four lower bytes are written in the next 4 bytes in big endian format.
    fn write_paddr_in_mailbox_data(start_offset: usize, paddr: PhysicalAddress, data: &mut [u8]) {
        let start_offset_h = start_offset;
        let end_offset_h = start_offset_h + SIZE_PADDR_H_IN_BYTES;
        let addr = (paddr.value() >> 32) as u32;
        data[start_offset_h..end_offset_h].copy_from_slice(&addr.to_be_bytes());

        let start_offset_l = end_offset_h;
        let end_offset_l = start_offset_l + SIZE_PADDR_L_IN_BYTES;
        let addr = (paddr.value() & 0xFFFF_FFFF) as u32;
        data[start_offset_l..end_offset_l].copy_from_slice(&addr.to_be_bytes());
    }

    /// Clears fields of all mailboxes, then sets the token, block number and next address fields for all mailboxes.
    fn initialize_mailboxes(&mut self, entry_num: usize, num_mailboxes: usize, mailbox_type: MailboxType) -> Result<(), CommandQueueError> {
        let mailbox_pages = if mailbox_type == MailboxType::Input{
            &mut self.mailbox_buffers_input[entry_num]
        } else {
            &mut self.mailbox_buffers_output[entry_num]
        };

        // Adding extra mailbox pages
        let available_mailboxes = mailbox_pages.len(); 
        if num_mailboxes > available_mailboxes {
            let num_mailbox_pages_required = num_mailboxes - available_mailboxes;
            trace!("Adding {} mailbox pages", num_mailbox_pages_required);
            
            for _ in 0..num_mailbox_pages_required {
                mailbox_pages.push(
                    create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)
                    .map_err(|_e| CommandQueueError::PageAllocationFailed)?
                );
            }
        }

        for block_num in 0..num_mailboxes {
            // record the next page address to set pointer for the next mailbox (can't have two borrows in the same loop)
            let next_mb_addr = if block_num < (num_mailboxes - 1) {
                mailbox_pages[block_num + 1].1.value()
            } else {
                0
            };

            let (mb_page, _mb_page_starting_addr) = &mut mailbox_pages[block_num];
            // trace!("Initializing mb: {}", block_num);

            let mailbox = mb_page.as_type_mut::<CommandInterfaceMailbox>(MAILBOX_OFFSET_IN_PAGE)
                            .map_err(|_e| CommandQueueError::InvalidMailboxOffset)?;
            mailbox.clear_all_fields();
            mailbox.block_number.write(U32::new(block_num as u32));
            mailbox.token_ctrl_signature.write(U32::new((self.token as u32) << 16));
            mailbox.next_pointer_h.write(U32::new((next_mb_addr >> 32) as u32));
            mailbox.next_pointer_l.write(U32::new((next_mb_addr & 0xFFFF_FFFF) as u32));
        }

        Ok(())
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
    /// Sets the type of transport which carries the command as PCIe.
    fn set_type_of_transport(&mut self) {
        self.type_of_transport.write(U32::new(CommandTransportType::PCIe as u32));
    }

    /// Sets length of input data in bytes.
    /// This value is different for every command, and can be taken from Chapter 23 of the PRM. 
    fn set_input_length_in_bytes(&mut self, length: u32) {
        self.input_length.write(U32::new(length))
    }

    /// Sets the first 16 bytes of input data that are written inline in the command.
    /// The valid values for each field are different for every command, and can be taken from Chapter 23 of the PRM.
    ///
    /// # Arguments
    /// * `opcode`: value identifying which command has to be carried out
    /// * `opmod`: opcode modifier. If None, field will be set to zero.
    /// * `command0`: the first 4 bytes of actual command data. If None, field will be set to zero.
    /// * `command1`: the second 4 bytes of actual command data. If None, field will be set to zero. 
    fn set_input_inline_data(&mut self, opcode: CommandOpcode, opmod: Option<u16>, command0: Option<u32>, command1: Option<u32>) {
        self.command_input_opcode.write(U32::new((opcode as u32) << 16));
        self.command_input_opmod.write(U32::new(opmod.unwrap_or(0) as u32));
        self.command_input_inline_data_0.write(U32::new(command0.unwrap_or(0)));
        self.command_input_inline_data_1.write(U32::new(command1.unwrap_or(0)));
    }

    /// Returns the value written to the input opcode field of the command
    /// A `None` returned value indicates that there was no valid value in the bitfield.
    fn get_command_opcode(&self) -> Option<CommandOpcode> {
        let opcode = self.command_input_opcode.read().get() >> 16;
        CommandOpcode::try_from(opcode).ok()
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
   
    /// Sets length of output data in bytes.
    /// This value is different for every command, and can be taken from Chapter 23 of the PRM. 
    fn set_output_length_in_bytes(&mut self, length: u32) {
        self.output_length.write(U32::new(length));
    }

    /// Sets the token of the command.
    /// This is a random value that should be the same in the command entry and all linked mailboxes.
    fn set_token(&mut self, token: u8) {
        let val = self.token_signature_status_own.read().get();
        self.token_signature_status_own.write(U32::new(val | ((token as u32) << 24)));
    }

    /// Returns the status of command delivery.
    /// This only informs us if the command was delivered to the NIC successfully, not if it was completed successfully.
    /// A `None` returned value indicates that there was no valid value in the bitfield.
    pub fn get_delivery_status(&self) -> Option<CommandDeliveryStatus> {
        let status = (self.token_signature_status_own.read().get() & 0xFE) >> 1;
        CommandDeliveryStatus::try_from(status).ok()
    }

    /// Sets the ownership bit so that HW can take control of the command entry
    pub fn change_ownership_to_hw(&mut self) {
        let ownership = self.token_signature_status_own.read().get() | 0x1;
        self.token_signature_status_own.write(U32::new(ownership));
    }

    /// Returns true if the command is currently under the ownership of HW (SW should not touch the fields).
    pub fn owned_by_hw(&self) -> bool {
        self.token_signature_status_own.read().get().get_bit(0)
    }

    /// Returns the status of command execution.
    /// A `None` returned value indicates that there was no valid value in the bitfield.
    pub fn get_return_status(&self) -> Option<CommandReturnStatus> {
        let (status, _syndrome, _, _) = self.get_output_inline_data();
        CommandReturnStatus::try_from(status).ok()
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
}

impl fmt::Debug for CommandInterfaceMailbox {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CommandQueueEntry")
            .field("mailbox_data", &self.mailbox_data.read())
            .field("next pointer h", &self.next_pointer_h.read().get())
            .field("next pointer l", &self.next_pointer_l.read().get())
            .field("block number", &self.block_number.read().get())
            .field("token ctrl signature", &self.token_ctrl_signature.read().get())
            .finish()
    }
}
