//! Application which checks the functionality of the ixgbe driver by creating multiple virtual NICs
//! and sending and receiving packets with them.
//! 
//! For the receiving functionality, you will have to set up a program on another machine that
//! sends packets to the IP addresses assigned to the virtual NICs.
//! Since we are not going through the network stack, I manually add the (mac address, ip address) pair
//! to the ARP table of the sending machine:  "sudo arp -i interface_name -s ip_address mac_address" 
//! e.g."sudo arp -i eno1 -s 192.168.0.20 0c:c4:7a:d2:ee:1a"

#![no_std]
#[macro_use] extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate network_interface_card;
extern crate ixgbe;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use ixgbe::{
    virtual_function,
    test_packets::create_raw_packet,
};
use network_interface_card::NetworkInterfaceCard;

pub fn main(_args: Vec<String>) -> isize {
    println!("Ixgbe test application");

    match rmain() {
        Ok(()) => {
            println!("Ixgbe test was successful");
            return 0;
        }
        Err(e) => {
            println!("Ixgbe test failed with error : {:?}", e);
            return -1;
        }
    }
}

fn rmain() -> Result<(), &'static str> {
    let mut nic1 = virtual_function::create_virtual_nic(1,vec!([192,168,0,13]))?;
    let mut nic2 = virtual_function::create_virtual_nic(1,vec!([192,168,0,14]))?;

    let mac_address1 = nic1.mac_address();
    let mac_address2 = nic2.mac_address();

    let buffer_a = create_raw_packet(&[0x0, 0x1F, 0xC6, 0x9C, 0xB2, 0x07], &mac_address1, &[1;46])?;
    nic1.send_packet(buffer_a)?;

    let buffer_b = create_raw_packet(&[0x0, 0x1F, 0xC6, 0x9C, 0xB2, 0x07], &mac_address2, &[1;46])?;
    nic2.send_packet(buffer_b)?;

    Ok(())
}


