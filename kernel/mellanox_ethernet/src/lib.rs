 //! Note: Mellanox manual refers to the NIC as HCA.

 #![no_std]
 #![feature(slice_pattern)]
 #![feature(core_intrinsics)]

#[macro_use]extern crate log;
extern crate memory;
extern crate volatile;
extern crate bit_field;
extern crate zerocopy;
extern crate alloc;
#[macro_use] extern crate static_assertions;
extern crate owning_ref;
extern crate byteorder;
extern crate nic_initialization;
extern crate kernel_config;
extern crate libm;

use core::{num, slice::SlicePattern};

use alloc::vec::Vec;
use memory::{PhysicalAddress, PhysicalMemoryRegion, MappedPages, create_contiguous_mapping};
use volatile::{Volatile, ReadOnly, WriteOnly};
use bit_field::BitField;
use zerocopy::*;
// {FromBytes, AsBytes, Unaligned};
use byteorder::BigEndian;
use owning_ref:: BoxRefMut;
use nic_initialization::NIC_MAPPING_FLAGS;
use kernel_config::memory::PAGE_SIZE;
use core::fmt;

// Taken from HCA BAR 
const MAX_CMND_QUEUE_ENTRIES: usize = 64;
#[derive(FromBytes)]
#[repr(C,packed)]
/// The fields are stored in BE ordering, so that's why every 32 bits field seems to be opposite to the diagram in the manual
pub struct InitializationSegment {
    fw_rev_minor:               ReadOnly<U16<BigEndian>>,
    fw_rev_major:               ReadOnly<U16<BigEndian>>,
    cmd_interface_rev:          ReadOnly<U16<BigEndian>>,
    fw_rev_subminor:            ReadOnly<U16<BigEndian>>,
    _padding1:                  [u8; 8],
    cmdq_phy_addr_high:         Volatile<U32<BigEndian>>,
    cmdq_phy_addr_low:          Volatile<U32<BigEndian>>,
    command_doorbell_vector:    Volatile<U32<BigEndian>>,
    _padding2a:                 [u8; 256],
    _padding2b:                 [u8; 128],
    _padding2c:                 [u8; 6],
    initializing_state:         ReadOnly<U32<BigEndian>>,
    health_buffer:              Volatile<[u8; 64]>,
    no_dram_nic_offset:         ReadOnly<U32<BigEndian>>,
    _padding3a:                 [u8; 2048],
    _padding3b:                 [u8; 1024],
    _padding3c:                 [u8; 256],
    _padding3d:                 [u8; 128],
    _padding3e:                 [u8; 60],
    internal_timer_h:           ReadOnly<U32<BigEndian>>,
    internal_timer_l:           ReadOnly<U32<BigEndian>>,
    _padding4:                  [u8; 8],
    health_counter:             ReadOnly<U32<BigEndian>>,
    _padding5:                  [u8; 44],
    real_time:                  ReadOnly<U64<BigEndian>>,
    _padding6a:                 [u8; 8192],
    _padding6b:                 [u8; 2048],
    _padding6c:                 [u8; 1024],
    _padding6d:                 [u8; 512],
    _padding6e:                 [u8; 256],
    _padding6f:                 [u8; 128],
    _padding6g:                 [u8; 64],
    _padding6h:                 [u8; 4],
}

// const_assert_eq!(core::mem::size_of::<InitializationSegment>(), 16400);

impl InitializationSegment {
    pub fn num_cmdq_entries(&self) -> u8 {
        let log = (self.cmdq_phy_addr_low.read().get() >> 4) & 0x0F;
        2_u8.pow(log)
    }

    pub fn cmdq_entry_stride(&self) -> u8 {
        let val = self.cmdq_phy_addr_low.read().get() & 0x0F;
        2_u8.pow(val)
    }
    pub fn set_physical_address_of_cmdq(&mut self, pa: PhysicalAddress) -> Result<(), &'static str> {
        if pa.value() & 0xFFF != 0 {
            return Err("cmdq physical address lower 12 bits must be zero.");
        }

        self.cmdq_phy_addr_high.write(U32::new((pa.value() >> 32) as u32));
        let val = self.cmdq_phy_addr_low.read().get() & 0xFFF;
        self.cmdq_phy_addr_low.write(U32::new(pa.value() as u32 | val));
        Ok(())
    }

    pub fn device_is_initializing(&self) -> bool {
        self.initializing_state.read().get().get_bit(31)
    }

    pub fn post_command(&mut self, command_bit: usize) {
        let val = self.command_doorbell_vector.read().get();
        self.command_doorbell_vector.write(U32::new(val | (1 << command_bit)));
    }

    pub fn print(&self) {
        trace!("{}.{}.{}, {}", self.fw_rev_major.read().get(), self.fw_rev_minor.read().get(), self.fw_rev_subminor.read().get(), self.cmd_interface_rev.read().get());
        trace!("{:#X} {:#X}", self.cmdq_phy_addr_high.read().get(), self.cmdq_phy_addr_low.read().get());
        trace!("{:#X}", self.command_doorbell_vector.read().get());
        trace!("{:#X}", self.initializing_state.read().get());
    }
}

pub enum InitializingState {
    NotAllowed = 0,
    WaitingPermetion = 1, // Is this a typo?
    WaitingResources = 2,
    Abort = 3
}

#[derive(FromBytes, Default)]
#[repr(C)]
pub struct CommandQueueEntry {
    type_of_transport:              Volatile<U32<BigEndian>>,
    input_length:                   Volatile<U32<BigEndian>>,
    input_mailbox_pointer_h:        Volatile<U32<BigEndian>>,
    input_mailbox_pointer_l:        Volatile<U32<BigEndian>>,
    command_input_opcode:           Volatile<U32<BigEndian>>,
    command_input_op_mod:           Volatile<U32<BigEndian>>,
    command_input_inline_data_0:    Volatile<U32<BigEndian>>,
    command_input_inline_data_1:    Volatile<U32<BigEndian>>,
    command_output_status:          Volatile<U32<BigEndian>>,
    command_output_syndrome:        Volatile<U32<BigEndian>>,
    command_output_inline_data_0:   Volatile<U32<BigEndian>>,
    command_output_inline_data_1:   Volatile<U32<BigEndian>>,
    output_mailbox_pointer_h:       Volatile<U32<BigEndian>>,
    output_mailbox_pointer_l:       Volatile<U32<BigEndian>>,
    output_length:                  Volatile<U32<BigEndian>>,
    token_signature_status_own:     Volatile<U32<BigEndian>>
}


const_assert_eq!(core::mem::size_of::<CommandQueueEntry>(), 64);

pub enum CommandTransportType {
    PCIe = 0x7 << 24
}

impl fmt::Debug for CommandQueueEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CQE:type of transport: {:#X} \n", self.type_of_transport.read().get())?;
        write!(f, "CQE:input length: {} \n", self.input_length.read().get())?;
        write!(f, "CQE:input_mailbox_ptr_h: {:#X} \n", self.input_mailbox_pointer_h.read().get())?;
        write!(f, "CQE:input_mailbox_ptr_l: {:#X} \n", self.input_mailbox_pointer_l.read().get())?;
        write!(f, "CQE:command_input_opcode: {:#X} \n", self.command_input_opcode.read().get())?;
        write!(f, "CQE:command_input_op_mod: {:#X} \n", self.command_input_op_mod.read().get())?;
        write!(f, "CQE:command_input_inline_data_0: {:#X} \n", self.command_input_inline_data_0.read().get())?;
        write!(f, "CQE:command_input_inline_data_1: {:#X} \n", self.command_input_inline_data_1.read().get())?;
        write!(f, "CQE:command_output_status: {:#X} \n", self.command_output_status.read().get())?;
        write!(f, "CQE:command_output_syndrome: {:#X} \n", self.command_output_syndrome.read().get())?;
        write!(f, "CQE:command_output_inline_data_0: {:#X} \n", self.command_output_inline_data_0.read().get())?;
        write!(f, "CQE:command_output_inline_data_1: {:#X} \n", self.command_output_inline_data_1.read().get())?;
        write!(f, "CQE:output_mailbox_pointer_h: {:#X} \n", self.output_mailbox_pointer_h.read().get())?;
        write!(f, "CQE:output_mailbox_pointer_l: {:#X} \n", self.output_mailbox_pointer_l.read().get())?;
        write!(f, "CQE:output_length: {} \n", self.output_length.read().get())?;
        write!(f, "CQE:token_signature_status_own: {:#X} \n", self.token_signature_status_own.read().get())
    }
}

impl CommandQueueEntry {
    fn set_type_of_transport(&mut self, transport: CommandTransportType) {
        self.type_of_transport.write(U32::new(CommandTransportType::PCIe as u32));
    }

    fn set_input_length_in_bytes(&mut self, length: u32) {
        self.input_length.write(U32::new(length))
    }

    fn set_input_mailbox_pointer(&mut self, pa: PhysicalAddress) -> Result<(), &'static str> {
        error!("Don't use this function yet! We don't fill out the mailbox fields");
        
        if pa.value() & 0x1FF != 0 {
            return Err("input mailbox pointer physical address lower 9 bits must be zero.");
        }

        self.input_mailbox_pointer_h.write(U32::new((pa.value() >> 32) as u32));
        let val = self.input_mailbox_pointer_l.read().get() & 0x1FF;
        self.input_mailbox_pointer_l.write(U32::new(pa.value() as u32 | val));
        Ok(())
    }

    // right now assume only 8 bytes of commands are passed
    fn set_input_inline_data(&mut self, opcode: CommandOpcode, op_mod: Option<u16>, command0: Option<u32>, command1: Option<u32>) {
        self.command_input_opcode.write(U32::new((opcode as u32) << 16));
        self.command_input_op_mod.write(U32::new(op_mod.unwrap_or(0) as u32));
        self.command_input_inline_data_0.write(U32::new(command0.unwrap_or(0)));
        self.command_input_inline_data_1.write(U32::new(command1.unwrap_or(0)));

    }

    fn get_command_opcode(&self) -> CommandOpcode {
        match self.command_input_opcode.read().get() >> 16 {
            0x100 => {CommandOpcode::QueryHcaCap},
            0x101 => {CommandOpcode::QueryAdapter}, 
            0x102 => {CommandOpcode::InitHca}, 
            0x103 => {CommandOpcode::TeardownHca}, 
            0x104 => {CommandOpcode::EnableHca}, 
            0x105 => {CommandOpcode::DisableHca}, 
            0x107 => {CommandOpcode::QueryPages}, 
            0x108 => {CommandOpcode::ManagePages}, 
            0x10A => {CommandOpcode::QueryIssi}, 
            0x10B => {CommandOpcode::SetIssi}, 
            _ => {CommandOpcode::Unknown}
        }
    }

    // right now assume only 8 bytes of commands are passed
    fn get_output_inline_data(&self) -> (u8, u32, u32, u32) {
        (
            (self.command_output_status.read().get() >> 24) as u8,
            self.command_output_syndrome.read().get(),
            self.command_output_inline_data_0.read().get(),
            self.command_output_inline_data_1.read().get()
        )
    }

    fn set_output_mailbox_pointer(&mut self, mp: &mut MappedPages, pa: PhysicalAddress) -> Result<(), &'static str> {
        error!("Don't use this function yet! We don't fill out the mailbox fields");

        if pa.value() & 0x1FF != 0 {
            return Err("output mailbox pointer physical address lower 9 bits must be zero.");
        }

        self.output_mailbox_pointer_h.write(U32::new((pa.value() >> 32) as u32));
        let val = self.output_mailbox_pointer_l.read().get() & 0x1FF;
        self.output_mailbox_pointer_l.write(U32::new(pa.value() as u32 | val));
        Ok(())
    }
    
    fn set_output_length_in_bytes(&mut self, length: u32) {
        self.output_length.write(U32::new(length));
    }

    fn get_output_length_in_bytes(&self) -> u32 {
        self.output_length.read().get()
    }

    fn set_token(&mut self, token: u8) {
        let val = self.token_signature_status_own.read().get();
        self.token_signature_status_own.write(U32::new(val | ((token as u32) << 24)));
    }

    fn get_token(&self) -> u8 {
        (self.token_signature_status_own.read().get() >> 24) as u8
    }

    fn get_signature(&self) -> u8 {
        (self.token_signature_status_own.read().get() >> 16) as u8
    }

    pub fn get_status(&self) -> CommandDeliveryStatus {
        let status = self.token_signature_status_own.read().get() & 0xFE;
        if status == 0 {
            CommandDeliveryStatus::Success
        } else if status == 1 {
            CommandDeliveryStatus::SignatureErr
        } else if status == 2 {
            CommandDeliveryStatus::TokenErr
        } else if status == 3 {
            CommandDeliveryStatus::BadBlockNumber
        } else if status == 4 {
            CommandDeliveryStatus::BadOutputPointer
        } else if status == 5 {
            CommandDeliveryStatus::BadInputPointer
        } else if status == 6 {
            CommandDeliveryStatus::InternalErr
        } else if status == 7 {
            CommandDeliveryStatus::InputLenErr
        } else if status == 8 {
            CommandDeliveryStatus::OutputLenErr
        } else if status == 9 {
            CommandDeliveryStatus::ReservedNotZero
        } else if status == 10 {
            CommandDeliveryStatus::BadCommandType
        } else {
            CommandDeliveryStatus::Unknown
        }
    }

    pub fn change_ownership_to_hw(&mut self) {
        let ownership = self.token_signature_status_own.read().get() | 0x1;
        self.token_signature_status_own.write(U32::new(ownership));
    }

    pub fn owned_by_hw(&self) -> bool {
        self.token_signature_status_own.read().get().get_bit(0)
    }
}

#[derive(FromBytes)]
#[repr(C)]
struct CommandInterfaceMailbox {
    mailbox_data:           Volatile<[u8; 512]>,
    _padding:               ReadOnly<[u8; 48]>,
    next_pointer_h:         Volatile<U32<BigEndian>>,
    next_pointer_l:         Volatile<U32<BigEndian>>,
    block_number:           Volatile<U32<BigEndian>>,
    token_ctrl_signature:   Volatile<U32<BigEndian>>
}
const_assert_eq!(core::mem::size_of::<CommandInterfaceMailbox>(), 576);

impl CommandInterfaceMailbox {
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
        for i in 0..(512/4) {
            let data = self.mailbox_data.read();
            write!(f, "mailbox data: {:#X} {:#X} {:#X} {:#X} \n", data[i*4], data[i*4+1], data[i*4+2], data[i*4 +3])?;
        }
        
        // write!(f, "padding: {}", self._padding.read())?;
        writeln!(f, "next pointer h: {:#X} \n", self.next_pointer_h.read().get())?;
        writeln!(f, "next pointer l: {:#X} \n", self.next_pointer_l.read().get())?;
        writeln!(f, "tokne ctrl signature: {:#X} \n", self.token_ctrl_signature.read().get())
    }
}

#[derive(Debug)]
pub enum CommandDeliveryStatus {
    Success = 0,
    SignatureErr = 1,
    TokenErr = 2,
    BadBlockNumber = 3,
    BadOutputPointer = 4,
    BadInputPointer = 5,
    InternalErr = 6,
    InputLenErr = 7,
    OutputLenErr = 8,
    ReservedNotZero = 9,
    BadCommandType = 10, //Should this be 10 or 16??
    Unknown,
}

#[derive(PartialEq, Debug)]
pub enum CommandOpcode {
    QueryHcaCap = 0x100,
    QueryAdapter = 0x101,
    InitHca = 0x102,
    TeardownHca = 0x103,
    EnableHca = 0x104,
    DisableHca = 0x105,
    QueryPages = 0x107,
    ManagePages = 0x108,
    QueryIssi = 0x10A,
    SetIssi = 0x10B,
    QuerySpecialContexts = 0x203,
    CreateEq = 0x301,
    AllocUar = 0x802,
    AllocPd = 0x800,
    AllocTransportDomain = 0x816,
    CreateCq = 0x400,
    CreateTis = 0x912,
    CreateSq = 0x904,
    ModifySq = 0x905,
    CreateRq = 0x908,
    ModifyRq = 0x909,
    Unknown
}

/// Section 8.24.1
/// A buffer of fixed-size entries that is used to pass commands to the HCA.
/// The number of enties and the entry stride is retrieved from the initialization segment of the HCA BAR.
/// It resides in a physically contiguous 4 KiB memory chunk.
#[repr(C)]
pub struct CommandQueue {
    entries: BoxRefMut<MappedPages, [CommandQueueEntry]>,
    available_entries: [bool; MAX_CMND_QUEUE_ENTRIES],
    token: u8, //taken from snabb, my assumption is that it is a random number that needs to be different for every command
    mailbox_buffers_input: Vec<(MappedPages, PhysicalAddress)>, // A page, physical address of the page, and if it's in use
    mailbox_buffers_output: Vec<(MappedPages, PhysicalAddress)> // A page, physical address of the page, and if it's in use
}

const MAILBOX_SIZE_IN_BYTES: usize = 576;
const MAILBOX_DATA_SIZE_IN_BYTES: usize = 512;
const MAX_MAILBOX_BUFFERS: usize = PAGE_SIZE / MAILBOX_SIZE_IN_BYTES;

const SIZE_PADDR_IN_BYTES: usize = 8; // each physical address takes 8 bytes in the mailbox
const SIZE_PADDR_H_IN_BYTES: usize = 4; // each physical address takes 8 bytes in the mailbox
const SIZE_PADDR_L_IN_BYTES: usize = 4; // each physical address takes 8 bytes in the mailbox
pub enum ManagePagesOpmod {
    AllocationFail = 0,
    AllocationSuccess = 1,
    HcaReturnPages = 2
}

pub enum QueryPagesOpmod {
    BootPages = 1,
    InitPages = 2,
    RegularPages = 3
}

impl CommandQueue {

    pub fn create(entries: BoxRefMut<MappedPages, [CommandQueueEntry]>, num_cmdq_entries: usize) -> Result<CommandQueue, &'static str> {
        let mut available_entries = [false; MAX_CMND_QUEUE_ENTRIES];
        for i in 0..num_cmdq_entries { available_entries[i] = true; }

        // allocate one page to be the mailbox buffer per entry
        let mut mailbox_buffers_input = Vec::with_capacity(num_cmdq_entries);
        let mut mailbox_buffers_output = Vec::with_capacity(num_cmdq_entries);
        for _ in 0..num_cmdq_entries {
            let (mailbox_mp, mailbox_pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            mailbox_buffers_input.push((mailbox_mp, mailbox_pa));

            let (mailbox_mp, mailbox_pa) = create_contiguous_mapping(PAGE_SIZE, NIC_MAPPING_FLAGS)?;
            mailbox_buffers_output.push((mailbox_mp, mailbox_pa));
        }

        Ok(CommandQueue{ entries, available_entries, token: 0xAA, mailbox_buffers_input, mailbox_buffers_output })
    }

    fn find_free_command_entry(&self) -> Option<usize> {
        self.available_entries.iter().position(|&x| x == true)
    }

    pub fn create_command(
        &mut self, opcode: CommandOpcode, 
        op_mod: Option<u16>, 
        allocated_pages: Option<Vec<PhysicalAddress>>,
        uar: Option<u32>,
        log_queue_size: Option<u8>
    ) -> Result<usize, &'static str> 
    {
        let entry_num = self.find_free_command_entry().ok_or("No command entry available")?; 

        let mut cmdq_entry = CommandQueueEntry::default();
        cmdq_entry.set_type_of_transport(CommandTransportType::PCIe);
        cmdq_entry.set_token(self.token);
        cmdq_entry.change_ownership_to_hw();

        match opcode {
            CommandOpcode::EnableHca => {
                cmdq_entry.set_input_length_in_bytes(12);
                cmdq_entry.set_output_length_in_bytes(8);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
            }
            CommandOpcode::QueryIssi => {
                warn!("running query issi with smaller output length, may be an error");
                cmdq_entry.set_input_length_in_bytes(8);
                cmdq_entry.set_output_length_in_bytes(12);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
            }
            CommandOpcode::SetIssi => {
                warn!("setting to 1 by default, could be wrong");
                cmdq_entry.set_input_length_in_bytes(12);
                cmdq_entry.set_output_length_in_bytes(8);
                cmdq_entry.set_input_inline_data(opcode, op_mod, Some(1), None);
            }
            CommandOpcode::InitHca => {
                cmdq_entry.set_input_length_in_bytes(12);
                cmdq_entry.set_output_length_in_bytes(8);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
            }
            CommandOpcode::QuerySpecialContexts => {
                cmdq_entry.set_input_length_in_bytes(8);
                cmdq_entry.set_output_length_in_bytes(16);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
            }
            CommandOpcode::QueryPages => {
                cmdq_entry.set_input_length_in_bytes(12);
                cmdq_entry.set_output_length_in_bytes(16);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
            }
            CommandOpcode::ManagePages => {
                let pages_pa = allocated_pages.ok_or("No pages were passed to the manage pages command")?;
                cmdq_entry.set_input_length_in_bytes(0x10 + pages_pa.len() as u32 *8); // taken from snabb
                cmdq_entry.set_output_length_in_bytes(16);
                cmdq_entry.set_input_inline_data(
                    opcode, 
                    op_mod, 
                    None, 
                    Some(pages_pa.len() as u32)
                );

                self.init_manage_pages_input_mailbox_buffers(entry_num, pages_pa)?;
                let mailbox_ptr = self.mailbox_buffers_input[entry_num].1.value();
                cmdq_entry.input_mailbox_pointer_h.write(U32::new((mailbox_ptr >> 32) as u32));
                cmdq_entry.input_mailbox_pointer_l.write(U32::new((mailbox_ptr & 0xFFFF_FFFF) as u32));
            }
            CommandOpcode::AllocUar => {
                cmdq_entry.set_input_length_in_bytes(8);
                cmdq_entry.set_output_length_in_bytes(12);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
            },
            CommandOpcode::CreateEq => {
                let pages_pa = allocated_pages.ok_or("No pages were passed to the create EQ command")?;

                cmdq_entry.set_input_length_in_bytes((0x110 + pages_pa.len()*8) as u32);
                cmdq_entry.set_output_length_in_bytes(12);
                cmdq_entry.set_input_inline_data(opcode, op_mod, None, None);
                
                self.create_page_request_eq(
                    entry_num, 
                    pages_pa,
                    uar.ok_or("uar not specified in EQ creation")?,
                    log_queue_size.ok_or("queue size not specified in EQ creation")?
                )?;
                let mailbox_ptr = self.mailbox_buffers_input[entry_num].1.value();
                cmdq_entry.input_mailbox_pointer_h.write(U32::new((mailbox_ptr >> 32) as u32));
                cmdq_entry.input_mailbox_pointer_l.write(U32::new((mailbox_ptr & 0xFFFF_FFFF) as u32));
            },
            _=> {
                debug!("unimplemented opcode");
            }
        }
        
        core::mem::swap(&mut cmdq_entry, &mut self.entries[entry_num]);
        warn!("{:?}", &mut self.entries[entry_num]);
        self.token = self.token.wrapping_add(1);
        self.available_entries[entry_num] = false;
        Ok(entry_num)
    }

    fn init_manage_pages_input_mailbox_buffers(&mut self, entry_num: usize, mut pages: Vec<PhysicalAddress>) -> Result<(), &'static str> {
        
        let mailbox_page = &mut self.mailbox_buffers_input[entry_num];
        let mailbox_starting_addr = mailbox_page.1;

        let num_mailboxes = libm::ceilf((pages.len() * SIZE_PADDR_IN_BYTES) as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;
        let pages_per_mailbox = MAILBOX_DATA_SIZE_IN_BYTES / SIZE_PADDR_IN_BYTES;

        if num_mailboxes > MAX_MAILBOX_BUFFERS {
            return Err("Too many maiboxes required, TODO: allocate more than a page for mailboxes");
        }

        for block_num in 0..num_mailboxes {
            let mailbox = mailbox_page.0.as_type_mut::<CommandInterfaceMailbox>(block_num * MAILBOX_SIZE_IN_BYTES)?;
            mailbox.clear_all_fields();

            if block_num != (num_mailboxes - 1){
                let mailbox_next_addr = mailbox_starting_addr.value() + ((block_num + 1) * MAILBOX_SIZE_IN_BYTES);
                mailbox.next_pointer_h.write(U32::new((mailbox_next_addr >> 32) as u32));
                mailbox.next_pointer_l.write(U32::new((mailbox_next_addr & 0xFFFF_FFFF) as u32));
            }
            mailbox.block_number.write(U32::new(block_num as u32));
            mailbox.token_ctrl_signature.write(U32::new((self.token as u32) << 16));

            let mut data = [0; 512];

            for page in 0..pages_per_mailbox {
                let paddr = pages.pop();
                match paddr {
                    Some(paddr) => {
                        let start_offset_h = page * SIZE_PADDR_IN_BYTES;
                        let end_offset_h = start_offset_h + SIZE_PADDR_H_IN_BYTES;
                        let addr = (paddr.value() >> 32) as u32;
                        data[start_offset_h..end_offset_h].copy_from_slice(&addr.to_be_bytes());

                        let start_offset_l = end_offset_h;
                        let end_offset_l = start_offset_l + SIZE_PADDR_L_IN_BYTES;
                        let addr = (paddr.value() & 0xFFFF_FFFF) as u32;
                        data[start_offset_l..end_offset_l].copy_from_slice(&addr.to_be_bytes());
                    },
                    None => { 
                        trace!("breaking out of loop on mailbox: {} and page: {}", block_num, page);
                        break; 
                    }
                }
            }

            mailbox.mailbox_data.write(data);
            debug!("token: {}", self.token);
            debug!("Mailbox {}", block_num);
            debug!("{:?}", mailbox);
        }

        Ok(())
    }

    pub fn wait_for_command_completion(&mut self, entry_num: usize) -> CommandDeliveryStatus {
        while self.entries[entry_num].owned_by_hw() {}
        self.available_entries[entry_num] = true;
        self.entries[entry_num].get_status()
    }

    pub fn get_query_issi_command_output(&self, entry_num: usize) -> Result<u16, &'static str> {
        if self.entries[entry_num].owned_by_hw() {
            error!("the command hasn't completed yet!");
            return Err("the command hasn't completed yet!");
        }

        let opcode = self.entries[entry_num].get_command_opcode();
        if opcode != CommandOpcode::QueryIssi {
            error!("Incorrect Command!");
            return Err("Incorrect Command!");
        }

        let (_status, _syndrome, current_issi , _command1) = self.entries[entry_num].get_output_inline_data();
        Ok(current_issi as u16)
    }

    pub fn get_query_pages_command_output(&self, entry_num: usize) -> Result<u32, &'static str> {
        if self.entries[entry_num].owned_by_hw() {
            error!("the command hasn't completed yet!");
            return Err("the command hasn't completed yet!");
        }

        let opcode = self.entries[entry_num].get_command_opcode();
        if opcode != CommandOpcode::QueryPages {
            error!("Incorrect Command!");
            return Err("Incorrect Command!");
        }

        let (_status, _syndrome, _function_id, num_pages) = self.entries[entry_num].get_output_inline_data();
        Ok(num_pages)
    }

    pub fn get_uar(&self, entry_num: usize) -> Result<u32, &'static str> {
        if self.entries[entry_num].owned_by_hw() {
            error!("the command hasn't completed yet!");
            return Err("the command hasn't completed yet!");
        }

        let opcode = self.entries[entry_num].get_command_opcode();
        if opcode != CommandOpcode::AllocUar {
            error!("Incorrect Command: {:?}!", opcode);
            return Err("Incorrect Command!");
        }

        let (_status, _syndrome, uar, _reserved) = self.entries[entry_num].get_output_inline_data();
        Ok(uar & 0xFF_FFFF)
    }

    fn create_page_request_eq(&mut self, entry_num: usize, mut pages: Vec<PhysicalAddress>, uar: u32, log_eq_size: u8) -> Result<(), &'static str> {
        
        let mailbox_page = &mut self.mailbox_buffers_input[entry_num];
        let mailbox_starting_addr = mailbox_page.1;

        let size_of_mailbox_data = (0x110 - 0x10) + SIZE_PADDR_IN_BYTES * pages.len();

        let num_mailboxes = libm::ceilf(size_of_mailbox_data as f32 / MAILBOX_DATA_SIZE_IN_BYTES as f32) as usize;

        if num_mailboxes > MAX_MAILBOX_BUFFERS {
            return Err("Too many maiboxes required, TODO: allocate more than a page for mailboxes");
        }

        // In mailbox 0 we have to set up the eq_context and event_bitmask
        // The Eq context lies at offset 0 of the first mailbox
        let block_num=0;
        // clear all fields of the mailbox
        mailbox_page.0.as_type_mut::<CommandInterfaceMailbox>(block_num * MAILBOX_SIZE_IN_BYTES)?.clear_all_fields();
      
        // initialize the event queue context
        let eq_context = mailbox_page.0.as_type_mut::<EventQueueContext>(0)?;
        eq_context.init(uar, log_eq_size);

        // initialize the bitmask. this function only activates the page request event
        let bitmask_offset_in_mailbox  = 0x58 - 0x10;
        let eq_bitmask = mailbox_page.0.as_type_mut::<u64>(bitmask_offset_in_mailbox)?;
        const PAGE_REQUEST_BIT: u64 = 1 << 0xB;
        *eq_bitmask = PAGE_REQUEST_BIT;

        // set the next pointer if required
        if block_num != (num_mailboxes - 1){
            let mailbox = mailbox_page.0.as_type_mut::<CommandInterfaceMailbox>(block_num * MAILBOX_SIZE_IN_BYTES)?;
            let mailbox_next_addr = mailbox_starting_addr.value() + ((block_num + 1) * MAILBOX_SIZE_IN_BYTES);
            mailbox.next_pointer_h.write(U32::new((mailbox_next_addr >> 32) as u32));
            mailbox.next_pointer_l.write(U32::new((mailbox_next_addr & 0xFFFF_FFFF) as u32));
        }

        // Now use the remainder of the mailbox for page entries
        let eq_pa_offset = 0x110 - 0x10;
        let data = mailbox_page.0.as_type_mut::<[u8;256]>(eq_pa_offset)?;
        let pages_in_mailbox_0 = (MAILBOX_DATA_SIZE_IN_BYTES - eq_pa_offset) / SIZE_PADDR_IN_BYTES;

        for page in 0..pages_in_mailbox_0 {
            let paddr = pages.pop();
            match paddr {
                Some(paddr) => {
                    let start_offset_h = page * SIZE_PADDR_IN_BYTES;
                    let end_offset_h = start_offset_h + SIZE_PADDR_H_IN_BYTES;
                    let addr = (paddr.value() >> 32) as u32;
                    data[start_offset_h..end_offset_h].copy_from_slice(&addr.to_be_bytes());

                    let start_offset_l = end_offset_h;
                    let end_offset_l = start_offset_l + SIZE_PADDR_L_IN_BYTES;
                    let addr = (paddr.value() & 0xFFFF_FFFF) as u32;
                    data[start_offset_l..end_offset_l].copy_from_slice(&addr.to_be_bytes());
                },
                None => { 
                    trace!("breaking out of loop on mailbox: {} and page: {}", block_num, page);
                    break; 
                }
            }
        }

        for block_num in 1..num_mailboxes {
            let mailbox = mailbox_page.0.as_type_mut::<CommandInterfaceMailbox>(block_num * MAILBOX_SIZE_IN_BYTES)?;
            mailbox.clear_all_fields();

            if block_num != (num_mailboxes - 1){
                let mailbox_next_addr = mailbox_starting_addr.value() + ((block_num + 1) * MAILBOX_SIZE_IN_BYTES);
                mailbox.next_pointer_h.write(U32::new((mailbox_next_addr >> 32) as u32));
                mailbox.next_pointer_l.write(U32::new((mailbox_next_addr & 0xFFFF_FFFF) as u32));
            }
            mailbox.block_number.write(U32::new(block_num as u32));
            mailbox.token_ctrl_signature.write(U32::new((self.token as u32) << 16));

            let mut data = [0; 512];

            let pages_per_mailbox = MAILBOX_DATA_SIZE_IN_BYTES / SIZE_PADDR_IN_BYTES;

            for page in 0..pages_per_mailbox {
                let paddr = pages.pop();
                match paddr {
                    Some(paddr) => {
                        let start_offset_h = page * SIZE_PADDR_IN_BYTES;
                        let end_offset_h = start_offset_h + SIZE_PADDR_H_IN_BYTES;
                        let addr = (paddr.value() >> 32) as u32;
                        data[start_offset_h..end_offset_h].copy_from_slice(&addr.to_be_bytes());

                        let start_offset_l = end_offset_h;
                        let end_offset_l = start_offset_l + SIZE_PADDR_L_IN_BYTES;
                        let addr = (paddr.value() & 0xFFFF_FFFF) as u32;
                        data[start_offset_l..end_offset_l].copy_from_slice(&addr.to_be_bytes());
                    },
                    None => { 
                        trace!("breaking out of loop on mailbox: {} and page: {}", block_num, page);
                        break; 
                    }
                }
            }

            mailbox.mailbox_data.write(data);
            debug!("token: {}", self.token);
            debug!("Mailbox {}", block_num);
            debug!("{:?}", mailbox);
        }

        Ok(())
    }

    pub fn get_eq_number(&self, entry_num: usize) -> Result<u8, &'static str> {
        if self.entries[entry_num].owned_by_hw() {
            error!("the command hasn't completed yet!");
            return Err("the command hasn't completed yet!");
        }

        let opcode = self.entries[entry_num].get_command_opcode();
        if opcode != CommandOpcode::CreateEq {
            error!("Incorrect Command!");
            return Err("Incorrect Command!");
        }

        let (_status, _syndrome, eq_number, _reserved) = self.entries[entry_num].get_output_inline_data();
        Ok(eq_number as u8)
    }
}

#[derive(FromBytes)]
#[repr(C)]
struct EventQueueContext {
    status:             Volatile<U32<BigEndian>>,
    _padding1:          ReadOnly<u32>,
    page_offset:        Volatile<U32<BigEndian>>,
    uar_log_eq_size:    Volatile<U32<BigEndian>>,
    _padding2:          ReadOnly<u32>,
    intr:               Volatile<U32<BigEndian>>,
    log_pg_size:        Volatile<U32<BigEndian>>,
    _padding3:          ReadOnly<u64>,
    consumer_counter:   Volatile<U32<BigEndian>>,
    producer_counter:   Volatile<U32<BigEndian>>,
    _padding4:          ReadOnly<[u8;12]>,
}

const_assert_eq!(core::mem::size_of::<EventQueueContext>(), 64);

impl EventQueueContext {
    pub fn init(&mut self, uar_page: u32, log_eq_size: u8) {
        let uar = uar_page & 0xFF_FFFF;
        let size = ((log_eq_size & 0x1F) as u32) << 24;
        self.uar_log_eq_size.write(U32::new(uar | size));
        self.log_pg_size.write(U32::new(0));
    }
}
