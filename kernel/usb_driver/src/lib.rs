#![no_std]
#![feature(alloc)]

#![allow(dead_code)]
extern crate usb_uhci;
extern crate usb_device;

pub enum Controller{

    UCHI,
    EHCI,

}
