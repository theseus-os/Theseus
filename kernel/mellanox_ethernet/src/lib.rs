 //! Note: Mellanox manual refers to the NIC as HCA.

 #![no_std]

#[macro_use]extern crate log;
extern crate memory;
extern crate volatile;
extern crate bit_field;
extern crate zerocopy;
extern crate alloc;
#[macro_use] extern crate static_assertions;
extern crate owning_ref;

use alloc::vec::Vec;
use memory::{PhysicalAddress, PhysicalMemoryRegion, MappedPages};
use volatile::{Volatile, ReadOnly, WriteOnly};
use bit_field::BitField;
use zerocopy::FromBytes;
use owning_ref:: BoxRefMut;

// Taken from HCA BAR 
const MAX_CMND_QUEUE_ENTRIES: usize = 64;
#[derive(FromBytes)]
#[repr(C,packed)]
pub struct InitializationSegment {
    fw_rev_major:               ReadOnly<u16>,
    fw_rev_minor:               ReadOnly<u16>,
    fw_rev_subminor:            ReadOnly<u16>,
    cmd_interface_rev:          ReadOnly<u16>,
    _padding1:                  [u8; 8],
    cmdq_phy_addr_high:         Volatile<u32>,
    cmdq_phy_addr_low:          Volatile<u32>,
    command_doorbell_vector:    Volatile<u32>,
    _padding2a:                 [u8; 256],
    _padding2b:                 [u8; 128],
    _padding2c:                 [u8; 98],
    initializing_state:         ReadOnly<u16>,
    health_buffer:              Volatile<[u8; 64]>,
    no_dram_nic_offset:         ReadOnly<u32>,
    _padding3a:                 [u8; 2048],
    _padding3b:                 [u8; 1024],
    _padding3c:                 [u8; 256],
    _padding3d:                 [u8; 128],
    _padding3e:                 [u8; 60],
    internal_timer:             ReadOnly<u64>,
    _padding4:                  [u8; 8],
    health_counter:             ReadOnly<u32>,
    _padding5:                  [u8; 44],
    real_time:                  ReadOnly<u64>,
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
        (self.cmdq_phy_addr_low.read() >> 4) as u8 & 0x0F
    }

    pub fn cmdq_entry_stride(&self) -> u8 {
        (self.cmdq_phy_addr_low.read()) as u8 & 0x0F
    }
    pub fn set_physical_address_of_cmdq(&mut self, pa: PhysicalAddress) {
        self.cmdq_phy_addr_high.write((pa.value() >> 32) as u32);
        self.cmdq_phy_addr_low.write(pa.value() as u32);
    }

    // pub fn set_doorbell(command_no: u8) {

    // }

    pub fn device_is_initializing(&self) -> bool {
        self.initializing_state.read().get_bit(15)
    }

    pub fn pf_reset(&mut self) {
        let val = self.cmdq_phy_addr_low.read();
        self.cmdq_phy_addr_low.write(val | (0x7 << 8));
    }

    // pub fn initializing_state() -> InitializingState {

    // }

    pub fn post_command(&mut self, command_bit: usize) {
        let val = self.command_doorbell_vector.read();
        self.command_doorbell_vector.write(val | (1 << command_bit));
    }

    pub fn print(&self) {
        trace!("{:#X} {:#X} {:#X} {:#X}",self.fw_rev_major.read(), self.fw_rev_minor.read(), self.fw_rev_subminor.read(), self.cmd_interface_rev.read());
        trace!("{:#X} {:#X}",self.cmdq_phy_addr_high.read(), self.cmdq_phy_addr_low.read());
        trace!("{:#X}",self.command_doorbell_vector.read());
        trace!("{:#X}",self.initializing_state.read());
    }
}

pub enum InitializingState {
    NotAllowed = 0,
    WaitingPermetion = 1, // Is this a typo?
    WaitingResources = 2,
    Abort = 3
}

/// Section 8.24.1
/// A buffer of fixed-size entries that is used to pass commands to the HCA.
/// The number of enties and the entry stride is retrieved from the initialization segment of the HCA BAR.
/// It resides in a physically contiguous 4 KiB memory chunk.
#[repr(C)]
pub struct CommandQueue {
    pub entries: BoxRefMut<MappedPages, [CommandQueueEntry]>,
    pub current_entry: usize
}

#[derive(FromBytes, Default)]
#[repr(C)]
pub struct CommandQueueEntry {
    type_of_transport:              Volatile<u32>,
    input_length:                   Volatile<u32>,
    input_mailbox_pointer:          Volatile<u64>,
    padding1:                       WriteOnly<u16>,
    command_input_opcode:           Volatile<u16>,
    command_input_op_mod:           Volatile<u16>,
    padding2:                       WriteOnly<u16>,
    command_input_inline_data:      Volatile<u64>,
    padding3:                       WriteOnly<u16>,
    padding4:                       WriteOnly<u8>,
    command_output_status:          Volatile<u8>,
    command_output_syndrome:        Volatile<u32>,
    command_output_inline_data:     Volatile<u64>,
    output_mailbox_pointer:         Volatile<u64>,
    output_length:                  Volatile<u32>,
    status:                         Volatile<u8>,
    padding5:                       WriteOnly<u8>,
    signature:                      Volatile<u8>,
    token:                          Volatile<u8>
}


const_assert_eq!(core::mem::size_of::<CommandQueueEntry>(), 64);

impl CommandQueueEntry {
    pub fn init_cmdq_entry(&mut self, command: CommandOpcode) {
        match command {
            CommandOpcode::EnableHca => {
                let mut init_hca = CommandQueueEntry::default();
                init_hca.type_of_transport.write(0x7 << 24);
                init_hca.input_length.write(12);
                init_hca.command_input_opcode.write(CommandOpcode::EnableHca as u16);
                init_hca.change_ownership_to_hw();
                core::mem::swap(&mut init_hca, self);
            }
            CommandOpcode::InitHca => {
                let mut init_hca = CommandQueueEntry::default();
                init_hca.type_of_transport.write(0x7 << 24);
                init_hca.input_length.write(16);
                init_hca.command_input_opcode.write(CommandOpcode::InitHca as u16);
                init_hca.change_ownership_to_hw();
                core::mem::swap(&mut init_hca, self);
            }
            _=> {
                debug!("unimplemented opcode");
            }
        }
    }

    pub fn change_ownership_to_hw(&mut self) {
        self.status.write(1);
    }
    pub fn owned_by_hw(&self) -> bool {
        self.status.read().get_bit(0)
    }
    pub fn status(&self) -> CommandDeliveryStatus {
        let status = self.status.read() & 0xFE;
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

pub enum CommandOpcode {
    QueryHcaCap = 0x100,
    QueryAdapter = 0x101,
    InitHca = 0x102,
    TeardownHca = 0x103,
    EnableHca = 0x104,
    DisableHca = 0x105,
    QueryPages = 0x107,
    ManagePages = 0x108
}
#[derive(FromBytes)]
#[repr(C)]
pub struct CommandInterfaceMailbox {
    mailbox_data:       Volatile<[u8; 512]>,
    _padding1:          [u8; 48],
    next_pointer:       Volatile<u64>,
    block_number:       Volatile<u32>,
    signature:          Volatile<u8>,
    ctrl_signature:     Volatile<u8>,
    token:              Volatile<u8>,
    _passing2:          u8
}

const_assert_eq!(core::mem::size_of::<CommandInterfaceMailbox>(), 576);

pub fn init_cmdq_entry(entry: &mut CommandQueueEntry, command: CommandOpcode) {
    match command {
        CommandOpcode::InitHca => {
            let mut init_hca = CommandQueueEntry::default();
            init_hca.command_input_opcode.write(CommandOpcode::InitHca as u16);
            core::mem::swap(&mut init_hca, entry);
        }
        _=> {
            debug!("unimplemented opcode");
        }
    }
}
// #[derive(FromBytes)]
// #[repr(C)]
// struct UARPageFormat {
//     _padding1: u32, //0x00 - 0x1C
//     cmds_cq_ci: u32,
//     cqn: u32,
//     _padding2: 

// }
