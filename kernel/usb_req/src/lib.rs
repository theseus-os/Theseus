#![no_std]
#![feature(alloc)]

#![allow(dead_code)]


// ------------------------------------------------------------------------------------------------
// USB Request Type

pub static RT_TRANSFER_MASK:u8 =                0x80;
pub static RT_DEV_TO_HOST:u8 =                  0x80;
pub static RT_HOST_TO_DEV:u8 =                  0x00;

pub static  RT_TYPE_MASK:u8 =                   0x60;
pub static  RT_STANDARD:u8 =                    0x00;
pub static  RT_CLASS:u8 =                       0x20;
pub static  RT_VENDOR:u8 =                      0x40;

pub static  RT_RECIPIENT_MASK:u8 =              0x1f;
pub static  RT_DEV:u8 =                         0x00;
pub static  RT_INTF:u8 =                        0x01;
pub static  RT_ENDP:u8 =                        0x02;
pub static  RT_OTHER:u8 =                       0x03;

// ------------------------------------------------------------------------------------------------
// USB Device Requests

pub static REQ_GET_STATUS:u8 =                  0x00;
pub static REQ_CLEAR_FEATURE:u8 =               0x01;
pub static REQ_SET_FEATURE:u8 =                 0x03;
pub static REQ_SET_ADDR:u8 =                    0x05;
pub static REQ_GET_DESC:u8 =                    0x06;
pub static REQ_SET_DESC:u8 =                    0x07;
pub static REQ_GET_CONF:u8 =                    0x08;
pub static REQ_SET_CONF:u8 =                    0x09;
pub static REQ_GET_INTF:u8 =                    0x0a;
pub static REQ_SET_INTF:u8 =                    0x0b;
pub static REQ_SYNC_FRAME:u8 =                  0x0c;

// ------------------------------------------------------------------------------------------------
// USB Hub Class Requests

pub static REQ_CLEAR_TT_BUFFER:u8 =              0x08;
pub static REQ_RESET_TT:u8 =                     0x09;
pub static REQ_GET_TT_STATE:u8 =                 0x0a;
pub static REQ_STOP_TT:u8 =                      0x0b;

// ------------------------------------------------------------------------------------------------
// USB HID Interface Requests

pub static REQ_GET_REPORT:u8 =                   0x01;
pub static REQ_GET_IDLE:u8 =                     0x02;
pub static REQ_GET_PROTOCOL:u8 =                 0x03;
pub static REQ_SET_REPORT:u8 =                   0x09;
pub static REQ_SET_IDLE:u8 =                     0x0a;
pub static REQ_SET_PROTOCOL:u8 =                 0x0b;

// ------------------------------------------------------------------------------------------------
// USB Standard Feature Selectors

pub static F_DEVICE_REMOTE_WAKEUP:u8 =          1;   // Device
pub static F_ENDPOINT_HALT:u8 =                 2;  // Endpoint
pub static F_TEST_MODE:u8 =                     3;   // Device

// ------------------------------------------------------------------------------------------------
// USB Hub Feature Seletcors

pub static F_C_HUB_LOCAL_POWER:u8 =              0;   // Hub
pub static F_C_HUB_OVER_CURRENT:u8 =             1;   // Hub
pub static F_PORT_CONNECTION:u8 =                0;   // Port
pub static F_PORT_ENABLE:u8 =                    1;   // Port
pub static F_PORT_SUSPEND:u8 =                   2;   // Port
pub static F_PORT_OVER_CURRENT:u8 =              3;   // Port
pub static F_PORT_RESET:u8 =                     4;   // Port
pub static F_PORT_POWER:u8 =                     8;   // Port
pub static F_PORT_LOW_SPEED:u8 =                 9;   // Port
pub static F_C_PORT_CONNECTION:u8 =              16;  // Port
pub static F_C_PORT_ENABLE:u8 =                  17;  // Port
pub static F_C_PORT_SUSPEND:u8 =                 18;  // Port
pub static F_C_PORT_OVER_CURRENT:u8 =            19;  // Port
pub static F_C_PORT_RESET:u8 =                   20;  // Port
pub static F_PORT_TEST:u8 =                      21;  // Port
pub static F_PORT_INDICATOR:u8 =                 22;  // Port

// ------------------------------------------------------------------------------------------------
// USB Device Request

#[repr(C,packed)]
pub struct UsbDevReq
{
    pub dev_req_type: u8,
    pub req:          u8,
    pub value:        u16,
    pub index:        u16,
    pub len:          u16,
}

impl UsbDevReq{

    pub fn new( dev_req_type: u8,req: u8, value: u16,
                index: u16, len: u16) -> UsbDevReq{

        UsbDevReq
            {
                dev_req_type,
                req,
                value,
                index,
                len,
            }

    }
}
