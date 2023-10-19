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
use core::{mem::size_of, num::NonZeroU8, str::from_utf8};
use alloc::string::String;

mod ehci;

pub mod descriptors;
pub mod allocators;

use descriptors::{Descriptor, DescriptorType};
use allocators::CommonUsbAlloc;

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

impl<'a> Request<'a> {
    pub(crate) fn get_raw(&self) -> RawRequest {
        match self {
            Self::GetStatus(target, _dev_status) => RawRequest::new(
                (*target).into(),
                RequestType::Standard,
                Direction::In,
                RequestName::GetStatus,
                0u16,
                target.index(),
                2u16,
            ),
            Self::ClearFeature(target, feature_id) => RawRequest::new(
                (*target).into(),
                RequestType::Standard,
                Direction::Out,
                RequestName::ClearFeature,
                *feature_id,
                target.index(),
                0u16,
            ),
            Self::SetFeature(target, feature_id) => RawRequest::new(
                (*target).into(),
                RequestType::Standard,
                Direction::Out,
                RequestName::SetFeature,
                *feature_id,
                target.index(),
                0u16,
            ),
            Self::SetAddress(dev_addr) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                RequestName::SetAddress,
                *dev_addr as u16,
                0u16,
                0u16,
            ),
            Self::GetDescriptor(desc_index, descriptor) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                RequestName::GetDescriptor,
                (descriptor.get_type() << 8) | (*desc_index as u16),
                0u16,
                descriptor.get_length(),
            ),
            Self::SetDescriptor(desc_index, descriptor) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                RequestName::SetDescriptor,
                (descriptor.get_type() << 8) | (*desc_index as u16),
                0u16,
                descriptor.get_length(),
            ),
            Self::GetConfiguration(_config) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                RequestName::GetConfiguration,
                0u16,
                0u16,
                1u16,
            ),
            Self::SetConfiguration(config) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::Out,
                RequestName::SetConfiguration,
                config.map(|v| v.into()).unwrap_or(0) as u16,
                0u16,
                0u16,
            ),
            Self::GetInterfaceAltSetting(interface_idx, _alt_setting) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Standard,
                Direction::In,
                RequestName::SetConfiguration,
                0u16,
                *interface_idx,
                1u16,
            ),
            Self::SetInterfaceAltSetting(interface_idx, alt_setting) => RawRequest::new(
                RawRequestRecipient::Interface,
                RequestType::Standard,
                Direction::Out,
                RequestName::SetConfiguration,
                *alt_setting as u16,
                *interface_idx,
                0u16,
            ),
            Self::ReadString(string_idx, _string) => RawRequest::new(
                RawRequestRecipient::Device,
                RequestType::Standard,
                Direction::In,
                RequestName::GetDescriptor,
                ((u8::from(DescriptorType::String) as u16) << 8) | (*string_idx as u16),
                0u16,
                2u16,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Target {
    Device,
    Interface(InterfaceIndex),
    Endpoint(EndpointAddress),
}

impl Target {
    fn index(self) -> u16 {
        match self {
            Self::Device => 0u16,
            Self::Interface(index) => index,
            Self::Endpoint(addr) => addr.ep_number().value() as u16,
        }
    }
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
