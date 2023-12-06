//! USB controller support
//!
//! Current support:
//! - EHCI controllers:
//!    * control transfers (requests): full support
//!    * interrupt transfers: full support
//!    * bulk: unused, no API support
//!    * isochronous transfers: unused, no API support
//!
//! Specfications:
//! * [USB 2.0](https://www.pjrc.com/teensy/beta/usb20.pdf)
//! * [xHCI](https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/extensible-host-controler-interface-usb-xhci.pdf)
//! * [EHCI](https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/ehci-specification-for-usb.pdf)
//! * [UHCI](https://stuff.mit.edu/afs/sipb/contrib/doc/specs/protocol/usb/UHCI11D.PDF)
//! * [OHCI](http://www.o3one.org/hwdocs/usb/hcir1_0a.pdf)

#![no_std]
#![allow(unused)]

// The allocator macros sometime mess with clippy
#![allow(clippy::needless_pub_self)]

extern crate alloc;

use memory::{MappedPages, BorrowedMappedPages, Mutable, PhysicalAddress, map_frame_range, create_identity_mapping, PAGE_SIZE, MMIO_FLAGS};
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
pub use controllers::ControllerType;

/// Global list of USB controllers in the system
static CONTROLLERS: RwLock<Vec<Mutex<Controller>>> = RwLock::new(Vec::new());

/// Initializes a USB controllers connected via PCI
pub fn init_pci(pci_dev: &'static PciDevice, ctrl_type: ControllerType) -> Result<(), &'static str> {
    // get rid of interrupts while we're setting things up
    pci_dev.pci_enable_interrupts(false);

    let base = pci_dev.determine_mem_base(0)?;
    let mut controller = match ctrl_type {
        ControllerType::Ehci => Controller::Ehci(controllers::ehci::EhciController::init(base)?),
    };

    controller.probe_ports()?;

    let controller_index;
    {
        let mut controllers = CONTROLLERS.write();
        controller_index = controllers.len();
        controllers.push(Mutex::new(controller));
    }

    new_task_builder(pci_usb_int_task, (pci_dev, controller_index)).spawn()?;

    Ok(())
}

// background task handling PCI interrupts
fn pci_usb_int_task(params: (&'static PciDevice, usize)) -> ! {
    let (pci_dev, controller_index) = params;
    let (waker, blocker) = new_waker();
    let _ = pci_dev.set_interrupt_waker(waker);

    loop {
        pci_dev.pci_enable_interrupts(true);
        blocker.block();
        log::info!("Now handling an interrupt in pci_usb_int_task");

        let controllers = CONTROLLERS.read();
        let mut controller = controllers[controller_index].lock();

        while pci_dev.pci_get_interrupt_status(false) {
            controller.handle_interrupt().unwrap();
        }
    }
}

type InterfaceId = usize;
type MaxPacketSize = u16;
type DeviceAddress = u8;
type InterfaceIndex = u8;
type StringIndex = u8;
type DescriptorIndex = u8;

/// Specific feature ID; must be appropriate to the recipient
type FeatureId = u16;

#[bitsize(16)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct DeviceStatus {
    self_powered: bool,
    remote_wakeup: bool,
    reserved: u14,
}

#[bitsize(8)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct EndpointAddress {
    ep_number: u4,
    reserved: u3,
    // Ignored for control endpoints
    direction: Direction,
}

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
