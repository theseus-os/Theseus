#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate alloc;
extern crate volatile;
extern crate owning_ref;
extern crate memory;

use alloc::boxed::Box;
use owning_ref::{BoxRef, BoxRefMut};
use volatile::{Volatile, ReadOnly, WriteOnly};
use memory::{Frame,PageTable, ActivePageTable, PhysicalAddress, VirtualAddress, EntryFlags,
             MappedPages, allocate_pages,allocate_frame,FRAME_ALLOCATOR};
// ------------------------------------------------------------------------------------------------
// USB Base Descriptor Types

pub static USB_DESC_DEVICE:u16 =                 0x01 << 8;
pub static USB_DESC_CONF:u16 =                   0x02 << 8;
pub static USB_DESC_STRING:u16 =                 0x03 << 8;
pub static USB_DESC_INTF:u16 =                   0x04 << 8;
pub static USB_DESC_ENDP:u16 =                   0x05 << 8;

// ------------------------------------------------------------------------------------------------
// USB HID Descriptor Types

static USB_DESC_REPORT:u8 =                 0x22;
static USB_DESC_PHYSICAL:u8 =               0x23;

// ------------------------------------------------------------------------------------------------
// USB HUB Descriptor Types

static USB_DESC_HUB:u8 =                    0x29;

// ------------------------------------------------------------------------------------------------
// USB Device Descriptor


#[repr(C,packed)]
pub struct UsbDeviceDesc
{
    pub len: Volatile<u8>,
    pub device_type: Volatile<u8>,
    pub usb_version: Volatile<u16>,
    pub class: Volatile<u8>,
    pub sub_class: Volatile<u8>,
    pub protocol: Volatile<u8>,
    pub max_packet_size: Volatile<u8>,
    pub vendor_id: Volatile<u16>,
    pub product_id: Volatile<u16>,
    pub device_version: Volatile<u16>,
    pub vendor_str: Volatile<u8>,
    pub product_str: Volatile<u8>,
    pub serial_str: Volatile<u8>,
    pub conf_count: Volatile<u8>,
}

impl UsbDeviceDesc {
    pub fn default() -> UsbDeviceDesc {
        UsbDeviceDesc {
            len: Volatile::new(0),
            device_type: Volatile::new(0),
            usb_version: Volatile::new(0),
            class: Volatile::new(0),
            sub_class: Volatile::new(0),
            protocol: Volatile::new(0),
            max_packet_size: Volatile::new(0),
            vendor_id: Volatile::new(0),
            product_id: Volatile::new(0),
            device_version: Volatile::new(0),
            vendor_str: Volatile::new(0),
            product_str: Volatile::new(0),
            serial_str: Volatile::new(0),
            conf_count: Volatile::new(0),
        }
    }
}



// ------------------------------------------------------------------------------------------------
// USB Configuration Descriptor

#[repr(C,packed)]
pub struct UsbConfDesc
{
    pub len: Volatile<u8>,
    pub config_type: Volatile<u8>,
    pub total_len: Volatile<u16>,
    pub intf_count: Volatile<u8>,
    pub conf_value: Volatile<u8>,
    pub conf_str: Volatile<u8>,
    pub attributes: Volatile<u8>,
    pub max_power: Volatile<u8>,
}

/// Box the the frame pointer
pub fn box_config_desc(active_table: &mut ActivePageTable,page: MappedPages)
                      -> Result<BoxRefMut<MappedPages, UsbConfDesc>, &'static str>{


    let config_desc: BoxRefMut<MappedPages, UsbConfDesc>  = BoxRefMut::new(Box::new(page))
        .try_map_mut(|mp| mp.as_type_mut::<UsbConfDesc>(0))?;

    Ok(config_desc)
}


// ------------------------------------------------------------------------------------------------
// USB String Descriptor

#[repr(C,packed)]
pub struct UsbStringDesc
{
    pub len: Volatile<u8>,
    pub string_type: Volatile<u8>,
    pub size: Volatile<u8>,
    pub str: [Volatile<u16>; 30],
}

// ------------------------------------------------------------------------------------------------
// USB Interface Descriptor

#[repr(C,packed)]
pub struct UsbIntfDesc
{

    pub len: Volatile<u8>,
    pub desc_type: Volatile<u8>,
    pub intf_num: Volatile<u8>,
    pub alt_setting: Volatile<u8>,
    pub endp_count: Volatile<u8>,
    pub class: Volatile<u8>,
    pub sub_class: Volatile<u8>,
    pub protocol: Volatile<u8>,
    pub inf_str: Volatile<u8>,
}

// ------------------------------------------------------------------------------------------------
// USB Endpoint Descriptor

#[repr(C,packed)]
pub struct UsbEndpDesc
{
    pub len: Volatile<u8>,
    pub endp_type: Volatile<u8>,
    pub addr: Volatile<u8>,
    pub attributes: Volatile<u8>,
    pub maxpacketsize: Volatile<u16>,
    pub interval: Volatile<u8>,
    _padding: u16,
}

// ------------------------------------------------------------------------------------------------
// USB HID Desciptor

#[repr(C,packed)]
pub struct UsbHidDesc
{
    pub len: Volatile<u8>,
    pub hid_type: Volatile<u8>,
    pub version: Volatile<u16>,
    pub country_code: Volatile<u8>,
    pub desc_count: Volatile<u8>,
    pub desc_type: Volatile<u8>,
    pub desc_len: Volatile<u16>,
    pub length: Volatile<u8>,
}

// ------------------------------------------------------------------------------------------------
// USB Hub Descriptor

// Hub Characteristics
static HUB_POWER_MASK:u8 =                  0x03;        // Logical Power Switching Mode
static HUB_POWER_GLOBAL:u8 =                0x00;
static HUB_POWER_INDIVIDUAL:u8 =            0x01;
static HUB_COMPOUND:u8 =                    0x04;        // Part of a Compound Device
static HUB_CURRENT_MASK:u8 =                0x18;        // Over-current Protection Mode
static HUB_TT_TTI_MASK:u8 =                 0x60;        // TT Think Time
static HUB_PORT_INDICATORS:u8 =             0x80;        // Port Indicators


#[repr(C,packed)]
pub struct UsbHubDesc
{

    pub len: Volatile<u8>,
    pub hub_type: Volatile<u8>,
    pub port_count: Volatile<u8>,
    pub chars: Volatile<u16>,
    pub power_time: Volatile<u8>,
    pub current: Volatile<u8>,

}




