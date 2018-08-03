#![allow(dead_code)]
#![no_std]


#![feature(const_fn)]

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


#[repr(C,packed)]
pub struct UsbDeviceDesc
{
    pub len: u8,
    pub deivce_type: u8,
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


// ------------------------------------------------------------------------------------------------
// USB Configuration Descriptor

#[repr(C,packed)]
pub struct UsbConfDesc
{
    pub len: u8,
    pub config_type: u8,
    pub total_len: u8,
    pub intf_count: u8,
    pub conf_value: u8,
    pub conf_str: u8,
    pub attributes: u8,
    pub max_power: u8,
}

// ------------------------------------------------------------------------------------------------
// USB String Descriptor

#[repr(C,packed)]
pub struct UsbStringDesc
{
    pub len: u8,
    pub string_type: u8,
    pub size: u8,
    pub str: [u16; 30],
}

// ------------------------------------------------------------------------------------------------
// USB Interface Descriptor

#[repr(C,packed)]
pub struct UsbIntfDesc
{

    pub len: u8,
    pub config_type: u8,
    pub intf_type: u8,
    pub intf_index: u8,
    pub alt_setting: u8,
    pub endp_count: u8,
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub inf_str: u8,
}

// ------------------------------------------------------------------------------------------------
// USB Endpoint Descriptor

#[repr(C,packed)]
pub struct UsbEndpDesc
{
    pub len: u8,
    pub endp_type: u8,
    pub addr: u8,
    pub attributes: u8,
    pub maxpacketsize: u16,
    pub interval: u8,
}

// ------------------------------------------------------------------------------------------------
// USB HID Desciptor

#[repr(C,packed)]
pub struct UsbHidDesc
{
    pub len: u8,
    pub hid_type: u8,
    pub version: u16,
    pub country_code: u8,
    pub desc_count: u8,
    pub desc_type: u8,
    pub desc_len: u16,
    pub length: u8,
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

    pub len: u8,
    pub hub_type: u8,
    pub port_count: u8,
    pub chars: u16,
    pub power_time: u8,
    pub current: u8,

}




