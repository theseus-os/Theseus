#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate usb_desc;
extern crate usb_req;


use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_req::{UsbDevReq};

#[derive(Debug,Eq, PartialEq, Copy, Clone)]
pub enum Controller{

    UCHI,
    EHCI,
    NONE,

}

#[derive(Debug,Eq, PartialEq,Copy, Clone)]
pub enum HIDType{

    Unknown,
    Keyboard,
    Mouse,

}

#[derive(Debug,Copy, Clone)]
pub struct UsbDevice{

    pub port: u8,
    pub speed: u8,
    pub addr: u32,
    pub maxpacketsize: u32,
    pub controller: Controller,
    pub device_type: HIDType,
    pub interrupt_endpoint: u8,
    pub control_endpoint: u8,
    pub iso_endpoint: u8,


}

impl UsbDevice{

    pub fn default() -> UsbDevice{

        UsbDevice{ port: 0, speed: 0, addr:0, maxpacketsize:0, controller: Controller::NONE,
            device_type: HIDType::Unknown, interrupt_endpoint: 0, control_endpoint:0, iso_endpoint:0,}

    }

    pub fn new(port: u8, speed: u8, addr: u32, maxpacketsize: u32, controller: Controller,
               device_type: HIDType, interrupt_endpoint: u8, control_endpoint: u8, iso_endpoint: u8
               ) -> UsbDevice{

        UsbDevice{
            port,
            speed,
            addr,
            maxpacketsize,
            controller,
            device_type,
            interrupt_endpoint,
            control_endpoint,
            iso_endpoint,
        }


    }

}

pub struct UsbControlTransfer{

    pub endpoint: u16,
    pub request: UsbDevReq,
    pub length: u16,
    pub complete: bool,
    pub success: bool,


}

impl UsbControlTransfer{

    pub fn new(endpoint: u16, request: UsbDevReq,
               length: u16, complete: bool, success: bool) -> UsbControlTransfer{

        UsbControlTransfer{

            endpoint,
            request,
            length,
            complete,
            success,

        }

    }
}





