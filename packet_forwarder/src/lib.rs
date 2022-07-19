//! Basic Packet Forwarder that receives as many packets as the PACKETS_LIMIT variable allows, changes the address by 1, and then forwards all the recieved packets back 

// TODO: Ring Buffer?
// TODO: Break up the code structure and tighten the loop to resemble other impls 
// TODO: Make it so it works on only one queue instead of having to iter through all of the avaliable ones
// Specifying Rx and Tx Devices
// slowing down poll times

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
extern crate pci;

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
use pci::{PciLocation};

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
        let received_frames: Vec<TransmitBuffer> = Vec::with_capacity(PACKETS_LIMIT);

        if IXGBE_NUM_RX_QUEUES_ENABLED != IXGBE_NUM_TX_QUEUES_ENABLED {
            return Err("Unequal number of Rx and Tx queues enabled")
        }
    
        forward(received_frames, 0, dev_id, 1)?;
    }
    Ok(())
}

fn forward(
    mut buffer: Vec<TransmitBuffer>,
    _tx_queue: usize,
    dev_id: PciLocation,
    _qid: usize
) -> Result<(), &'static str> {
    batch_rx(PACKETS_LIMIT, &mut buffer, dev_id)?;
    batch_tx(& mut buffer, 1, dev_id);
    Ok(())
}

fn batch_rx
(
    packets_limit: usize,
    buffer: &mut Vec<TransmitBuffer>,
    dev_id: PciLocation,
) -> Result<& mut Vec<TransmitBuffer>, &'static str> 

{
    for qid in 0..IXGBE_NUM_RX_QUEUES_ENABLED {
        'receive: loop {
            if let Ok(b) = rx_poll_mq(qid as usize, dev_id) {
                println!("Received packet on queue {}", qid);
                let mut frame = b.0;
                if buffer.len() != packets_limit {
                    let mut frame = frame.remove(0);
                    let mut old_mp = core::mem::replace(& mut frame.mp, MappedPages::empty());
                    let m: &mut [u8] = old_mp.as_slice_mut(0, 16)?;

                    // we offset by 1
                    m[0] += 1; 

                    let frame = TransmitBuffer { 
                        mp: old_mp, 
                        phys_addr: frame.phys_addr,
                        length: frame.length
                    };
                    
                    buffer.push(frame);
                } else {
                    println!("Done Receiving Packets");
                    break 'receive;
                }
            }
        }  
    } 
    Ok(buffer)
}

fn batch_tx (
    buffer: &mut Vec<TransmitBuffer>,
    qid: usize,
    dev_id: PciLocation,
) {
    println!("Sending received frames..");
    while !buffer.is_empty() {
        if let Ok(_frame) = tx_send_mq(qid, dev_id, Some(buffer.remove(0))) {
            println!("Sent packet on queue {}", 1);
        }           
    } 
}