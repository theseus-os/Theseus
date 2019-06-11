//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate print;
extern crate getopts;
extern crate ixgbe;
extern crate smoltcp;
extern crate ota_update_client;
extern crate network_manager;
extern crate rand;
#[macro_use] extern crate hpet;
extern crate spin;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::{Matches, Options};
// use ixgbe::rx_poll_mq;
use ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    test_nic_ixgbe_driver(None);

    // let _ = rx_poll_mq(None);

    0
}
