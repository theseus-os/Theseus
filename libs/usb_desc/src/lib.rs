#![allow(dead_code)]
#![no_std]


#![feature(const_fn)]

extern crate alloc;
// ------------------------------------------------------------------------------------------------
// USB Base Descriptor Types

static USB_DESC_DEVICE:u8 =                 0x01;
static USB_DESC_CONF:u8 =                   0x02;
static USB_DESC_STRING:u8 =                 0x03;
static USB_DESC_INTF:u8 =                   0x04;
static USB_DESC_ENDP:u8 =                   0x05;

// ------------------------------------------------------------------------------------------------
// USB HID Descriptor Types

static USB_DESC_REPORT:u8 =                 0x22;
static USB_DESC_PHYSICAL:u8 =               0x23;

// ------------------------------------------------------------------------------------------------
// USB HUB Descriptor Types

static USB_DESC_HUB:u8 =                    0x29;

// ------------------------------------------------------------------------------------------------
// USB Device Descriptor

pub struct UsbDeviceDesc
{
    len: u8,
    deivce_type: u8,
    usb_version: u16,
    class: u8,
    sub_class: u8,
    protocol: u8,
    max_packet_size: u8,
    vendor_id: u16,
    product_id: u16,
    device_version: u16,
    vendor_str: u8,
    product_str: u8,
    serial_str: u8,
    conf_count: u8,
}

// ------------------------------------------------------------------------------------------------
// USB Configuration Descriptor

struct UsbConfDesc
{
    len: u8,
    config_type: u8,
    total_len: u8,
    intf_count: u8,
    conf_value: u8,
    conf_str: u8,
    attributes: u8,
    max_power: u8,
}

// ------------------------------------------------------------------------------------------------
// USB String Descriptor

pub struct UsbStringDesc
{
    len: u8,
    string_type: u8,
    size: u8,
    str: [u16; 30],
}

// ------------------------------------------------------------------------------------------------
// USB Interface Descriptor

pub struct UsbIntfDesc
{

    len: u8,
    config_type: u8,
    intf_type: u8,
    intf_index: u8,
    alt_setting: u8,
    endp_count: u8,
    class: u8,
    sub_class: u8,
    protocol: u8,
    inf_str: u8,
}

// ------------------------------------------------------------------------------------------------
// USB Endpoint Descriptor

pub struct UsbEndpDesc
{
    len: u8,
    endp_type: u8,
    addr: u8,
    attributes: u8,
    maxpacketsize: u16,
    interval: u8,
}

// ------------------------------------------------------------------------------------------------
// USB HID Desciptor

pub struct UsbHidDesc
{
    len: u8,
    hid_type: u8,
    version: u16,
    country_code: u8,
    desc_count: u8,
    desc_type: u8,
    desc_len: u16,
    length: u8,
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


pub struct UsbHubDesc
{

    len: u8,
    hub_type: u8,
    port_count: u8,
    chars: u16,
    power_time: u8,
    current: u8,

}




