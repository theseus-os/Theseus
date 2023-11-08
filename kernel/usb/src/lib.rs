//! USB controller support
//!
//! Current support:
//! - EHCI controllers:
//!    * control transfers (requests): full support
//!    * interrupt transfers: full support
//!    * bulk: unused, no API support
//!    * isochronous transfers: unused, no API support

#![no_std]
#![allow(unused)]

extern crate alloc;

use memory::{MappedPages, PhysicalAddress, map_frame_range, create_identity_mapping, PAGE_SIZE, MMIO_FLAGS};
use core::{mem::size_of, num::NonZeroU8, str::from_utf8, any::TypeId, ops::{Deref, DerefMut}};
use alloc::{string::String, vec::Vec};
use volatile::{Volatile, ReadOnly};
use sync_irq::{RwLock, Mutex};
use sleep::{Duration, sleep};
use spawn::new_task_builder;
use zerocopy::FromBytes;
use bilge::prelude::*;
use waker::new_waker;
use pci::PciDevice;

mod interfaces;
mod controllers;
mod descriptors;
mod allocators;
mod request;

use descriptors::DescriptorType;
use allocators::{CommonUsbAlloc, AllocSlot, UsbPointer, invalid_ptr_slot};
use request::Request;
use controllers::Controller;
pub use controllers::PciInterface;

static CONTROLLERS: RwLock<Vec<Mutex<Controller>>> = RwLock::new(Vec::new());

pub fn init(pci_dev: &'static PciDevice, interface: PciInterface) -> Result<(), &'static str> {
    // get rid of interrupts while we're setting things up
    pci_dev.pci_enable_interrupts(false);

    let base = pci_dev.determine_mem_base(0)?;
    let mut controller = match interface {
        PciInterface::Ehci => Controller::Ehci(controllers::ehci::EhciController::init(base)?),
    };

    controller.probe_ports()?;

    let controller_index;
    {
        let mut controllers = CONTROLLERS.write();
        controller_index = controllers.len();
        controllers.push(Mutex::new(controller));
    }

    new_task_builder(pci_usb_bridge_int_handler, (pci_dev, controller_index)).spawn()?;

    Ok(())
}

fn pci_usb_bridge_int_handler(params: (&'static PciDevice, usize)) -> ! {
    let (pci_dev, controller_index) = params;
    let (waker, blocker) = new_waker();
    let _ = pci_dev.set_interrupt_waker(waker);

    loop {
        pci_dev.pci_enable_interrupts(true);
        blocker.block();
        log::info!("Now handling an interrupt in pci_usb_bridge_int_handler");

        let controllers = CONTROLLERS.read();
        let mut controller = controllers[controller_index].lock();

        while pci_dev.pci_get_interrupt_status(false) {
            controller.handle_interrupt().unwrap();
        }
    }
}

type InterfaceId = usize;
type DeviceAddress = u8;
type MaxPacketSize = u16;

#[bitsize(16)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct DeviceStatus {
    self_powered: bool,
    remote_wakeup: bool,
    reserved: u14,
}

/// Specific feature ID; must be appropriate to the recipient
type FeatureId = u16;

type InterfaceIndex = u8;

#[bitsize(8)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct EndpointAddress {
    ep_number: u4,
    reserved: u3,
    // Ignored for control endpoints
    direction: Direction,
}

type StringIndex = u8;

type DescriptorIndex = u8;

#[derive(Debug, Clone, Copy)]
enum Target {
    Device,
    Interface(InterfaceIndex),
    Endpoint(EndpointAddress),
}

impl Target {
    fn index(self) -> u16 {
        match self {
            Self::Device => 0u16,
            Self::Interface(index) => index as u16,
            Self::Endpoint(addr) => addr.ep_number().value() as u16,
        }
    }
}

#[bitsize(64)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct RawRequest {
    recipient: RawRequestRecipient,
    req_type: RequestType,
    direction: Direction,

    request_name: u8,
    value: u16,
    index: u16,
    len: u16,
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
enum RawRequestRecipient {
    Device = 0x0,
    Interface = 0x1,
    Endpoint = 0x2,
    Other = 0x3,
    #[fallback]
    Reserved = 0x4,
}

impl From<Target> for RawRequestRecipient {
    fn from(target: Target) -> RawRequestRecipient {
        match target {
            Target::Device => RawRequestRecipient::Device,
            Target::Interface(_) => RawRequestRecipient::Interface,
            Target::Endpoint(_) => RawRequestRecipient::Endpoint,
        }
    }
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum RequestType {
    Standard = 0x0,
    Class = 0x1,
    Vendor = 0x2,
    Reserved = 0x3,
}

mod std_req {
    pub const GET_STATUS: u8 = 0x00;
    pub const CLEAR_FEATURE: u8 = 0x01;
    pub const SET_FEATURE: u8 = 0x03;
    pub const SET_ADDRESS: u8 = 0x05;
    pub const GET_DESCRIPTOR: u8 = 0x06;
    pub const SET_DESCRIPTOR: u8 = 0x07;
    pub const GET_CONFIGURATION: u8 = 0x08;
    pub const SET_CONFIGURATION: u8 = 0x09;
    pub const GET_INTERFACE_ALT_SETTING: u8 = 0x0a;
    pub const SET_INTERFACE_ALT_SETTING: u8 = 0x0b;
    pub const _SYNC_FRAME: u8 = 0x0c;
}

mod hid_req {
    pub const GET_REPORT: u8 = 0x01;
    pub const SET_REPORT: u8 = 0x09;
    pub const GET_PROTOCOL: u8 = 0x03;
    pub const SET_PROTOCOL: u8 = 0x0B;
    pub const _GET_IDLE: u8 = 0x02;
    pub const _SET_IDLE: u8 = 0x0A;
}
