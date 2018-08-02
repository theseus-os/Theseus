#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate usb_desc;
extern crate usb_req;


use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_req::{UsbDevReq};

pub enum Controller{

    UCHI,
    EHCI,

}

pub struct UsbEndpoint{

    pub description: UsbEndpDesc,
    pub toggle: u8,
}

impl UsbEndpoint{

    pub fn new(description: UsbEndpDesc, toggle: u8) -> UsbEndpoint{

        UsbEndpoint{

            description,
            toggle,
        }
    }
}

pub struct UsbDevice{

    pub port: u8,
    pub speed: u8,
    pub addr: u32,
    pub maxpacketsize: u32,
    pub controller: Controller,
}

impl UsbDevice{

    pub fn new(port: u8, speed: u8, addr: u32, maxpacketsize: u32, controller: Controller) -> UsbDevice{

        UsbDevice{
            port,
            speed,
            addr,
            maxpacketsize,
            controller,
        }


    }

}

pub struct UsbTransfer{

    pub endpoint: u16,
    pub request: UsbDevReq,
    pub length: u16,
    pub complete: bool,
    pub success: bool,


}

impl UsbTransfer{

    pub fn new(endpoint: u16, request: UsbDevReq,
               length: u16, complete: bool, success: bool) -> UsbTransfer{

        UsbTransfer{

            endpoint,
            request,
            length,
            complete,
            success,

        }

    }
}





