#![no_std]
/// To have another machine send on these IP addresses, add them to the ARP table since we're testing the driver without the network stack
/// and an ARP message reply won't be sent:  "sudo arp -i eno1 -s ip_address mac_address" 
/// e.g."sudo arp -i eno1 -s 192.168.0.20 0c:c4:7a:d2:ee:1a"

// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate ixgbe;
extern crate hpet;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use hpet::get_hpet;
use ixgbe::virtual_function;

pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    // println!("Ixgbe test application");
    // {
    //     let mut nic = ixgbe::get_ixgbe_nic().unwrap().lock();
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,13],0,0,1,7,0).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,14],0,0,1,7,1).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,15],0,0,1,7,2).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,16],0,0,1,7,3).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,17],0,0,1,7,4).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,18],0,0,1,7,5).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,19],0,0,1,7,6).expect("couldn't set filter");
    //     nic.set_5_tuple_filter([192,168,0,12],[192,168,0,20],0,0,1,7,7).expect("couldn't set filter");
    // }
    // ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver(None);
    // ixgbe::tx_send_mq(0).expect("couldn't send");
    // ixgbe::tx_send_mq(1).expect("couldn't send");
    // ixgbe::tx_send_mq(2).expect("couldn't send");
    // ixgbe::tx_send_mq(3).expect("couldn't send");
    // ixgbe::tx_send_mq(4).expect("couldn't send");
    // ixgbe::tx_send_mq(5).expect("couldn't send");
    // ixgbe::tx_send_mq(6).expect("couldn't send");
    // ixgbe::tx_send_mq(7).expect("couldn't send");


    // let taskref = spawn::new_task_builder(measure_dca_time, ())
    //     .name(String::from("dca_time"))
    //     .pin_on_core(8)
    //     .spawn()
    //     .expect("couldn't create task");

    // let _ = taskref.join();
    {
        let a = virtual_function::create_virtual_nic(1,vec!([192,168,0,1]),1);
        if a.is_err() {println!("can't create nic");}
        let b = virtual_function::create_virtual_nic(1,vec!([192,168,0,1]),1);
        if b.is_err() {println!("can't create nic");}

    }
    0
}

fn measure_dca_time(_:()) {
    let hpet = get_hpet().ok_or("couldn't get HPET timer").unwrap();
	// to warm cache and remove error
	let _start_hpet_tmp = hpet.get_counter();

	let start_hpet = hpet.get_counter();
    for i in 0..256 {
        let packet = ixgbe::rx_poll_mq(0).expect("rx failed");
        let address = packet.0[0].start_address();
        unsafe{*(address.value() as *mut u8) = 32};
        // ixgbe::rx_poll_mq(1).expect("rx failed");
        // ixgbe::rx_poll_mq(2).expect("rx failed");
        // ixgbe::rx_poll_mq(3).expect("rx failed");
        // ixgbe::rx_poll_mq(4).expect("rx failed");
        // ixgbe::rx_poll_mq(5).expect("rx failed");
        // ixgbe::rx_poll_mq(6).expect("rx failed");
        // ixgbe::rx_poll_mq(7).expect("rx failed");
    }
	let end_hpet = hpet.get_counter();

    error!("time: {} ticks", end_hpet - start_hpet);
}