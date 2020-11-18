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
extern crate libtest;
extern crate irq_safety;

use alloc::vec::Vec;
use alloc::string::String;
use hpet::get_hpet;
use ixgbe::virtual_function;
use ixgbe::test_ixgbe_driver::create_raw_packet;
use libtest::{hpet_timing_overhead, hpet_2_us, hpet_2_ns};
use network_interface_card::NetworkInterfaceCard;

pub fn main(_args: Vec<String>) -> isize {
    println!("Ixgbe raw packet io application");

    match rmain() {
        Ok(()) => return 0,
        Err(e) => {
            println!("error: {}", e);
            return -1;
        }
    }
}

fn rmain() -> Result<(), &'static str> {
    let mut nic = ixgbe::get_ixgbe_nic().unwrap().lock();
    let mut vnic = nic.create_virtual_nic_0()?;

    // nic.link_status();

    let batch_size = 32;
    let mut buffers = Vec::with_capacity(batch_size);

    for _ in 0..batch_size {
        let transmit_buffer = ixgbe::test_ixgbe_driver::create_raw_packet(&[0x90,0xe2,0xba,0x88,0x65,0x81], &vnic.mac_address(), &[1;46])?;
        buffers.push(transmit_buffer);
    }

    let overhead = hpet_timing_overhead()?;
	let hpet = get_hpet().ok_or("Could not retrieve hpet counter")?;

    irq_safety::disable_interrupts();

    let mut counter = 0;
    let mut start_time = hpet.get_counter();

    loop {
        // for _ in 0..total_packets/batch_size {
            let _ = vnic.send_batch(&buffers, 1);
        // }

        if counter & 0xFFF == 0 {
            let elapsed_time = hpet.get_counter();
            let ns = hpet_2_ns(elapsed_time - start_time - overhead);

            // every second
            if ns > 1_000_000_000 {
                let (rx_pkts, rx_bytes, tx_pkts, tx_bytes) = nic.get_stats();
                println!("Tx Pkts: {}, Tx bytes: {}, {} ns, {:.3} Mpps", tx_pkts, tx_bytes, ns, tx_pkts as f64 * 1000.0 / ns as f64);

                start_time = hpet.get_counter();
            }
        }

        counter += 1;
    }

    irq_safety::enable_interrupts();

    Ok(())
}
