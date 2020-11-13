#![no_std]
/// To have another machine send on these IP addresses, add them to the ARP table since we're testing the driver without the network stack
/// and an ARP message reply won't be sent:  "sudo arp -i eno1 -s ip_address mac_address" 
/// e.g."sudo arp -i eno1 -s 192.168.0.20 0c:c4:7a:d2:ee:1a"

// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate network_interface_card;
extern crate ixgbe;
extern crate hpet;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use hpet::get_hpet;
use ixgbe::virtual_function;
use ixgbe::test_ixgbe_driver::create_raw_packet;

use network_interface_card::NetworkInterfaceCard;

pub fn main(_args: Vec<String>) -> isize {
    println!("Ixgbe raw packet io application");
    let mut nic = ixgbe::get_ixgbe_nic().unwrap().lock();

    let mut vnic = nic.create_virtual_nic_0().expect("Couldn't create virtual nic");

    let batch_size = 1;
    let mut buffers = Vec::with_capacity(batch_size);

    for _ in 0..batch_size {
        let transmit_buffer = ixgbe::test_ixgbe_driver::create_raw_packet(&[0x90,0xe2,0xba,0x88,0x65,0x80], &vnic.mac_address(), &[1;46]).unwrap();
        buffers.push(transmit_buffer);
    }

    for _ in 0..10 {
        vnic.send_batch(&buffers).unwrap();
    }
    0
}

