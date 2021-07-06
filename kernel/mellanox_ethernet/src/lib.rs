//! This crate defines the layout of memory objects that make up the software interface between the Mellanox hardware and the driver,
//! as well as functions to access different fields of these objects.
//!
//! The Mellanox ethernet card is referred to as both the NIC (Network Interface Card) and the HCA (Host Channel Adapter).
//!
//! All information is taken from the Mellanox Adapters Programmer’s Reference Manual (PRM) [Rev 0.54], unless otherwise specified. 

 #![no_std]
 #![feature(slice_pattern)]
 #![feature(core_intrinsics)]
 #![allow(dead_code)] //  to suppress warnings for unused functions/methods

#[macro_use]extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate static_assertions;
extern crate memory;
extern crate volatile;
extern crate bit_field;
extern crate zerocopy;
extern crate owning_ref;
extern crate byteorder;
extern crate nic_initialization;
extern crate kernel_config;
extern crate libm;

use memory::PhysicalAddress;
use volatile::{Volatile, ReadOnly};
use bit_field::BitField;
use zerocopy::*;
use byteorder::BigEndian;
use core::fmt;

pub mod command_queue;

#[derive(FromBytes)]
#[repr(C,packed)]
/// The initialization segment is located at offset 0 of PCI BAR0.
/// It is used in the initialization procedure of the device,
/// and it contains the 32-bit command doorbell vector used to inform the HW when a command is ready to be processed.
pub struct InitializationSegment {
    /// Firmware Revision - Minor
    fw_rev_minor:               ReadOnly<U16<BigEndian>>,
    /// Firmware Revision - Major
    fw_rev_major:               ReadOnly<U16<BigEndian>>,
    /// Command Interface Interpreter Revision ID
    cmd_interface_rev:          ReadOnly<U16<BigEndian>>,
    /// Firmware Sub-minor version (Patch level)
    fw_rev_subminor:            ReadOnly<U16<BigEndian>>,
    _padding1:                  [u8; 8],
    /// MSBs of the physical address of the command queue record.
    cmdq_phy_addr_high:         Volatile<U32<BigEndian>>,
    /// LSBs of the physical address of the command queue record.
    cmdq_phy_addr_low:          Volatile<U32<BigEndian>>,
    /// Bit per command in the cmdq.
    /// When the bit is set, that command entry in the queue is moved to HW ownership.
    command_doorbell_vector:    Volatile<U32<BigEndian>>,
    _padding2:                  [u8; 390],
    /// If bit 31 is set, the device is still initializing and driver should not post commands
    initializing_state:         ReadOnly<U32<BigEndian>>,
    /// Advanced debug information.
    health_buffer:              Volatile<[u8; 64]>,
    /// The offset in bytes, inside the initialization segment, where the NODNIC registers can be found.
    no_dram_nic_offset:         ReadOnly<U32<BigEndian>>,
    _padding3:                  [u8; 3516],
    /// MSBs of the current internal timer value
    internal_timer_h:           ReadOnly<U32<BigEndian>>,
    /// LSBs of the current internal timer value
    internal_timer_l:           ReadOnly<U32<BigEndian>>,
    _padding4:                  [u8; 8],
    /// Advanced debug information
    health_counter:             ReadOnly<U32<BigEndian>>,
    _padding5:                  [u8; 44],
    real_time:                  ReadOnly<U64<BigEndian>>,
    _padding6:                  [u8; 12228],
}

// const_assert_eq!(core::mem::size_of::<InitializationSegment>(), 16400);

impl InitializationSegment {
    /// Returns the maximum number of entries that can be in the command queue
    pub fn num_cmdq_entries(&self) -> u8 {
        let log = (self.cmdq_phy_addr_low.read().get() >> 4) & 0x0F;
        2_u8.pow(log)
    }

    /// Returns the required stride of command queue entries (bytes between the start of consecutive entries)
    pub fn cmdq_entry_stride(&self) -> u8 {
        let val = self.cmdq_phy_addr_low.read().get() & 0x0F;
        2_u8.pow(val)
    }
    
    /// Sets the physical address of the command queue within the initialization segment.
    ///
    /// # Arguments
    /// * `cmdq_physical_addr`: the starting physical address of the command queue, the lower 12 bits of which must be zero. 
    pub fn set_physical_address_of_cmdq(&mut self, cmdq_physical_addr: PhysicalAddress) -> Result<(), &'static str> {
        if cmdq_physical_addr.value() & 0xFFF != 0 {
            return Err("cmdq physical address lower 12 bits must be zero.");
        }

        self.cmdq_phy_addr_high.write(U32::new((cmdq_physical_addr.value() >> 32) as u32));
        let val = self.cmdq_phy_addr_low.read().get() & 0xFFF;
        self.cmdq_phy_addr_low.write(U32::new(cmdq_physical_addr.value() as u32 | val));
        Ok(())
    }

    /// Returns true if the device is still initializing, and driver should not pass any commands to the device.
    pub fn device_is_initializing(&self) -> bool {
        self.initializing_state.read().get().get_bit(31)
    }

    /// Sets a bit in the command doorbell vector to inform HW that the command needs to be executed.
    ///
    /// # Arguments
    /// * `command bit`: the command entry that needs to be executed. (e.g. bit 0 corresponds to entry at index 0).
    pub fn post_command(&mut self, command_bit: usize) {
        let val = self.command_doorbell_vector.read().get();
        self.command_doorbell_vector.write(U32::new(val | (1 << command_bit)));
    }
}

impl fmt::Debug for InitializationSegment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Initialization Segment \n")?;
        write!(f, "Firmware: {}.{}.{}, command interface: {} \n", self.fw_rev_major.read().get(), self.fw_rev_minor.read().get(), self.fw_rev_subminor.read().get(), self.cmd_interface_rev.read().get())?;
        write!(f, "Command queue address: {:#X} {:#X} \n", self.cmdq_phy_addr_high.read().get(), self.cmdq_phy_addr_low.read().get())?;
        write!(f, "Command doorbell vector: {:#X} \n", self.command_doorbell_vector.read().get())?;
        write!(f, "Initializing state: {:#X} \n", self.initializing_state.read().get())
    }
}

/// The possible values of the initialization state of the device as taken from the intialization segment.
pub enum InitializingState {
    NotAllowed = 0,
    WaitingPermetion = 1, // Is this a typo in the PRM?
    WaitingResources = 2,
    Abort = 3
}
