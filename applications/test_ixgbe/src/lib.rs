//! This application sends a packet on the ixgbe device and then polls for incoming messages

#![no_std]

#[macro_use] extern crate print;
extern crate ixgbe;

use ixgbe::rx_poll_mq;
use ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver;

#[no_mangle]
pub fn main() -> isize {
    test_nic_ixgbe_driver(None);

    let _ = rx_poll_mq(None);

    0
}
