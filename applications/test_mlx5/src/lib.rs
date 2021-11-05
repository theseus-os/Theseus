//! Application which checks the functionality of the mlx5 driver.
//! Currently we use functions directly provided by the driver for sending and receiving packets rather than 
//! the [`NetworkInterfaceCard`] trait.

#![no_std]
#[macro_use] extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate mlx5;
extern crate ixgbe;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use ixgbe::{
    test_packets::create_raw_packet,
};


pub fn main(_args: Vec<String>) -> isize {
    println!("mlx5 test application");

    match rmain() {
        Ok(()) => {
            println!("mlx5 test was successful");
            return 0;
        }
        Err(e) => {
            println!("mlx5 test failed with error : {:?}", e);
            return -1;
        }
    }
}

fn rmain() -> Result<(), &'static str> {
    
    let mut nic = mlx5::get_mlx5_nic().ok_or("mlx5 nic isn't initialized")?.lock();

    let mac_address = nic.mac_address();
    
    let buffer = create_raw_packet(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], &mac_address, &[1;46])?;
    
    nic.send(buffer)?;

    let buffer = create_raw_packet(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], &mac_address, &[1;46])?;
    
    nic.send(buffer)?;

    Ok(())
}


