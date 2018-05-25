#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate port_io;
extern crate kernel_config;
extern crate keyboard;
extern crate mouse;
extern crate spin;
extern crate dfqueue;
extern crate console_types;

use dfqueue::DFQueueProducer;
use spin::{Mutex, Once};
use port_io::Port;
use console_types::ConsoleEvent;

static PS2_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(0x60));
static PS2_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));

pub fn init(console_producer: DFQueueProducer<ConsoleEvent>) {

    //disable PS2 ports first
    unsafe {
        PS2_COMMAND_PORT.lock().write(0xA7);
        PS2_COMMAND_PORT.lock().write(0xAD);
    }

    // clean the output buffer of ps2
    loop {
        if PS2_COMMAND_PORT.lock().read() & 0x01 == 0 {
            break;
        } else {
            PS2_PORT.lock().read();
        }
    }

    // set the configuration
    {
        // read the old configuration
        unsafe { PS2_COMMAND_PORT.lock().write(0x20); }
        let old_config = PS2_PORT.lock().read();

        debug!("see the initial configuration of the PS2 port{:b}", old_config);

        // set the new configuration
        let new_config = (old_config & 0xCF) | 0x03;
        unsafe {
            PS2_COMMAND_PORT.lock().write(0x60);
            PS2_PORT.lock().write(new_config);
        }
    }

    // test the ps2 controller
    {
        unsafe { PS2_COMMAND_PORT.lock().write(0xAA) };
        let test_flag = PS2_PORT.lock().read();
        if test_flag == 0xFC {
            warn!("ps2 controller test failed!!!")
        } else if test_flag == 0x55 {
            info!("ps2 controller test pass!!!")
        }
    }

    // test the ps2 data port
    {
        unsafe { PS2_COMMAND_PORT.lock().write(0xAB) }
        let test_flag = PS2_PORT.lock().read();
        match test_flag {
            0x00 => info!("first ps2 port test pass!!!"),
            0x01 => warn!("first ps2 port clock line stuck low"),
            0x02 => warn!("first ps2 port clock line stuck high"),
            0x03 => warn!("first ps2 port data line stuck low"),
            0x04 => warn!("first ps2 port data line stuck high"),
            _ => {}
        }

        unsafe { PS2_COMMAND_PORT.lock().write(0xA9) }
        let test_flag = PS2_PORT.lock().read();
        match test_flag {
            0x00 => info!("second ps2 port test pass!!!"),
            0x01 => warn!("second ps2 port clock line stuck low"),
            0x02 => warn!("second ps2 port clock line stuck high"),
            0x03 => warn!("second ps2 port data line stuck low"),
            0x04 => warn!("second ps2 port data line stuck high"),
            _ => {}
        }
    }

    // enable PS2 interrupt and see the new config
    {
        unsafe {
            PS2_COMMAND_PORT.lock().write(0xA8);
            PS2_COMMAND_PORT.lock().write(0xAE);
            PS2_COMMAND_PORT.lock().write(0x20);
        }

        let config = PS2_PORT.lock().read();
        if config & 0x01 == 0x01{
            info!("first ps2 port interrupt enabled")
        }else{
            warn!("first ps2 port interrupt disabled")
        }
        if config &0x02 == 0x02{
            info!("second ps2 port interrupt enabled")
        }else{
            warn!("second ps2 port interrupt disabled")
        }
        if config &0x10 != 0x10{
            info!("first ps2 data port enabled")
        }else{
            warn!("first ps2 data port disabled")
        }
        if config &0x20 != 0x20{
            info!("second ps2 data port enabled")
        }else{
            warn!("second ps2 data port disabled")
        }
    }

    mouse::init();
    info!("\n\n\nmouse driver initialized\n\n\n");
    keyboard::init(console_producer);
    info!("\n\n\nkeyboard driver initialized\n\n\n");
}







