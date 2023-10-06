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
use core::mem::size_of;

mod ehci;

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

#[derive(Copy, Clone, Debug, Default, FromBytes)]
#[repr(C)]
pub struct DeviceDescriptor {
    pub len: u8,
    pub device_type: u8,
    pub usb_version: u16,
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub max_packet_size: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_version: u16,
    pub vendor_str: u8,
    pub product_str: u8,
    pub serial_str: u8,
    pub conf_count: u8,
}

#[bitsize(64)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct Request {
    pub recipient: RequestRecipient,
    pub req_type: RequestType,
    pub direction: Direction,

    pub request_name: RequestName,
    pub value: u16,
    pub index: u16,
    pub len: u16,
}

#[bitsize(1)]
#[derive(Debug, FromBits)]
enum Direction {
    /// Host to function
    Out = 0,
    /// Function to host
    In = 1,
}

#[bitsize(5)]
#[derive(Debug, FromBits)]
enum RequestRecipient {
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
    GetIntf = 0x0a,
    SetIntf = 0x0b,
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

            pub fn alloc(&mut self, value: $ty) -> Result<(usize, u32), &'static str> {
                log::info!("Allocating one {} out of {}", stringify!($ty), $cap);
                for i in 0..$cap {
                    if self.occupied[i] == Self::OCCUPIED_FALSE {
                        self.occupied[i] = Self::OCCUPIED_TRUE;
                        let mut_ref = &mut self.slots[i];
                        *mut_ref = value;
                        let addr = mut_ref as *mut _ as usize as u32;

                        log::info!("slot addr: 0x{:x}", addr);
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
        }

    }
}
