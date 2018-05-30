#![no_std]
#![feature(alloc)]

#[macro_use] extern crate log;
extern crate port_io;
extern crate spin;



use spin::Mutex;
use port_io::Port;


static PS2_PORT: Mutex<Port<u8>> = Mutex::new( Port::new(0x60));
static PS2_COMMAND_PORT: Mutex<Port<u8>> = Mutex::new(Port::new(0x64));

///initialize the first ps2 data port
pub fn init_ps2_port1() {

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

        // set the new configuration
        let new_config = (old_config & 0xEF) | 0x01;
        unsafe {
            PS2_COMMAND_PORT.lock().write(0x60);
            PS2_PORT.lock().write(new_config);
        }
    }

    unsafe { PS2_COMMAND_PORT.lock().write(0xAE); }
}

/// test the first ps2 data port
pub fn test_ps2_port1() {
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

    // test the first ps2 data port
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
    }

    // enable PS2 interrupt and see the new config
    {
        unsafe {
            PS2_COMMAND_PORT.lock().write(0x20);
        }

        let config = PS2_PORT.lock().read();
        if config & 0x01 == 0x01{
            info!("first ps2 port interrupt enabled")
        }else{
            warn!("first ps2 port interrupt disabled")
        }
        if config &0x10 != 0x10{
            info!("first ps2 data port enabled")
        }else{
            warn!("first ps2 data port disabled")
        }
    }
}

///initialize the second ps2 data port
pub fn init_ps2_port2() {

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


        // set the new configuration
        let new_config = (old_config & 0xDF) | 0x02;
        unsafe {
            PS2_COMMAND_PORT.lock().write(0x60);
            PS2_PORT.lock().write(new_config);

        }
    }
    unsafe {
        PS2_COMMAND_PORT.lock().write(0xA8);
        PS2_COMMAND_PORT.lock().write(0xAE);
    }
}

/// test the second ps2 data port
pub fn test_ps2_port2(){
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
    // test the second ps2 data port
    {
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

            PS2_COMMAND_PORT.lock().write(0x20);
        }

        let config = PS2_PORT.lock().read();
        if config &0x02 == 0x02{
            info!("second ps2 port interrupt enabled")
        }else{
            warn!("second ps2 port interrupt disabled")
        }
        if config &0x20 != 0x20{
            info!("second ps2 data port enabled")
        }else{
            warn!("second ps2 data port disabled")
        }
    }
}

/// write data to the first ps2 data port and return the response
fn data_to_port1(value:u8)->u8{
    unsafe { PS2_PORT.lock().write(value) };
    PS2_PORT.lock().read()
}

/// write data to the second ps2 data port and return the response
fn data_to_port2(value:u8)->u8{
    unsafe { PS2_COMMAND_PORT.lock().write(0xD4) };
    unsafe { PS2_PORT.lock().write(value) };
    PS2_PORT.lock().read()
}


/// set ps2 mouse's sampling rate
pub fn set_sampling_rate(value:u8){
    let response = data_to_port2(0xF3);
    if response == 0xFA {
        data_to_port2(value);
    }else {
        warn!("mouse command is not accepted!");

        }
}

/// set the mouse ID (3 or 4 ) by magic sequence
pub fn set_mouse_id(id:u8){
    let response = data_to_port2(0xF5);
    if response == 0xFA{
        if id == 3 {
            set_sampling_rate(200);
            set_sampling_rate(100);
            set_sampling_rate(80);
        }
            else if id == 4{
                set_sampling_rate(200);
                set_sampling_rate(100);
                set_sampling_rate(80);
                set_sampling_rate(200);
                set_sampling_rate(200);
                set_sampling_rate(80);
            }else{
                warn!("invalid id");
            }

        data_to_port2(0xF4);
    }else{
        warn!("mouse command is not accepted!");
    }

    
}

/// check mouse's id
pub fn check_mouse_id()->u8{
    let id_num :u8;
    let response = data_to_port2(0xF2);
    id_num = PS2_PORT.lock().read();
    id_num

}

/// reset the mouse
pub fn reset_mouse(){
    let response = data_to_port1(0xFF);
    if response != 0xFA{
        warn!("reset command is not accepted");
        return;
    }
    let mut count:u8 = 0;
    loop{
            if count == 20{
                warn!("mouse reset failed!");
                return;
            }
            let more_bytes = PS2_PORT.lock().read();
            if more_bytes == 0xAA{
                info!("mouse is reset");
                break;
            }
            count += 1;
        }
}

///resend the most recent packet again
pub fn mouse_resend(){
    let response = data_to_port2(0xFE);
    if response != 0xFA{
        warn!("resend command is not accepted");
    }
}

///set the mouse to the default
pub fn mouse_default(){
    let response = data_to_port2(0xf6);
    if response != 0xFA{
        warn!("default command is not accepted");
    }
}

///enable or disable the packet streaming
/// 0 enable, 1 disable
pub fn mouse_packet_streaming(value:u8) {
    if value == 1 {
        let response = data_to_port2(0xf5);
        if response != 0xFA {
            warn!("disable streaming failed");
        }
    } else if value == 0 {
        let response = data_to_port2(0xf4);
        if response != 0xFA {
            warn!("disable streaming failed");
        }
    }else{
        warn!("invalid parameter");
    }
}

///set the resolution of the mouse
/// 0x00: 1 count/mm; 0x01 2 count/mm;
/// 0x02 4 count/mm; 0x03 8 count/mm;
pub fn mouse_resolution(value:u8){
    let response = data_to_port2(0xE8);
    if response == 0xFA {
        let ack = data_to_port2(value);
        if ack != 0xFA{
            warn!("failed to set the resolution");
        }
    }else {
        warn!("command is not accepted!");
    }
}

///set LED status of the keyboard
/// 0: ScrollLock; 1: NumberLock; 2: CapsLock
pub fn keyboard_led(value:u8){
    let response = data_to_port1(0xED);
    if response == 0xFA{
        let ack = data_to_port1(value);
        if ack != 0xFA{
            warn!("set LED failed")
        }
    }else if response == 0xFE{
        warn!("plz send the set led command again!");
    }else{
        warn!("command is not accepted")
    }
}

/// set the scancode set of the keyboard
/// 0: get the current set; 1: set 1
/// 2: set 2; 3: set 3
pub fn scancode_set(value:u8){
    let response = data_to_port1(0xF0);
    if response == 0xFA{
        let ack = data_to_port1(value);
        if ack != 0xFA && ack != 0xFE{
            warn!("request failed")
        }
    } else if response == 0xFE{
        warn!("plz resend the command again!");
    }else{
        warn!("command is not accepted!");
    }

}

///detect the keyboard
pub fn keyboard_detect(){
    let response = data_to_port1(0xF2);
        if response == 0xFA{
            let reply = PS2_PORT.lock().read();
            match reply {
                0xAB => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0x41 => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0xAB => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0xC1 => info!("MF2 keyboard with translation enabled in the PS/Controller"),
                0x83 => info!("MF2 keyboard"),
                _=> {}
            }
        }else {
            warn!("command is not accepted!")
        }

}