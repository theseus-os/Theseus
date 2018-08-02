#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate usb_desc;
extern crate usb_req;


use usb_desc::{UsbEndpDesc,UsbDeviceDesc,UsbConfDesc,UsbIntfDesc};
use usb_req::{UsbDevReq};


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


pub fn usb_dev_request(dev: UsbDevice, dev_req_type: u8,request: u8, value: u16,
                       index: u16, len: u16){

    let dev_request = UsbDevReq::new(dev_req_type,request, value, index, len);
    let usb_transfer = UsbTransfer::new(0,dev_request,len,false,false);
    match dev.controller{

        Controller::EHCI => {

        },

        Controller::UCHI => {

        },

        _=> {

        }
    }

}

pub fn uhci_dev_control(dev: &UsbDevice, trans: &UsbTransfer){

    let speed = dev.speed;
    let addr = dev.addr;
    let endpoint = 0;
    let size = dev.maxpacketsize;
    let req_type = trans.request.dev_req_type;
    let len = trans.request.len;


}
