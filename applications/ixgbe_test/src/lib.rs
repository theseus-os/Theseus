#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate ixgbe;

use alloc::vec::Vec;
use alloc::string::String;


pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    println!("Ixgbe test application");
    {
        let mut nic = ixgbe::get_ixgbe_nic().unwrap().lock();
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,13],0,0,1,7,0).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,14],0,0,1,7,1).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,15],0,0,1,7,2).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,16],0,0,1,7,3).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,17],0,0,1,7,4).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,18],0,0,1,7,5).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,19],0,0,1,7,6).expect("couldn't set filter");
        nic.set_5_tuple_filter([192,168,0,12],[192,168,0,20],0,0,1,7,7).expect("couldn't set filter");
    }
    ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver(None);

    loop {
        ixgbe::rx_poll_mq(0).expect("rx failed");
        ixgbe::rx_poll_mq(1).expect("rx failed");
        ixgbe::rx_poll_mq(2).expect("rx failed");
        ixgbe::rx_poll_mq(3).expect("rx failed");
        ixgbe::rx_poll_mq(4).expect("rx failed");
        ixgbe::rx_poll_mq(5).expect("rx failed");
        ixgbe::rx_poll_mq(6).expect("rx failed");
        ixgbe::rx_poll_mq(7).expect("rx failed");
    }

    0
}
