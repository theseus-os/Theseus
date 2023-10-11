//! USB controller support
//!
//! Current support:
//! - EHCI controllers:
//!    * control transfers: full support
//!    * other transfer types: unused, no API support

#![no_std]

extern crate alloc;

use pci::PciDevice;
use memory::{MappedPages, PhysicalAddress, map_frame_range, create_identity_mapping, PAGE_SIZE, MMIO_FLAGS};
use zerocopy::FromBytes;
use volatile::{Volatile, ReadOnly};
use bilge::prelude::*;
use sleep::{Duration, sleep};
use core::{mem::size_of, num::NonZeroU8};
use alloc::string::String;

mod ehci;

pub mod descriptors;

use descriptors::Descriptor;

pub enum Standard<T> {
    Ehci(T),
}

pub fn init(pci_device: Standard<&PciDevice>) -> Result<(), &'static str> {
    let mut ctrl = match pci_device {
        Standard::Ehci(dev) => ehci::EhciController::new(dev)?,
    };

    ctrl.probe_ports()?;
    ctrl.turn_off()?;

    Ok(())
}

pub type DeviceAddress = u8;

#[bitsize(16)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
pub struct DeviceStatus {
    self_powered: bool,
    remote_wakeup: bool,
    reserved: u14,
}

/// Specific feature ID; must be appropriate to the recipient
pub type FeatureId = u16;

pub type InterfaceIndex = u16;

#[bitsize(8)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
pub struct EndpointAddress {
    ep_number: u4,
    reserved: u3,
    // Ignored for control endpoints
    direction: Direction,
}

pub type StringIndex = u8;

pub type DescriptorIndex = u8;

pub enum Request<'a> {
    GetStatus(Target, &'a mut DeviceStatus),

    ClearFeature(Target, FeatureId),
    SetFeature(Target, FeatureId),

    SetAddress(DeviceAddress),

    GetDescriptor(DescriptorIndex, &'a mut Descriptor),
    SetDescriptor(DescriptorIndex, Descriptor),

    GetConfiguration(&'a mut Option<NonZeroU8>),
    SetConfiguration(Option<NonZeroU8>),

    GetInterfaceAltSetting(InterfaceIndex, &'a mut u8),
    SetInterfaceAltSetting(InterfaceIndex, u8),

    ReadString(StringIndex, &'a mut String),

    // not supported by this driver
    // SynchFrame(EndpointAddress, u16),
}

pub enum Target {
    Device,
    Interface(InterfaceIndex),
    Endpoint(EndpointAddress),
}

#[bitsize(64)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct RawRequest {
    pub recipient: RawRequestRecipient,
    pub req_type: RequestType,
    pub direction: Direction,

    pub request_name: RequestName,
    pub value: u16,
    pub index: u16,
    pub len: u16,
}

#[bitsize(1)]
#[derive(Debug, FromBits)]
pub enum Direction {
    /// Host to function
    Out = 0,
    /// Function to host
    In = 1,
}

#[bitsize(5)]
#[derive(Debug, FromBits)]
enum RawRequestRecipient {
    Device = 0x0,
    Interface = 0x1,
    Endpoint = 0x2,
    Other = 0x3,
    #[fallback]
    Reserved = 0x4,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum RequestType {
    Standard = 0x0,
    Class = 0x1,
    Vendor = 0x2,
    Reserved = 0x3,
}

#[bitsize(8)]
#[derive(Debug, FromBits)]
enum RequestName {
    GetStatus = 0x00,
    ClearFeature = 0x01,
    SetFeature = 0x03,
    SetAddress = 0x05,
    GetDescriptor = 0x06,
    SetDescriptor = 0x07,
    GetConfiguration = 0x08,
    SetConfiguration = 0x09,
    GetInterfaceAltSetting = 0x0a,
    SetInterfaceAltSetting = 0x0b,
    SyncFrame = 0x0c,

    #[fallback]
    Reserved = 0xff,
}

#[macro_export]
macro_rules! allocator {
    ($name:ident, $ty:ty, $cap:literal) => {

        #[derive(Debug, FromBytes)]
        struct $name {
            slots: [$ty; $cap],
            occupied: [u8; $cap],
        }

        impl $name {
            const OCCUPIED_TRUE: u8 = 1;
            const OCCUPIED_FALSE: u8 = 0;

            pub fn init(&mut self) {
                self.occupied.fill(Self::OCCUPIED_FALSE);
            }

            pub fn allocate(&mut self, init_to: Option<$ty>) -> Result<(usize, u32), &'static str> {
                for i in 0..$cap {
                    if self.occupied[i] == Self::OCCUPIED_FALSE {
                        self.occupied[i] = Self::OCCUPIED_TRUE;
                        let mut_ref = &mut self.slots[i];
                        if let Some(value) = init_to {
                            *mut_ref = value;
                        }
                        let addr = mut_ref as *mut _ as usize as u32;

                        return Ok((i, addr))
                    }
                }

                Err(concat!(stringify!($name), ": Out of slots"))
            }

            pub fn get(&self, index: usize) -> Result<&$ty, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                self.slots.get(index).ok_or(err_msg)
            }

            pub fn get_mut(&mut self, index: usize) -> Result<&mut $ty, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                self.slots.get_mut(index).ok_or(err_msg)
            }

            pub fn find(&self, addr: u32) -> Result<usize, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid address");
                let mut_ref = &self.slots[0];
                let addr_of_first = mut_ref as *const _ as usize as u32;

                let offset = addr.checked_sub(addr_of_first).ok_or(err_msg)? as usize;

                let type_size = core::mem::size_of::<$ty>();
                let index = (offset / type_size);

                let valid_offset = offset % type_size == 0;
                let valid_index = index < $cap;

                match (valid_offset, valid_index) {
                    (true, true) => Ok(index),
                    _ => Err(err_msg),
                }
            }

            pub fn get_by_addr(&self, addr: u32) -> Result<&$ty, &'static str> {
                self.get(self.find(addr)?)
            }

            pub fn get_mut_by_addr(&mut self, addr: u32) -> Result<&mut $ty, &'static str> {
                self.get_mut(self.find(addr)?)
            }

            pub fn address_of(&self, index: usize) -> Result<u32, &'static str> {
                let err_msg = concat!(stringify!($name), ": Invalid slot index");
                let reference = self.slots.get(index).ok_or(err_msg)?;
                Ok(reference as *const _ as usize as u32)
            }
        }

    }
}
