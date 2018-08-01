#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate alloc;




pub struct UsbDevice{

    port: u32,
    speed: u32,
    addr: u32,
    maxpacketsize: u32,
    endpoint: UsbEndpDesc,
}