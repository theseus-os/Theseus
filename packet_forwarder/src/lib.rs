//! Basic Packet Forwarder that receives as many packets as the PACKETS_LIMIT variable allows, changes the address by 1, and then forwards all the recieved packets back 


#![no_std]
#[macro_use] extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate network_interface_card;
extern crate ixgbe;
extern crate spawn;
extern crate getopts;
extern crate nic_buffers;
extern crate memory;
extern crate memory_structs;

use core::ops::Add;

use alloc::vec::Vec;
use alloc::string::String;
use ixgbe::{
    get_ixgbe_nics_list,
    IXGBE_NUM_RX_QUEUES_ENABLED, IXGBE_NUM_TX_QUEUES_ENABLED, tx_send_mq, rx_poll_mq 
};
use network_interface_card::NetworkInterfaceCard;
use getopts::{Matches, Options};
use nic_buffers::{ReceivedFrame, TransmitBuffer, ReceiveBuffer};
use memory::{MappedPages};
use memory_structs::{PhysicalAddress};

const PACKETS_LIMIT: usize = 16;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "prints the help menu");
    opts.optflag("c", "commence", "starts the operation");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_) => {
            println!("invalid flag!");
            return -1;
        }
    };

    if matches.opt_present("h") {
        println!("Run with flag -c to commence the packet forwarding");
        return 0;
    }

    match rmain(&matches, &opts) {
        Ok(()) => {
            println!("Packet forwarding successful");
            return 0;
        }
        Err(e) => {
            println!("Packet forwarding failed with error: {:?}", e);
            return -1;
        }

    }

}

fn rmain(matches: &Matches, opts: &Options) -> Result<(), &'static str> {
    println!("Waiting to receive packets...");
    let (dev_id, mac_address) = {
        let ixgbe_devs = get_ixgbe_nics_list().ok_or("IXGBE NICs list not initialized")?;
        if ixgbe_devs.is_empty() {
            return Err("No ixgbe device init'd");
        }
        let nic = ixgbe_devs[0].lock();
        (nic.device_id(), nic.mac_address())
    };

    if matches.opt_present("commence") {

        if IXGBE_NUM_RX_QUEUES_ENABLED != IXGBE_NUM_TX_QUEUES_ENABLED {
            return Err("Unequal number of Rx and Tx queues enabled")
        }

        // receving data

        let mut received_frames: Vec<TransmitBuffer> = Vec::with_capacity(PACKETS_LIMIT);
        
        'receive: loop {
            for qid in 0..IXGBE_NUM_RX_QUEUES_ENABLED { 
                if let Ok(buffer) = rx_poll_mq(qid as usize, dev_id) {
                    println!("Received packet on queue {}", qid);
                    let mut frame = buffer.0;
                    if received_frames.len() == PACKETS_LIMIT {
                        break 'receive; 
                    } else {
                        let mut frame = frame.remove(0);
                        let mut old_mp = core::mem::replace(& mut frame.mp, MappedPages::empty());
                        let m: &mut [u8] = old_mp.as_slice_mut(0, 16)?;

                        // we offset by 1
                        m[0] += 1; 

                        let mut frame = TransmitBuffer { 
                            mp: old_mp, 
                            phys_addr: frame.phys_addr,
                            length: frame.length
                        };
                        
                        received_frames.push(frame);
                    }

                } 
            }
        }

        // sending what was received

        println!("Sending received frames..");

        while !received_frames.is_empty() {
            if let Ok(_frame) = tx_send_mq(1, dev_id, Some(received_frames.remove(0))) {
                println!("Sent packet on queue {}", 1)
            }           
        }
    }
    Ok(())
}